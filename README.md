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

Benchmarks from the last run on main (2026-10-17, barre [`52d96baf8c`](https://github.com/bluesky-social/barre/commit/52d96baf8c9960fe328e5ae9e0acf113babb95a9), GitHub-hosted macOS arm64 runner):
time per `YGNodeCalculateLayout` through the same C API, Yoga built as React Native builds it; **barre/yoga below 1 means barre is faster**.
Shared runners are noisy (treat differences under ~10% as noise). A **MISMATCH** is a layout that differs from Yoga's,
expected where Yoga's compiler contracts float operations into FMA ([yoga#2038](conformance/divergences.md#yoga2038-flexshrink-runaway-from-an-fma-residual), docs/PLAN.md).
History: the [bench-results](https://github.com/bluesky-social/barre/tree/bench-results) branch; methodology: [docs/BENCH.md](docs/BENCH.md).

- barre: `52d96baf8c99` (cargo --release, CARGO_PROFILE_RELEASE_LTO=fat, CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1), rustc 1.99.1 (58cf8f9c3 2026-10-13)
- yoga: `vendor/yoga` (pin 52f6d0bd), `-std=c++20 -gline-tables-only -O2 -DNDEBUG -flto=thin -fno-omit-frame-pointer -fexceptions -frtti -ffunction-sections -fdata-sections -fPIC`, Apple clang version 21.0.0 (clang-2100.1.1.101)
- Apple M1 (Virtual), 26.6.2, load avg at end 12.9/17.5/10.7
- 5 interleaved rounds × 300 ms per workload per engine (+100 ms warmup); times are ns per YGNodeCalculateLayout (or per op for `micro/`, per build+layout+free for `yoga/*` blocks)
- geometric mean barre/yoga over 35 workloads: **0.66×**

| workload | yoga median (p10–p90) | barre median (p10–p90) | barre/yoga | measure calls/layout (y, b) | new-layout nodes (y, b) | check |
|---|---|---|---|---|---|---|
| `yoga/stack_with_flex` | 4.12 µs (4.06 µs–5.25 µs) | 2.57 µs (2.53 µs–4.18 µs) | **0.62×** | 0 | - | ok |
| `yoga/align_stretch_undefined_axis` | 3.77 µs (3.72 µs–7.49 µs) | 2.52 µs (2.48 µs–4.57 µs) | **0.67×** | 10 | - | ok |
| `yoga/nested_flex` | 73.5 µs (72.3 µs–113 µs) | 42.1 µs (40.5 µs–56 µs) | **0.57×** | 100 | - | ok |
| `yoga/huge_nested` | 4.59 ms (4.02 ms–6.51 ms) | 4.92 ms (3.56 ms–8 ms) | **1.07×** | 0 | - | ok |
| `yoga/huge_nested.layout_only` | 3.48 ms (2.97 ms–5.43 ms) | 2.53 ms (1.82 ms–3.39 ms) | **0.73×** | 0 | - | ok |
| `capture/feed-android` | 978 µs (930 µs–1.62 ms) | 452 µs (428 µs–654 µs) | **0.46×** | 57 | - | ok |
| `capture/feed-android.clean` | 32 ns (32 ns–46 ns) | 20 ns (19 ns–59 ns) | **0.61×** | 0 | - | ok |
| `capture/profile-ios` | 188 µs (175 µs–270 µs) | 83.1 µs (79.7 µs–122 µs) | **0.44×** | 13 | - | ok |
| `capture/profile-ios.clean` | 31 ns (30 ns–41 ns) | 20 ns (19 ns–24 ns) | **0.63×** | 0 | - | ok |
| `capture/rendering-sample-mac` | 77.1 µs (72.8 µs–105 µs) | 36 µs (34.2 µs–57.2 µs) | **0.47×** | 24 | - | ok |
| `capture/rendering-sample-mac.clean` | 31 ns (30 ns–42 ns) | 20 ns (19 ns–59 ns) | **0.65×** | 0 | - | ok |
| `capture/chat-mac` | 5.37 ms (5.05 ms–9.11 ms) | 2.72 ms (2.49 ms–5.45 ms) | **0.51×** | 162 | - | ok |
| `capture/chat-mac.clean` | 32 ns (30 ns–104 ns) | 19 ns (19 ns–58 ns) | **0.61×** | 0 | - | ok |
| `rn/feed50/fresh` | 2.35 ms (2.26 ms–4.17 ms) | 1.07 ms (988 µs–1.5 ms) | **0.45×** | 548 | 1451 | ok |
| `rn/feed50/clean` | 31 ns (30 ns–38 ns) | 20 ns (19 ns–29 ns) | **0.64×** | 0 | 1 | ok |
| `rn/feed50/text_change` | 53.2 µs (50.5 µs–65.9 µs) | 29.3 µs (28.6 µs–41.2 µs) | **0.55×** | 1 | 63 | ok |
| `rn/feed50/text_same_size` | 51.1 µs (49.6 µs–70.5 µs) | 15.7 µs (15.2 µs–21.8 µs) | **0.31×** | 1 | 62 | ok |
| `rn/feed50/rotate` | 393 µs (368 µs–536 µs) | 349 µs (321 µs–522 µs) | **0.89×** | 0 | 1101 | ok |
| `rn/feed50/resize` | 2.29 ms (2.07 ms–2.96 ms) | 994 µs (947 µs–1.39 ms) | **0.43×** | 135/136/139, 144/148 | 1077/1078/1080 | **MISMATCH**: 27 nodes differ, max Δ 0.333344 at /0/0/6/0/0/1/0/2; measure calls differ [148, 136, 139, 135] vs [148, 148, 148, 144] |
| `rn/feed50/insert_remove` | 42.5 µs (40.2 µs–52.9 µs) | 23.5 µs (22.8 µs–29.8 µs) | **0.55×** | 0 | 52/54 | ok |
| `rn/deep5/fresh` | 18.7 µs (18.4 µs–23.6 µs) | 8.33 µs (8.17 µs–10.5 µs) | **0.45×** | 17 | 12 | ok |
| `rn/deep10/fresh` | 248 µs (232 µs–323 µs) | 101 µs (98 µs–125 µs) | **0.41×** | 160 | 22 | **MISMATCH**: 1 nodes differ, max Δ 0.333328 at /0/1/1/1/1/0 |
| `rn/deep15/fresh` | 3.93 ms (3.69 ms–5.23 ms) | 1.54 ms (1.42 ms–2.07 ms) | **0.39×** | 4356, 4382 | 32 | ok; measure calls differ [4356, 4356, 4356, 4356] vs [4382, 4382, 4382, 4382] |
| `rn/deep20/fresh` | 52.9 ms (46.9 ms–79.1 ms) | 18.9 ms (17.7 ms–28.1 ms) | **0.36×** | 66064, 66937 | 42 | **MISMATCH**: 6 nodes differ, max Δ 0.333374 at /0/1/1/1/1/1/1/1/1/1/1/1/0; measure calls differ [66064, 66064, 66064, 66064] vs [66937, 66937, 66937, 66937] |
| `rn/deep20/clean` | 31 ns (30 ns–38 ns) | 20 ns (19 ns–25 ns) | **0.63×** | 0 | 1 | **MISMATCH**: 6 nodes differ, max Δ 0.333374 at /0/1/1/1/1/1/1/1/1/1/1/1/0 |
| `rn/wide_column1000/fresh` | 1.36 ms (1.29 ms–1.8 ms) | 786 µs (650 µs–1.02 ms) | **0.58×** | 1000 | 4003 | ok |
| `rn/wide_row200/fresh` | 78.2 µs (74.8 µs–102 µs) | 36.2 µs (35.7 µs–45.1 µs) | **0.46×** | 400 | 201 | ok |
| `rn/wrap_grid300/fresh` | 694 µs (648 µs–1.17 ms) | 397 µs (368 µs–743 µs) | **0.57×** | 300 | 1203 | ok |
| `rn/absolute100/fresh` | 413 µs (366 µs–703 µs) | 197 µs (180 µs–314 µs) | **0.48×** | 200 | 703 | ok |
| `micro/node_new_free` | 62 ns (60 ns–146 ns) | 90 ns (87 ns–152 ns) | **1.46×** | 0 | - | n/a |
| `micro/style_set_x20` | 68 ns (65 ns–92 ns) | 305 ns (298 ns–425 ns) | **4.49×** | 0 | - | n/a |
| `micro/style_get_x20` | 12 ns (11 ns–33 ns) | 18 ns (17 ns–44 ns) | **1.51×** | 0 | - | n/a |
| `micro/insert_remove_child` | 23 ns (22 ns–38 ns) | 34 ns (34 ns–101 ns) | **1.52×** | 0 | - | n/a |
| `micro/build_feed50_tree` | 183 µs (176 µs–379 µs) | 265 µs (249 µs–476 µs) | **1.45×** | 0 | - | ok |
| `micro/layout_get_feed50` | 6.91 µs (6.5 µs–14.5 µs) | 10.4 µs (9.92 µs–24.6 µs) | **1.51×** | 0 | - | n/a |

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
