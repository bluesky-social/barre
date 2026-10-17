"""Clusters conformance failures on the integration branch so the architect
(or a cheap drafting model) can turn the biggest clusters into tickets."""

import re
from collections import Counter, defaultdict

from common import WORK, git, log, load_config
from conformance import run_conf
from gates import build, read_results


def normalize(reason):
    r = re.sub(r"«[^»]*»", "«…»", reason)
    r = re.sub(r"'[^']*'|\"[^\"]*\"", "'…'", r)
    r = re.sub(r"\b0x[0-9a-f]+\b|-?\b\d+(\.\d+)?(e[+-]?\d+)?\b", "N", r)
    return r[:120]


def group(test_id):
    """The test file (or gtest suite) a test belongs to:
    generated/YGAlignItemsTest/YogaTest.align_baseline -> generated/YGAlignItemsTest."""
    if "/" in test_id:
        return test_id.rsplit("/", 1)[0]
    return test_id.split(".", 1)[0]


def main(args):
    if args.fresh:
        import merge
        cfg = load_config()
        tip = git("rev-parse", cfg.branch)
        merge.prepare(tip)
        target_dir = WORK / "merge-target"
        b = build(cfg, merge.MERGE_DIR, target_dir)
        if b.returncode != 0:
            raise SystemExit(b.stderr[-2000:])
        log("running the full suite")
        adir = WORK / "triage"
        _, s = run_conf([], merge.MERGE_DIR, adir, "full", target_dir=target_dir, cfg=cfg.conf)
        if s is None:
            raise SystemExit(f"the conformance runner produced no results; see {adir / 'full.log'}")
        (WORK / "last-full.jsonl").write_text((adir / "full.jsonl").read_text())
    results = [r for r in read_results(WORK / "last-full.jsonl") if r["status"] != "skip"]
    if not results:
        raise SystemExit("no results yet: run with --fresh, or merge something first")
    failing = [r for r in results if r["status"] != "pass"]
    print(f"{len(results) - len(failing)}/{len(results)} tests passing; {len(failing)} failing\n")

    statuses = Counter(r["status"] for r in failing)
    if statuses:
        print("failing by status: " + ", ".join(f"{k} {v}" for k, v in statuses.most_common()) + "\n")

    ni = Counter(r["not_implemented"] for r in failing if r.get("not_implemented"))
    if ni:
        print("not implemented (what the engine says is missing):")
        for what, n in ni.most_common(args.top):
            print(f"  {n:>6}  {what}")
        print()

    msgs = Counter(normalize(r["reason"]) for r in failing if not r.get("not_implemented"))
    if msgs:
        print("most common failure messages:")
        for msg, n in msgs.most_common(args.top):
            print(f"  {n:>6}  {msg}")
        print()

    files = defaultdict(lambda: [0, 0])
    for r in results:
        key = group(r["id"])
        files[key][1] += 1
        if r["status"] == "pass":
            files[key][0] += 1
    print("test files with the most failing tests:")
    worst = sorted(files.items(), key=lambda kv: kv[1][0] - kv[1][1])[: args.top]
    for d, (p, t) in worst:
        print(f"  {t - p:>6} failing  {p:>5}/{t:<5}  {d}")
