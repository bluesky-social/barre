"""Mechanical gates: cheap, deterministic checks that run before any model
looks at an attempt. Everything is recomputed here from the commit itself."""

import json
import os
from pathlib import Path

from common import ROOT, WORK, run, tail, write_json
from policy import PROTECTED, lint, matches

T262 = WORK / "bin" / "t262"


def changed_files(cwd, base, head):
    out = run(["git", "diff", "--numstat", base, head], cwd=cwd).stdout
    files = []
    for line in out.splitlines():
        added, removed, path = line.split("\t", 2)
        files.append({"path": path, "added": int(added) if added != "-" else 0,
                      "removed": int(removed) if removed != "-" else 0})
    return files


def vendor_tampered():
    dirty = []
    for sub in ("vendor/test262", "vendor/ecma262"):
        out = run(["git", "status", "--porcelain"], cwd=ROOT / sub, check=False).stdout.strip()
        if out:
            dirty.append(f"{sub}: {out.splitlines()[0]}")
    status = run(["git", "submodule", "status"], cwd=ROOT, check=False).stdout
    dirty += [l.strip() for l in status.splitlines() if l.startswith("+")]
    return dirty


def cargo(args, cwd, target_dir):
    env = dict(os.environ, CARGO_TARGET_DIR=str(target_dir))
    return run(["cargo", *args], cwd=cwd, env=env, check=False)


def run_t262(filters, engine, adir, name, baseline=None, write_baseline=None, extra=()):
    """Runs the trusted runner build (work/bin/t262) from the main checkout."""
    cmd = [str(T262), "run", *filters, "--engine", str(engine), "--quiet", "--show", "0",
           "--json", str(adir / f"{name}.jsonl"), "--summary-json", str(adir / f"{name}-summary.json"), *extra]
    if baseline:
        cmd += ["--baseline", str(baseline)]
    if write_baseline:
        cmd += ["--write-baseline", str(write_baseline)]
    proc = run(cmd, cwd=ROOT, check=False)
    summary = json.loads((adir / f"{name}-summary.json").read_text()) if (adir / f"{name}-summary.json").exists() else None
    return proc, summary


def read_results(path):
    out = []
    try:
        for line in Path(path).read_text().splitlines():
            out.append(json.loads(line))
    except FileNotFoundError:
        pass
    return out


def check_attempt(cfg, ticket, slot_index, cwd, base, head, adir):
    """Returns the gate report dict (also written to adir/gate.json)."""
    adir = Path(adir)
    rep = {"ticket": ticket.id, "base": base, "head": head, "reject": [], "flags": []}
    files = changed_files(cwd, base, head)
    rep["files"] = files
    diff = run(["git", "diff", base, head], cwd=cwd).stdout

    out_of_scope = [f["path"] for f in files if not matches(f["path"], ticket.scope)]
    protected = [f["path"] for f in files if matches(f["path"], PROTECTED)]
    if out_of_scope:
        rep["reject"].append(f"files outside scope: {', '.join(out_of_scope)}")
    if protected:
        rep["reject"].append(f"protected files changed: {', '.join(protected)}")
    rejects, flags = lint(diff)
    rep["lint_reject"], rep["lint_flag"] = rejects, flags
    for r in rejects:
        rep["reject"].append(f"lint: {r['rule']} at {r['file']}:{r['line']}: {r['text']}")
    tamper = vendor_tampered()
    if tamper:
        rep["reject"].append(f"vendor checkout modified: {'; '.join(tamper)}")
    if rep["reject"]:
        return finish(rep, adir)

    target_dir = WORK / "gate-target" / f"slot-{slot_index}"
    build = cargo(["build", "-q", "-p", "kahvi-cli"], cwd, target_dir)
    rep["build"] = "ok" if build.returncode == 0 else tail(build.stderr, 3000)
    if build.returncode != 0:
        rep["reject"].append("build failed")
        return finish(rep, adir)
    clippy = cargo(["clippy", "-q", "--workspace", "--all-targets", "--", "-D", "warnings"], cwd, target_dir)
    rep["clippy"] = "ok" if clippy.returncode == 0 else tail(clippy.stderr, 3000)
    if clippy.returncode != 0:
        rep["reject"].append("clippy failed")
        return finish(rep, adir)

    engine = target_dir / "debug" / "kahvi"
    rep["checks"] = []
    for c in ticket.check:
        env = dict(os.environ, KAHVI_BIN=str(engine))
        proc = run(["sh", "-c", c], cwd=cwd, env=env, check=False, timeout=300)
        rep["checks"].append({"cmd": c, "exit": proc.returncode, "output": tail(proc.stdout + proc.stderr, 1500)})
        if proc.returncode != 0:
            rep["reject"].append(f"check failed: {c}")

    if ticket.target:
        baseline = ROOT / "conformance" / "expected-pass.txt"
        _, s = run_t262(ticket.target, engine, adir, "target", baseline=baseline)
        if s is None:
            rep["reject"].append("test262 runner produced no summary")
            return finish(rep, adir)
        from worker import target_pass_count
        before = target_pass_count(ticket.target)
        rep["t262"] = {
            "runs": s["runs"], "before": before, "after": s["pass"], "new_passes": len(s["new_passes"]),
            "regressions": s["regressions"], "timeout": s["timeout"], "crash": s["crash"],
            "not_implemented": s["not_implemented"], "canaries_ok": s["canaries_ok"],
        }
        failing = [r for r in read_results(adir / "target.jsonl") if r["status"] != "pass"]
        rep["t262"]["failing_sample"] = [f"{r['id']}  {r['reason'][:160]}" for r in failing[:15]]
        if not s["canaries_ok"]:
            rep["reject"].append("canaries failed: the engine reports success for code that throws")
        if s["regressions"]:
            rep["reject"].append(f"{len(s['regressions'])} test262 regressions in target")
        if not s["new_passes"] and not ticket.check:
            rep["reject"].append("no new test262 passes in target")
    return finish(rep, adir)


def finish(rep, adir):
    rep["decision"] = "reject" if rep["reject"] else "pass"
    write_json(Path(adir) / "gate.json", rep)
    return rep


def render(rep):
    """Human/verifier-readable gate report. Contains numbers, never the worker's words."""
    lines = []
    files = rep.get("files", [])
    added = sum(f["added"] for f in files)
    removed = sum(f["removed"] for f in files)
    lines.append(f"files changed: {len(files)} (+{added} -{removed})")
    for f in files:
        lines.append(f"  {f['path']} (+{f['added']} -{f['removed']})")
    lines.append(f"build: {rep.get('build', 'not run')}")
    lines.append(f"clippy: {rep.get('clippy', 'not run')}")
    t = rep.get("t262")
    if t:
        lines.append(
            f"test262 target: {t['runs']} runs; passing before {t['before']}, after {t['after']} "
            f"(+{t['new_passes']} new, {len(t['regressions'])} regressions, "
            f"{t['timeout']} timeouts, {t['crash']} crashes, {t['not_implemented']} not-implemented)"
        )
        if t["failing_sample"]:
            lines.append("  still failing (sample):")
            lines += [f"    {x}" for x in t["failing_sample"]]
    for c in rep.get("checks", []):
        lines.append(f"check `{c['cmd']}`: exit {c['exit']}")
    flags = rep.get("lint_flag", [])
    lines.append(f"lint flags: {len(flags)}")
    for f in flags[:30]:
        lines.append(f"  {f['file']}:{f['line']}  [{f['rule']}]  {f['text']}")
    if rep.get("reject"):
        lines.append("REJECTED: " + "; ".join(rep["reject"]))
    return "\n".join(lines)
