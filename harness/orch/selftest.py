#!/usr/bin/env python3
"""Checks the harness pieces that need no model and no engine build:
`./spec` lookups, the lint rules, conformance result handling, ticket
parsing and the red-team cases. Run: python3 harness/orch/selftest.py"""

import io
import json
import sys
import tempfile
import tomllib
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import spec  # noqa: E402  (harness/spec.py)
from common import ROOT  # noqa: E402

FAILS = []


def check(cond, what):
    if not cond:
        FAILS.append(what)
        print(f"FAIL {what}")


def test_spec():
    want = {
        "computeFlexBasisForChild": "algorithm/CalculateLayout.cpp",
        "compute_flex_basis_for_child": "algorithm/CalculateLayout.cpp",
        "Node::resolveFlexGrow": "node/Node.cpp",
        "canUseCachedMeasurement": "algorithm/Cache.cpp",
        "YGNodeStyleSetFlexBasisPercent": "YGNodeStyle.cpp",
        "calculateBaseline": "algorithm/Baseline.cpp",
        "isBaselineLayout": "algorithm/Baseline.cpp",
        "roundLayoutResultsToPixelGrid": "algorithm/PixelGrid.cpp",
        "roundValueToPixelGrid": "algorithm/PixelGrid.cpp",
        "layoutAbsoluteChild": "algorithm/AbsoluteLayout.cpp",
        "calculateFlexLine": "algorithm/FlexLine.cpp",
        "boundAxis": "algorithm/BoundAxis.h",
        "LayoutResults": "node/LayoutResults.h",
        "FlexLine": "algorithm/FlexLine.h",
        "Style::flexBasis": "style/Style.h",
        "Config::setErrata": "config/Config.cpp",
        "YGConfigSetPointScaleFactor": "YGConfig.cpp",
        "YGNodeCalculateLayout": "YGNode.cpp",
        "YGAlign": "YGEnums.h",
        "Align": "enums/Align.h",
        "YGValueAuto": "YGValue.cpp",
        "Node::setDirty": "node/Node.cpp",
        "LayoutResults::operator==": "node/LayoutResults.cpp",
        "calculateLayoutInternal": "algorithm/CalculateLayout.cpp",
    }
    for name, file in want.items():
        found = spec.lookup(name)
        check(found and any(d.file.endswith(file) for d in found), f"spec {name} -> {file}: {[d.file for d in found]}")
    d = spec.lookup("canUseCachedMeasurement")[0]
    check(d.text.rstrip().endswith("}") and "newSizeIsStricterAndStillValid" in d.text, "spec body is complete")
    refs = [r.name for r in spec.refs_for(spec.lookup("canUseCachedMeasurement"), spec.load())]
    check("sizeIsExactAndMatchesOldMeasuredSize" in refs, f"spec --refs follows calls: {refs}")
    check(len(spec.lookup("Node::Node")) >= 3, "constructors with initialiser lists are indexed")
    with redirect_stdout(io.StringIO()), redirect_stderr(io.StringIO()):
        check(spec.main(["--find", "baseline"]) == 0, "spec --find")
        check(spec.main(["noSuchFunctionAnywhere"]) == 1, "spec on an unknown name fails")


def test_lint():
    from policy import RULES, lint

    def added(path, *lines):
        return f"+++ b/{path}\n@@ -1,1 +1,{len(lines)} @@\n" + "".join(f"+{l}\n" for l in lines)

    engine = "crates/barre/src/algorithm/pixel_grid.rs"
    rejects = {
        "unsafe { x }": "unsafe",
        "#[test]": "tests",
        "#[allow(clippy::float_cmp)]": "lint suppression",
        "std::process::exit(1);": "exiting",
        "let v = std::env::var(\"X\");": "environment",
        "let r = std::panic::catch_unwind(|| f());": "catching panics",
        "static CACHE: Mutex<Vec<f32>> = Mutex::new(Vec::new());": "global state",
        "thread_local! { static X: u32 = 0; }": "global state",
        "// makes YGAlignItemsTest pass": "test harness",
        "todo!()": "todo",
        "println!(\"{}\", x);": "printing",
    }
    for line, why in rejects.items():
        r, _ = lint(added(engine, line))
        check(any(why in x["rule"] for x in r), f"lint rejects {line!r}: {[x['rule'] for x in r]}")
    flags = {
        "if value == 33.333 { return 33.5; }": "precise float",
        "if children.len() == 3 {": "child count",
        "146.5 => 147.0,": "match arm",
        "let w = node.width().unwrap_or(0.0);": "default",
        "// simplified for now": "shortcut",
    }
    for line, why in flags.items():
        _, f = lint(added(engine, line))
        check(any(why in x["rule"] for x in f), f"lint flags {line!r}: {[x['rule'] for x in f]}")
    clean = ["let half = size * 0.5;", "if (a - b).abs() < 0.0001 {", "not_implemented!(\"calculate_baseline\")",
             "let x = 2.0 * margin;", "match align { Align::Center => 0.5, _ => 0.0 }"]
    for line in clean:
        r, f = lint(added(engine, line))
        check(not r, f"lint does not reject {line!r}: {r}")
        check(not [x for x in f if "precise float" in x["rule"] or "match arm" in x["rule"]],
              f"lint does not flag {line!r} as a magic number: {f}")
    r, _ = lint(added("crates/barre-ffi/src/node.rs", "unsafe { &*node }"))
    check(not r, "unsafe is allowed in the FFI crate")
    r, _ = lint(added(engine, "unsafe { x }"), allow=["unsafe"])
    check(not r, "a ticket can waive a rule")
    check("unsafe" in RULES and "global-state" in RULES, "rule ids")


def test_conformance():
    from conformance import normalise, read_ids, summarise, write_baseline
    raw = {
        "generated/YGAlignItemsTest/YogaTest.align_baseline": {"status": "crash",
                                                              "reason": "not implemented: calculate_baseline"},
        "generated/YGAlignItemsTest/YogaTest.align_center": {"status": "pass", "reason": ""},
        "generated/YGAlignItemsTest/YogaTest.align_end": {"status": "fail", "reason": "Expected 10 got 0"},
        "generated/YGGapTest/YogaTest.gap": {"status": "skip", "reason": "skip.txt"},
        "YGMeasureTest/YogaTest.measure": "pass",
        "YGRoundingMeasureFuncTest/YogaTest.r": {"status": "crash",
                                                "reason": "not implemented: round_layout_results_to_pixel_grid: text nodes"},
    }
    res = normalise(raw)
    check(len(res) == 6 and res[0]["id"].startswith("YGMeasureTest"), "normalise sorts and accepts both shapes")
    ni = {r["id"]: r["not_implemented"] for r in res}
    check(ni["generated/YGAlignItemsTest/YogaTest.align_baseline"] == "calculate_baseline",
          f"not_implemented extracted: {ni}")
    check(ni["YGRoundingMeasureFuncTest/YogaTest.r"] == "round_layout_results_to_pixel_grid: text nodes",
          f"not_implemented keeps the whole name: {ni}")
    from triage import group
    check(group("generated/YGAlignItemsTest/YogaTest.a") == "generated/YGAlignItemsTest" and
          group("YGFlexBasisFitContentTest/YogaTest/YGFlexBasisFitContentTest.x/0") == "YGFlexBasisFitContentTest",
          "triage groups by test file")
    base = {"generated/YGAlignItemsTest/YogaTest.align_end", "generated/YGGapTest/YogaTest.gap",
            "YGMeasureTest/YogaTest.measure"}
    s = summarise(res, base, ["generated/YGAlignItemsTest"], valid=True)
    check(s["regressions"] == ["generated/YGAlignItemsTest/YogaTest.align_end"], f"regressions: {s['regressions']}")
    check(s["new_passes"] == ["generated/YGAlignItemsTest/YogaTest.align_center"], f"new passes: {s['new_passes']}")
    check(s["runs"] == 5 and s["crash"] == 2 and s["not_implemented"] == 2, f"counts: {s}")
    s = summarise(res, base, [], valid=True)
    check("generated/YGGapTest/YogaTest.gap" in s["vanished"] and
          "generated/YGGapTest/YogaTest.gap" not in s["regressions"], "a newly skipped test is not a regression")
    with tempfile.TemporaryDirectory() as d:
        p = Path(d) / "expected-pass.txt"
        p.write_text("# header\n# more\n\nb\na\n")
        write_baseline(p, {"c", "a"})
        check(p.read_text() == "# header\n# more\n\na\nc\n", f"baseline keeps its header: {p.read_text()!r}")
        check(read_ids(p) == {"a", "c"}, "read_ids")


def test_tickets():
    import tickets
    from pipeline import own_not_implemented
    text = (ROOT / "tickets" / "EXAMPLE.md.txt").read_text()
    t = tickets.parse(text, "EXAMPLE.md.txt")
    check(t.spec == ["calculateBaseline", "isBaselineLayout"] and t.target, "example ticket parses")
    own = own_not_implemented(t)
    check(bool(own.search("crash  not implemented: calculate_baseline")), "own not_implemented (snake)")
    check(bool(own.search("not implemented: is_baseline_layout")), "own not_implemented (second spec name)")
    check(not own.search("not implemented: calculate_baseline_for_grid"), "own not_implemented is exact")
    try:
        tickets.parse(text.replace("needs = []", 'allow = ["nope"]'), "x")
        check(False, "unknown allow rule rejected")
    except ValueError:
        pass
    check(not list(Path(tickets.TICKETS).glob("*.md.txt")) or
          "T-0042" not in tickets.load_all(), "EXAMPLE is not loaded")


def test_redteam():
    from calibrate import CASES, build_diff
    from policy import lint
    cases = sorted(CASES.glob("*.toml"))
    check(len(cases) >= 10, f"red-team cases: {len(cases)}")
    for p in cases:
        c = tomllib.loads(p.read_text())
        for key in ("expect", "reject", "path", "gate", "ticket", "files"):
            check(key in c, f"{p.name}: has {key}")
        check(all(e in ("accept", "revise", "reject") for e in c.get("expect", [])), f"{p.name}: expect values")
        check(not (c.get("reject") and "accept" in c.get("expect", [])), f"{p.name}: a planted defect cannot expect accept")
        diff = build_diff(c)
        check(diff.count("\n+") > 0, f"{p.name}: diff is not empty")
        for name in c["ticket"].get("spec", []):
            check(bool(spec.lookup(name)), f"{p.name}: spec name {name} exists in Yoga")
        r, _ = lint(diff)
        check(not r, f"{p.name}: passes lint (cases model diffs that reached the verifier): {[x['rule'] for x in r]}")


def main():
    for t in (test_spec, test_lint, test_conformance, test_tickets, test_redteam):
        try:
            t()
        except Exception as e:  # noqa: BLE001
            FAILS.append(f"{t.__name__}: {e!r}")
            print(f"FAIL {t.__name__}: {e!r}")
    print(json.dumps({"failures": len(FAILS)}))
    return 1 if FAILS else 0


if __name__ == "__main__":
    sys.exit(main())
