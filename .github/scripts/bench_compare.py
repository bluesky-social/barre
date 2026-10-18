#!/usr/bin/env python3
"""Compare a bench/run result with a baseline and report "faster than Yoga".

Usage: bench_compare.py CURRENT.json [BASELINE.json] [--list FILE] [--md OUT.md]
                        [--json OUT.json] [--update-list] [--headline] [--enforce]

Phase 1 is REPORT-ONLY: the exit status is 0 whatever the data says (also on
unreadable input) unless --enforce is given, which exits 1 when the gate
`would_fail`. Nothing in CI passes --enforce yet. Methodology, thresholds and
the noise data behind them: docs/BENCH.md, "Regression check and the
faster-than-Yoga metric".

What is compared. Each workload runs interleaved on both engines, so the
quantity that survives a noisy shared runner is the ratio barre/yoga *within
one run*, not absolute times. Per workload:

  ratio     = barre median / yoga median
  change    = current ratio / baseline ratio - 1, in percent (+ is worse)
  interval  = [barre p10, barre p90] divided by the same run's yoga median.
              Current and baseline intervals are compared after this
              normalisation, so a runner that is slower overall in one run
              moves both engines and the interval together. The intervals
              "overlap" if cur.lo <= base.hi and base.lo <= cur.hi. (Yoga's
              own spread is not propagated: only barre's interval is used.
              Shared-runner spread is so large that this is already lenient.)

Statuses (constants below): REGRESSION, WATCH, IMPROVED, steady.
"Faster than Yoga" is judged on the current ratio alone.
"""
import argparse
import json
import math
import sys
from pathlib import Path

# --- thresholds: every number that decides anything is here -----------------
REGRESSION_PCT = 30.0   # ratio worse by more than this AND intervals disjoint -> REGRESSION
WATCH_PCT = 10.0        # ratio worse by more than this (any overlap) -> WATCH
IMPROVED_PCT = 10.0     # ratio better by more than this -> IMPROVED
GEOMEAN_PCT = 5.0       # non-micro geomean worse by more than this -> "geomean regression"
FASTER_BELOW = 0.95     # ratio < this: faster than Yoga
SLOWER_ABOVE = 1.05     # ratio > this: slower; in between is a tie
RATCHET_MAX_RATIO = 0.90        # --update-list: listed if ratio <= this
RATCHET_FAIL_ABOVE = 1.05       # listed workload above this: "would fail"
RATCHET_MARGINAL_ABOVE = 1.00   # above this (up to FAIL_ABOVE): "would fail (marginal)"
HEADLINE_EXCLUDES = ("micro/",)  # C-API call cost; no cross-language LTO, barre is ~1.5x there
CATEGORIES = ("rn", "capture", "yoga", "micro")
DEFAULT_LIST = Path(__file__).resolve().parents[2] / "bench" / "expected-faster.txt"

SEVERITY = {"REGRESSION": 0, "WATCH": 1, "IMPROVED": 2, "steady": 3}


class Row:
    """One workload of one run: ratio plus barre's interval in yoga-median units."""

    def __init__(self, yoga, barre):
        self.ratio = barre["median"] / yoga["median"]
        p10, p90 = barre.get("p10"), barre.get("p90")
        self.lo = p10 / yoga["median"] if p10 else None
        self.hi = p90 / yoga["median"] if p90 else None


def read_run(path):
    """Returns (meta, {workload: Row}, notes). Workloads that errored or lack a
    median on either engine are left out, with a note."""
    data = json.loads(Path(path).read_text())
    rows, notes = {}, []
    for name, r in (data.get("results") or {}).items():
        y, b = r.get("yoga"), r.get("barre")
        if not isinstance(y, dict) or not isinstance(b, dict):
            notes.append(f"`{name}`: no yoga/barre pair in {Path(path).name}, skipped")
        elif "error" in y or "error" in b:
            notes.append(f"`{name}`: errored in {Path(path).name}, skipped")
        elif not y.get("median") or not b.get("median"):
            notes.append(f"`{name}`: no median in {Path(path).name}, skipped")
        else:
            rows[name] = Row(y, b)
    load = data.get("load") or [None]
    meta = {"label": data.get("label", "?"), "time": data.get("time", "?"),
            "cpu": data.get("cpu", "?"), "load": load[0]}
    return meta, rows, notes


def category(name):
    head = name.split("/", 1)[0]
    return head if head in CATEGORIES else "other"


def is_headline(name):
    return not name.startswith(HEADLINE_EXCLUDES)


def geomean(xs):
    xs = list(xs)
    return math.exp(sum(map(math.log, xs)) / len(xs)) if xs else None


def pct(new, old):
    return (new / old - 1.0) * 100.0


def verdict(ratio):
    return "faster" if ratio < FASTER_BELOW else "slower" if ratio > SLOWER_ABOVE else "tie"


def classify(cur, base):
    """(change %, overlap True/False/None, status). overlap None: an interval is
    missing, which can never make a REGRESSION (needs disjoint intervals)."""
    change = pct(cur.ratio, base.ratio)
    if None in (cur.lo, cur.hi, base.lo, base.hi):
        overlap = None
    else:
        overlap = cur.lo <= base.hi and base.lo <= cur.hi
    if change > REGRESSION_PCT and overlap is False:
        status = "REGRESSION"
    elif change > WATCH_PCT:
        status = "WATCH"
    elif change < -IMPROVED_PCT:
        status = "IMPROVED"
    else:
        status = "steady"
    return change, overlap, status


def headline_counts(rows):
    """(faster, total) over a read_run() result, micro/ excluded: the headline."""
    ratios = [r.ratio for n, r in rows.items() if is_headline(n)]
    return sum(verdict(x) == "faster" for x in ratios), len(ratios)


def read_list(path):
    names = []
    for line in Path(path).read_text().splitlines():
        line = line.split("#", 1)[0].strip()
        if line:
            names.append(line)
    return names


def analyse(cur_meta, cur, base_meta, base, listed, notes):
    """Everything the markdown and the JSON say, as one dict."""
    out = {"current": cur_meta, "baseline": base_meta, "notes": list(notes),
           "thresholds": {k: v for k, v in globals().items()
                          if k.isupper() and isinstance(v, (int, float))}}
    # per-workload comparison
    works = []
    for name in cur:
        w = {"workload": name, "category": category(name), "ratio": cur[name].ratio,
             "verdict": verdict(cur[name].ratio)}
        if base is not None:
            if name in base:
                change, overlap, status = classify(cur[name], base[name])
                w.update(baseline_ratio=base[name].ratio, change_pct=change,
                         intervals_overlap=overlap, status=status)
                if overlap is None:
                    out["notes"].append(f"`{name}`: no p10/p90 in one of the runs, interval check skipped")
            else:
                w.update(status="new")
        works.append(w)
    if base is not None:
        for name in base:
            if name not in cur:
                out["notes"].append(f"`{name}` is in the baseline but not in this run")
    out["workloads"] = works

    # faster-than-Yoga counts
    def counts(items):
        c = {"faster": 0, "tie": 0, "slower": 0, "total": 0}
        for w in items:
            c[w["verdict"]] += 1
            c["total"] += 1
        return c
    out["faster_by_category"] = {cat: counts(w for w in works if w["category"] == cat)
                                 for cat in CATEGORIES + ("other",)
                                 if any(w["category"] == cat for w in works)}
    out["headline"] = counts(w for w in works if is_headline(w["workload"]))
    out["micro"] = counts(w for w in works if not is_headline(w["workload"]))

    # geomeans, over workloads present in both runs so the change is like for like
    out["geomean"] = {}
    if base is not None:
        common = [w for w in works if "baseline_ratio" in w]
        groups = {cat: [w for w in common if w["category"] == cat] for cat in CATEGORIES}
        groups["non-micro"] = [w for w in common if is_headline(w["workload"])]
        for g, ws in groups.items():
            if ws:
                now, then = geomean(w["ratio"] for w in ws), geomean(w["baseline_ratio"] for w in ws)
                out["geomean"][g] = {"n": len(ws), "current": now, "baseline": then, "change_pct": pct(now, then)}

    # ratchet (report-only)
    ratchet = []
    for name in listed or []:
        if name not in cur:
            out["notes"].append(f"ratchet list: `{name}` is not in this run")
            continue
        r = cur[name].ratio
        v = ("would fail" if r > RATCHET_FAIL_ABOVE else
             "would fail (marginal)" if r > RATCHET_MARGINAL_ABOVE else
             "ok")
        ratchet.append({"workload": name, "ratio": r, "verdict": v})
    out["ratchet"] = ratchet

    # what a gate would have done
    reasons = []
    for w in works:
        if w.get("status") == "REGRESSION":
            reasons.append(f"REGRESSION `{w['workload']}`: barre/yoga {w['baseline_ratio']:.2f}x -> "
                           f"{w['ratio']:.2f}x ({w['change_pct']:+.0f}%, intervals disjoint)")
    g = out["geomean"].get("non-micro")
    if g and g["change_pct"] > GEOMEAN_PCT:
        reasons.append(f"geomean regression: non-micro geomean {g['baseline']:.3f}x -> "
                       f"{g['current']:.3f}x ({g['change_pct']:+.1f}%)")
    for r in ratchet:
        if r["verdict"] != "ok":
            reasons.append(f"ratchet `{r['workload']}`: {r['ratio']:.2f}x, {r['verdict']}")
    out["gate"] = {"status": "would_fail" if reasons else "would_pass", "reasons": reasons,
                   "enforced": False}
    return out


def headline_text(out):
    h = out["headline"]
    return f"faster than Yoga: {h['faster']} of {h['total']} (excluding micro/)"


def render_md(out, have_baseline):
    h, m, g = out["headline"], out["micro"], out["gate"]
    cur, base = out["current"], out["baseline"]
    L = ["## barre vs Yoga: regression check (report-only)", ""]
    L.append(f"**{headline_text(out)}**: {h['tie']} tie, {h['slower']} slower "
             f"(faster below {FASTER_BELOW:.2f}x, slower above {SLOWER_ABOVE:.2f}x). "
             f"`micro/`: {m['faster']} of {m['total']} faster.")
    L.append("")
    if have_baseline:
        L.append(f"Gate, not enforced yet: **{g['status']}**" + ("" if not g["reasons"] else ":"))
        L += [f"- {r}" for r in g["reasons"]]
        L.append("")
        ld = lambda m_: "n/a" if m_["load"] is None else f"{m_['load']:.1f}"
        L.append(f"Current `{cur['label']}` (load avg {ld(cur)}) against baseline `{base['label']}` "
                 f"(load avg {ld(base)}), both on {cur['cpu']}.")
        L.append("")
        flagged = [w for w in out["workloads"] if w.get("status") in ("REGRESSION", "WATCH", "IMPROVED", "new")]
        flagged.sort(key=lambda w: (SEVERITY.get(w["status"], 4), -abs(w.get("change_pct", 0))))
        if flagged:
            L += ["| workload | baseline | current | change | barre intervals | status |", "|---|--:|--:|--:|---|---|"]
            for w in flagged:
                if w["status"] == "new":
                    L.append(f"| `{w['workload']}` | | {w['ratio']:.2f}x | | | new |")
                    continue
                ov = {True: "overlap", False: "disjoint", None: "n/a"}[w["intervals_overlap"]]
                st = f"**{w['status']}**" if w["status"] == "REGRESSION" else w["status"]
                L.append(f"| `{w['workload']}` | {w['baseline_ratio']:.2f}x | {w['ratio']:.2f}x "
                         f"| {w['change_pct']:+.0f}% | {ov} | {st} |")
        else:
            L.append("No workload moved by more than "
                     f"{min(WATCH_PCT, IMPROVED_PCT):.0f}% against the baseline.")
        L.append("")
        L.append(f"REGRESSION: ratio worse by more than {REGRESSION_PCT:.0f}% and barre's p10-p90 "
                 f"(in yoga-median units) disjoint from the baseline's. WATCH: worse by more than {WATCH_PCT:.0f}%. "
                 f"IMPROVED: better by more than {IMPROVED_PCT:.0f}%.")
        L.append("")
    else:
        L += ["No baseline available, so only the faster-than-Yoga metric is reported.", ""]

    L += ["### Faster than Yoga, by category", ""]
    show_geo = bool(out["geomean"])
    L.append("| category | faster | tie | slower | workloads |" + (" geomean now | geomean baseline | change |" if show_geo else ""))
    L.append("|---|--:|--:|--:|--:|" + ("--:|--:|--:|" if show_geo else ""))
    for cat, c in out["faster_by_category"].items():
        row = f"| {cat}/ | {c['faster']} | {c['tie']} | {c['slower']} | {c['total']} |"
        if show_geo:
            gm = out["geomean"].get(cat)
            row += (f" {gm['current']:.3f}x | {gm['baseline']:.3f}x | {gm['change_pct']:+.1f}% |" if gm else " | | |")
        L.append(row)
    row = f"| **non-micro** | {h['faster']} | {h['tie']} | {h['slower']} | {h['total']} |"
    if show_geo:
        gm = out["geomean"].get("non-micro")
        row += (f" **{gm['current']:.3f}x** | {gm['baseline']:.3f}x | **{gm['change_pct']:+.1f}%** |" if gm else " | | |")
    L += [row, ""]
    if show_geo:
        L += ["Geomeans are over the workloads present in both runs; "
              f"\"geomean regression\" is a non-micro change above {GEOMEAN_PCT:.0f}%.", ""]

    if out["ratchet"]:
        bad = [r for r in out["ratchet"] if r["verdict"] != "ok"]
        L += ["### Ratchet list (`bench/expected-faster.txt`)", ""]
        if bad:
            L += ["| workload | barre/yoga | verdict |", "|---|--:|---|"]
            L += [f"| `{r['workload']}` | {r['ratio']:.2f}x | {r['verdict']} |" for r in bad]
        else:
            L.append(f"All {len(out['ratchet'])} listed workloads are at or below 1.00x.")
        L.append("")
    if out["notes"]:
        L += ["### Notes", ""] + [f"- {n}" for n in out["notes"]] + [""]
    L.append("Shared-runner numbers: read single flags as hints (docs/BENCH.md, \"Regression check and the "
             "faster-than-Yoga metric\"). Reported only; this check cannot fail the job.")
    return "\n".join(L) + "\n"


def write_list(path, cur_meta, cur):
    names = [n for n in cur if is_headline(n) and cur[n].ratio <= RATCHET_MAX_RATIO]
    lines = [
        f"# Workloads that comfortably beat Yoga: barre/yoga <= {RATCHET_MAX_RATIO:.2f} in bench run {cur_meta['label']}.",
        "# One name per line, # starts a comment. .github/scripts/bench_compare.py reports",
        f"# \"would fail\" for any of these above {RATCHET_FAIL_ABOVE:.2f}x (\"would fail (marginal)\" above {RATCHET_MARGINAL_ABOVE:.2f}x).",
        "# Report-only for now (docs/BENCH.md). Regenerate from a run, in its own commit:",
        "#   .github/scripts/bench_compare.py RESULT.json --update-list",
        *names,
    ]
    Path(path).write_text("\n".join(lines) + "\n")
    return len(names)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("current")
    ap.add_argument("baseline", nargs="?")
    ap.add_argument("--list", help=f"ratchet list (default {DEFAULT_LIST.name} in bench/ if it exists)")
    ap.add_argument("--md")
    ap.add_argument("--json")
    ap.add_argument("--update-list", action="store_true", help="rewrite the ratchet list from CURRENT, then exit")
    ap.add_argument("--headline", action="store_true", help="print only the faster-than-Yoga line")
    ap.add_argument("--enforce", action="store_true", help="exit 1 if the gate would fail (not used by CI yet)")
    args = ap.parse_args(argv)

    try:
        cur_meta, cur, notes = read_run(args.current)
        if args.update_list:
            n = write_list(args.list or DEFAULT_LIST, cur_meta, cur)
            print(f"wrote {n} workloads to {args.list or DEFAULT_LIST}")
            return 0
        base_meta = base = None
        if args.baseline:
            try:
                base_meta, base, bnotes = read_run(args.baseline)
                notes += bnotes
            except (OSError, ValueError) as e:
                notes.append(f"baseline {args.baseline} unreadable ({e}); comparison skipped")
        list_path = Path(args.list) if args.list else (DEFAULT_LIST if DEFAULT_LIST.exists() else None)
        listed = None
        if list_path:
            try:
                listed = read_list(list_path)
            except OSError as e:
                notes.append(f"ratchet list {list_path} unreadable ({e})")
        out = analyse(cur_meta, cur, base_meta, base, listed, notes)
        if args.headline:
            print(headline_text(out))
            return 0
        md = render_md(out, base is not None)
        if args.md:
            Path(args.md).write_text(md)
        if args.json:
            Path(args.json).write_text(json.dumps(out, indent=1) + "\n")
        print(md)
        return 1 if args.enforce and out["gate"]["status"] == "would_fail" else 0
    except Exception as e:  # report-only: never take the job down
        print(f"bench_compare: error: {type(e).__name__}: {e}", file=sys.stderr)
        return 1 if args.enforce else 0


if __name__ == "__main__":
    sys.exit(main())
