# barre

A reimplementation of [Yoga](https://github.com/react/yoga), the flexbox layout
engine behind React Native, in Rust. It exports Yoga's C API, so code written
against `yoga/Yoga.h` links against barre instead of Yoga; it aims to lay
everything out exactly as Yoga does, except where Yoga is wrong, and to do it
faster.

> [!WARNING]
> **barre is very early and not ready for production use.** It was started in
> October 2026, most of its code was written by AI agents, and it has not
> shipped in any app. Passing Yoga's test suite does not mean it behaves like
> Yoga on every input. The C API may change, and the React Native integration
> does not exist yet. Use Yoga.

Most of the code was written by a swarm of cheap models (mostly Haiku, with
harder tickets escalated to Sonnet and Opus) working one ticket at a time under
a harness adapted from [kahvi](https://github.com/mozzius/kahvi), with an
architect model doing the design and the hard parts.

## Results

CI on `main` rewrites these rows after every push.

<!-- results:start -->

| Suite | Passing | Pass rate | |
|---|--:|--:|---|
| [Yoga's test suite](https://github.com/react/yoga/tree/52f6d0bd3824296b8e852cb4fcdf3b9821ab2aa5/tests) <!-- yoga-suite --> | 752 / 752 | 100.00% | Yoga 52f6d0bd's gtest files, unmodified, linked against barre's C API; 752 tests in 54 files; 107 more need Yoga's C++ internals ([skip.txt](conformance/skip.txt)) |
| [barre's own tests](conformance/tests) <!-- barre --> | 100 / 102 | 98.04% | the internals-only Yoga tests ported to the C API, and regression tests for Yoga bugs |
| [Yoga bugs fixed](conformance/divergences.md) <!-- yoga-bugs --> | 34 / 34 | 100.00% | tests that real Yoga fails by design, in 10 files; the same runner linked against Yoga passes everything else in scope (820 tests) |

<!-- results:end -->

- **Yoga's own test suite is the only judge of correctness.** `vendor/yoga`
  is pinned; its gtest files are compiled unmodified against barre's C ABI
  and every test runs in its own process, with must-fail canaries so a faked
  pass makes the run invalid ([docs/CONFORMANCE.md](docs/CONFORMANCE.md)).
  Tests that reach into Yoga's C++ internals (`yoga::Node`, the event
  stream) cannot link against a C ABI and are out of scope; what they check
  through the C API is ported to `conformance/tests/`. Passing tests are a
  ratchet (`conformance/expected-pass.txt`): CI fails on any regression.
- **Relayout must equal a fresh layout.** Yoga caches layout state on nodes
  without recording which layout produced it, so a relayout after a resize
  can differ from laying out the same tree from scratch. barre follows the
  fresh layout. Each such Yoga bug has tests that fail on real Yoga (and
  pass on barre), documented in
  [conformance/divergences.md](conformance/divergences.md):
  [#2019](https://github.com/react/yoga/issues/2019) and
  [#1552](https://github.com/react/yoga/issues/1552) (stale flex basis after
  rotation), [#1626](https://github.com/react/yoga/issues/1626) (stale
  percentage padding), [#1784](https://github.com/react/yoga/issues/1784)
  (measure funcs see a stale width), [#1003](https://github.com/react/yoga/issues/1003)
  (cached measurement ignores min/max),
  [#2035](https://github.com/react/yoga/issues/2035) (free space from
  min-clamped bases), [#1932](https://github.com/react/yoga/issues/1932)
  (baseline of an unmeasured child), [#2038](https://github.com/react/yoga/issues/2038)
  (flexShrink runaway from an FMA residual) and
  [#2007](https://github.com/react/yoga/issues/2007) (exponential min-content
  recursion).
- **The runner checks itself.** The same suite linked against real Yoga
  (`./conf --yoga`) must pass everything in scope, with exactly the
  documented Yoga bugs failing.

<!-- benchmarks:start -->

Benchmarks from the last run on main (2026-10-18, barre [`9122b937ed`](https://github.com/bluesky-social/barre/commit/9122b937ed7e82c811960867d852ef38474b5ec5), GitHub-hosted macOS arm64 runner):
time per `YGNodeCalculateLayout` through the same C API, Yoga built as React Native builds it; **barre/yoga below 1 means barre is faster**.
Shared runners are noisy (treat differences under ~10% as noise). A **MISMATCH** is a layout that differs from Yoga's,
expected where Yoga's compiler contracts float operations into FMA ([yoga#2038](conformance/divergences.md#yoga2038-flexshrink-runaway-from-an-fma-residual), docs/PLAN.md).
History: the [bench-results](https://github.com/bluesky-social/barre/tree/bench-results) branch; methodology: [docs/BENCH.md](docs/BENCH.md).

- barre: `9122b937ed7e` (cargo --release, CARGO_PROFILE_RELEASE_LTO=fat, CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1), rustc 1.99.1 (58cf8f9c3 2026-10-13)
- yoga: `vendor/yoga` (pin 52f6d0bd), `-std=c++20 -gline-tables-only -O2 -DNDEBUG -flto=thin -fno-omit-frame-pointer -fexceptions -frtti -ffunction-sections -fdata-sections -fPIC`, Apple clang version 21.0.0 (clang-2100.1.1.101)
- Apple M1 (Virtual), 26.6.2, load avg at end 14.3/29.6/18.3
- 5 interleaved rounds × 300 ms per workload per engine (+100 ms warmup); times are ns per YGNodeCalculateLayout (or per op for `micro/`, per build+layout+free for `yoga/*` blocks)
- geometric mean barre/yoga over 35 workloads: **0.70×**

| workload | yoga median (p10–p90) | barre median (p10–p90) | barre/yoga | measure calls/layout (y, b) | new-layout nodes (y, b) | check |
|---|---|---|---|---|---|---|
| `yoga/stack_with_flex` | 4.32 µs (4.04 µs–10.9 µs) | 2.67 µs (2.56 µs–6.94 µs) | **0.62×** | 0 | - | ok |
| `yoga/align_stretch_undefined_axis` | 3.85 µs (3.68 µs–9.84 µs) | 2.6 µs (2.51 µs–6.84 µs) | **0.68×** | 10 | - | ok |
| `yoga/nested_flex` | 79.9 µs (72.4 µs–194 µs) | 46.7 µs (42.8 µs–117 µs) | **0.58×** | 100 | - | ok |
| `yoga/huge_nested` | 5.53 ms (4.37 ms–9.72 ms) | 6.54 ms (3.54 ms–10.7 ms) | **1.18×** | 0 | - | ok |
| `yoga/huge_nested.layout_only` | 3.92 ms (3.17 ms–6.34 ms) | 2.84 ms (2.47 ms–4.64 ms) | **0.72×** | 0 | - | ok |
| `capture/feed-android` | 1.1 ms (975 µs–2.08 ms) | 514 µs (460 µs–1.06 ms) | **0.47×** | 57 | - | ok |
| `capture/feed-android.clean` | 33 ns (32 ns–100 ns) | 24 ns (24 ns–77 ns) | **0.74×** | 0 | - | ok |
| `capture/profile-ios` | 201 µs (178 µs–471 µs) | 90.7 µs (82 µs–233 µs) | **0.45×** | 13 | - | ok |
| `capture/profile-ios.clean` | 33 ns (31 ns–100 ns) | 24 ns (23 ns–78 ns) | **0.74×** | 0 | - | ok |
| `capture/rendering-sample-mac` | 84.6 µs (75.5 µs–178 µs) | 40.4 µs (36.2 µs–107 µs) | **0.48×** | 24 | - | ok |
| `capture/rendering-sample-mac.clean` | 31 ns (30 ns–66 ns) | 25 ns (24 ns–78 ns) | **0.79×** | 0 | - | ok |
| `capture/chat-mac` | 5.8 ms (5.18 ms–8.86 ms) | 3.27 ms (2.66 ms–6.02 ms) | **0.56×** | 162 | - | ok |
| `capture/chat-mac.clean` | 32 ns (30 ns–51 ns) | 24 ns (24 ns–63 ns) | **0.76×** | 0 | - | ok |
| `rn/feed50/fresh` | 2.7 ms (2.26 ms–5.25 ms) | 1.17 ms (1.03 ms–2.42 ms) | **0.43×** | 548 | 1451 | ok |
| `rn/feed50/clean` | 32 ns (31 ns–102 ns) | 26 ns (24 ns–79 ns) | **0.79×** | 0 | 1 | ok |
| `rn/feed50/text_change` | 55.5 µs (52.6 µs–147 µs) | 32.2 µs (30.2 µs–57.5 µs) | **0.58×** | 1 | 63 | ok |
| `rn/feed50/text_same_size` | 52.6 µs (49.8 µs–83.5 µs) | 19 µs (16.5 µs–49.6 µs) | **0.36×** | 1 | 62 | ok |
| `rn/feed50/rotate` | 408 µs (375 µs–569 µs) | 407 µs (358 µs–680 µs) | **1.00×** | 0 | 1101 | ok |
| `rn/feed50/resize` | 2.4 ms (2.12 ms–4.13 ms) | 1.16 ms (978 µs–1.91 ms) | **0.48×** | 135/136/139, 144/148 | 1077/1078/1080 | **MISMATCH**: 27 nodes differ, max Δ 0.333344 at /0/0/6/0/0/1/0/2; measure calls differ [148, 136, 139, 135] vs [148, 148, 148, 144] |
| `rn/feed50/insert_remove` | 45.3 µs (40.8 µs–116 µs) | 26.2 µs (24.5 µs–75 µs) | **0.58×** | 0 | 52/54 | ok |
| `rn/deep5/fresh` | 19.2 µs (18.5 µs–47.5 µs) | 8.67 µs (8.42 µs–11 µs) | **0.45×** | 17 | 12 | ok |
| `rn/deep10/fresh` | 251 µs (233 µs–378 µs) | 106 µs (100 µs–131 µs) | **0.42×** | 160 | 22 | **MISMATCH**: 1 nodes differ, max Δ 0.333328 at /0/1/1/1/1/0 |
| `rn/deep15/fresh` | 4.17 ms (3.87 ms–5.49 ms) | 1.56 ms (1.46 ms–2.07 ms) | **0.37×** | 4356, 4382 | 32 | ok; measure calls differ [4356, 4356, 4356, 4356] vs [4382, 4382, 4382, 4382] |
| `rn/deep20/fresh` | 50.6 ms (47.8 ms–77.4 ms) | 19.4 ms (17.5 ms–27 ms) | **0.38×** | 66064, 66937 | 42 | **MISMATCH**: 6 nodes differ, max Δ 0.333374 at /0/1/1/1/1/1/1/1/1/1/1/1/0; measure calls differ [66064, 66064, 66064, 66064] vs [66937, 66937, 66937, 66937] |
| `rn/deep20/clean` | 32 ns (30 ns–40 ns) | 24 ns (23 ns–78 ns) | **0.78×** | 0 | 1 | **MISMATCH**: 6 nodes differ, max Δ 0.333374 at /0/1/1/1/1/1/1/1/1/1/1/1/0 |
| `rn/wide_column1000/fresh` | 1.52 ms (1.3 ms–2.27 ms) | 794 µs (664 µs–1.27 ms) | **0.52×** | 1000 | 4003 | ok |
| `rn/wide_row200/fresh` | 83.2 µs (75.5 µs–164 µs) | 38.4 µs (36 µs–83.6 µs) | **0.46×** | 400 | 201 | ok |
| `rn/wrap_grid300/fresh` | 749 µs (670 µs–1.3 ms) | 413 µs (375 µs–697 µs) | **0.55×** | 300 | 1203 | ok |
| `rn/absolute100/fresh` | 421 µs (373 µs–639 µs) | 216 µs (196 µs–408 µs) | **0.51×** | 200 | 703 | ok |
| `micro/node_new_free` | 62 ns (59 ns–147 ns) | 91 ns (88 ns–210 ns) | **1.47×** | 0 | - | n/a |
| `micro/style_set_x20` | 69 ns (66 ns–222 ns) | 328 ns (298 ns–878 ns) | **4.73×** | 0 | - | n/a |
| `micro/style_get_x20` | 12 ns (11 ns–33 ns) | 19 ns (17 ns–51 ns) | **1.55×** | 0 | - | n/a |
| `micro/insert_remove_child` | 24 ns (22 ns–63 ns) | 37 ns (34 ns–102 ns) | **1.54×** | 0 | - | n/a |
| `micro/build_feed50_tree` | 197 µs (177 µs–468 µs) | 293 µs (256 µs–683 µs) | **1.49×** | 0 | - | ok |
| `micro/layout_get_feed50` | 6.8 µs (6.49 µs–14.6 µs) | 10.5 µs (10 µs–24 µs) | **1.54×** | 0 | - | n/a |

<!-- benchmarks:end -->

## Using it

Don't, yet (see above). To try it:

```sh
git clone https://github.com/bluesky-social/barre && cd barre
git submodule update --init --depth 1     # vendor/yoga and vendor/googletest
cargo build --release -p barre-ffi        # target/release/libbarre_ffi.a
```

- **C API.** `crates/barre-ffi` is a static library exporting the `YG*`
  functions declared in Yoga's own `yoga/Yoga.h` (compile against Yoga's
  headers, link `libbarre_ffi.a` instead of Yoga's library). Measure,
  baseline and dirtied callbacks, context pointers, errata, experimental
  features and the point scale factor behave as Yoga's. Yoga's C++ API
  (`yoga::Node` and friends) is not provided.
- **React Native** (in progress, [docs/RN-SHIM.md](docs/RN-SHIM.md)): RN uses
  Yoga's C++ internals, not just the C API, so the shim keeps Yoga's
  `Node`/`Style`/`Config` and replaces only the layout algorithm with
  barre's, through a node-access trait the algorithm is generic over.
- **Rust.** The `barre` crate is the engine; its API is internal for now
  ([docs/API.md](docs/API.md)).

## Testing

```sh
./conf                                    # Yoga's suite + barre's tests against barre
./conf generated/YGFlexWrapTest           # a test-ID prefix
./conf --yoga                             # the same suite against real Yoga
./spec computeFlexBasisForChild           # print a Yoga function (the spec)
bench/run                                 # barre vs Yoga through the C API (macOS)
cargo clippy --workspace --all-targets -- -D warnings
```

Layout behaviour is tested by the suite only; the few Rust tests check
internals the suite cannot reach.

## How it is built

- **Yoga's source is the spec.** Algorithm functions keep Yoga's names in
  snake_case (`compute_flex_basis_for_child`), in files mirroring Yoga's
  (`algorithm/CalculateLayout.cpp` → `crates/barre/src/algorithm/calculate_layout.rs`),
  with Yoga's control flow and float arithmetic. Where barre deliberately
  differs, the code says so and why.
- **Never fake progress.** Anything unimplemented calls
  `not_implemented!(...)`, which aborts and counts as a failure.
  Special-cased tests, hardcoded results and swallowed errors are caught by
  mechanical gates and a blind verifier that never sees the worker's
  explanation.
- **Landing.** Accepted work goes onto a stack of pull requests; CI runs
  the full suite on each layer against the layer below it, and layers merge
  into `main` from the bottom ([docs/STACKS.md](docs/STACKS.md)).

```sh
./orch run       # work through the ticket pool (needs the claude CLI)
./orch status    # tickets, outcomes and cost
./orch stack     # the stack of PRs and their CI
```

[docs/PLAN.md](docs/PLAN.md) has the goals, design and milestones,
[docs/HARNESS.md](docs/HARNESS.md) how to operate the harness,
[docs/BENCH.md](docs/BENCH.md) the benchmarks and
[docs/YOGA-ISSUES.md](docs/YOGA-ISSUES.md) the audit of Yoga's issue tracker.

## Layout

| Path | Contents |
|---|---|
| `crates/barre` | The engine: nodes, style, config, the layout algorithm |
| `crates/barre-ffi` | Yoga's C API over it, as a static library |
| `crates/barre-rn`, `rn/` | The React Native shim (in progress) |
| `harness/` | Conformance runner, orchestrator, gates, verifier, spec lookup |
| `conformance/` | The ratchet, the skip list, barre's own tests, documented Yoga bugs |
| `bench/` | barre vs Yoga benchmarks |
| `tickets/` | The ticket pool |
| `vendor/` | Pinned Yoga and googletest (submodules) |

## Contributing

Read [CLAUDE.md](CLAUDE.md) and [docs/HARNESS.md](docs/HARNESS.md) first.
`vendor/`, `harness/`, `conformance/`, `tickets/`, `docs/`, the Cargo
manifests and `CLAUDE.md` are architect-only.
