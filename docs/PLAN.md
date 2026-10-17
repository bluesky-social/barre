# barre: plan

barre is a reimplementation of [Yoga](https://github.com/react/yoga) in Rust.
Goals, in order:

1. **Drop-in.** Exports Yoga's C API (`YGNodeNew`, `YGNodeStyleSetWidth`,
   `YGNodeCalculateLayout`, ...) with the same semantics, so Yoga's own test
   suite and React Native can link against it instead of Yoga.
2. **More accurate.** Pass everything Yoga passes, then fix Yoga's known
   bugs. The first two targets are cache-invalidation bugs:
   - [yoga#1552](https://github.com/react/yoga/issues/1552): `flexBasis: '100%'`
     not recomputed when the parent's size changes (rotation).
   - [yoga#2019](https://github.com/react/yoga/issues/2019): stale
     `computedFlexBasis` reused across layouts when an ancestor's measure cache
     skips a subtree; a `flex: 1` node keeps its old size.
   Both come from caching layout state on nodes without recording which
   layout produced it. barre's design should make that class of bug hard
   (every cached value is keyed by the constraints/generation that produced it).
3. **Faster.** Measured against Yoga on the same workloads through the same C
   API (`bench/`), then in React Native (social-app, profiled with argent).

## The oracle

Yoga's own test suite is the measure of correctness, the way test262 is for
kahvi.

- `vendor/yoga` is pinned (submodule). Its gtest suite (`tests/generated/*.cpp`
  are generated from Chrome by `gentest/`; `tests/*.cpp` are hand-written) is
  compiled **unmodified** against barre's C ABI (`crates/barre-ffi`, a
  staticlib exporting the `YG*` symbols declared by `vendor/yoga/yoga/Yoga.h`).
- Tests that reach into Yoga's C++ internals (`yoga::Node`, `yoga::Style`,
  `CacheTest`, `StyleValuePoolTest`, ...) cannot link against a C ABI. They are
  listed in `conformance/skip.txt` with a reason. Everything else is in scope.
- `./conf [FILTER]` builds and runs the suite and reports pass/fail per test
  (`Suite.Name`). Each test runs in its own process with a timeout, so crashes
  and hangs are failures, not aborted runs.
- `conformance/expected-pass.txt` is the ratchet: every test passing on main.
  A change may never lose one.
- The same runner can build the suite against real Yoga (`./conf --yoga`), which
  must pass everything in scope; that validates the runner.
- New behaviour (bug fixes where barre intentionally differs from Yoga) gets
  new tests under `conformance/tests/`, written by the architect, ideally
  generated from Chrome with Yoga's own `gentest` (fixtures in
  `conformance/fixtures/`).

Differential fuzzing against real Yoga (random trees, compare layouts) comes
after the suite is green; divergences are either barre bugs or documented Yoga
bugs (`conformance/divergences.md`).

## Design

The Yoga C++ source in `vendor/yoga/yoga/` is the spec, the way ECMA-262 is for
kahvi. `./spec <name>` prints a Yoga function or type by name
(e.g. `./spec computeFlexBasisForChild`).

- `crates/barre` (the engine): node tree, style, config, layout results, the
  algorithm. Pure Rust, no unsafe outside the arena, no global state.
- **Port structure, not bugs.** Algorithm functions keep Yoga's names in
  snake_case (`compute_flex_basis_for_child`, `calculate_layout_impl`) and live
  in files mirroring `yoga/algorithm/*.cpp`, so a worker can put the two side
  by side. A function's doc comment names the Yoga function it corresponds to.
  Where barre deliberately differs (a bug fix, a faster structure), the comment
  says so and why.
- **Nodes live in an arena** owned by a tree/context, not in individually
  heap-allocated objects with parent pointers. The C ABI hands out stable
  handles (`YGNodeRef` is a pointer to a small handle struct) so the C API's
  ownership model (free a node, free recursive, clone, insert into another
  parent) still works.
- **Measure functions, baseline functions, dirtied callbacks, context
  pointers** are part of the C ABI and must behave exactly like Yoga's.
- **Errata and experimental features** (`YGConfigSetErrata`,
  `YGExperimentalFeatureWebFlexBasis`, `UseWebDefaults`, point scale factor)
  are supported: the suite exercises them.
- **Never fake progress.** No special-casing test values, no hardcoded results.
  Anything unimplemented panics with `not_implemented!("name")`, which the C ABI
  turns into an abort; the runner counts that as a failure.

## Pipeline (from kahvi)

The harness in `harness/orch` is kahvi's, adapted (docs/HARNESS.md):
tickets in `tickets/`, cheap workers (Haiku, escalating to Sonnet then Opus),
mechanical gates (scope, lint, build, clippy, the ticket's conformance target
with no regressions and at least one new pass), a blind verifier, and a serial
merge bot that runs the full suite against the ratchet. Locally only for now
(no GitHub stack).

Ticket breakdown follows the algorithm's seams, with the generated test files
as targets: absolute positioning (`YGAbsolutePositionTest`,
`YGStaticPositionTest`), align-content/wrap, baseline, aspect ratio, gap,
display: contents, min/max, percentages, box-sizing, rounding (pixel grid),
measure functions and the measure cache, dirty marking, intrinsic sizing.

## Milestones

1. **Spine** (architect): workspace, `barre` core types, the full C ABI surface
   (every function exists; unimplemented ones panic), the conformance runner,
   the harness adapted, and the core flex algorithm for the simple
   row/column/justify/align cases so the pipeline has something to ratchet.
2. **Grind**: workers take the suite to Yoga parity.
3. **Bugs**: tests for #1552 and #2019 (and whatever fuzzing finds), fixed.
4. **Speed**: `bench/` (Yoga's `benchmark/` workloads plus RN-shaped trees),
   profile, optimise. The ratchet keeps it honest.
5. **React Native**: RN's Fabric renderer uses Yoga's C++ internals
   (`yoga::Node`, `yoga::Style` in `YogaLayoutableShadowNode` and
   `YogaStylableProps`), not just the C API. Per docs/RN-SHIM.md, the shim
   keeps Yoga's C++ `Node`/`Style`/`Config` and the C API, and replaces only
   `yoga::calculateLayout` with barre (a `barre-rn` staticlib walking the C++
   nodes through a host interface, its cache stored as a POD blob in the
   node). So the algorithm must be generic over node access, and per-node
   persistent layout state must be `Copy`, fixed-size and pointer-free.
   Wired into bluesky-social/social-app via a patch, A/B against Yoga at
   launch, tested and profiled with argent.
