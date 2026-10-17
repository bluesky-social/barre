"""The conformance runner (`./conf`, docs/CONFORMANCE.md) as the gates and the
merge bot use it.

Trust: the runner always runs from the main checkout (ROOT), so the runner
code, conformance/skip.txt and vendor/ (Yoga's gtest suite) are never the
worker's copies. Only the engine source comes from the tree under test
(`--engine-root`), and the gates have already refused any diff that touches
a protected path (harness/, conformance/, vendor/, Cargo manifests, build.rs).

The runner reports a status per test; everything else (new passes,
regressions against the ratchet, counts) is computed here, so the gate
report, the merge bot, triage and `orch status` share one notion of it.
Results are normalised to JSON lines `{id, status, reason, not_implemented}`.
"""

import json
import re
from pathlib import Path

from common import ROOT, run, write_json

RUNNER = ROOT / "conf"
BASELINE = "conformance/expected-pass.txt"
INVALID_EXIT = 3  # canary failure or an otherwise invalid run
PASS = "pass"
# Statuses that are not a verdict on the engine.
NEUTRAL = {"skip"}

# The runner's reason for a not_implemented! panic is `not implemented: <name>`
# (the whole rest of the line is the name).
NOT_IMPLEMENTED_RX = re.compile(r"not implemented: ?(.+?)\s*$", re.M)


def read_ids(path):
    """The test ids in a baseline file (one per line, `#` comments)."""
    try:
        lines = Path(path).read_text().splitlines()
    except FileNotFoundError:
        return set()
    return {l.strip() for l in lines if l.strip() and not l.lstrip().startswith("#")}


def write_baseline(path, ids):
    """Rewrites a baseline file to `ids`, keeping its leading comment block."""
    path = Path(path)
    header = []
    if path.exists():
        for line in path.read_text().splitlines():
            if line.startswith("#") or not line.strip():
                header.append(line)
            else:
                break
    path.write_text("\n".join(header + sorted(ids)) + "\n")


def in_scope(test_id, filters):
    return not filters or any(test_id.startswith(f) for f in filters)


def normalise(raw):
    """The runner's --json output as a list of {id, status, reason, not_implemented}.
    Accepts {id: status}, {id: {status, reason...}}, a list of records, or
    {"results": ...} around either."""
    if isinstance(raw, dict) and isinstance(raw.get("results"), (dict, list)):
        raw = raw["results"]
    items = raw.items() if isinstance(raw, dict) else ((r.get("id"), r) for r in raw or [])
    out = []
    for tid, v in items:
        if isinstance(v, str):
            status, reason = v, ""
        else:
            v = v or {}
            status = str(v.get("status", "fail"))
            reason = str(v.get("reason") or v.get("message") or v.get("error") or "")
        ni = v.get("not_implemented") if isinstance(v, dict) else None
        if not ni:
            m = NOT_IMPLEMENTED_RX.search(reason)
            ni = m.group(1) if m else None
        out.append({"id": tid, "status": status, "reason": reason or status, "not_implemented": ni})
    out.sort(key=lambda r: r["id"])
    return out


def summarise(results, baseline_ids, filters, valid):
    status = {r["id"]: r["status"] for r in results}
    passing = {t for t, s in status.items() if s == PASS}
    expected = {t for t in baseline_ids if in_scope(t, filters)}
    counted = [s for s in status.values() if s not in NEUTRAL]
    return {
        "runs": len(counted),
        "pass": len(passing),
        "new_passes": sorted(passing - baseline_ids),
        # A baseline test that ran and did not pass. One that no longer runs at
        # all (skipped, renamed upstream) is not the change's fault; it drops
        # out of the baseline on the next rewrite (`vanished`).
        "regressions": sorted(t for t in expected if t in status and status[t] != PASS
                              and status[t] not in NEUTRAL),
        "vanished": sorted(t for t in expected if t not in status or status[t] in NEUTRAL),
        "fail": sum(1 for s in counted if s == "fail"),
        "timeout": sum(1 for s in counted if s == "timeout"),
        "crash": sum(1 for s in counted if s == "crash"),
        "build_fail": sum(1 for s in counted if s == "build-fail"),
        "not_implemented": sum(1 for r in results if r["status"] != PASS and r["not_implemented"]),
        "canaries_ok": valid,
    }


def run_conf(filters, engine_root, adir, name, *, target_dir, baseline=None, cfg=None):
    """Runs the trusted runner (from ROOT) over `filters` (id prefixes; none =
    the whole suite), building the engine from `engine_root` into `target_dir`.
    Writes adir/{name}.json (raw), {name}.jsonl (normalised) and
    {name}-summary.json. Returns (proc, summary); summary is None if the
    runner produced no results."""
    cfg = cfg or {}
    adir = Path(adir)
    adir.mkdir(parents=True, exist_ok=True)
    raw_path = adir / f"{name}.json"
    raw_path.unlink(missing_ok=True)
    cmd = [str(RUNNER), *filters, "--json", str(raw_path),
           "--engine-root", str(engine_root), "--target-dir", str(target_dir)]
    if baseline:
        cmd += ["--baseline", str(baseline)]
    if cfg.get("timeout"):
        cmd += ["--timeout", str(cfg["timeout"])]
    if cfg.get("jobs"):
        cmd += ["--jobs", str(cfg["jobs"])]
    cmd += [str(a) for a in cfg.get("extra_args", [])]
    proc = run(cmd, cwd=ROOT, check=False, timeout=int(cfg.get("run_timeout", 3600)))
    (adir / f"{name}.log").write_text((proc.stdout or "")[-20000:] + (proc.stderr or "")[-20000:])
    try:
        raw = json.loads(raw_path.read_text())
    except (FileNotFoundError, json.JSONDecodeError):
        return proc, None
    results = normalise(raw)
    with open(adir / f"{name}.jsonl", "w") as f:
        for r in results:
            f.write(json.dumps(r) + "\n")
    summary = summarise(results, read_ids(baseline) if baseline else set(), filters,
                        valid=proc.returncode != INVALID_EXIT)
    summary["exit"] = proc.returncode
    write_json(adir / f"{name}-summary.json", summary)
    return proc, summary


def runner_error(proc):
    """The tail of a runner run's output, for a report."""
    text = (proc.stdout or "") + (proc.stderr or "")
    return text[-2000:].strip() or f"exit {proc.returncode}"
