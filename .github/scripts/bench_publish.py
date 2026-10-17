#!/usr/bin/env python3
"""Add a bench/run result to the bench-results branch's history.

Usage: bench_publish.py RESULT_JSON RESULT_MD BRANCH_DIR --sha SHA --repo OWNER/NAME

Copies the run into BRANCH_DIR/results/ (named by date and barre commit) and
rewrites BRANCH_DIR/README.md: the latest table, then one line per run with
its geometric mean barre/Yoga ratio, newest first.
"""
import argparse
import datetime
import json
import math
import shutil
from pathlib import Path


def geomean(path):
    data = json.loads(Path(path).read_text())
    ratios = []
    for r in data.get("results", {}).values():
        y, b = r.get("yoga", {}), r.get("barre", {})
        if "error" in y or "error" in b or not y.get("median") or not b.get("median"):
            continue
        ratios.append(b["median"] / y["median"])
    return (math.exp(sum(map(math.log, ratios)) / len(ratios)), len(ratios)) if ratios else (None, 0)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("json")
    ap.add_argument("md")
    ap.add_argument("branch_dir")
    ap.add_argument("--sha", required=True)
    ap.add_argument("--repo", required=True)
    args = ap.parse_args()

    out = Path(args.branch_dir) / "results"
    out.mkdir(parents=True, exist_ok=True)
    stamp = f"{datetime.datetime.now(datetime.UTC):%Y%m%d-%H%M%S}-{args.sha[:10]}"
    shutil.copy(args.json, out / f"{stamp}.json")
    shutil.copy(args.md, out / f"{stamp}.md")

    runs = []
    for j in sorted(out.glob("*.json"), reverse=True):
        g, n = geomean(j)
        date, time_, sha = j.stem.split("-", 2)
        runs.append(f"| {date[:4]}-{date[4:6]}-{date[6:]} {time_[:2]}:{time_[2:4]} UTC "
                    f"| [`{sha}`](https://github.com/{args.repo}/commit/{sha}) "
                    f"| {f'{g:.3f}×' if g else '–'} | {n} | [table](results/{j.stem}.md) · [json](results/{j.name}) |")
    latest = Path(args.md).read_text().strip()
    readme = "\n".join([
        "# barre benchmark history",
        "",
        f"Written by `.github/workflows/bench.yml` on every push to `main` of [{args.repo}](https://github.com/{args.repo}).",
        "barre against Yoga through the same C API on a GitHub-hosted macOS arm64 runner (`bench/run`, docs/BENCH.md).",
        "Shared runners are noisy: compare trends, not single runs.",
        "",
        "| run | barre commit | geomean barre/Yoga (lower is faster) | workloads | |",
        "|---|---|--:|--:|---|",
        *runs,
        "",
        "## Latest run",
        "",
        latest,
        "",
    ])
    (Path(args.branch_dir) / "README.md").write_text(readme)
    print(runs[0])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
