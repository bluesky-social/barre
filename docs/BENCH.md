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
| source | `vendor/yoga/yoga/**/*.cpp` (pin 52f6d0bd) | `git archive <rev> crates Cargo.*` into `bench/build/src/<sha>`, so the swarm's uncommitted edits never leak in (`--worktree` to override); one `CARGO_TARGET_DIR` per revision (`bench/build/target/<sha>`): with a shared one, cargo considers an older revision "fresh" and does not re-copy its staticlib, so the bench silently linked whichever revision was built last (runs before `b2935a4` that alternated revisions may be affected) |
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

`c11d1b3` (2026-10-17, after the perf work below and T-0006/T-0008),
Apple M4 Pro, 5 rounds × 300 ms. Full table:
[`bench/results/latest.md`](../bench/results/latest.md). Selected rows
(the `ece98bb` column is barre before the perf work, from the previous
full run):

| workload | yoga | barre | barre/yoga | `ece98bb` barre/yoga | measure calls/layout | new-layout nodes |
|---|---|---|---|---|---|---|
| `capture/feed-android` | 648 µs | 380 µs | 0.59× | 0.92× | 57 | |
| `capture/chat-mac` | 3.51 ms | 2.15 ms | 0.61× | 0.92× | 162 | |
| `yoga/huge_nested.layout_only` | 1.73 ms | 1.15 ms | 0.67× | 0.90× | 0 | |
| `rn/feed50/fresh` | 1.58 ms | 946 µs | 0.60× | 0.86× | 548 | 1451 |
| `rn/feed50/text_change` | 33.3 µs | 21.8 µs | 0.65× | 0.92× | 1 | 63 |
| `rn/feed50/text_same_size` | 31.2 µs | 12.7 µs | 0.41× | 0.93× | 1 | 62 |
| `rn/feed50/rotate` | 256 µs | 292 µs | **1.14×** | 0.84× | 0 | 1101 |
| `rn/feed50/resize` | 1.47 ms | 934 µs | 0.64× | 0.87× | 135–139 | ~1078 |
| `rn/feed50/insert_remove` | 25.5 µs | 16.4 µs | 0.64× | 0.95× | 0 | 52/54 |
| `rn/deep20/fresh` | 26.5 ms | 22.5 ms | 0.85× | 1.14× | 66 064 | 42 |
| `rn/wide_column1000/fresh` | 878 µs | 535 µs | 0.61× | 0.87× | 1000 | 4003 |
| `micro/node_new_free` | 26 ns | 40 ns | 1.51× | 1.78× | | |
| `micro/style_set_x20` | 51 ns | 52 ns | 1.01× | 0.69× | | |
| `micro/build_feed50_tree` | 131 µs | 151 µs | 1.16× | 1.17× | | |
| geometric mean, 35 workloads | | | **0.75×** | 0.91× | | |

`rotate` regressed with T-0006 (stale flex basis fix, `9bdbf48`), not with
the perf work: with the same perf commits on top of `ee0dc1f` (before
T-0006) it was 178 µs (0.68×), on top of `0e42d3c` (after) 323 µs. Measure
calls and new-layout nodes are unchanged, so it is extra relayout work in
T-0006's flex-basis anchor path; worth a look by whoever owns it.

Perf commits (each `./conf` clean, checksums unchanged; numbers are
`--quick` A/B against the previous commit):

| commit | change | effect |
|---|---|---|
| `cd11304` | incremental pixel-grid rounding (idea 1) | `text_same_size` −47%, `insert_remove` −24%, `text_change` −19%; fresh unchanged |
| `f91e07d` | pre-resolved edge fallback chain (idea 2) | fresh/resize/rotate/capture −25…−30%; `style_set_x20` +45% (still ≈ Yoga) |
| `b2935a4` | bench: one cargo target dir per revision | fixes stale-lib comparisons (see "How it is built") |
| `97cec15` | inline storage for flex-line items (idea 5) | fresh/resize/capture −12…−19% |
| `c11d1b3` | all-undefined `Edges` bitmask fast path | fresh −2…−6% |

## Findings

1. **barre is ~1.6× faster than RN-Android-built Yoga** on layout
   workloads (0.58–0.72×; incremental `text_same_size` 0.41×), and slower
   only on node creation (`node_new_free` 1.5×, tree building 1.16×),
   cross-language getters (not RN-relevant) and `rn/feed50/rotate` since
   T-0006 (1.14×, see Results). Before the perf work it was 0.84–0.95×.
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
   per level). T-0007 (yoga#2007) didn't change it. It is not a cache-key
   gap: tracing `rn/deep15/fresh` (8 540 `calculate_layout_impl` calls),
   only 59 calls repeat a (node, constraints) pair, and ignoring sizing
   modes merges only 2% more. 99.9% are measure passes (`MeasureChild` 70%,
   `FlexMeasure` 30%), each handed a *different* available width (every
   shrink distribution of an ancestor measure yields a new width: 126
   distinct widths per node at depth 16, against an 8-entry cache), and
   the available height multiplies that by ~8.5× (keyed on width alone
   there would be 996 calls). See idea 7. RN's feed is fine (548 calls for
   ~500 texts), so it bites only deep shrinking row/column chains.
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

Hotspots (`rn/feed50/fresh` at `97cec15`, after ideas 1, 2 and 5; percent
of samples under `YGNodeCalculateLayout`): `Edges::compute` 26% self, almost
all on its first load (the `resolved` table): a dependent-load chain
(table → value → unit) repeated for every `compute*` call, called many
times per node per pass by Yoga's structure; `resolved_max/min_dimension`
8% (again the first load of the field); `compute_margin_for_axis` 4%,
`StyleLength::resolve` 4%, `max_or_defined` 3%; cache lookups ~1%;
pixel-grid rounding ~2%; no allocation left. Compacting the `Edges`
storage (f32 values + unit bytes, 84 → 60 B each) measured neutral, so
this is latency per call, not cache footprint.

Before the perf work (feed profiles at `ece98bb`): `style::compute_edge`
30% inclusive, `round_layout_results_to_pixel_grid` 47% of `text_change`
and 62% of `insert_remove`, malloc/free of the per-line `Vec`s 3% (16%
once T-0008 added two more per `resolve_flexible_length` and the edge work
had removed the rest). `rn/deep15/fresh`: `compute_edge` + padding helpers
dominate; `micro/node_new_free`: `Box::new(Node::new(config))` builds the
832-byte node on the stack and `memmove`s it into the box.

Not hotspots: trait dispatch (the algorithm is monomorphised over
`&Node`, no dynamic calls), `Cell` (compiles to plain loads/stores; it only
appears as a line attribution of inlined reads), allocation in general
(~3% via `calculate_flex_line`).

Sizes (at `c11d1b3`): `barre::Node` 1040 B, `Style` 448 B (the
resolved-edge tables), `LayoutResults` 480 B (T-0006's flex-basis anchor
and 32-byte cache entries, plus 24 B of rounding memo); at `34517d7` they
were 832, 400 and 320 B. Yoga's `Node` 624 B, `Style` 160 B (+ pool overflow), `LayoutResults`
336 B. In the RN shim the style snapshot + `LayoutResults` live in a blob
memcpy'd on *every* ShadowNode clone (docs/RN-SHIM.md §3c), so their size is
RN memory and clone cost, not just layout cache footprint.

## Optimisation ideas, ranked by expected impact

Each is phrased as a ticket. "Measure" means `bench/run <prefix>` (the check
column must stay `ok` and the measure/new-layout counts unchanged) plus
`./conf` staying green, and `bench/profile` for the share.

Done (see Results): incremental pixel-grid rounding (`cd11304`; memo in
`LayoutResults`, debug builds verify every skip, see `pixel_grid.rs`),
pre-resolved edges (`f91e07d`, plus the all-undefined bitmask `c11d1b3`),
no allocation per flex line (`97cec15`, `InlineVec` in `flex_line.rs`).

1. **Fewer edge lookups per node.** Still the top self-time item (26%).
   Yoga's structure calls `compute{Margin,Padding,Border}` and their
   `*ForAxis`/`PaddingAndBorder` sums many times per node per pass with the
   same arguments. Options: per-`Edges` precomputed point values (when the
   chosen value is points, `resolve` needs no reference length), so the
   common case is one load; or compute a node's padding+border once per
   `calculate_layout_impl` (as Yoga's `paddingAndBorderForAxis` callers
   could) and pass it down. Touches `calculate_layout.rs` heavily, so wait
   for the swarm to be idle. Expected −10–15% fresh.
2. **`rn/feed50/rotate` regression from T-0006.** 178 → 323 µs with no
   change in measure calls or new-layout nodes. Profile it
   (`bench/profile rn/feed50/rotate`) and see what the flex-basis anchor
   replays on a pure resize; owner: whoever maintains T-0006's code.
3. **Compact `Style` (and `LayoutResults`)** for RN memory, not speed:
   `Style` is 448 B (the resolved-edge tables added 48 B), Yoga's 160 B.
   Splitting `Edges` into f32 values + unit bytes saves 96 B and measured
   neutral; dimensions/gap the same way save ~40 B more. Do it when
   `barre-rn` exists and the blob size matters (docs/RN-SHIM.md §3c).
4. **Cache lookup.** `find_cached_result` / `can_use_cached_measurement`
   are now ~1% of fresh; leave them to T-0010's cache-key work.
5. **Build nodes in place.** `node/tree.rs`, `node_new`:
   `Box::new(Node::new(config))` copies the node from the stack. Use
   `Box::new_uninit()` + field-wise writes. C-API-only (RN keeps
   `yoga::Node`). Expected: `micro/node_new_free` 40 → ~30 ns.
6. **Incremental rounding of moved subtrees.** Cells below an edit move,
   so their subtrees are re-rounded (`text_change` is 0.65× where
   `text_same_size` is 0.41×). A subtree translated by a whole number of
   pixels rounds to the same values translated, but not bit-exactly in
   general (f64 sums), so this needs either a proof for the grid-aligned
   case or acceptance of a difference from Yoga; probably not worth it.
7. **Deep-nesting re-layout amplification** (ticket proposal, beyond
   Yoga; for the cache owner after T-0010). Finding 6: the misses are
   genuinely distinct constraints, so a bigger cache or looser sizing-mode
   matching would not help. What would: recognising that a measurement at
   available height `H` (`FitContent`) whose result `h` fit (`h < H`) is
   also the result for any other `FitContent` height `H' ≥ h` when nothing
   in the subtree depends on the available height (no percentage heights,
   no `flex-grow` along a column main axis, no stretch to the available
   height), generalising Yoga's `newSizeIsStricterAndStillValid` to looser
   sizes. Upper bound on `rn/deep15`: 8 540 → ~1 000 layout calls. Needs a
   precise "height-independent subtree" predicate, computed and cached per
   node, and tests where it must *not* apply. Low RN priority: real feeds
   measure each text ~once. Metric: measure-call column of
   `bench/run rn/deep`.
8. **FMA policy** (fidelity, not speed): see Finding 3; decided in
   docs/PLAN.md (accept). Unchanged.

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
