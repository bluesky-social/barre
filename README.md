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
| [barre's own tests](conformance/tests) <!-- barre --> | 95 / 95 | 100.00% | the internals-only Yoga tests ported to the C API, and regression tests for Yoga bugs |
| [Yoga bugs fixed](conformance/divergences.md) <!-- yoga-bugs --> | 34 / 34 | 100.00% | tests that real Yoga fails by design, in 10 files; the same runner linked against Yoga passes everything else in scope (813 tests) |

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

Benchmarks from the last run on main (2026-10-17, barre [`1854783423`](https://github.com/bluesky-social/barre/commit/1854783423e4198b8da6c9096760ca63877f2f37), GitHub-hosted macOS arm64 runner):
time per `YGNodeCalculateLayout` through the same C API, Yoga built as React Native builds it; **barre/yoga below 1 means barre is faster**.
Shared runners are noisy (treat differences under ~10% as noise). A **MISMATCH** is a layout that differs from Yoga's,
expected where Yoga's compiler contracts float operations into FMA ([yoga#2038](conformance/divergences.md#yoga2038-flexshrink-runaway-from-an-fma-residual), docs/PLAN.md).
History: the [bench-results](https://github.com/bluesky-social/barre/tree/bench-results) branch; methodology: [docs/BENCH.md](docs/BENCH.md).

- barre: `1854783423e4` (cargo --release, CARGO_PROFILE_RELEASE_LTO=fat, CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1), rustc 1.99.1 (58cf8f9c3 2026-10-13)
- yoga: `vendor/yoga` (pin 52f6d0bd), `-std=c++20 -gline-tables-only -O2 -DNDEBUG -flto=thin -fno-omit-frame-pointer -fexceptions -frtti -ffunction-sections -fdata-sections -fPIC`, Apple clang version 21.0.0 (clang-2100.1.1.101)
- Apple M1 (Virtual), 26.6.2, load avg at end 8.1/28.9/19.0
- 5 interleaved rounds × 300 ms per workload per engine (+100 ms warmup); times are ns per YGNodeCalculateLayout (or per op for `micro/`, per build+layout+free for `yoga/*` blocks)
- geometric mean barre/yoga over 35 workloads: **0.90×**

| workload | yoga median (p10–p90) | barre median (p10–p90) | barre/yoga | measure calls/layout (y, b) | new-layout nodes (y, b) | check |
|---|---|---|---|---|---|---|
| `yoga/stack_with_flex` | 4.1 µs (4.04 µs–5.44 µs) | 3.09 µs (3.07 µs–7.85 µs) | **0.75×** | 0 | - | ok |
| `yoga/align_stretch_undefined_axis` | 3.69 µs (3.66 µs–5.4 µs) | 2.99 µs (2.97 µs–4.14 µs) | **0.81×** | 10 | - | ok |
| `yoga/nested_flex` | 72.9 µs (72.2 µs–112 µs) | 58.9 µs (58.4 µs–146 µs) | **0.81×** | 100 | - | ok |
| `yoga/huge_nested` | 4.17 ms (3.76 ms–7.9 ms) | 4.09 ms (3.49 ms–7.91 ms) | **0.98×** | 0 | - | ok |
| `yoga/huge_nested.layout_only` | 2.97 ms (2.74 ms–6.06 ms) | 2.17 ms (1.86 ms–4.72 ms) | **0.73×** | 0 | - | ok |
| `capture/feed-android` | 983 µs (944 µs–1.87 ms) | 574 µs (543 µs–1.4 ms) | **0.58×** | 57 | - | ok |
| `capture/feed-android.clean` | 32 ns (32 ns–100 ns) | 49 ns (48 ns–119 ns) | **1.51×** | 0 | - | ok |
| `capture/profile-ios` | 180 µs (175 µs–303 µs) | 112 µs (110 µs–173 µs) | **0.62×** | 13 | - | ok |
| `capture/profile-ios.clean` | 31 ns (30 ns–72 ns) | 49 ns (48 ns–119 ns) | **1.59×** | 0 | - | ok |
| `capture/rendering-sample-mac` | 75.5 µs (75.1 µs–97.7 µs) | 46.2 µs (45.4 µs–101 µs) | **0.61×** | 24 | - | ok |
| `capture/rendering-sample-mac.clean` | 31 ns (30 ns–98 ns) | 49 ns (48 ns–88 ns) | **1.57×** | 0 | - | ok |
| `capture/chat-mac` | 5.8 ms (5.19 ms–9.92 ms) | 3.64 ms (3.26 ms–6.75 ms) | **0.63×** | 162 | - | ok |
| `capture/chat-mac.clean` | 31 ns (30 ns–100 ns) | 52 ns (48 ns–167 ns) | **1.68×** | 0 | - | ok |
| `rn/feed50/fresh` | 2.38 ms (2.28 ms–4.45 ms) | 1.6 ms (1.47 ms–3.29 ms) | **0.67×** | 548 | 1451 | ok |
| `rn/feed50/clean` | 31 ns (30 ns–99 ns) | 48 ns (48 ns–118 ns) | **1.57×** | 0 | 1 | ok |
| `rn/feed50/text_change` | 53 µs (52.3 µs–145 µs) | 38.5 µs (38.2 µs–49.8 µs) | **0.73×** | 1 | 63 | ok |
| `rn/feed50/text_same_size` | 50 µs (49.5 µs–64.8 µs) | 24.1 µs (23.8 µs–60.9 µs) | **0.48×** | 1 | 62 | ok |
| `rn/feed50/rotate` | 384 µs (370 µs–540 µs) | 519 µs (498 µs–966 µs) | **1.35×** | 0 | 1101 | ok |
| `rn/feed50/resize` | 2.27 ms (2.14 ms–4.42 ms) | 1.5 ms (1.42 ms–2.95 ms) | **0.66×** | 135/136/139, 144/148 | 1077/1078/1080 | **MISMATCH**: 27 nodes differ, max Δ 0.333344 at /0/0/6/0/0/1/0/2; measure calls differ [148, 136, 139, 135] vs [148, 148, 148, 144] |
| `rn/feed50/insert_remove` | 42.2 µs (40.2 µs–63 µs) | 30.8 µs (29.6 µs–85.3 µs) | **0.73×** | 0 | 52/54 | ok |
| `rn/deep5/fresh` | 18.6 µs (18.5 µs–30.2 µs) | 12.3 µs (12.1 µs–29.5 µs) | **0.66×** | 17 | 12 | ok |
| `rn/deep10/fresh` | 237 µs (231 µs–308 µs) | 149 µs (143 µs–255 µs) | **0.63×** | 160 | 22 | **MISMATCH**: 1 nodes differ, max Δ 0.333328 at /0/1/1/1/1/0 |
| `rn/deep15/fresh` | 4.09 ms (3.79 ms–7.56 ms) | 2.13 ms (2.01 ms–3.82 ms) | **0.52×** | 4356, 4382 | 32 | ok; measure calls differ [4356, 4356, 4356, 4356] vs [4382, 4382, 4382, 4382] |
| `rn/deep20/fresh` | 54.5 ms (48.1 ms–60.4 ms) | 29.5 ms (23.9 ms–36.6 ms) | **0.54×** | 66064, 66937 | 42 | **MISMATCH**: 6 nodes differ, max Δ 0.333374 at /0/1/1/1/1/1/1/1/1/1/1/1/0; measure calls differ [66064, 66064, 66064, 66064] vs [66937, 66937, 66937, 66937] |
| `rn/deep20/clean` | 31 ns (30 ns–61 ns) | 48 ns (48 ns–119 ns) | **1.57×** | 0 | 1 | **MISMATCH**: 6 nodes differ, max Δ 0.333374 at /0/1/1/1/1/1/1/1/1/1/1/1/0 |
| `rn/wide_column1000/fresh` | 1.36 ms (1.29 ms–2.44 ms) | 865 µs (775 µs–2.49 ms) | **0.64×** | 1000 | 4003 | ok |
| `rn/wide_row200/fresh` | 77.5 µs (75.7 µs–207 µs) | 47.2 µs (46.4 µs–126 µs) | **0.61×** | 400 | 201 | ok |
| `rn/wrap_grid300/fresh` | 682 µs (648 µs–1.25 ms) | 455 µs (439 µs–628 µs) | **0.67×** | 300 | 1203 | ok |
| `rn/absolute100/fresh` | 379 µs (361 µs–509 µs) | 256 µs (245 µs–356 µs) | **0.68×** | 200 | 703 | ok |
| `micro/node_new_free` | 62 ns (59 ns–148 ns) | 87 ns (86 ns–206 ns) | **1.39×** | 0 | - | n/a |
| `micro/style_set_x20` | 67 ns (66 ns–99 ns) | 91 ns (88 ns–114 ns) | **1.36×** | 0 | - | n/a |
| `micro/style_get_x20` | 11 ns (11 ns–14 ns) | 17 ns (17 ns–26 ns) | **1.54×** | 0 | - | n/a |
| `micro/insert_remove_child` | 23 ns (22 ns–62 ns) | 35 ns (34 ns–95 ns) | **1.52×** | 0 | - | n/a |
| `micro/build_feed50_tree` | 189 µs (176 µs–459 µs) | 252 µs (230 µs–614 µs) | **1.33×** | 0 | - | ok |
| `micro/layout_get_feed50` | 6.88 µs (6.57 µs–14.1 µs) | 11 µs (10.6 µs–24.5 µs) | **1.60×** | 0 | - | n/a |

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
