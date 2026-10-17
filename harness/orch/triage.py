"""Clusters test262 failures on the integration branch so the architect (or
a cheap drafting model) can turn the biggest clusters into tickets."""

import re
from collections import Counter, defaultdict

from common import WORK, git, log, load_config
from gates import cargo, read_results, run_t262


def normalize(reason):
    r = re.sub(r"«[^»]*»", "«…»", reason)
    r = re.sub(r"'[^']*'|\"[^\"]*\"", "'…'", r)
    r = re.sub(r"\b0x[0-9a-f]+\b|\b\d+(\.\d+)?\b", "N", r)
    return r[:120]


def main(args):
    if args.fresh:
        import merge
        cfg = load_config()
        tip = git("rev-parse", cfg.branch)
        merge.prepare(tip)
        target_dir = WORK / "merge-target"
        build = cargo(["build", "-q", "-p", "kahvi-cli"], merge.MERGE_DIR, target_dir)
        if build.returncode != 0:
            raise SystemExit(build.stderr[-2000:])
        log("running the full suite")
        adir = WORK / "triage"
        adir.mkdir(parents=True, exist_ok=True)
        run_t262([], target_dir / "debug" / "kahvi", adir, "full")
        (WORK / "last-full.jsonl").write_text((adir / "full.jsonl").read_text())
    results = read_results(WORK / "last-full.jsonl")
    if not results:
        raise SystemExit("no results yet: run with --fresh, or merge something first")
    failing = [r for r in results if r["status"] != "pass"]
    print(f"{len(results) - len(failing)}/{len(results)} runs passing; {len(failing)} failing\n")

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

    dirs = defaultdict(lambda: [0, 0])
    for r in results:
        key = "/".join(r["id"].split("/")[:3])
        dirs[key][1] += 1
        if r["status"] == "pass":
            dirs[key][0] += 1
    print("directories with the most failing runs:")
    worst = sorted(dirs.items(), key=lambda kv: kv[1][0] - kv[1][1])[: args.top]
    for d, (p, t) in worst:
        print(f"  {t - p:>6} failing  {p:>5}/{t:<5}  {d}")
