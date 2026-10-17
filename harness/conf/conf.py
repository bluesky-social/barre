#!/usr/bin/env python3
"""barre conformance runner: Yoga's gtest suite against barre (or real Yoga).

See docs/CONFORMANCE.md for the CLI contract. Outline:

1. Build (ninja, generated build.ninja under the build dir): googletest, the
   runner's gtest main, tests/util/TestUtil.cpp and one object per test source
   are engine-independent and shared by every variant. Only the final link of
   one binary per test source depends on the engine: barre's staticlib (built
   with cargo) or libyoga.a (built from vendor/yoga/yoga).
2. A file that fails to compile or link is reported as build-fail for all its
   tests. Test IDs come from --gtest_list_tests (or, for files that did not
   build, from a cached listing or the source).
3. Every test runs in its own process with a timeout and a memory cap, with a
   random nonce that only the runner's main prints after a test passed.
4. Canary tests with random names and deliberately wrong assertions are
   generated, built and run with every run; if any passes, the run is invalid
   (exit 3). On barre, the linked binaries are checked to contain barre's Rust
   code and none of Yoga's layout code.
"""

from __future__ import annotations

import argparse
import concurrent.futures as cf
import fcntl
import hashlib
import json
import os
import random
import re
import shutil
import signal
import string
import subprocess
import sys
import tempfile
import time
from pathlib import Path

RUNNER_ROOT = Path(__file__).resolve().parents[2]
HARNESS = Path(__file__).resolve().parent
CONF_DIR = RUNNER_ROOT / "conformance"

STATUSES = ["pass", "fail", "crash", "timeout", "build-fail", "skip"]
MEM_EXIT_CODE = 97
OUTPUT_KEEP = 4000

EXIT_OK, EXIT_REGRESSION, EXIT_ERROR, EXIT_INVALID = 0, 1, 2, 3


class ConfError(Exception):
    pass


# ---------------------------------------------------------------------------
# paths and options


def vendor_dir() -> Path:
    v = os.environ.get("BARRE_VENDOR")
    vendor = Path(v).resolve() if v else RUNNER_ROOT / "vendor"
    if not (vendor / "yoga" / "yoga" / "Yoga.h").exists():
        raise ConfError(
            f"no Yoga checkout at {vendor}/yoga (submodules not checked out?). "
            "Run `git submodule update --init` or set BARRE_VENDOR to a checkout's vendor/ dir."
        )
    if not (vendor / "googletest" / "googletest" / "src" / "gtest-all.cc").exists():
        raise ConfError(f"no googletest checkout at {vendor}/googletest")
    return vendor


def parse_args(argv):
    p = argparse.ArgumentParser(
        prog="conf",
        description="Build and run Yoga's gtest suite against barre (default) or real Yoga (--yoga).",
    )
    p.add_argument("filters", nargs="*", metavar="FILTER",
                   help="test ID prefixes, e.g. generated/YGFlexTest or YGMeasureTest/YogaTest.foo")
    eng = p.add_argument_group("engine")
    eng.add_argument("--yoga", action="store_true", help="link against real Yoga built from vendor/yoga")
    eng.add_argument("--release", action="store_true", help="build barre-ffi with --release")
    eng.add_argument("--engine-root", type=Path, default=None,
                     help="cargo workspace to build barre-ffi from (default: this checkout)")
    eng.add_argument("--target-dir", type=Path, default=None, help="CARGO_TARGET_DIR for the barre-ffi build")
    eng.add_argument("--lib", type=Path, default=None,
                     help="use this prebuilt barre-ffi staticlib instead of running cargo")
    run = p.add_argument_group("running")
    run.add_argument("--jobs", "-j", type=int, default=os.cpu_count() or 4)
    run.add_argument("--timeout", type=float, default=10.0, help="per-test timeout in seconds (default 10)")
    run.add_argument("--mem-limit-mb", type=int, default=2048, help="per-test memory cap, 0 = none (default 2048)")
    run.add_argument("--canaries", type=int, default=4, help="number of must-fail canary tests (default 4, min 1)")
    run.add_argument("--no-skip", action="store_true",
                     help="also build and run tests listed in conformance/skip.txt (diagnostics only)")
    run.add_argument("--build-dir", type=Path, default=None,
                     help="build directory (default: $BARRE_CONF_BUILD_DIR or <vendor>/../build/conf)")
    run.add_argument("--list", action="store_true", help="build, list test IDs with their file, and exit")
    out = p.add_argument_group("output")
    out.add_argument("--show", type=int, default=0, metavar="N", help="print the first N failures with output")
    out.add_argument("--by-file", action="store_true", help="print a per-file table")
    out.add_argument("--json", type=Path, default=None, metavar="F",
                     help='write {id: {"status": ..., "reason": ...}}')
    out.add_argument("--summary-json", type=Path, default=None, metavar="F", help="write the run summary")
    out.add_argument("--baseline", type=Path, default=None, metavar="F",
                     help="expected-pass list; report regressions (exit 1) and new passes")
    out.add_argument("--update-expected", action="store_true",
                     help="rewrite the baseline (default conformance/expected-pass.txt) with this run's passes")
    out.add_argument("--quiet", "-q", action="store_true", help="no progress output")
    a = p.parse_args(argv)
    if a.yoga and (a.release or a.engine_root or a.target_dir or a.lib):
        p.error("--yoga cannot be combined with barre build options")
    if a.update_expected and a.yoga:
        p.error("--update-expected records barre's passes; it cannot be used with --yoga")
    if a.update_expected and a.no_skip:
        p.error("--update-expected cannot be used with --no-skip")
    a.canaries = max(1, a.canaries)
    return a


def log(args, msg):
    if not args.quiet:
        print(msg, file=sys.stderr, flush=True)


# ---------------------------------------------------------------------------
# sources, skips, filters


def discover_sources(vendor: Path):
    """[(key, path)]: key is the file part of test IDs."""
    out = []
    tests = vendor / "yoga" / "tests"
    for p in sorted(tests.glob("*.cpp")):
        out.append((p.stem, p))
    for p in sorted((tests / "generated").glob("*.cpp")):
        out.append(("generated/" + p.stem, p))
    for p in sorted((CONF_DIR / "tests").glob("*.cpp")):
        out.append(("barre/" + p.stem, p))
    return out


def load_skips():
    """[(pattern, reason)] from conformance/skip.txt: '<id-prefix>  <reason>'."""
    path = CONF_DIR / "skip.txt"
    out = []
    if not path.exists():
        return out
    for n, line in enumerate(path.read_text().splitlines(), 1):
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        parts = line.split(None, 1)
        if len(parts) < 2:
            raise ConfError(f"conformance/skip.txt:{n}: entry without a reason: {line}")
        out.append((parts[0], parts[1].strip()))
    return out


def id_matches(test_id: str, prefixes) -> bool:
    return not prefixes or any(test_id.startswith(f) for f in prefixes)


def file_selected(key: str, filters) -> bool:
    k = key + "/"
    return not filters or any(k.startswith(f) or f.startswith(k) for f in filters)


def file_fully_skipped(key: str, skips):
    k = key + "/"
    for pat, reason in skips:
        if k.startswith(pat):
            return reason
    return None


def skip_reason(test_id: str, skips):
    for pat, reason in skips:
        if test_id.startswith(pat):
            return reason
    return None


TEST_RE = re.compile(r"\b(TEST|TEST_F|TEST_P)\s*\(\s*(\w+)\s*,\s*(\w+)\s*\)")


def tests_from_source(path: Path):
    return [f"{suite}.{name}" for _, suite, name in TEST_RE.findall(path.read_text(errors="replace"))]


def parse_gtest_list(text: str):
    tests, suite = [], None
    for line in text.splitlines():
        if not line.strip() or line.startswith("CONF-RESULT"):
            continue
        if not line.startswith(" "):
            suite = line.split("#")[0].strip()
        elif suite:
            tests.append(suite + line.split("#")[0].strip())
    return tests


# ---------------------------------------------------------------------------
# build


def ninja_escape(p) -> str:
    return str(p).replace("$", "$$").replace(" ", "$ ").replace(":", "$:")


def shash(s: str, n=10) -> str:
    return hashlib.sha256(s.encode()).hexdigest()[:n]


def native_libs():
    if sys.platform == "darwin":
        return ["-lSystem", "-lc", "-lm", "-liconv"]
    return ["-lpthread", "-ldl", "-lm", "-lrt", "-lutil", "-lc"]


class Build:
    """Paths and the generated build.ninja for one run."""

    def __init__(self, args, vendor: Path, build_dir: Path):
        self.args = args
        self.vendor = vendor
        self.dir = build_dir
        self.cxx = os.environ.get("CXX") or shutil.which("clang++") or "c++"
        yoga = vendor / "yoga"
        gtest = vendor / "googletest" / "googletest"
        self.common_flags = ["-std=c++20", "-O0", "-DDEBUG", "-fexceptions", "-frtti",
                             "-fno-omit-frame-pointer", "-pthread", "-Wall",
                             "-Wno-unknown-warning-option", "-Wno-character-conversion"]
        self.test_includes = [f"-I{yoga}", f"-I{yoga / 'tests'}", f"-I{gtest / 'include'}"]
        obj = build_dir / "obj"
        hdir = obj / ("harness-" + shash(str(HARNESS)))
        self.gtest_src = gtest / "src" / "gtest-all.cc"
        self.gtest_obj = obj / "gtest" / "gtest-all.o"
        self.main_obj = hdir / "main.o"
        self.shim_obj = hdir / "event_shim.o"
        self.util_obj = obj / "util" / "TestUtil.o"
        self.yoga_lib = build_dir / "yoga" / "libyoga.a"
        self.yoga_srcs = sorted((yoga / "yoga").rglob("*.cpp"))
        self.obj = obj

    def test_obj(self, key):
        if key.startswith("barre/"):
            # barre's own tests come from the runner's checkout, which differs
            # between the main checkout and worktrees sharing this build dir.
            return self.obj / ("barre-" + shash(str(CONF_DIR))) / (key[len("barre/"):] + ".o")
        return self.obj / "tests" / (key + ".o")

    def bin_dir(self, variant):
        return self.dir / "bin" / variant

    def bin(self, variant, key):
        return self.bin_dir(variant) / key

    def common_objs(self, barre: bool):
        objs = [self.main_obj, self.util_obj, self.gtest_obj]
        if barre:
            objs.append(self.shim_obj)
        return objs

    def write_ninja(self, keys_srcs, variant, engine_lib: Path, barre: bool):
        e = ninja_escape
        L = ["ninja_required_version = 1.8",
             f"cxx = {e(self.cxx)}",
             "rule cxx",
             "  command = $cxx $flags -MMD -MF $out.d -c $in -o $out",
             "  depfile = $out.d",
             "  deps = gcc",
             "  description = CXX $in",
             "rule ar",
             "  command = rm -f $out && ar crs $out $in",
             "  description = AR $out",
             "rule link",
             "  command = $cxx $in $libs -o $out",
             "  description = LINK $out",
             ""]
        flags = " ".join(self.common_flags)
        tflags = flags + " " + " ".join(self.test_includes)
        gt = self.vendor / "googletest" / "googletest"

        def cxx(out, src, f):
            L.append(f"build {e(out)}: cxx {e(src)}")
            L.append(f"  flags = {f}")

        cxx(self.gtest_obj, self.gtest_src, f"{flags} -I{gt / 'include'} -I{gt} -Wno-error")
        cxx(self.main_obj, HARNESS / "main.cpp", tflags)
        cxx(self.shim_obj, HARNESS / "event_shim.cpp", tflags)
        cxx(self.util_obj, self.vendor / "yoga" / "tests" / "util" / "TestUtil.cpp", tflags)
        yoga = self.vendor / "yoga"
        yobjs = []
        for src in self.yoga_srcs:
            o = self.dir / "obj" / "yoga" / src.relative_to(yoga).with_suffix(".o")
            cxx(o, src, f"{flags} -I{yoga}")
            yobjs.append(o)
        L.append(f"build {e(self.yoga_lib)}: ar {' '.join(e(o) for o in yobjs)}")
        libs = "" if not barre else " ".join(native_libs())
        for key, src in keys_srcs:
            cxx(self.test_obj(key), src, tflags)
            ins = [self.test_obj(key), *self.common_objs(barre), engine_lib]
            L.append(f"build {e(self.bin(variant, key))}: link {' '.join(e(i) for i in ins)}")
            L.append(f"  libs = {libs}")
        text = "\n".join(L) + "\n"
        path = self.dir / "build.ninja"
        if not path.exists() or path.read_text() != text:
            path.write_text(text)

    def ninja(self, targets, dry=False):
        cmd = ["ninja", "-C", str(self.dir), "-k", "0", "-j", str(self.args.jobs)]
        if dry:
            cmd.append("-n")
        cmd += [str(t) for t in targets]
        return subprocess.run(cmd, capture_output=True, text=True, errors="replace")


def parse_ninja_failures(out: str):
    """{output path: error text} from a ninja run's stdout."""
    fails, cur, buf, skip_cmd = {}, None, [], False

    def flush():
        if cur:
            for o in cur:
                fails[o] = "\n".join(buf).strip()

    for line in out.splitlines():
        if line.startswith("FAILED: "):
            flush()
            rest = re.sub(r"^\[code=\d+\]\s*", "", line[len("FAILED: "):])
            cur, buf, skip_cmd = rest.split(), [], True
        elif re.match(r"^\[\d+/\d+\] ", line) or line.startswith("ninja: "):
            flush()
            cur, buf = None, []
        elif cur is not None:
            if skip_cmd:
                skip_cmd = False
                continue
            buf.append(line)
    flush()
    return fails


def build_error_reason(text: str) -> str:
    if not text:
        return "build failed"
    lines = text.splitlines()
    for ln in lines:
        if " error: " in ln or ln.startswith("error:"):
            return ln.strip()[:300]
    undef = [ln for ln in lines if "Undefined symbols" in ln or "undefined reference" in ln]
    if undef:
        syms = [ln.strip().strip('"') for ln in lines if ln.startswith('  "')]
        if syms:
            return f"link: {len(syms)} undefined symbols, e.g. {', '.join(syms[:3])}"[:300]
        return undef[0].strip()[:300]
    return lines[0].strip()[:300]


def cargo_build(args) -> Path:
    root = (args.engine_root or RUNNER_ROOT).resolve()
    if not (root / "crates" / "barre-ffi").is_dir():
        raise ConfError(f"no crates/barre-ffi in {root} (use --yoga to run the suite against real Yoga)")
    cmd = ["cargo", "build", "-p", "barre-ffi", "--message-format=json-render-diagnostics"]
    if args.release:
        cmd.append("--release")
    env = dict(os.environ)
    if args.target_dir:
        env["CARGO_TARGET_DIR"] = str(args.target_dir.resolve())
    proc = subprocess.run(cmd, cwd=root, env=env, capture_output=True, text=True, errors="replace")
    if proc.returncode != 0:
        raise ConfError("cargo build -p barre-ffi failed:\n" + proc.stderr[-4000:])
    lib = None
    for line in proc.stdout.splitlines():
        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            continue
        if msg.get("reason") != "compiler-artifact":
            continue
        if "staticlib" not in msg.get("target", {}).get("kind", []):
            continue
        for f in msg.get("filenames", []):
            if f.endswith(".a") or f.endswith(".lib"):
                lib = Path(f)
    if lib is None or not lib.exists():
        raise ConfError("cargo build -p barre-ffi produced no staticlib (crate-type must include \"staticlib\")")
    return lib


def nm_defined(path: Path):
    proc = subprocess.run(["nm", "--defined-only", str(path)], capture_output=True, text=True, errors="replace")
    names = set()
    for line in proc.stdout.splitlines():
        parts = line.split()
        if len(parts) >= 2 and not line.endswith(":"):
            names.add(parts[-1])
    return proc.returncode, names


YOGA_CODE_RE = re.compile(r"8facebook4yoga(15calculateLayout|4NodeC[12])")
RUST_RE = re.compile(r"(_ZN(3std|4core|5alloc)\d|rust_begin_unwind|__rust_alloc|rust_eh_personality|_R[NC][a-z]*\d)")


def check_barre_lib(lib: Path, cache: dict):
    key = f"lib:{lib}:{lib.stat().st_mtime_ns}:{lib.stat().st_size}"
    if key in cache:
        return cache[key]
    rc, names = nm_defined(lib)
    problems = []
    plain = {n.lstrip("_") for n in names}
    for sym in ("YGNodeCalculateLayout", "YGNodeNew", "YGConfigNew"):
        if sym not in plain:
            problems.append(f"{lib.name} does not define {sym}")
    if any(YOGA_CODE_RE.search(n) for n in names):
        problems.append(f"{lib.name} contains Yoga's layout code (facebook::yoga::calculateLayout / Node)")
    if not any(RUST_RE.search(n) for n in names):
        problems.append(f"{lib.name} contains no Rust symbols")
    cache[key] = problems
    return problems


def check_barre_binary(path: Path, cache: dict):
    st = path.stat()
    key = f"bin:{path}:{st.st_mtime_ns}:{st.st_size}"
    if key in cache:
        return cache[key]
    rc, names = nm_defined(path)
    problems = []
    if rc != 0:
        problems.append(f"nm failed on {path}")
    if any(YOGA_CODE_RE.search(n) for n in names):
        problems.append(f"{path.name} contains Yoga's layout code")
    if not any(RUST_RE.search(n) for n in names):
        problems.append(f"{path.name} contains no Rust code (not linked against barre?)")
    if not any(n.lstrip("_") == "YGNodeCalculateLayout" for n in names):
        problems.append(f"{path.name} does not define YGNodeCalculateLayout")
    cache[key] = problems
    return problems


# ---------------------------------------------------------------------------
# canaries


def rand_ident(rng, n=10):
    return rng.choice(string.ascii_letters) + "".join(rng.choice(string.ascii_letters + string.digits) for _ in range(n - 1))


def canary_source(rng, count):
    suite = "C" + rand_ident(rng, 12)
    names = [rand_ident(rng, 14) for _ in range(count)]
    lines = ["#include <gtest/gtest.h>", "#include <yoga/Yoga.h>", ""]
    for i, name in enumerate(names):
        w, h = rng.randint(60, 400), rng.randint(60, 400)
        direction = rng.choice(["YGFlexDirectionRow", "YGFlexDirectionColumn",
                                "YGFlexDirectionRowReverse", "YGFlexDirectionColumnReverse"])
        body = [f"TEST({suite}, {name}) {{",
                "  YGConfigRef config = YGConfigNew();",
                "  YGNodeRef root = YGNodeNewWithConfig(config);",
                f"  YGNodeStyleSetFlexDirection(root, {direction});",
                f"  YGNodeStyleSetWidth(root, {w});",
                f"  YGNodeStyleSetHeight(root, {h});"]
        nchild = rng.randint(1, 4)
        for c in range(nchild):
            body.append(f"  YGNodeRef c{c} = YGNodeNewWithConfig(config);")
            if rng.random() < 0.5:
                body.append(f"  YGNodeStyleSetFlexGrow(c{c}, {rng.randint(1, 3)});")
            else:
                body.append(f"  YGNodeStyleSetWidth(c{c}, {rng.randint(5, 150)});")
                body.append(f"  YGNodeStyleSetHeight(c{c}, {rng.randint(5, 150)});")
            body.append(f"  YGNodeInsertChild(root, c{c}, {c});")
        body.append("  YGNodeCalculateLayout(root, YGUndefined, YGUndefined, YGDirectionLTR);")
        wrong = round(rng.uniform(1.25, 40.0), 2)
        big = round(rng.uniform(2000.0, 9000.0), 2)
        target = f"c{rng.randrange(nchild)}"
        kind = (i + rng.randrange(4)) % 4
        body.append([
            f"  ASSERT_FLOAT_EQ({w + wrong}f, YGNodeLayoutGetWidth(root));",
            f"  ASSERT_FLOAT_EQ({h - wrong}f, YGNodeLayoutGetHeight(root));",
            f"  ASSERT_FLOAT_EQ({big}f, YGNodeLayoutGetLeft({target}));",
            f"  ASSERT_FLOAT_EQ({big}f, YGNodeLayoutGetTop({target}));",
        ][kind])
        body += ["  YGNodeFreeRecursive(root);", "  YGConfigFree(config);", "}", ""]
        lines += body
    return "\n".join(lines), [f"{suite}.{n}" for n in names]


def compile_canary_obj(build: Build, tmp: Path, rng, count):
    src_text, tests = canary_source(rng, count)
    stem = "t" + rand_ident(rng, 10)
    src = tmp / f"{stem}.cpp"
    src.write_text(src_text)
    obj = tmp / f"{stem}.o"
    cmd = [build.cxx, *build.common_flags, *build.test_includes, "-c", str(src), "-o", str(obj)]
    proc = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, errors="replace")
    return proc, obj, tests


def link_canary(build: Build, tmp: Path, obj: Path, barre: bool, engine_lib: Path):
    out = tmp / ("c" + obj.stem)
    ins = [obj, *build.common_objs(barre), engine_lib]
    cmd = [build.cxx, *map(str, ins), *(native_libs() if barre else []), "-o", str(out)]
    proc = subprocess.run(cmd, capture_output=True, text=True, errors="replace")
    return out, proc


# ---------------------------------------------------------------------------
# running


NOT_IMPL_RE = re.compile(r"not[ _]implemented!?\s*[:(]\s*\"?([^\"\n)]*)")
PANIC_RE = re.compile(r"panicked at [^\n]*?:\d+:\d+:\n([^\n]+)")


def failure_reason(status: str, rc, out: str, timeout: float) -> str:
    m = NOT_IMPL_RE.search(out)
    if m:
        return f"not implemented: {m.group(1).strip()}"
    if status == "timeout":
        return f"timeout after {timeout:g}s"
    m = PANIC_RE.search(out)
    if m:
        return f"panic: {m.group(1).strip()}"[:300]
    if status == "crash":
        if rc == MEM_EXIT_CODE or "memory limit of" in out:
            return "memory limit exceeded"
        if rc is not None and rc < 0:
            try:
                sig = signal.Signals(-rc).name
            except ValueError:
                sig = f"signal {-rc}"
            for ln in out.splitlines():
                if "Assertion failed" in ln or "terminate" in ln or "fatal" in ln.lower():
                    return f"{sig}: {ln.strip()}"[:300]
            return sig
        return f"exited with code {rc} before the test finished"
    lines = out.splitlines()
    for i, ln in enumerate(lines):
        if ln.endswith(": Failure") or ": Failure" in ln:
            where = ln.split(": Failure")[0].rsplit("/", 1)[-1]
            detail = " | ".join(x.strip() for x in lines[i + 1:i + 5] if x.strip() and not x.startswith("["))
            return f"{where}: {detail}"[:300]
    for ln in lines:
        if ln.strip():
            return ln.strip()[:300]
    return "failed"


def run_one(binary: Path, test: str, args, cwd: Path):
    nonce = "".join(random.SystemRandom().choice(string.ascii_lowercase + string.digits) for _ in range(24))
    env = {"CONF_NONCE": nonce, "RUST_BACKTRACE": "0", "PATH": "/usr/bin:/bin"}
    if args.mem_limit_mb > 0:
        env["CONF_MEM_LIMIT_MB"] = str(args.mem_limit_mb)
    cmd = [str(binary), f"--gtest_filter={test}", "--gtest_color=no", "--gtest_brief=1"]
    try:
        p = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL,
                             env=env, cwd=cwd, start_new_session=True)
    except OSError as ex:
        return "crash", f"could not start: {ex}", ""
    try:
        raw, _ = p.communicate(timeout=args.timeout)
        timed_out = False
    except subprocess.TimeoutExpired:
        try:
            os.killpg(p.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        raw, _ = p.communicate()
        timed_out = True
    out = raw.decode("utf-8", errors="replace")
    rc = p.returncode
    marker = re.search(rf"^CONF-RESULT {nonce} ran=(\d+) passed=(\d+) failed=(\d+) skipped=(\d+)$", out, re.M)
    if timed_out:
        status = "timeout"
    elif marker:
        ran, passed, failed, skipped = map(int, marker.groups())
        if ran == 1 and passed == 1 and failed == 0 and skipped == 0 and rc == 0:
            status = "pass"
        elif ran == 0:
            return "fail", "test did not run (not matched by --gtest_filter)", out[-OUTPUT_KEEP:]
        elif skipped:
            return "fail", "test skipped itself (GTEST_SKIP)", out[-OUTPUT_KEEP:]
        elif failed == 0 and rc != 0:
            status = "crash"
            return status, f"crashed after the test finished (exit {rc})", out[-OUTPUT_KEEP:]
        else:
            status = "fail"
    else:
        status = "crash"
    if status == "pass":
        return status, "", ""
    return status, failure_reason(status, rc, out, args.timeout), out[-OUTPUT_KEEP:]


def list_tests(binary: Path, cache_path: Path):
    st = binary.stat()
    stamp = f"{st.st_mtime_ns}:{st.st_size}"
    try:
        cached = json.loads(cache_path.read_text())
        if cached.get("stamp") == stamp:
            return cached["tests"], None
    except (OSError, ValueError, KeyError):
        pass
    proc = subprocess.run([str(binary), "--gtest_list_tests"], capture_output=True, text=True,
                          errors="replace", timeout=60, env={"PATH": "/usr/bin:/bin"})
    if proc.returncode != 0:
        return None, f"--gtest_list_tests failed (exit {proc.returncode}): {(proc.stdout + proc.stderr)[-300:]}"
    tests = parse_gtest_list(proc.stdout)
    cache_path.parent.mkdir(parents=True, exist_ok=True)
    cache_path.write_text(json.dumps({"stamp": stamp, "tests": tests}))
    return tests, None


# ---------------------------------------------------------------------------
# main


def read_id_list(path: Path):
    if not path.exists():
        return []
    return [ln.strip() for ln in path.read_text().splitlines() if ln.strip() and not ln.startswith("#")]


def prune_lib_dirs(build: Build, keep_variant: str, keep=8):
    root = build.dir / "bin"
    if not root.is_dir():
        return
    dirs = sorted((d for d in root.iterdir() if d.is_dir() and d.name.startswith("lib-")),
                  key=lambda d: d.stat().st_mtime, reverse=True)
    for d in dirs[keep:]:
        if d.name != keep_variant:
            shutil.rmtree(d, ignore_errors=True)


def main(argv=None):
    args = parse_args(sys.argv[1:] if argv is None else argv)
    t0 = time.monotonic()
    try:
        return run(args, t0)
    except ConfError as ex:
        print(f"conf: error: {ex}", file=sys.stderr)
        return EXIT_ERROR


def run(args, t0):
    vendor = vendor_dir()
    build_dir = (args.build_dir or (Path(os.environ["BARRE_CONF_BUILD_DIR"]) if os.environ.get("BARRE_CONF_BUILD_DIR")
                                    else vendor.parent / "build" / "conf")).resolve()
    build_dir.mkdir(parents=True, exist_ok=True)
    if not shutil.which("ninja"):
        raise ConfError("ninja not found on PATH")
    build = Build(args, vendor, build_dir)
    filters = args.filters
    skips = [] if args.no_skip else load_skips()

    # files to consider
    all_sources = discover_sources(vendor)
    selected = [(k, p) for k, p in all_sources if file_selected(k, filters)]
    if not selected:
        raise ConfError(f"no test files match {' '.join(filters)}")
    to_build = [(k, p) for k, p in selected if not file_fully_skipped(k, skips)]

    barre = not args.yoga
    lock_fd = os.open(build_dir / ".lock", os.O_RDWR | os.O_CREAT, 0o644)
    try:
        fcntl.flock(lock_fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        log(args, "conf: waiting for the build lock (another ./conf is using this build dir)")
        fcntl.flock(lock_fd, fcntl.LOCK_EX)

    tmp = Path(tempfile.mkdtemp(prefix="conf-"))
    rng = random.SystemRandom()
    try:
        canary_proc, canary_obj, canary_tests = compile_canary_obj(build, tmp, rng, args.canaries)

        # engine
        if args.yoga:
            variant, engine_lib = "yoga", build.yoga_lib
        else:
            if args.lib:
                engine_lib = args.lib.resolve()
                if not engine_lib.exists():
                    raise ConfError(f"--lib {engine_lib} does not exist")
            else:
                log(args, "conf: cargo build -p barre-ffi" + (" --release" if args.release else ""))
                engine_lib = cargo_build(args)
            default_lib = RUNNER_ROOT / "target" / ("release" if args.release else "debug") / engine_lib.name
            variant = (f"barre-{'release' if args.release else 'debug'}" if engine_lib == default_lib
                       else "lib-" + shash(str(engine_lib)))
        build.write_ninja(to_build, variant, engine_lib, barre)
        if variant.startswith("lib-"):
            prune_lib_dirs(build, variant)

        # build
        common = build.common_objs(barre) + ([build.yoga_lib] if args.yoga else [])
        bins = {k: build.bin(variant, k) for k, _ in to_build}
        log(args, f"conf: building {len(bins)} test binaries ({variant}) in {build_dir}")
        tb = time.monotonic()
        proc = build.ninja(common + list(bins.values()))
        fails = parse_ninja_failures(proc.stdout)
        if proc.returncode != 0 and "ninja: error:" in proc.stdout + proc.stderr and not fails:
            raise ConfError("ninja failed:\n" + (proc.stdout + proc.stderr)[-4000:])
        dry = build.ninja(common + list(bins.values()), dry=True)
        pending = set(re.findall(r"^\[\d+/\d+\] (?:LINK|CXX|AR) (.+)$", dry.stdout, re.M))
        for c in common:
            if str(c) in pending or str(c) in fails:
                raise ConfError(f"failed to build {c}:\n{fails.get(str(c), '')[-4000:]}")
        build_fail = {}
        for k, b in bins.items():
            if str(b) in pending or not b.exists():
                text = fails.get(str(build.test_obj(k))) or fails.get(str(b)) or ""
                for root in {str(vendor.parent), str(RUNNER_ROOT)}:
                    text = text.replace(root + "/", "")
                build_fail[k] = (build_error_reason(text), text)
        log(args, f"conf: built in {time.monotonic() - tb:.1f}s"
                  + (f", {len(build_fail)} files failed to build" if build_fail else ""))

        # canary link
        rc = canary_proc.wait()
        canary_out = canary_proc.stdout.read() if canary_proc.stdout else ""
        if rc != 0:
            raise ConfError("failed to compile the canary tests:\n" + canary_out[-3000:])
        canary_bin, lp = link_canary(build, tmp, canary_obj, barre, engine_lib)
        canary_build_error = None if lp.returncode == 0 else build_error_reason(lp.stdout + lp.stderr)

        # sanity: barre binaries contain barre, not Yoga
        invalid = []
        cache_path = build_dir / "symcheck.json"
        try:
            symcache = json.loads(cache_path.read_text())
        except (OSError, ValueError):
            symcache = {}
        if barre:
            invalid += check_barre_lib(engine_lib, symcache)
            to_check = [b for k, b in bins.items() if k not in build_fail]
            if canary_build_error is None:
                to_check.append(canary_bin)
            with cf.ThreadPoolExecutor(args.jobs) as ex:
                for probs in ex.map(lambda b: check_barre_binary(b, symcache), to_check):
                    invalid += probs
            symcache = {k: v for k, v in symcache.items() if k.startswith("lib:") or Path(k.split(":")[1]).exists()}
            cache_path.write_text(json.dumps(symcache))
        if canary_build_error:
            invalid.append(f"canary tests failed to build: {canary_build_error}")

        # discover tests
        lists_dir = build_dir / "lists"
        tests_by_file = {}
        list_errors = {}

        def discover(item):
            k, src = item
            src_cache = lists_dir / "src" / (k + ".json")
            src_hash = hashlib.sha256(src.read_bytes()).hexdigest()
            if k not in build_fail and k in bins:
                tests, err = list_tests(bins[k], lists_dir / variant / (k + ".json"))
                if tests is not None:
                    src_cache.parent.mkdir(parents=True, exist_ok=True)
                    src_cache.write_text(json.dumps({"src": src_hash, "tests": tests}))
                    return k, tests, None
            else:
                err = None
            try:
                cached = json.loads(src_cache.read_text())
                if cached.get("src") == src_hash:
                    return k, cached["tests"], err
            except (OSError, ValueError, KeyError):
                pass
            return k, tests_from_source(src), err

        with cf.ThreadPoolExecutor(args.jobs) as ex:
            for k, tests, err in ex.map(discover, selected):
                tests_by_file[k] = tests
                if err:
                    list_errors[k] = err

        # downgrade to a shared lock: other runs may run, but not rebuild, until we finish
        fcntl.flock(lock_fd, fcntl.LOCK_SH)

        results = {}  # id -> (status, reason, output)
        jobs = []
        file_of = {}
        for k, src in selected:
            for t in tests_by_file[k]:
                tid = f"{k}/{t}"
                if not id_matches(tid, filters):
                    continue
                file_of[tid] = k
                reason = skip_reason(tid, skips)
                if reason is not None:
                    results[tid] = ("skip", reason, "")
                elif k in build_fail:
                    results[tid] = ("build-fail", build_fail[k][0], build_fail[k][1][-OUTPUT_KEEP:])
                elif k in list_errors:
                    results[tid] = ("crash", list_errors[k], "")
                else:
                    jobs.append((tid, bins[k], t))

        if args.list:
            for tid in sorted(results.keys() | {j[0] for j in jobs}):
                print(tid)
            return EXIT_OK

        canary_results = {}
        if canary_build_error is None:
            jobs += [(f"canary/{t}", canary_bin, t) for t in canary_tests]
        log(args, f"conf: running {len(jobs)} tests ({args.jobs} jobs)")
        tr = time.monotonic()
        with cf.ThreadPoolExecutor(args.jobs) as ex:
            futs = {ex.submit(run_one, b, t, args, tmp): tid for tid, b, t in jobs}
            for fut in cf.as_completed(futs):
                tid = futs[fut]
                if tid.startswith("canary/"):
                    canary_results[tid] = fut.result()
                else:
                    results[tid] = fut.result()
        run_s = time.monotonic() - tr

        passed_canaries = [t for t, r in canary_results.items() if r[0] == "pass"]
        if passed_canaries:
            invalid.append(f"{len(passed_canaries)} canary tests with wrong assertions passed: "
                           + ", ".join(passed_canaries))
    finally:
        shutil.rmtree(tmp, ignore_errors=True)
        os.close(lock_fd)

    return report(args, results, file_of, invalid, canary_results, variant, time.monotonic() - t0, run_s)


def report(args, results, file_of, invalid, canary_results, variant, elapsed, run_s):
    counts = {s: 0 for s in STATUSES}
    for st, _, _ in results.values():
        counts[st] += 1
    total = len(results)
    in_scope = total - counts["skip"]
    rate = 100.0 * counts["pass"] / in_scope if in_scope else 0.0
    not_impl = {}
    for st, reason, _ in results.values():
        if reason.startswith("not implemented: "):
            name = reason[len("not implemented: "):]
            not_impl[name] = not_impl.get(name, 0) + 1

    regressions, new_passes = [], []
    baseline_path = args.baseline or (CONF_DIR / "expected-pass.txt" if args.update_expected else None)
    if args.baseline:
        base = set(read_id_list(args.baseline))
        for tid in sorted(base):
            if not id_matches(tid, args.filters):
                continue
            st = results.get(tid, ("missing",))[0]
            if st != "pass":
                regressions.append((tid, st))
        new_passes = sorted(t for t, r in results.items() if r[0] == "pass" and t not in base)

    valid = not invalid
    summary = {
        "variant": variant, "valid": valid, "invalid_reasons": invalid,
        "canaries": len(canary_results), "canaries_ok": not any(r[0] == "pass" for r in canary_results.values())
        and len(canary_results) > 0,
        "total": total, "in_scope": in_scope, **{k.replace("-", "_"): v for k, v in counts.items()},
        "pass_rate": round(rate, 2), "not_implemented": dict(sorted(not_impl.items(), key=lambda x: -x[1])),
        "regressions": [t for t, _ in regressions], "new_passes": new_passes,
        "elapsed_s": round(elapsed, 2), "run_s": round(run_s, 2),
    }

    if args.json:
        args.json.write_text(json.dumps({t: {"status": r[0], "reason": r[1]} for t, r in sorted(results.items())},
                                        indent=1) + "\n")
    if args.summary_json:
        args.summary_json.write_text(json.dumps(summary, indent=1) + "\n")

    # human output
    out = sys.stdout
    if args.show:
        shown = 0
        for tid in sorted(results):
            st, reason, text = results[tid]
            if st in ("pass", "skip"):
                continue
            if shown >= args.show:
                break
            shown += 1
            print(f"--- {st.upper()} {tid}: {reason}", file=out)
            tail = [ln for ln in text.splitlines() if ln.strip() and not ln.startswith("CONF-RESULT")]
            for ln in tail[-12:]:
                print(f"    {ln[:200]}", file=out)
    if args.by_file:
        rows = {}
        for tid, (st, _, _) in results.items():
            rows.setdefault(file_of.get(tid, tid.rsplit("/", 1)[0]), {s: 0 for s in STATUSES})[st] += 1
        w = max([len(k) for k in rows] + [4])
        hdr = f"{'file':<{w}}  {'pass':>5} {'fail':>5} {'crash':>5} {'tmout':>5} {'bfail':>5} {'skip':>5} {'total':>5}"
        print(hdr, file=out)
        for k in sorted(rows):
            r = rows[k]
            print(f"{k:<{w}}  {r['pass']:>5} {r['fail']:>5} {r['crash']:>5} {r['timeout']:>5} "
                  f"{r['build-fail']:>5} {r['skip']:>5} {sum(r.values()):>5}", file=out)
    parts = [f"{counts[s]} {s}" for s in STATUSES if counts[s] or s == "pass"]
    print(f"conf [{variant}]: {total} tests: {', '.join(parts)}; "
          f"{rate:.1f}% of {in_scope} in scope pass ({elapsed:.1f}s, tests {run_s:.1f}s)", file=out)
    if not_impl:
        top = ", ".join(f"{n} ({c})" for n, c in list(summary["not_implemented"].items())[:8])
        print(f"not implemented: {top}", file=out)
    if args.baseline:
        print(f"baseline {args.baseline}: {len(regressions)} regressions, {len(new_passes)} new passes", file=out)
        for tid, st in regressions[:30]:
            print(f"  REGRESSION {tid}: {st}", file=out)
        if len(regressions) > 30:
            print(f"  ... and {len(regressions) - 30} more", file=out)
    if not valid:
        print("conf: RUN INVALID:", file=out)
        for r in invalid:
            print(f"  {r}", file=out)
        return EXIT_INVALID

    if args.update_expected:
        old = read_id_list(baseline_path)
        keep = {t for t in old if not id_matches(t, args.filters)}
        new = sorted(keep | {t for t, r in results.items() if r[0] == "pass"})
        baseline_path.write_text("".join(t + "\n" for t in new))
        print(f"wrote {baseline_path} ({len(new)} tests)", file=out)
    return EXIT_REGRESSION if regressions else EXIT_OK


if __name__ == "__main__":
    sys.exit(main())
