#!/usr/bin/env python3
"""barre relayout == fresh fuzzer: builds fuzzer/fuzz.cpp against an engine and runs it.

See docs/FUZZ.md. Usage:

    ./fuzz [engine options] COMMAND [fuzzer options]

Engine options (before COMMAND):
    --lib PATH          link this prebuilt barre-ffi staticlib, don't run cargo
    --engine-root DIR   cargo workspace to build barre-ffi from (default: this checkout)
    --target-dir DIR    CARGO_TARGET_DIR for that build
    --debug             build barre-ffi without --release (debug assertions on)
    --yoga              link real Yoga built from vendor/yoga/yoga/**/*.cpp
    --slots N           diagnostic variant: barre with MAX_CACHED_MEASUREMENTS = N,
                        built from a patched copy under the build dir (the checkout
                        is not touched)
    --build-dir DIR     default: $BARRE_FUZZ_BUILD_DIR, else <checkout>/build/fuzz
    --yoga-passes       for min / run --minimise: also build against real Yoga and
                        keep only candidates Yoga passes (hunts barre-only failures)
    --slots-pass N      likewise with the --slots N variant (hunts failures that more
                        measure-cache slots make go away)

Commands are fuzz.cpp's (run, show, check, min, snippet). `snippet FILE --run`
also compiles the snippet against the same engine and runs it.

    ./fuzz compare A.json B.json
    ./fuzz calls-compare A.txt B.txt

`calls` prints each case's measure callbacks per relayout step (no fresh
layouts); `calls-compare` lines up two engines' outputs (report-only).

compares two `run --json` outputs (e.g. barre and --yoga, or barre and --slots 64)
by (seed, mode): which failures both have, which only one has, by class. It needs
no engine.
"""

import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "fuzzer" / "fuzz.cpp"
SLOTS_RE = re.compile(r"pub const MAX_CACHED_MEASUREMENTS: usize = \d+;")


def shash(s: str) -> str:
    return hashlib.sha256(s.encode()).hexdigest()[:10]


class FuzzError(Exception):
    pass


def vendor_dir() -> Path:
    v = os.environ.get("BARRE_VENDOR")
    vendor = Path(v).resolve() if v else ROOT / "vendor"
    if not (vendor / "yoga" / "yoga" / "Yoga.h").exists():
        raise FuzzError(
            f"no Yoga checkout at {vendor}/yoga (submodules not checked out?). "
            "Run `git submodule update --init` or set BARRE_VENDOR to a checkout's vendor/ dir."
        )
    return vendor


def native_libs():
    if sys.platform == "darwin":
        return ["-liconv"]
    return ["-lpthread", "-ldl", "-lm", "-lrt", "-lutil", "-lc"]


def cxx():
    return os.environ.get("CXX") or shutil.which("clang++") or "c++"


def run(cmd, **kw):
    proc = subprocess.run(cmd, capture_output=True, text=True, errors="replace", **kw)
    if proc.returncode != 0:
        raise FuzzError(f"{' '.join(map(str, cmd[:4]))} ... failed:\n" + (proc.stdout + proc.stderr)[-4000:])
    return proc


def cargo_build(root: Path, target_dir, release: bool) -> Path:
    cmd = ["cargo", "build", "-p", "barre-ffi", "--message-format=json-render-diagnostics"]
    if release:
        cmd.append("--release")
    env = dict(os.environ)
    if target_dir:
        env["CARGO_TARGET_DIR"] = str(Path(target_dir).resolve())
    proc = run(cmd, cwd=root, env=env)
    lib = None
    for line in proc.stdout.splitlines():
        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            continue
        if msg.get("reason") == "compiler-artifact" and "staticlib" in msg.get("target", {}).get("kind", []):
            for f in msg.get("filenames", []):
                if f.endswith(".a"):
                    lib = Path(f)
    if lib is None or not lib.exists():
        raise FuzzError("cargo build -p barre-ffi produced no staticlib")
    return lib


def slots_variant(root: Path, build_dir: Path, slots: int, release: bool) -> Path:
    """Copy the engine's sources, set MAX_CACHED_MEASUREMENTS, build. Diagnostics only."""
    var = build_dir / f"slots-{slots}"
    var.mkdir(parents=True, exist_ok=True)
    for name in ("Cargo.toml", "Cargo.lock", "rust-toolchain.toml"):
        if (root / name).exists():
            shutil.copy2(root / name, var / name)
    shutil.copytree(root / "crates", var / "crates", dirs_exist_ok=True,
                    ignore=shutil.ignore_patterns("target"))
    f = var / "crates" / "barre" / "src" / "node" / "layout_results.rs"
    text = f.read_text()
    new, n = SLOTS_RE.subn(f"pub const MAX_CACHED_MEASUREMENTS: usize = {slots};", text)
    if n != 1:
        raise FuzzError(f"--slots: no `pub const MAX_CACHED_MEASUREMENTS: usize = N;` in {f}")
    if new != text:
        f.write_text(new)
    return cargo_build(var, build_dir / f"slots-{slots}-target", release)


def yoga_lib(vendor: Path, build_dir: Path) -> Path:
    srcs = sorted((vendor / "yoga" / "yoga").rglob("*.cpp"))
    out = build_dir / "yoga"
    out.mkdir(parents=True, exist_ok=True)
    objs = []
    for s in srcs:
        o = out / (s.relative_to(vendor / "yoga" / "yoga").as_posix().replace("/", "_") + ".o")
        if not o.exists() or o.stat().st_mtime < s.stat().st_mtime:
            run([cxx(), "-std=c++20", "-O2", "-DNDEBUG", f"-I{vendor / 'yoga'}", "-c", str(s), "-o", str(o)])
        objs.append(o)
    lib = out / "libyoga.a"
    if not lib.exists() or any(o.stat().st_mtime > lib.stat().st_mtime for o in objs):
        if lib.exists():
            lib.unlink()
        run(["ar", "rcs", str(lib), *map(str, objs)])
    return lib


def compile_against(src: Path, lib: Path, out: Path, vendor: Path, extra=()):
    if out.exists() and out.stat().st_mtime > max(src.stat().st_mtime, lib.stat().st_mtime):
        return out
    run([cxx(), "-std=c++20", "-O2", "-g", "-Wall", *extra, f"-I{vendor / 'yoga'}", str(src), str(lib),
         *native_libs(), "-o", str(out)])
    return out


def parse(argv):
    opts = {"lib": None, "engine_root": None, "target_dir": None, "debug": False, "yoga": False,
            "slots": None, "build_dir": None, "yoga_passes": False, "slots_pass": None}
    i = 0
    while i < len(argv) and argv[i].startswith("--"):
        a = argv[i]
        if a in ("--debug", "--yoga", "--yoga-passes"):
            opts[a[2:].replace("-", "_")] = True
            i += 1
            continue
        key = a[2:].replace("-", "_")
        if key not in opts or i + 1 >= len(argv):
            break
        opts[key] = argv[i + 1]
        i += 2
    if i < len(argv) and argv[i] in ("-h", "--help", "help"):
        print(__doc__)
        sys.exit(0)
    return opts, argv[i:]


def load_json(path):
    out, nonfinite = {}, set()
    for line in Path(path).read_text().splitlines():
        if line.strip():
            r = json.loads(line)
            if r.get("kind") == "nonfinite":
                nonfinite.add((r["seed"], r["mode"], r["scale"]))
            else:
                out[(r["seed"], r["mode"])] = r
    return out, nonfinite


def compare(a_path, b_path):
    (a, na), (b, nb) = load_json(a_path), load_json(b_path)
    keys = sorted(set(a) | set(b))
    rows = {}
    for k in keys:
        ca = a[k]["class"] if k in a else "pass"
        cb = b[k]["class"] if k in b else "pass"
        rows.setdefault((ca, cb), []).append(k)
    print(f"A = {a_path} ({len(a)} failing (seed,mode))\nB = {b_path} ({len(b)} failing (seed,mode))")
    print(f"{'class in A':<14} {'class in B':<14} {'pairs':>6}  examples (seed/mode)")
    for (ca, cb), ks in sorted(rows.items(), key=lambda kv: -len(kv[1])):
        same = ""
        if ca != "pass" and cb != "pass":
            n = sum(1 for k in ks if (a[k]["path"], a[k]["field"], a[k]["relayout"], a[k]["fresh"])
                    == (b[k]["path"], b[k]["field"], b[k]["relayout"], b[k]["fresh"]))
            same = f"  ({n} with the same first difference)"
        ex = " ".join(f"{s}/{m}" for s, m in ks[:6])
        print(f"{ca:<14} {cb:<14} {len(ks):>6}  {ex}{same}")
    scales = {"both": {0, 1}, "pass": set()}
    def failing(r):
        c = r["class"] if r else "pass"
        return scales.get(c, {0} if c == "scale0-only" else {1})
    for name, x, y in (("A", a, b), ("B", b, a)):
        only = [(k, sorted(failing(x[k]) - failing(y.get(k)))) for k in sorted(x)]
        only = [(k, sc) for k, sc in only if sc]
        if only:
            print(f"{name} fails where the other passes, at that scale ({len(only)}; seed/mode/scale): "
                  + " ".join(f"{k[0]}/{k[1]}/{','.join(map(str, sc))}" for k, sc in only[:40])
                  + (" ..." if len(only) > 40 else ""))
    if na or nb:
        print(f"NaN/infinite values in fresh layouts, (seed, mode, scale) runs: A {len(na)}, B {len(nb)}, "
              f"both {len(na & nb)}")
        for name, only in (("A", sorted(na - nb)), ("B", sorted(nb - na))):
            if only:
                print(f"  only in {name}: " + " ".join(f"{s}/{m}/{sc}" for s, m, sc in only[:12]))
    return 0


def load_calls(path):
    out = {}
    for line in Path(path).read_text().splitlines():
        f = line.split()
        if len(f) == 5:
            out[(int(f[0]), int(f[1]), f[2])] = (int(f[3]), [int(x) for x in f[4].split(",")])
    return out


def calls_compare(a_path, b_path):
    """Lines up two `calls` outputs (e.g. barre and --yoga) by (seed, mode, scale)."""
    a, b = load_calls(a_path), load_calls(b_path)
    keys = sorted(set(a) & set(b))
    if not keys:
        raise FuzzError("calls-compare: no (seed, mode, scale) in both files")
    more = [k for k in keys if a[k][0] > b[k][0]]
    fewer = [k for k in keys if a[k][0] < b[k][0]]
    ta, tb = sum(a[k][0] for k in keys), sum(b[k][0] for k in keys)
    steps_more = sum(1 for k in keys for x, y in zip(a[k][1], b[k][1]) if x > y)
    steps_fewer = sum(1 for k in keys for x, y in zip(a[k][1], b[k][1]) if x < y)
    nsteps = sum(min(len(a[k][1]), len(b[k][1])) for k in keys)
    print(f"A = {a_path}\nB = {b_path}")
    print(f"(seed, mode, scale) runs: {len(keys)}; A more callbacks in {len(more)}, fewer in {len(fewer)}, "
          f"equal in {len(keys) - len(more) - len(fewer)}")
    print(f"layouts: {nsteps}; A more in {steps_more}, fewer in {steps_fewer}")
    print(f"callbacks: A {ta}, B {tb} ({(ta / tb - 1) * 100 if tb else float('nan'):+.2f}%)")
    for scale in sorted({k[2] for k in keys}):
        ks = [k for k in keys if k[2] == scale]
        sa, sb = sum(a[k][0] for k in ks), sum(b[k][0] for k in ks)
        print(f"  scale {scale}: A {sa}, B {sb}; A more in {sum(1 for k in ks if a[k][0] > b[k][0])}, "
              f"fewer in {sum(1 for k in ks if a[k][0] < b[k][0])} of {len(ks)}")
    worst = sorted(more, key=lambda k: b[k][0] - a[k][0])[:8]
    if worst:
        print("most extra in A (seed/mode/scale: A vs B): "
              + " ".join(f"{k[0]}/{k[1]}/{k[2]}: {a[k][0]} vs {b[k][0]}" for k in worst))
    return 0


def main(argv):
    opts, rest = parse(argv)
    if not rest:
        print(__doc__)
        return 2
    if rest[0] == "compare":
        if len(rest) != 3:
            raise FuzzError("usage: ./fuzz compare A.json B.json")
        return compare(rest[1], rest[2])
    if rest[0] == "calls-compare":
        if len(rest) != 3:
            raise FuzzError("usage: ./fuzz calls-compare A.txt B.txt")
        return calls_compare(rest[1], rest[2])
    vendor = vendor_dir()
    build_dir = Path(opts["build_dir"] or os.environ.get("BARRE_FUZZ_BUILD_DIR") or ROOT / "build" / "fuzz")
    build_dir = build_dir.resolve()
    build_dir.mkdir(parents=True, exist_ok=True)
    release = not opts["debug"]
    if opts["yoga"]:
        lib, tag = yoga_lib(vendor, build_dir), "yoga"
    elif opts["lib"]:
        lib, tag = Path(opts["lib"]).resolve(), "lib-" + shash(str(Path(opts["lib"]).resolve()))
    elif opts["slots"]:
        root = Path(opts["engine_root"] or ROOT).resolve()
        lib, tag = slots_variant(root, build_dir, int(opts["slots"]), release), f"slots{opts['slots']}"
    else:
        root = Path(opts["engine_root"] or ROOT).resolve()
        lib = cargo_build(root, opts["target_dir"], release)
        tag = "barre" if root == ROOT else "root-" + shash(str(root))
    if not release:
        tag += "-debug"
    exe = compile_against(SRC, lib, build_dir / f"fuzz-{tag}", vendor)

    if rest[0] == "snippet" and "--run" in rest:
        args = [a for a in rest if a != "--run"]
        proc = run([str(exe), *args[: 2]])
        src = build_dir / "snippet.cpp"
        src.write_text(proc.stdout)
        snip = compile_against(src, lib, build_dir / f"snippet-{tag}", vendor, extra=["-Wno-unused-variable"])
        r = subprocess.run([str(snip)])
        print(f"snippet ({src}) exited {r.returncode}"
              + (": relayout != fresh" if r.returncode == 1 else ": no difference" if r.returncode == 0 else ""))
        return r.returncode
    if opts["yoga_passes"]:
        yexe = compile_against(SRC, yoga_lib(vendor, build_dir), build_dir / "fuzz-yoga", vendor)
        rest = [*rest, "--pass-on", str(yexe)]
    if opts["slots_pass"]:
        n = int(opts["slots_pass"])
        root = Path(opts["engine_root"] or ROOT).resolve()
        sexe = compile_against(SRC, slots_variant(root, build_dir, n, release), build_dir / f"fuzz-slots{n}", vendor)
        rest = [*rest, "--pass-on", str(sexe)]
    os.execv(str(exe), [str(exe), *rest])


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except FuzzError as e:
        print(f"fuzz: {e}", file=sys.stderr)
        sys.exit(2)
