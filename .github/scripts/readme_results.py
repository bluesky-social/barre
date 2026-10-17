#!/usr/bin/env python3
"""Update the rows of the README's results table from CI results.

Usage: readme_results.py README_MD [--barre DIR] [--yoga DIR]

DIR is a results directory from .github/scripts/conf_results.py
(results.jsonl + summary.json): --barre from `./conf`, --yoga from
`./conf --yoga`. The table lives between <!-- results:start --> and
<!-- results:end -->, one row per key; this rewrites the rows it has data
for and leaves the others alone:

  yoga-suite  Yoga's own tests in scope, passing on barre      (--barre)
  barre       barre's own tests (conformance/tests/)            (--barre)
  yoga-bugs   tests real Yoga fails by design (xfail on --yoga)
              that barre passes                                 (--barre and --yoga)

Exits 2 without touching the README when nothing may be published (the
canaries say a run was invalid, or barre did not build).
"""
import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

START, END = "<!-- results:start -->", "<!-- results:end -->"
HEADER = ["| Suite | Passing | Pass rate | |", "|---|--:|--:|---|"]
ORDER = ["yoga-suite", "barre", "yoga-bugs"]
ROOT = Path(__file__).resolve().parents[2]


def load(d):
    d = Path(d)
    summary = json.loads((d / "summary.json").read_text())
    results = [json.loads(l) for l in (d / "results.jsonl").read_text().splitlines() if l.strip()]
    return summary, results


def publishable(summary):
    return summary.get("valid") and not summary.get("build_failed")


def yoga_pin():
    # The gitlink, so this works without the submodule checked out.
    r = subprocess.run(["git", "-C", str(ROOT), "rev-parse", "HEAD:vendor/yoga"],
                       capture_output=True, text=True)
    return r.stdout.strip() if r.returncode == 0 else None


def row(key, label, passed, total, note):
    pct = 100.0 * passed / total if total else 0.0
    return (f"| {label} <!-- {key} --> | {passed:,} / {total:,} | {pct:.2f}% | "
            f"{note.replace('|', '/')} |")


def barre_rows(results):
    pin = yoga_pin()
    yoga = [r for r in results if not r["id"].startswith("barre/")]
    ours = [r for r in results if r["id"].startswith("barre/")]
    in_scope = [r for r in yoga if r["status"] != "skip"]
    skipped = len(yoga) - len(in_scope)
    files = len({r["id"].split("/")[0] if not r["id"].startswith("generated/") else "/".join(r["id"].split("/")[:2])
                 for r in in_scope})
    link = (f"https://github.com/react/yoga/tree/{pin}/tests" if pin else "https://github.com/react/yoga/tree/main/tests")
    rows = {
        "yoga-suite": row(
            "yoga-suite", f"[Yoga's test suite]({link})",
            sum(r["status"] == "pass" for r in in_scope), len(in_scope),
            f"Yoga {pin[:8] if pin else '(pinned)'}'s gtest files, unmodified, linked against barre's C API; "
            f"{len(in_scope):,} tests in {files} files; {skipped} more need Yoga's C++ internals "
            "([skip.txt](conformance/skip.txt))"),
    }
    ours_scope = [r for r in ours if r["status"] != "skip"]
    rows["barre"] = row(
        "barre", "[barre's own tests](conformance/tests)",
        sum(r["status"] == "pass" for r in ours_scope), len(ours_scope),
        "the internals-only Yoga tests ported to the C API, and regression tests for Yoga bugs")
    return rows


def bugs_row(barre_results, yoga_summary, yoga_results):
    xfail = [r["id"] for r in yoga_results if r["status"] == "xfail"]
    status = {r["id"]: r["status"] for r in barre_results}
    fixed = sum(status.get(t) == "pass" for t in xfail)
    files = len({t.split("/")[1] for t in xfail if t.startswith("barre/")})
    yoga_pass = yoga_summary.get("pass", 0)
    return row(
        "yoga-bugs", "[Yoga bugs fixed](conformance/divergences.md)", fixed, len(xfail),
        f"tests that real Yoga fails by design, in {files} files; the same runner linked against "
        f"Yoga passes everything else in scope ({yoga_pass:,} tests)")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("readme")
    ap.add_argument("--barre")
    ap.add_argument("--yoga")
    args = ap.parse_args()

    new = {}
    barre = yoga = None
    if args.barre and (Path(args.barre) / "summary.json").exists():
        s, r = load(args.barre)
        if publishable(s):
            barre = r
            new.update(barre_rows(r))
        else:
            print("barre run invalid or did not build: not publishing its rows", file=sys.stderr)
    if args.yoga and (Path(args.yoga) / "summary.json").exists():
        s, r = load(args.yoga)
        if publishable(s):
            yoga = (s, r)
        else:
            print("Yoga run invalid: not publishing the Yoga bugs row", file=sys.stderr)
    if barre is not None and yoga is not None:
        new["yoga-bugs"] = bugs_row(barre, *yoga)
    if not new:
        return 2

    path = Path(args.readme)
    text = path.read_text()
    pattern = re.compile(re.escape(START) + r"(.*?)" + re.escape(END), re.S)
    m = pattern.search(text)
    if not m:
        print(f"{path}: results markers not found", file=sys.stderr)
        return 1
    rows = {}
    for line in m.group(1).splitlines():
        key = re.search(r"<!-- ([a-z0-9-]+) -->", line)
        if key and line.startswith("|"):
            rows[key.group(1)] = line
    rows.update(new)
    ordered = [rows[k] for k in ORDER if k in rows] + [rows[k] for k in sorted(rows) if k not in ORDER]
    table = "\n".join(HEADER + ordered)
    path.write_text(text[:m.start()] + f"{START}\n\n{table}\n\n{END}" + text[m.end():])
    for line in new.values():
        print(line)
    return 0


if __name__ == "__main__":
    sys.exit(main())
