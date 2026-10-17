"""Mechanical gates: cheap, deterministic checks that run before any model
looks at an attempt. Everything is recomputed here from the commit itself."""

import json
import os
from pathlib import Path

from common import ROOT, WORK, run, tail, write_json
from conformance import BASELINE, run_conf, runner_error
from policy import PROTECTED, lint, matches

# Submodules the engine must never see modified.
VENDOR = ("vendor/yoga", "vendor/googletest")
DEFAULT_BUILD = ["build", "-q", "--workspace"]
DEFAULT_CLIPPY = ["clippy", "-q", "--workspace", "--all-targets", "--", "-D", "warnings"]


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
    for sub in VENDOR:
        if not (ROOT / sub).exists():
            continue
        out = run(["git", "status", "--porcelain"], cwd=ROOT / sub, check=False).stdout.strip()
        if out:
            dirty.append(f"{sub}: {out.splitlines()[0]}")
    status = run(["git", "submodule", "status"], cwd=ROOT, check=False).stdout
    dirty += [l.strip() for l in status.splitlines() if l.startswith("+")]
    return dirty


def cargo(args, cwd, target_dir):
    env = dict(os.environ, CARGO_TARGET_DIR=str(target_dir))
    return run(["cargo", *args], cwd=cwd, env=env, check=False)


def build(cfg, cwd, target_dir):
    """The gate's build and clippy commands ([gate] in orch.toml)."""
    return cargo(list(cfg.gate.get("build", DEFAULT_BUILD)), cwd, target_dir)


def clippy(cfg, cwd, target_dir):
    return cargo(list(cfg.gate.get("clippy", DEFAULT_CLIPPY)), cwd, target_dir)


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
    rejects, flags = lint(diff, allow=getattr(ticket, "allow", ()))
    rep["lint_reject"], rep["lint_flag"] = rejects, flags
    for r in rejects:
        rep["reject"].append(f"lint: {r['rule']} at {r['file']}:{r['line']}: {r['text']}")
    tamper = vendor_tampered()
    if tamper:
        rep["reject"].append(f"vendor checkout modified: {'; '.join(tamper)}")
    if rep["reject"]:
        return finish(rep, adir)

    target_dir = WORK / "gate-target" / f"slot-{slot_index}"
    b = build(cfg, cwd, target_dir)
    rep["build"] = "ok" if b.returncode == 0 else tail(b.stderr, 3000)
    if b.returncode != 0:
        rep["reject"].append("build failed")
        return finish(rep, adir)
    c = clippy(cfg, cwd, target_dir)
    rep["clippy"] = "ok" if c.returncode == 0 else tail(c.stderr, 3000)
    if c.returncode != 0:
        rep["reject"].append("clippy failed")
        return finish(rep, adir)

    rep["checks"] = []
    for cmd in ticket.check:
        # Checks run in the worker's tree against the gate's build.
        env = dict(os.environ, CARGO_TARGET_DIR=str(target_dir), BARRE_TARGET_DIR=str(target_dir),
                   BARRE_VENDOR=str(ROOT / "vendor"))
        proc = run(["sh", "-c", cmd], cwd=cwd, env=env, check=False, timeout=600)
        rep["checks"].append({"cmd": cmd, "exit": proc.returncode, "output": tail(proc.stdout + proc.stderr, 1500)})
        if proc.returncode != 0:
            rep["reject"].append(f"check failed: {cmd}")

    if ticket.target:
        proc, s = run_conf(ticket.target, cwd, adir, "target", target_dir=target_dir,
                           baseline=ROOT / BASELINE, cfg=cfg.conf)
        if s is None:
            rep["reject"].append("the conformance runner produced no results: " + runner_error(proc)[-600:])
            return finish(rep, adir)
        from worker import target_pass_count
        before = target_pass_count(ticket.target)
        rep["conf"] = {
            "runs": s["runs"], "before": before, "after": s["pass"], "new_passes": len(s["new_passes"]),
            "new_sample": s["new_passes"][:15],
            "regressions": s["regressions"], "fail": s["fail"], "timeout": s["timeout"], "crash": s["crash"],
            "build_fail": s["build_fail"], "not_implemented": s["not_implemented"], "canaries_ok": s["canaries_ok"],
        }
        failing = [r for r in read_results(adir / "target.jsonl") if r["status"] not in ("pass", "skip")]
        rep["conf"]["failing_sample"] = [f"{r['id']}  [{r['status']}] {r['reason'][:160]}" for r in failing[:15]]
        if not s["canaries_ok"]:
            rep["reject"].append("conformance run invalid (canaries failed): the build reports success for a "
                                 "layout that must fail")
        if s["build_fail"] and not s["pass"]:
            rep["reject"].append("the conformance suite does not build against this change")
        if s["regressions"]:
            rep["reject"].append(f"{len(s['regressions'])} conformance regressions in target: "
                                 + ", ".join(s["regressions"][:10]))
        if not s["new_passes"] and not ticket.check:
            rep["reject"].append("no new conformance passes in target")

    # A diff can look clean against its own narrow target and still break
    # tests elsewhere (T-0004's cache port regressed two YGStaleFlexBasisTest
    # cases outside its target, unnoticed through 80+ accepted attempts until
    # the merge bot's full run caught it, every time, at far higher cost than
    # this). The full suite is a couple of seconds on a warm build
    # (docs/CONFORMANCE.md); run it here so a cross-target regression is a
    # gate rejection, not a wasted verifier call and a merge refusal.
    if cfg.gate.get("full_check", True):
        full_proc, full_s = run_conf([], cwd, adir, "gate-full", target_dir=target_dir,
                                     baseline=ROOT / BASELINE, cfg=cfg.conf)
        if full_s is None:
            rep["reject"].append("the full-suite regression check produced no results: "
                                 + runner_error(full_proc)[-600:])
        elif not full_s["canaries_ok"]:
            rep["reject"].append("full-suite regression check invalid (canaries failed)")
        else:
            rep["conf_full"] = {"runs": full_s["runs"], "regressions": full_s["regressions"]}
            if full_s["regressions"]:
                rep["reject"].append(f"{len(full_s['regressions'])} regressions in the full suite "
                                     "(outside this ticket's own target): "
                                     + ", ".join(full_s["regressions"][:10]))
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
    t = rep.get("conf")
    if t:
        lines.append(
            f"conformance target: {t['runs']} tests; passing before {t['before']}, after {t['after']} "
            f"(+{t['new_passes']} new, {len(t['regressions'])} regressions, {t.get('fail', 0)} failing, "
            f"{t['timeout']} timeouts, {t['crash']} crashes, {t['not_implemented']} not-implemented)"
        )
        if t.get("new_sample"):
            lines.append("  newly passing (sample):")
            lines += [f"    {x}" for x in t["new_sample"]]
        if t["failing_sample"]:
            lines.append("  still failing (sample):")
            lines += [f"    {x}" for x in t["failing_sample"]]
    cf = rep.get("conf_full")
    if cf:
        lines.append(f"full suite (regression check only): {cf['runs']} tests; {len(cf['regressions'])} regressions")
    for c in rep.get("checks", []):
        lines.append(f"check `{c['cmd']}`: exit {c['exit']}")
    flags = rep.get("lint_flag", [])
    lines.append(f"lint flags: {len(flags)}")
    for f in flags[:30]:
        lines.append(f"  {f['file']}:{f['line']}  [{f['rule']}]  {f['text']}")
    if rep.get("reject"):
        lines.append("REJECTED: " + "; ".join(rep["reject"]))
    return "\n".join(lines)
