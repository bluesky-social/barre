#!/usr/bin/env python3
"""Tests for bench_compare.py against trimmed real CI results (fixtures/).

Run: python3 -m unittest discover -s .github/scripts -p 'test_*.py'

The six fixtures are runs of .github/workflows/bench.yml on GitHub's virtual
M1, oldest first. The first four measured identical barre code (no crates/,
bench/ or vendor/ change between their commits), so comparisons among them
show pure runner noise; the fifth (52d96baf8c) is the run where
micro/style_set_x20 went from 1.36x to 4.49x; the sixth (9122b937ed) is the
reference for the faster-than-Yoga counts.
"""
import contextlib
import copy
import io
import json
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import bench_compare as B  # noqa: E402

FIX = HERE / "fixtures"
RUNS = sorted(FIX.glob("*.json"))
SHA = {p.stem.split("-")[-1]: p for p in RUNS}
LIST = HERE.parents[1] / "bench" / "expected-faster.txt"
SAME_CODE = RUNS[:4]


def analyse(cur, base=None, listed=None):
    cm, c, n = B.read_run(cur)
    bm, b = None, None
    if base:
        bm, b, bn = B.read_run(base)
        n += bn
    return B.analyse(cm, c, bm, b, listed, n)


def flagged(out, status):
    return [w["workload"] for w in out["workloads"] if w.get("status") == status]


class FixtureTests(unittest.TestCase):
    def test_style_set_regression_and_nothing_else(self):
        out = analyse(SHA["52d96baf8c"], SHA["1854783423"])
        self.assertEqual(flagged(out, "REGRESSION"), ["micro/style_set_x20"])
        self.assertEqual(out["gate"]["status"], "would_fail")
        print(f"\n(a) 52d96baf8c vs 1854783423: WATCH={flagged(out, 'WATCH')} "
              f"IMPROVED={len(flagged(out, 'IMPROVED'))} of {len(out['workloads'])}")
        print("    IMPROVED:", ", ".join(flagged(out, "IMPROVED")))

    def test_self_comparison_flags_nothing(self):
        for p in RUNS:
            out = analyse(p, p)
            self.assertEqual([w for w in out["workloads"] if w["status"] != "steady"], [], p.name)
            self.assertEqual(out["gate"]["reasons"], [], p.name)
        ref = SHA["9122b937ed"]
        self.assertEqual(analyse(ref, ref, B.read_list(LIST))["gate"]["status"], "would_pass")

    def test_faster_than_yoga_counts_on_reference_run(self):
        out = analyse(SHA["9122b937ed"])
        h = out["headline"]
        self.assertEqual((h["faster"], h["tie"], h["slower"], h["total"]), (27, 1, 1, 29))
        self.assertEqual(B.headline_text(out), "faster than Yoga: 27 of 29 (excluding micro/)")
        self.assertEqual(out["micro"]["faster"], 0)
        self.assertEqual(out["micro"]["total"], 6)
        slower = [w["workload"] for w in out["workloads"] if w["verdict"] == "slower" and B.is_headline(w["workload"])]
        tie = [w["workload"] for w in out["workloads"] if w["verdict"] == "tie" and B.is_headline(w["workload"])]
        self.assertEqual(slower, ["yoga/huge_nested"])
        self.assertEqual(tie, ["rn/feed50/rotate"])

    def test_ratchet_list_matches_reference_run(self):
        listed = B.read_list(LIST)
        self.assertEqual(len(listed), 27)
        out = analyse(SHA["9122b937ed"], listed=listed)
        self.assertTrue(all(r["verdict"] == "ok" for r in out["ratchet"]))
        self.assertTrue(all(B.is_headline(n) for n in listed))

    def test_false_positive_rate_on_real_ci_noise(self):
        print("\n(d) thresholds on consecutive CI runs "
              f"(REGRESSION > {B.REGRESSION_PCT:.0f}% + disjoint, WATCH > {B.WATCH_PCT:.0f}%, "
              f"geomean > {B.GEOMEAN_PCT:.0f}%)")
        print("    pair                         code    REGRESSION  WATCH  IMPROVED  non-micro geomean change")
        for i, (a, b) in enumerate(zip(RUNS, RUNS[1:])):
            out = analyse(b, a)
            g = out["geomean"]["non-micro"]["change_pct"]
            same = "same" if b in SAME_CODE else "changed"
            print(f"    {a.stem.split('-')[-1][:10]} -> {b.stem.split('-')[-1][:10]}  {same:8s}"
                  f"{len(flagged(out, 'REGRESSION')):6d}{len(flagged(out, 'WATCH')):11d}"
                  f"{len(flagged(out, 'IMPROVED')):8d}{g:+14.1f}%")
            if b in SAME_CODE:  # identical code: nothing may be a REGRESSION or a geomean regression
                self.assertEqual(flagged(out, "REGRESSION"), [])
                self.assertLess(g, B.GEOMEAN_PCT)

        # all ordered pairs among the four identical-code runs: how bad does pure noise get?
        changes = []
        for a in SAME_CODE:
            for b in SAME_CODE:
                if a != b:
                    changes += [w["change_pct"] for w in analyse(b, a)["workloads"]]
        n = len(changes)
        print(f"    identical-code runs, all {n // 35} ordered pairs, {n} workload comparisons: "
              f"worse than 10%: {sum(c > 10 for c in changes)}, 20%: {sum(c > 20 for c in changes)}, "
              f"30%: {sum(c > 30 for c in changes)}; largest {max(changes):+.0f}%")
        self.assertEqual(sum(c > B.REGRESSION_PCT for c in changes), 0)


class RobustnessTests(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.base = json.loads(SHA["52d96baf8c"].read_text())

    def write(self, name, data):
        p = self.tmp / name
        p.write_text(json.dumps(data))
        return p

    def test_missing_workloads_intervals_and_errors(self):
        cur = copy.deepcopy(self.base)
        base = copy.deepcopy(self.base)
        del base["results"]["rn/deep5/fresh"]                    # only in current
        del cur["results"]["rn/deep10/fresh"]                    # only in baseline
        del cur["results"]["rn/feed50/fresh"]["barre"]["p10"]     # older format
        cur["results"]["rn/feed50/fresh"]["barre"]["median"] *= 2  # +100%, but no interval to prove it
        cur["results"]["yoga/nested_flex"]["barre"] = {"error": "timeout"}
        out = analyse(self.write("c.json", cur), self.write("b.json", base))
        st = {w["workload"]: w["status"] for w in out["workloads"]}
        self.assertEqual(st["rn/deep5/fresh"], "new")
        self.assertEqual(st["rn/feed50/fresh"], "WATCH")        # never REGRESSION without an interval
        self.assertNotIn("yoga/nested_flex", st)
        text = " ".join(out["notes"])
        for needle in ("rn/deep10/fresh", "no p10/p90", "yoga/nested_flex"):
            self.assertIn(needle, text)
        self.assertIn("rn/deep5/fresh", B.render_md(out, True))

    def test_ratchet_levels(self):
        cur = copy.deepcopy(self.base)
        for name, ratio in (("rn/feed50/fresh", 1.03), ("rn/feed50/resize", 1.20)):
            r = cur["results"][name]
            r["barre"]["median"] = r["yoga"]["median"] * ratio
        out = analyse(self.write("c.json", cur), listed=["rn/feed50/fresh", "rn/feed50/resize", "rn/deep5/fresh", "nope/x"])
        v = {r["workload"]: r["verdict"] for r in out["ratchet"]}
        self.assertEqual(v, {"rn/feed50/fresh": "would fail (marginal)", "rn/feed50/resize": "would fail",
                             "rn/deep5/fresh": "ok"})
        self.assertEqual(out["gate"]["status"], "would_fail")
        self.assertTrue(any("nope/x" in n for n in out["notes"]))

    def test_cli_exit_codes_outputs_and_update_list(self):
        cur, base = str(SHA["52d96baf8c"]), str(SHA["1854783423"])
        md, js = self.tmp / "o.md", self.tmp / "o.json"
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(B.main([cur, base, "--md", str(md), "--json", str(js)]), 0)  # report-only
            self.assertEqual(B.main([cur, base, "--enforce"]), 1)
            self.assertEqual(B.main([cur, cur, "--enforce", "--list", str(LIST)]), 0)
            self.assertEqual(B.main([str(self.tmp / "missing.json"), base]), 0)           # never fails
            self.assertEqual(B.main([cur, str(self.tmp / "missing.json")]), 0)
        self.assertIn("REGRESSION", md.read_text())
        j = json.loads(js.read_text())
        self.assertEqual(j["gate"]["status"], "would_fail")
        self.assertFalse(j["gate"]["enforced"])
        # --update-list reproduces the committed list's names from the reference run
        out = self.tmp / "list.txt"
        with contextlib.redirect_stdout(io.StringIO()):
            B.main([str(SHA["9122b937ed"]), "--update-list", "--list", str(out)])
        self.assertEqual(B.read_list(out), B.read_list(LIST))
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            B.main([str(SHA["9122b937ed"]), "--headline"])
        self.assertEqual(buf.getvalue().strip(), "faster than Yoga: 27 of 29 (excluding micro/)")


if __name__ == "__main__":
    unittest.main()
