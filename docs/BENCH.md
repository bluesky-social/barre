# Benchmarks (`bench/`)

Milestone 4 (docs/PLAN.md): barre against real Yoga, **through the identical C
API**. One C++ benchmark source, compiled once, linked twice: against
`vendor/yoga` built the way React Native builds it, and against `barre-ffi`.
Every workload checks that both engines computed the same layout, and counts
measure-func calls and `hasNewLayout` nodes, because in RN the cache
behaviour matters as much as raw speed.

```
bench/run                      # build both, 5 interleaved rounds, table + JSON
bench/run --quick              # 2 rounds × 60 ms, ~30 s
bench/run rn/feed50 capture/   # name-prefix filter
bench/run --rev HEAD~3         # benchmark another barre commit
bench/run --worktree           # the live working tree instead of a commit
bench/profile rn/feed50/fresh  # samply profile of barre, hotspot tables
```

Latest full run: [`bench/results/latest.md`](../bench/results/latest.md).
Raw results (`bench/results/*.json`, `*.md`) are gitignored except
`latest.md`, which only full unfiltered runs overwrite.

## How it is built

| | Yoga | barre |
|---|---|---|
| source | `vendor/yoga/yoga/**/*.cpp` (pin 52f6d0bd) | `git archive <rev> crates Cargo.*` into `bench/build/src/<sha>`, so the swarm's uncommitted edits never leak in (`--worktree` to override) |
| flags | RN Android's: `-std=c++20 -O2 -DNDEBUG -flto=thin -fno-omit-frame-pointer -fexceptions -frtti -ffunction-sections -fdata-sections -fPIC`, `-Wl,-dead_strip` | `cargo build --release -p barre-ffi` with `CARGO_PROFILE_RELEASE_LTO=fat CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1` (env, so no manifest change; `--cargo-env K=V` to vary), `CARGO_TARGET_DIR=bench/build/target` |
| benchmark objects | `bench/src/{bench,capture}.cpp` + Yoga's own `benchmark/TreeDeserialization.cpp`, same flags, compiled once, shared by both links | same objects |

Where the flags come from: RN's `ReactCommon/yoga/CMakeLists.txt` forces
`CMAKE_BUILD_TYPE Release` (`-O3 -DNDEBUG`, then `cmake/project-defaults.cmake`
appends `-O2`, which wins) and `yoga/CMakeLists.txt` turns on IPO, which is
`-flto=thin` for Clang. **iOS is different**: `Yoga.podspec` sets no
optimisation flags and social-app's `Pods.xcodeproj` sets
`GCC_OPTIMIZATION_LEVEL` only for Debug (`0`), so Release gets Xcode's default
`-Os`, without LTO. `bench/run --yoga-cflags "-Os -DNDEBUG -fno-omit-frame-pointer -fexceptions -frtti -fPIC"`
reproduces that (see Findings: Yoga is ~1.6× slower there).

`-gline-tables-only` is added on the C++ side (no codegen effect) and the
ThinLTO objects are kept (`-object_path_lto`) so `bench/profile` can resolve
inlined frames. `bench/run` checks that the Yoga binary has no Rust in it and
the barre binary has barre's symbols and no `facebook::yoga::Node`.

Notes on fairness: under LTO, Yoga's C API (getters, setters,
`YGNodeCalculateLayout`) inlines into the benchmark; barre's can't (no
cross-language LTO: rustc ships LLVM 22, Apple clang is LLVM-21-based). That
only shows in `micro/` (getters ~1.8×). It is irrelevant to the RN plan
(docs/RN-SHIM.md §3c), where RN keeps `yoga::Node` and only
`calculateLayout` crosses into Rust. Both sides target the default
`apple-m1` CPU.

## Methodology

- `bench/src/bench.cpp --run NAME` runs one workload in its own process and
  prints JSON. `bench/run` runs every workload on both engines, **interleaved**
  (yoga/barre order alternates per workload and round), for N rounds, and
  merges the samples: median, p10, p90 of ns per layout.
- The thread asks for `QOS_CLASS_USER_INTERACTIVE` (macOS has no core
  pinning; this keeps it on performance cores in practice).
- Per workload: `setup` (untimed), a **check phase** of exactly 4
  iterations (identical on both engines: checksum, measure calls and
  `hasNewLayout` nodes per iteration, layout dump), 100 ms warmup, then a
  300 ms timed phase (`--quick`: 20 + 60 ms).
- The timed region is one `YGNodeCalculateLayout` call. Tree building,
  mutations (`YGNodeMarkDirty`, insert/remove, root resize) and RN's readback
  happen untimed in `prepare`/`after`. Workloads with no per-iteration setup
  (clean relayouts, `micro/`, Yoga's blocks) are timed in batches doubled
  until a batch spans ≥ 20 µs (clock ticks are 41.7 ns on Apple silicon).
- **Checksum**: FNV-1a over the bits of every node's layout left/top/width/
  height + child counts, after the check phase. On a mismatch `bench/run`
  diffs the two layout dumps (`bench/build/dumps/`) and reports how many
  nodes differ, the largest difference, and where. Differing measure-call or
  `hasNewLayout` counts are reported in the same column. Nothing is hidden.
- **RN readback**: RN workloads mimic `YogaLayoutableShadowNode::layout()`
  after every layout: walk children flagged `hasNewLayout`, clear the flag,
  read metrics, recurse only into flagged ones. The count is the "new-layout
  nodes" column, i.e. how many `onLayout`/mount updates RN would do; barre
  must flag exactly Yoga's set (docs/RN-SHIM.md §2.4.5).
- RN trees use one `YGConfig` per node (RN gives every shadow node its own),
  point scale 3, errata `1|2|4|8` (what RN 0.88's Yoga knows), and the
  surface constraints as root min/max size + `YGNodeCalculateLayout(root, w, h)`,
  exactly like `YogaLayoutableShadowNode::layoutTree`.
- Measure funcs are **cheap and deterministic** (text: glyph count × width,
  greedy wrap to the available width). In RN each call is a real text layout
  (the captures record 10–30 µs each), so the measure-call column is the
  real-world cost multiplier, not the ns column: `rn/feed50/fresh`'s 548
  calls would be ~8 ms of text layout against ~1.4 ms of flexbox.
- Noise: the orchestrator swarm runs concurrently on the same machine (load
  average 5–9 during the recorded runs). Interleaving cancels drift between
  engines, but treat ratio differences under ~5% as noise.

## Workloads

| group | workload | what |
|---|---|---|
| `yoga/` | `stack_with_flex`, `align_stretch_undefined_axis`, `nested_flex`, `huge_nested` | Yoga's `YGBenchmark.c`, verbatim; each sample is build + layout + free, as Yoga times it |
| | `huge_nested.layout_only` | same 11 111-node tree, layout only |
| `capture/` | `feed-android`, `profile-ios`, `rendering-sample-mac`, `chat-mac` | Yoga's `benchmark/captures/*.json` (real app trees, 101–1516 nodes), rebuilt per iteration through the C API exactly as `Benchmark.cpp` does; recorded measure outputs are replayed **without** the recorded `sleep_for`. A measure request that was never recorded counts as a miss (none so far) |
| | `*.clean` | relayout of the same tree with nothing dirty |
| `rn/feed50/` | `fresh` | social-app-like feed: surface → ScrollView → 50 FlatList cells, each a post (repost header every 5th, avatar column, meta row of shrinking name/handle/time texts, post text, an embed: image with aspect ratio / link card / quote post, action row with `space-between` and gaps). 1451 nodes, ~500 texts |
| | `clean` | relayout, nothing dirty |
| | `text_change` | cell 25's post text toggles 120 ↔ 200 glyphs (line count changes), `YGNodeMarkDirty` |
| | `text_same_size` | 120 ↔ 121 glyphs: dirty, but the measured size doesn't change |
| | `rotate` | surface toggles 390×844 ↔ 844×390 |
| | `resize` | width steps through 40 values (window drag): more distinct constraints than the 8-entry measure cache |
| | `insert_remove` | cell 25 alternately removed / reinserted |
| `rn/` | `deep{5,10,15,20}/fresh`, `deep20/clean` | nested views alternating row/column, each level with a shrinkable text sibling |
| | `wide_column1000/fresh` | settings-style list, 1000 rows × (icon, flex text, chevron) |
| | `wide_row200/fresh` | 200 shrinking/growing texts on one line |
| | `wrap_grid300/fresh` | 300 photo tiles: `flexWrap`, 31% width, `aspectRatio: 1`, gaps, absolute fill + caption |
| | `absolute100/fresh` | media cards with absolute badge / gradient / centred play button (% insets, negative margins) |
| `micro/` | `node_new_free`, `style_set_x20`, `style_get_x20`, `insert_remove_child`, `build_feed50_tree`, `layout_get_feed50` | C API costs: per op, per 20 setters/getters, per feed tree built (no layout), per feed read back |

## Results

`ece98bb` (2026-10-17, after T-0007 and T-0009; T-0006, the stale flex
basis fix, had not landed), Apple M4 Pro, 5 rounds × 300 ms. Full table:
[`bench/results/latest.md`](../bench/results/latest.md). Selected rows:

| workload | yoga | barre | barre/yoga | measure calls/layout | new-layout nodes |
|---|---|---|---|---|---|
| `capture/feed-android` | 642 µs | 589 µs | 0.92× | 57 | |
| `capture/chat-mac` | 3.46 ms | 3.20 ms | 0.92× | 162 | |
| `yoga/huge_nested.layout_only` | 1.69 ms | 1.52 ms | 0.90× | 0 | |
| `rn/feed50/fresh` | 1.59 ms | 1.36 ms | 0.86× | 548 | 1451 |
| `rn/feed50/text_change` | 33.4 µs | 30.8 µs | 0.92× | 1 | 63 |
| `rn/feed50/text_same_size` | 31.2 µs | 29.0 µs | 0.93× | 1 | 62 |
| `rn/feed50/rotate` | 258 µs | 217 µs | 0.84× | 0 | 1101 |
| `rn/feed50/resize` | 1.48 ms | 1.30 ms | 0.87× | 135–139 | ~1078 |
| `rn/feed50/insert_remove` | 25.5 µs | 24.1 µs | 0.95× | 0 | 52/54 |
| `rn/deep20/fresh` | 26.4 ms | 30.0 ms | 1.14× | 66 064 | 42 |
| `rn/wide_column1000/fresh` | 877 µs | 766 µs | 0.87× | 1000 | 4003 |
| `micro/node_new_free` | 27 ns | 47 ns | 1.78× | | |
| `micro/style_set_x20` | 53 ns | 37 ns | 0.69× | | |
| `micro/build_feed50_tree` | 132 µs | 154 µs | 1.17× | | |
| geometric mean, 35 workloads | | | **0.91×** | | |

## Findings

1. **barre is already slightly faster than RN-Android-built Yoga** on almost
   every layout workload (0.84–0.95×), and slower only on node creation
   (`node_new_free` 1.78×, tree building 1.17×), cross-language getters
   (not RN-relevant) and the pathological `deep20` (1.14×).
2. **Cache behaviour is identical.** Measure-call counts and `hasNewLayout`
   sets match Yoga on every workload, including all five relayout
   scenarios. The measure cache does its job: one text change costs one
   measure call and relays out 63 nodes; rotating back and forth costs zero
   measure calls (both sizes stay in the 8-entry cache), while a window drag
   (`resize`) re-measures ~136 texts per layout.
3. **Checksum mismatches are FMA contraction, not bugs.** `rn/deep10` and
   `rn/deep20` differ by exactly one 1/3-pt pixel on 1–6 text leaves. With
   Yoga built `-ffp-contract=off`, every checksum of every workload matches. Clang contracts
   `a * b + c` into FMA by default on arm64 (docs/API.md "perf notes"
   predicted this; this is the first input that shows it). RN builds Yoga
   with contraction on, on both platforms, so an A/B in social-app will show
   rare 1-physical-pixel differences on text in deep, shrinking rows. Worth a
   decision: accept and document, or find the contracted sites (only inside
   rounding-sensitive arithmetic matters) and use `f32::mul_add` there.
4. **iOS ships a much slower Yoga.** With Yoga at `-Os`, no LTO (what
   social-app's Pods build uses), barre is 0.54× geomean on a 13-workload
   subset (Yoga 1.5–1.8× slower on layouts; `rn/feed50/fresh` 2.28 ms vs
   1.38 ms, `text_change` 46.9 vs 31.0 µs, `resize` 2.23 vs 1.32 ms).
   For M5 on iOS, barre-rn built with `opt-level=3` + LTO wins by that margin
   before any optimisation.
5. **The Cargo profile matters ~5–10%.** Default `release` (no LTO, 16 codegen
   units) vs fat LTO + 1 CGU: `rn/feed50/fresh` 0.95× → 0.86×,
   `capture/feed-android` 1.01× → 0.92×, `yoga/huge_nested` (API-call heavy:
   barre-ffi → barre crate boundary) 1.21× → 1.02×.
6. **Deep alternating nesting is exponential in both engines.** Measure calls
   for `deep{5,10,15,20}`: 17, 160, 4356, 66 064 for 6–21 text nodes (~1.7×
   per level). T-0007 (yoga#2007) didn't change it; this shape misses the
   cache through the flex-basis/stretch/final passes instead. RN's feed is
   fine (548 calls for ~500 texts), so it bites only deep shrinking
   row/column chains.
7. Clean relayouts are an early-out in both (12 vs 21 ns): RN pays nothing
   for an unchanged tree.

## Profiles

`bench/profile WORKLOAD [--engine yoga]` records with
[samply](https://github.com/mstange/samply) at 4 kHz (`xctrace record`
hangs headless here), resolves frames in the engine with `atos -i` against a
`dsymutil`'d copy (fat LTO inlines most of barre into a few symbols; inlined
frames come back as their own frames), demangles with `rustfilt`, and prints
self time by function, inclusive time, and self time by source line, counting
only samples under `YGNodeCalculateLayout`. Install the tools with
`cargo install --locked samply rustfilt --root /tmp/samply-install`
(override with `$SAMPLY_ROOT`). Output also goes to
`bench/build/prof/<engine>-<workload>.md`. Samples attributed to
`mach_absolute_time` in `micro/` profiles are an artefact; ignore them.

Hotspots (feed profiles at `ece98bb`, `deep15` at `34517d7`; percent of samples under `YGNodeCalculateLayout`;
inclusive percentages overlap because the layout recurses):

- **`rn/feed50/fresh`**: `style::compute_edge` 30% inclusive (its
  `edges[e]` closure at `style/mod.rs:1042` alone is 18% self), reached
  through `bound_axis::padding_and_border_for_axis` (22%),
  `Style::compute_margin(_for_axis)` (13–15%) and the `compute_inline_*` /
  `compute_flex_*` padding/border helpers; `resolved_min/max_dimension`
  7%; `round_value_to_pixel_grid_double` 4%; cache lookups
  (`find_cached_result` 8% incl.); `calculate_flex_line` 9% incl., of which
  3% is its `Vec::with_capacity` allocation. Yoga's profile of the same tree
  has the same shape: `Style::resolve` + `computeMargin/Padding/Border` +
  `computeLeft/RightEdge` ≈ 45% self. Edge resolution is *the* cost of
  Yoga's algorithm; barre pays a bit less of it because its values are inline
  instead of behind `StyleValuePool` handles.
- **`rn/feed50/resize`**: same as fresh (`compute_edge` 28%,
  `padding_and_border_for_axis` 21%), plus `find_cached_result` 12% incl.
- **`rn/feed50/text_change`**: `round_layout_results_to_pixel_grid` **47%**
  inclusive (`round_value_to_pixel_grid_double` 17%, the `fmod` replacement
  at `pixel_grid.rs:24` is the hottest line), the rest is the dirty path's
  `calculate_layout_internal`/cache checks on the 50 siblings.
- **`rn/feed50/insert_remove`**: `round_layout_results_to_pixel_grid`
  **62%** inclusive. The rounding pass walks all 1451 nodes after every
  layout even when ~60 changed (Yoga does the same: 22% self in Yoga's
  `text_change` profile).
- **`rn/deep15/fresh`**: `compute_edge` + padding helpers dominate
  (`compute_edge` 26% self, `compute_padding` 7%, `compute_flex_start_padding`
  6%): the exponential re-layouts recompute the same edges.
- **`micro/node_new_free`**: `Box::new(Node::new(config))` builds the
  832-byte node on the stack and `memmove`s it into the box
  (`_platform_memmove` 14% + `node_new`/`Node::new` 26%).

Not hotspots: trait dispatch (the algorithm is monomorphised over
`&Node`, no dynamic calls), `Cell` (compiles to plain loads/stores; it only
appears as a line attribution of inlined reads), allocation in general
(~3% via `calculate_flex_line`).

Sizes (at `34517d7`): `barre::Node` 832 B, `Style` 400 B, `LayoutResults`
320 B; Yoga's `Node` 624 B, `Style` 160 B (+ pool overflow), `LayoutResults`
336 B. In the RN shim the style snapshot + `LayoutResults` live in a blob
memcpy'd on *every* ShadowNode clone (docs/RN-SHIM.md §3c), so their size is
RN memory and clone cost, not just layout cache footprint.

## Optimisation ideas, ranked by expected impact

Each is phrased as a ticket. "Measure" means `bench/run <prefix>` (the check
column must stay `ok` and the measure/new-layout counts unchanged) plus
`./conf` staying green, and `bench/profile` for the share.

1. **Incremental pixel-grid rounding.** `algorithm/pixel_grid.rs`,
   `round_layout_results_to_pixel_grid`. Today it re-rounds the whole tree
   after every layout (as Yoga does), which is 47–62% of an incremental RN
   relayout. Skip a subtree when nothing in it was visited by this layout
   pass **and** its root's absolute left/top equals the one used when it was
   last rounded (store those two values in `LayoutResults`); its stored
   values are then already the rounded result. "Nothing visited" is
   `generation_count ≠ current` for the root *only if* no descendant was
   laid out from above: absolutely positioned descendants are laid out by
   their containing block, which can sit above a cache-hit static parent, so
   either propagate a "subtree touched this generation" bit up from
   `calculate_layout_internal`, or don't skip subtrees that contain
   absolute descendants.
   Must reproduce Yoga's output bit-for-bit (mind yoga#877: cache-hit nodes
   hold rounded values), which the bench checksum and the suite verify.
   Expected: `rn/feed50/insert_remove` and `text_same_size` −50%,
   `text_change` −30% (cells below the edit move and still need rounding),
   no change to fresh layouts. Measure: `bench/run rn/feed50`.
2. **Pre-resolved edges.** `style/mod.rs`, `compute_edge` and the
   `compute_{margin,padding,border,position}` / `compute_inline_*` /
   `compute_flex_*` helpers, used by `bound_axis::padding_and_border_for_axis`.
   The 5-step fallback chain (start/end → left/right → horizontal → all) is
   re-evaluated on every call: ~30% of fresh and resize layouts. Precompute,
   whenever an `Edges` array is set (C API setters; in RN, at
   `style_snapshot` time), the winning index per physical edge × direction
   (8 nibbles per `Edges`), plus an "all undefined" bit so the very common
   no-padding/no-border/no-margin case returns 0 without touching the
   array. `compute_edge` becomes one indexed load. Yoga has the same cost
   (≈45%), so this widens the lead rather than catching up. Expected:
   −15–25% on `rn/feed50/fresh`, `resize`, `rn/deep*`, `capture/*`.
   Measure: `bench/run rn/ capture/`, `bench/profile rn/feed50/fresh`
   (`compute_edge` share).
3. **Compact `Style` (and `LayoutResults`).** `style/mod.rs`
   (`StyleLength` = `f32` + unit byte, padded to 8 B; `Edges` = 9 of them =
   72 B, four `Edges` = 288 B of the 400 B `Style`). Store the 9 values as
   `f32` with units packed into one `u32` per `Edges` (2 bits × 9), or as
   Yoga-style handles; the `CachedMeasurement` array (8 × 20 B) could drop
   to f32 + packed modes. Halving `Style` shrinks the RN blob and the memcpy
   on every ShadowNode clone, and the fresh-layout cache footprint (1451
   nodes × 832 B ≈ 1.2 MB for the feed). Expected: `micro/node_new_free`,
   `micro/build_feed50_tree` to parity; a few % on fresh layouts; RN memory.
   Measure: `bench/run micro/ rn/`, plus `size_of` in the ticket's own
   architect check. Combine with (2): the resolved-edge table fits in the
   space saved.
4. **Cache lookup.** `algorithm/cache.rs`, `find_cached_result` /
   `can_use_cached_measurement` (8% of fresh, 12% of resize inclusive).
   Reject on the cheap integer keys (sizing modes, generation) before the
   f64 `inexact_equals`, check the layout slot before the 8 measurement
   slots, and keep the entries contiguous plain data rather than
   `Cell<CachedMeasurement>` copies. Expected: −5% on `resize` and
   `deep*`. Measure: `bench/run rn/feed50/resize rn/deep`.
5. **No allocation per flex line.** `algorithm/flex_line.rs:67`,
   `calculate_flex_line`'s `Vec::with_capacity(node.child_count())` (3% of
   fresh: malloc + free per line per layout). Use a reusable buffer in
   `LayoutData` (one per recursion depth, or a single stack with
   start/end indices), as docs/API.md already suggests. Expected: −3% on
   fresh/resize. Measure: `bench/run rn/`; `bench/profile` should show no
   `libsystem_malloc` under `calculate_layout`.
6. **Build nodes in place.** `node/tree.rs:12`, `node_new`:
   `Box::new(Node::new(config))` copies 832 B from the stack. Use
   `Box::new_uninit()` + field-wise writes (or make `Node::new` trivially
   inlinable so LLVM elides the copy). C-API-only (RN keeps `yoga::Node`), so
   low priority for M5. Expected: `micro/node_new_free` 47 → ~30 ns.
   Measure: `bench/run micro/node_new_free`.
7. **Deep-nesting re-layout amplification** (beyond Yoga, like T-0007).
   `algorithm/calculate_layout.rs` + `cache.rs`: instrument which pass
   (flex basis, stretch, final, min-content) misses the cache in
   `rn/deep*`, and whether some of those misses are equivalent constraints
   (e.g. differing only in a sizing mode that cannot change the result).
   Ticket is "find out", with the measure-call column of
   `bench/run rn/deep` as the metric (66 064 calls for 21 texts at depth
   20). Low RN priority: real feeds measure each text ~once.
8. **FMA policy** (fidelity, not speed): see Finding 3. Ticket: decide;
   if matching RN's Yoga bit-for-bit matters, locate the contracted
   expressions on the deep10 path (compare `-ffp-contract=off` codegen) and
   mirror them with `mul_add`. Measure: `bench/run rn/deep` checksums.

## Requested Cargo profile change

None is required: the bench sets `CARGO_PROFILE_RELEASE_LTO=fat` and
`CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1` from the environment. When
`barre-rn` lands, its release build should use the same (it is ~5–10%;
Finding 5), e.g. a profile in the root `Cargo.toml`:

```toml
[profile.release-lto]
inherits = "release"
lto = "fat"
codegen-units = 1
```

## Limitations

- Engine overhead only: cheap measure funcs (see Methodology) and C-API
  readback; the RN shim's vtable/blob path (docs/RN-SHIM.md §3c) isn't
  modelled until `barre-rn` exists. It could be added as a third engine.
- macOS/Apple silicon only (`QOS_CLASS_USER_INTERACTIVE`, `-liconv`,
  `dsymutil`/`atos`). Android numbers need an arm64 Linux/Android run.
- The FMA mismatch means a checksum can differ for reasons that are not
  barre bugs; the dump diff says how much and where, read it before filing.
