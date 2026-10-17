+++
id = "T-SMOKE"
title = "calculate_baseline"
kind = "algorithm"
tier = "haiku"
scope = ["crates/barre/src/algorithm/baseline.rs"]
read = ["docs/API.md", "crates/barre/src/algorithm/baseline.rs", "crates/barre/src/algorithm/align.rs"]
spec = ["calculateBaseline"]
target = ["generated/YGAlignItemsTest/YogaTest.align_baseline", "YGAlignBaselineTest/", "YGBaselineFuncTest/"]
+++
Pipeline smoke test: run it against a scratch branch, never main
(`./orch run --tickets tickets/smoke --branch <scratch>` with
`BARRE_WORK=/tmp/...`; see docs/HARNESS.md). It is a real port, small and
with a clear target, so it exercises the worker's tools (`./conf` and
`./spec` inside a slot worktree), the gates, the verifier and the merge bot.

Port `calculateBaseline` into `calculate_baseline`
(`crates/barre/src/algorithm/baseline.rs`; it calls `not_implemented!` now).
`is_baseline_layout` is already done. `resolve_child_alignment` is in
`algorithm/align.rs`. barre has no event system: Yoga's `Event::publish`
calls are not ported.
