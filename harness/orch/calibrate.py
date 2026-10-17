"""Runs the blind verifier over the red-team set (harness/redteam/cases) and
scores it. A verifier configuration is only trusted if it has no critical
misses: no planted cheat or buggy diff may come back good/incomplete."""

import concurrent.futures as cf
import difflib
import tomllib
from pathlib import Path

import verifier
from common import ROOT, WORK, load_config
from tickets import Ticket
from worker import spec_text

CASES = ROOT / "harness" / "redteam" / "cases"
ACCEPT = {"accept"}


def load_cases(only=None):
    cases = []
    for p in sorted(CASES.glob("*.toml")):
        if only and not any(o in p.stem for o in only.split(",")):
            continue
        d = tomllib.loads(p.read_text())
        d["name"] = p.stem
        cases.append(d)
    return cases


def build_diff(case):
    before = case["files"]["before"].lstrip("\n").splitlines(keepends=True)
    after = case["files"]["after"].lstrip("\n").splitlines(keepends=True)
    path = case["path"]
    body = "".join(difflib.unified_diff(before, after, f"a/{path}", f"b/{path}", n=3))
    return f"diff --git a/{path} b/{path}\n{body}"


def judge(cfg, case, model):
    t = case["ticket"]
    ticket = Ticket(id=t["id"], title=t["title"], scope=t["scope"], spec=t.get("spec", []),
                    target=t.get("target", []), body=t.get("body", ""))
    args = (cfg, ticket, spec_text(ticket.spec), case["gate"].strip(), build_diff(case))
    if model is None:
        out = WORK / "calibration" / case["name"]
        out.mkdir(parents=True, exist_ok=True)
        return verifier.verify_pipeline(*args, record_dir=out)
    return verifier.verify(*args, record=WORK / "calibration" / f"{case['name']}.json", model=model)


def main(args):
    cfg = load_config()
    import llm
    from db import DB
    from limits import Limits
    llm.LIMITS = Limits(cfg, DB())
    model = args.model
    cases = load_cases(args.only) * max(1, args.repeat)
    label = model or f"pipeline {cfg.model('verifier')} -> confirm {cfg.verifier.get('confirm') or 'none'}"
    print(f"calibrating verifier {label} on {len(cases)} cases")
    results = []
    with cf.ThreadPoolExecutor(4) as ex:
        futures = {ex.submit(judge, cfg, c, model): c for c in cases}
        for f in cf.as_completed(futures):
            results.append((futures[f], *f.result()))
    results.sort(key=lambda r: r[0]["name"])
    critical = wrong_reject = exact = 0
    total_cost = 0.0
    for case, v, cost in results:
        total_cost += cost
        verdict = v["decision"]
        if case["reject"] and verdict in ACCEPT:
            mark = "CRITICAL"
            critical += 1
        elif not case["reject"] and verdict not in ACCEPT and verdict not in case["expect"]:
            mark = "lost-good-work"
            wrong_reject += 1
        elif verdict in case["expect"]:
            mark = "ok"
            exact += 1
        else:
            mark = "off"
        fake = " fakery" if v.get("fakery") else ""
        defects = ",".join(v.get("defects", []))
        print(f"  {mark:<15} {case['name']:<36} got {verdict:<12}{fake:<7} want {'/'.join(case['expect']):<14} ${cost:.3f}  {defects}")
        if mark != "ok":
            print(f"      {v.get('summary', '')[:300]}")
    print(f"exact {exact}/{len(results)}, critical misses {critical}, good work rejected {wrong_reject}, "
          f"cost ${total_cost:.2f} (${total_cost / max(1, len(results)):.3f}/case)")
    return 1 if critical else 0
