# Tickets

One file per unit of work, `tickets/T-NNNN.md`, written by the architect and
picked up by `./orch run` (docs/HARNESS.md). Only `tickets/*.md` are loaded:
not this README, not `*.md.txt`, not subdirectories (`tickets/smoke/` holds
pipeline smoke tests, run with `./orch run --tickets tickets/smoke`). The
files say what to do; what has happened to a ticket (status, tier,
attempts, notes) lives in the orchestrator's database, so editing a ticket
does not reset it (`./orch reset T-0001` does).

## Format

TOML frontmatter between `+++` lines, then free-form notes for the worker:

```
+++
id = "T-0001"
title = "calculate_baseline"
kind = "algorithm"          # algorithm | style | api | node | infra
tier = "haiku"              # starting tier: haiku | sonnet | opus | architect (never claimed)
scope = ["crates/barre/src/algorithm/baseline.rs"]
read = ["docs/API.md", "crates/barre/src/algorithm/align.rs"]
spec = ["calculateBaseline", "isBaselineLayout"]
target = ["generated/YGAlignItemsTest/YogaTest.align_baseline", "YGAlignBaselineTest/"]
check = []
needs = ["T-0000"]
allow = []
priority = 0
+++
Notes from the architect, shown to the worker verbatim.
```

- **`scope`** (required): globs of the only files the worker may change. A
  live hook blocks edits elsewhere, and the gate rejects any diff that
  strays. Protected paths (`policy.py`: `vendor/`, `harness/`,
  `conformance/`, `tickets/`, `docs/`, Cargo manifests, `build.rs`, tests)
  are refused whatever the scope says. Keep it to the file that mirrors the
  Yoga file being ported (`yoga/algorithm/Baseline.cpp` ->
  `crates/barre/src/algorithm/baseline.rs`), plus the C ABI file when the
  ticket implements `YG*` functions.
- **`spec`**: Yoga names, as `./spec` takes them (`computeFlexBasisForChild`,
  `Node::resolveFlexGrow`, `YGNodeStyleSetFlexBasisPercent`). The worker and
  the verifier both get `./spec --refs` output for each: the definition plus
  the short functions it calls. This is the spec the diff is judged against,
  so list every Yoga function the ticket ports.
- **`target`**: conformance test id prefixes (`./conf --list` prints ids:
  `generated/YGAlignItemsTest/YogaTest.align_baseline_child`,
  `YGMeasureCacheTest/YogaTest.measure_once_single_flexible_child`). The
  gate requires at least one new pass in the target and no regressions; the
  ticket is `done` when every test under the target passes after a merge.
  A prefix ending in `/` is a whole file; a prefix ending mid-name is every
  test whose name starts with it.
- **`check`**: shell commands that must exit 0, run from the worker's tree
  with `CARGO_TARGET_DIR` set to the gate's build (for infra tickets with no
  conformance target). A ticket needs a target, a check, or both.
- **`read`**: files worth reading first (always `docs/API.md`).
- **`needs`**: tickets that must be `done` (or stacked) before this one is
  claimed.
- **`allow`**: lint rules (`policy.py` REJECT ids) this ticket waives, e.g.
  `["unsafe"]` for a C ABI ticket outside `crates/barre-ffi`. Unknown ids
  make the ticket file invalid.
- **`tier`**: the first model to try. Escalation moves it up after strikes.
- **`priority`**: higher first; ties go by id.
- **`title`**: short. If it is the snake_case name of the main function,
  `not implemented: <title>` failures reopen the ticket after a partial
  merge (so do the `spec` names, snake_cased).

## Writing good tickets

- One Yoga function or one small group of tightly coupled ones per ticket,
  with a target that only that work can move. A target the ticket cannot
  move on its own (blocked on another ticket) gets `needs`.
- The notes say what already exists, what the worker must not change, and
  anything non-obvious about the port (an errata flag, how barre represents
  something differently from Yoga). Never put expected sizes in a ticket.
- The architect writes the skeleton first: the function exists with Yoga's
  signature and calls `not_implemented!("name")`. Workers fill bodies; they
  do not invent signatures or restructure modules.
- `EXAMPLE.md.txt` is a complete, well-formed ticket (not loaded).
