#!/usr/bin/env python3
"""Turn one `./conf` run into the results directory CI uploads.

Usage:
  conf_results.py OUT_DIR --json RESULTS_JSON --summary SUMMARY_JSON
  conf_results.py OUT_DIR --build-failed

OUT_DIR gets what the stacker reads from the `conf-results` artifact
(harness/orch/stack.py, docs/STACKS.md), and the README job too:

  results.jsonl      one {id, status, reason, not_implemented} per test, as
                     harness/orch/conformance.py normalises `./conf --json`
  summary.json       ./conf's --summary-json plus `runs` (tests in scope),
                     `build_failed`; `valid`, `regressions` and `new_passes`
                     decide the layer
  expected-pass.txt  written by `./conf --update-expected` beforehand (the
                     pass set the layer above must keep); left alone here

--build-failed: barre-ffi did not compile. That is the change's fault, so
the summary is valid with no results, and CI fails (the stacker reads a
failed check with a valid summary as red, not as a CI problem).
"""
import argparse
import json
import re
import sys
from pathlib import Path

NOT_IMPLEMENTED = re.compile(r"not implemented: ?(.+?)\s*$", re.M)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("out")
    ap.add_argument("--json")
    ap.add_argument("--summary")
    ap.add_argument("--build-failed", action="store_true")
    args = ap.parse_args()
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)

    if args.build_failed:
        summary = {"valid": True, "build_failed": True, "pass": 0, "runs": 0,
                   "regressions": [], "new_passes": []}
        (out / "summary.json").write_text(json.dumps(summary, indent=1) + "\n")
        (out / "results.jsonl").write_text("")
        print("barre-ffi failed to build", file=sys.stderr)
        return 1

    if not (args.json and args.summary and Path(args.json).exists() and Path(args.summary).exists()):
        # ./conf stopped before running anything (exit 2): no result at all,
        # which the stacker treats as CI's problem and re-runs once.
        print("no ./conf results to collect", file=sys.stderr)
        return 1

    raw = json.loads(Path(args.json).read_text())
    lines = []
    for tid in sorted(raw):
        v = raw[tid]
        reason = v.get("reason") or ""
        m = NOT_IMPLEMENTED.search(reason)
        lines.append(json.dumps({"id": tid, "status": v["status"], "reason": reason or v["status"],
                                 "not_implemented": m.group(1) if m else None}))
    (out / "results.jsonl").write_text("\n".join(lines) + ("\n" if lines else ""))

    summary = json.loads(Path(args.summary).read_text())
    summary["runs"] = summary.get("in_scope", 0)
    summary["build_failed"] = False
    (out / "summary.json").write_text(json.dumps(summary, indent=1) + "\n")
    print(f"{summary['pass']}/{summary['runs']} pass, valid={summary['valid']}, "
          f"{len(summary.get('regressions', []))} regressions, {len(summary.get('new_passes', []))} new passes")
    return 0


if __name__ == "__main__":
    sys.exit(main())
