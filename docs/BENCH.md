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
bench/run --rn                 # + RN's patched Yoga with barre-rn, switch off/on
bench/profile rn/feed50/fresh --engine rn-barre -- --rn
```

`--rn` (milestone 5, docs/RN-SHIM.md) adds a third binary: React Native
0.88's `ReactCommon/yoga` with `rn/patches/` applied (the tree `./conf
--engine rn-*` uses), the benchmark compiled against its headers, and
`crates/barre-rn` (same cargo flags as barre-ffi), run with the layout
switch off (`rn-yoga`) and on (`rn-barre`, `BARRE_LAYOUT=1`; each run checks
the switch took). The table's ratios: `rn-barre/rn-yoga` is the in-app A/B,
`rn-barre/barre` the cost of the host interface over RN's C++ nodes against
barre's own C ABI, `rn-yoga/yoga` the cost of the patch (and version skew)
on Yoga. Results and their analysis: docs/RN-SHIM.md, "Phase A".

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
| `rn/fabric50/` | `commit` | Fabric's commit pattern (no tree is mutated in place): 50 posts in a scroll view, every node with its own config carrying the clone callback. Before each layout a commit clones the root-to-leaf path (`YGNodeClone`, a new config, `YGNodeSwapChild`) and edits one text, in turn: a timestamp 0.04 pt wider or narrower next to a name and handle that shrink to fit (fractional glyph advances), a post text 120 ↔ 200 glyphs, a like count 1 ↔ 2 glyphs; `YGNodeMarkDirty` on it. Layout clones the path's other children through the callback. Widths on the 1/3-pt grid (`flex: 1` texts sharing a row, a 31/3 padding) and off it (a 12.1 padding); a `flex: 1` wrapper in an auto-height column. Replaced nodes are freed once nothing reachable names them as owner |
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

**Update (`371c971`).** T-0010 (`8abfb2e`) added an owner-size check to
every cache hit that resolved eight style values twice: `rotate` went to
344 µs (1.43×), the clean-relayout early-out from 14 to 31 ns (1.5× Yoga),
`fresh` 0.61 → 0.67×. `371c971` skips that check when the owner size is
unchanged or the style has no percentage in those inputs (same answers):
`rotate` 276 µs (1.13×), clean 14 ns (0.67×), `fresh` 0.60×,
`text_same_size` 0.41×, `resize` 0.59×, `capture/*` 0.58–0.64× (`--quick`).
What is left of the `rotate` gap is T-0006's replays (Layout calls per
node, item 3; idea 1).

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
2. **Cache behaviour is Yoga's, except where barre's measure-leaf cache
   compares exactly.** `hasNewLayout` sets match Yoga on every workload.
   Measure-call counts match on every workload except three, where barre
   makes more: `rn/deep15/fresh` 4382 vs 4356 (+0.6%), `rn/deep20/fresh`
   66 937 vs 66 064 (+1.3%), `rn/feed50/resize` 148, 148, 148, 144 vs 148,
   136, 139, 135 in the check phase (299.1 vs 292.1 per layout over 80
   steps, +2.4%), plus `rn/fabric50/commit` (1.25 vs 1.01 per commit over
   its first 80 commits). All of it is the yoga#1784 decision (barre's
   lookup compares a measure-function leaf's available sizes exactly, Yoga
   rounds them to the pixel grid first): with Yoga's comparison, every count
   is Yoga's ("Measure calls against Yoga" below). The `rn-barre` arm's
   counts are `barre`'s on every workload. Otherwise the measure cache does
   its job: one text change costs one measure call and relays out 63 nodes;
   rotating back and forth costs zero measure calls (both sizes stay in the
   8-entry cache), while a window drag (`resize`) re-measures ~140 texts
   per layout.
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
   there would be 996 calls). See ideas 2 and 8. RN's feed is fine (548 calls for
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

## Layout calls per node (`stats` feature)

`cargo build --release -p barre-ffi --features stats` builds barre with
per-layout counters (`crates/barre/src/stats.rs`; compiled out otherwise).
Link it like the bench does (`bench/build/build.ninja`, the `bench-barre-*`
link line with this `libbarre_ffi.a`) and run
`BARRE_STATS=1 bench-barre --run NAME --time-ms 0 --warmup-ms 0 --captures vendor/yoga/benchmark/captures`:
one summary per `YGNodeCalculateLayout` on stderr (calls by pass type and
cache outcome, calls and runs per node, `calculate_layout_impl` calls,
T-0006 anchor replays, measure callbacks, and runs whose result equals an
entry this layout already stored under other constraints, by class).

Steady state (4th layout), at `371c971`:

| workload | nodes visited | `_internal` calls (/node) | `_impl` runs | of which replays | leaf runs (= measure callbacks) | container runs: measure / layout | runs equal to a same-layout entry |
|---|---|---|---|---|---|---|---|
| `rn/feed50/fresh` | 1451 | 11 858 (8.2) | 5272 | 0 | 548 | 3768 / 956 | 3717 |
| `rn/feed50/rotate` | 1101 | 4131 (3.8) | 908 | 435 | 0 | 0 / 473 (+435 replays) | 0 |
| `rn/feed50/resize` | 1451 | 11 334 (7.8) | 4080 | 0 | 148 | 3484 / 448 | 3209 |
| `rn/feed50/text_change` | 79 | 414 (5.2) | 79 | 0 | 1 | 71 / 7 | 62 |
| `rn/feed50/insert_remove` | 52 | 202 (3.9) | 6 | 0 | 0 | 4 / 2 | 3 |
| `capture/feed-android` | 559 | 3615 (6.5) | 2869 | 0 | 57 | 2307 / 505 | 2200 |
| `capture/chat-mac` | 1512 | 21 691 (14.3) | 13 162 | 0 | 162 | 11 704 / 1296 | 11 062 |

1. **Leaves are fine; containers are measured ~4 times per layout.** Every
   text is measured about once (548 callbacks for ~500 texts), but each
   container *runs* `calculate_layout_impl` 4–5 times in a fresh feed
   layout (8 in `chat-mac`): a measure pass per distinct constraint its
   parent asks for (flex basis at max-content height, then fit-content,
   then the stretch/flex layout), and each run re-asks its own children
   under new constraints. Yoga does exactly the same calls (the counts are
   the same algorithm); it is where the remaining time goes.
2. **70% of container runs return a result the node already computed this
   layout under other constraints** (3717 of 5272 in `fresh`, 11 062 of
   13 162 in `chat-mac`). By class (`fresh`): a node measured at
   `(StretchFit w, MaxContent)` and then asked `(StretchFit w, FitContent H)`
   with the content height fitting `H` (667), the same from `FitContent w`
   (540 + 540), `MaxContent` height then `StretchFit` at the measured height
   (287 measure + 287 layout). `capture/feed-android`: 1260 of 2812
   container runs are the first class alone. Yoga reuses exactly this
   (`oldSizeIsMaxContentAndStillFits`, `sizeIsExactAndMatchesOldMeasuredSize`)
   only for measure-function leaves; containers need an exact key. Extending
   it to containers is idea 2 below: the biggest lever left, and the one
   that goes beyond Yoga, so it needs a proof per class, not a guess.
3. **`rotate` spends half its runs in T-0006 replays.** 435 of 908 runs
   are anchor replays (measure-only re-runs of a container's first call,
   answered from an earlier layout's cache, before its layout run); every
   call they make is a cache hit (no measure callback, no store). With
   replays disabled `rotate` is 174 µs (0.70×), with them 276 µs (1.13×).
   152 of the 435 replay nodes have a descendant with a retainable basis
   (`flex: 1` `content` column in each post and its ancestors), 283 do
   not. See idea 1 for why they can't simply be skipped.
4. Same-generation cache hits are 50–60% of all calls and cost ~1% (lookup
   only); `HitEarlierGen` dominates relayouts (`text_change`: 329 of 414).

## Measure calls against Yoga (attribution, 2026-10-18)

On the iOS simulator (social-app, RN 0.88, the same binary with the layout
switch off and on, an `rn-shim` build from before the barre-only relayout
fixes) barre made ~30% more text-measure callbacks than Yoga in a scripted
~35 s scenario (Yoga 418, barre 554; equal cost per call; the busiest pass
44-45 vs 54-55 calls; cold start about equal; geometry equal at every
checkpoint). Where that comes from was attributed on the host by ablation:
branch `measure-ablation` (experimental, not for landing) adds switches,
compiled in only with `RUSTFLAGS=--cfg=barre_ablate` and chosen at run time
with `$BARRE_ABLATE`, that turn barre-only machinery off or back to Yoga's
behaviour: `d5` (T-0006's refusal of an earlier layout's entry computed under
another anchor), `replay` (T-0006's anchor replays), `sfc` (the
`singleFlexChild` write at a hit), `round` (Yoga's pixel-grid-tolerant
`canUseCachedMeasurement` for measure-function leaves, i.e. the yoga#1784
decision reverted), `owner` (T-0010's owner-size key), `d1` (keep an earlier
layout's flex basis in the resolved-basis branch, Yoga without
`WebFlexBasis`). Counts are deterministic: measure callbacks per layout in
the check phase, and the mean over 80 more layouts (`resize`: two full
40-width cycles). `barre` and `rn-barre` gave the same counts in every
variant, as did `yoga` and `rn-yoga`.

| workload | Yoga | barre (`stack` 2800e38) | (a) `d5`+`replay` | (b) `round` | (c) (a)+(b) | `d1` | `owner` | all six |
|---|---|---|---|---|---|---|---|---|
| `rn/feed50/resize`, check | 148, 136, 139, 135 | 148, 148, 148, 144 | = barre | = Yoga | = Yoga | = barre | = barre | = Yoga |
| `rn/feed50/resize`, mean of 80 | 292.06 | 299.05 | 299.05 | 292.06 | 292.06 | 299.05 | 299.05 | 292.06 |
| `rn/deep15/fresh` | 4356 | 4382 | 4382 | 4356 | 4356 | 4382 | 4382 | 4356 |
| `rn/deep20/fresh` | 66 064 | 66 937 | 66 937 | 66 064 | 66 064 | 66 937 | 66 937 | 66 064 |
| `rn/fabric50/commit`, check | 1, 1, 1, 1 | 1, 1, 3, 1 | 150, 1, 3, 2 | 1, 1, 1, 1 | 150, 1, 1, 2 | = barre | = barre | 1, 1, 1, 1 |
| `rn/fabric50/commit`, mean of 80 | 1.01 | 1.25 | 1.57 | 1.01 | 1.34 | 1.25 | 1.25 | 1.01 |

Controls, equal to Yoga in every variant: `rn/feed50/{fresh,rotate,text_change}`
(548; 494 then 0; 1), `rn/deep10/fresh` (160), `rn/wide_row200/fresh` (400),
`capture/feed-android` (57), `capture/chat-mac` (162). Over 400 commits
`rn/fabric50/commit` is 1.07 (barre) vs 1.00 (Yoga, and barre with `round`):
once a timestamp toggles back, the measurement at its earlier width is still
in the cache.

So **on RN-shaped trees the whole excess is the exact measure-leaf
comparison** (hypothesis 2), and T-0006's refusal and replays (hypothesis 1),
T-0010's owner key and the flex-basis rewrite contribute nothing. (Variant
(a) is worse on the clone workload: without replays, `d1`'s rewritten bases
are no longer recomputed the way a fresh layout does, and the first commit
re-measures 150 texts.) It shows in two forms:

- **Within one layout** (`deep15`, `deep20`, fresh trees): a leaf is asked
  for widths that differ by less than a pixel in one pass; Yoga's tolerance
  answers from the first, barre measures again. In these trees the layout is
  the same either way.
- **Across layouts** (`resize`, `fabric50/commit`): a leaf is asked for a
  width less than a pixel from one it was measured at in an earlier layout.
  This is yoga#1784's mechanism, and Yoga's reuse changes the result:
  `resize` with `round` has Yoga's checksum, not barre's; in `fabric50` the
  timestamp of the edited cell ends 1/3 pt to the left in barre. (Its
  checksum also differs from Yoga's in every variant because of one other
  timestamp that is 1/3 pt apart from the first layout on, presumably
  finding 3's FMA difference; the bench reports it as a mismatch.) The clone
  workload's sub-pixel timestamp edit is what reproduces the device's excess:
  real glyph advances are fractional, so editing one text (a timestamp, a
  count) moves the available width of a shrinking neighbour by a fraction of
  a pixel, and only barre re-measures it. Without that edit (an earlier
  version that edited only the post text and like count) all engines made
  exactly one call per commit.

On the fuzzer's random trees (`./fuzz calls`, docs/FUZZ.md: callbacks of the
relayout sequence, summed; seeds 1-2000, modes 0-2, scales 0 and 1) the
picture differs, because those trees are full of percentage bases relaid out
at very different root sizes:

| variant | callbacks | vs Yoga | `./fuzz run` failing pairs (10 000 seeds) |
|---|---|---|---|
| Yoga | 158 899 | | 19 315 (64.4%) |
| barre | 174 602 | +9.9% | 2290 (7.6%) |
| `round` | 171 768 | +8.1% | 7609 (25.4%) |
| `d5` | 174 250 | +9.7% | 2730 (9.1%) |
| `d5`+`replay` | 177 808 | +11.9% | 11 717 (39.1%) |
| `d1` | 161 254 | +1.5% | 17 025 (56.8%) |
| `d1`+`round` | 159 058 | +0.1% | 18 311 (61.0%) |
| `d1`+`d5`+`replay`+`round` | 158 787 | -0.1% | |

`owner` and `sfc` change nothing there. With `--steps 20`, `--wide` and
`--siblings 6 --measure 100 --steps 12` (1000 seeds) the shares are the same:
`d1` takes barre from +14.9 / +9.3 / +10.6% to +2.8 / +2.0 / +1.4%. There the
excess is mostly yoga#1552 (a stale percent or content basis that Yoga keeps
and barre rewrites), and Yoga's lower count comes with layouts that differ
from fresh ones: every variant that gets closer to Yoga's count fails the
fuzzer far more often.

**A narrower rule was tried and rejected.** Branch `measure-round-samegen`
(prototype, not for landing): the pixel-grid tolerance only against an entry
stored in the same layout, exact against an earlier layout's. It recovers the
within-layout form (`deep15` 4356, `deep20` 66 064; every other count
unchanged, `resize` and `fabric50/commit` included) and keeps `./conf` at 852
pass / 2 expected fail with no regressions, but the fuzzer gets worse:
failing pairs 2290 → 2318 (default knobs), 2875 → 2897 (`--wide`), 3950 → 3990
(`--steps 20`), barre-only 14 → 17, 22 → 22, 14 → 17 (the minimised cases in
`fuzzer/cases/` are unchanged). Presumably because in a relayout a
cross-layout hit higher up can skip the call that measured the leaf first in
a fresh layout, so even a same-layout tolerance depends on history (not
traced). By the
correctness bar that is not acceptable. The cross-layout form, which is the
one the device shows, has no exact counterpart: whether a measurement at
140.6 pt can stand for 141 pt depends on the measure function.

Not reproducible on the host: whether the device's commits are of this kind
(timestamps, counts, `onLayout`-driven re-renders) and how many layouts per
second Fabric runs; hypothesis 3 (more commits from layout feedback) needs
the device.

## Optimisation ideas, ranked by expected impact

Each is phrased as a ticket. "Measure" means `bench/run <prefix>` (the check
column must stay `ok` and the measure/new-layout counts unchanged) plus
`./conf` staying green, and `bench/profile` for the share.

Done (see Results): incremental pixel-grid rounding (`cd11304`; memo in
`LayoutResults`, debug builds verify every skip, see `pixel_grid.rs`),
pre-resolved edges (`f91e07d`, plus the all-undefined bitmask `c11d1b3`),
no allocation per flex line (`97cec15`, `InlineVec` in `flex_line.rs`).

1. **`rotate`: skip T-0006 replays that cannot matter** (needs a
   decision; branch `perf-replay-skip`). The replay exists to re-establish
   which call *anchors* a container's retained flex bases. A sticky
   per-node flag (`flex_basis_history`) records whether any run in the
   subtree ever wrote a retainable basis that is not one constant under
   every call (a content-measured or percent basis, a `singleFlexChild`
   zero that differs from the resolved one); while it is clear, every
   retainable basis is that constant whichever call anchors it, and the
   replay of a call identical to the entry that answered it is skipped.
   Result: `rotate` 276 → 174 µs (0.67×), every bench checksum, measure
   count and new-layout count unchanged, `./conf` 847/847. **But it is not
   exact**: a differential fuzzer (random trees, 10 relayouts with dirty
   text edits, hashing layouts per step) differs from main on 14 of
   18 000 runs, all through Yoga's measure-cache *compatibility* reuse
   (a skipped replay would have measured a leaf at another size and
   stored it; a later call reuses that entry at a compatible size, so the
   leaf's height differs by a pixel). Against fresh layouts it is a wash
   (relayout≠fresh on 523 vs 522 of 9000 fuzz runs: one fixed, two
   broken). Making it exact needs proof that the replay (and every
   replay it would cause in descendants) is all cache hits, i.e. a record
   of each container's last executed replay per anchor key plus
   per-subtree cache-rewrite stamps (~70 B per node), not worth it. So:
   accept the residual-class difference (and document it in
   conformance/divergences.md), or keep paying the replays.
2. **Reuse a container's result across compatible constraints** (beyond
   Yoga; Findings "Layout calls per node", item 2). 70% of container runs
   recompute a result the node already has from this layout, mostly
   `MaxContent` height → `FitContent H` with the content height fitting
   (`capture/feed-android`: 1260 of 2812 runs). Yoga reuses exactly that
   for measure-function leaves (`oldSizeIsMaxContentAndStillFits`) but
   requires an exact key for containers, because a container's layout can
   read the available height: percent heights/min/max/basis/gaps of its
   children, `flex-grow` along a column main axis, `justify-content` free
   space, aspect ratios, and measure funcs below it receiving `AtMost`
   instead of `Undefined`. A per-node, per-axis "result independent of
   the available size while the content fits" predicate (style-only for
   the node and its descendants, cached per node and invalidated with
   style/children changes) would let `find_cached_result` accept those
   entries for containers. Upper bound: ~40% of container runs in
   `fresh`/`capture` (and their subtrees' calls). Needs its own design and
   fuzzing against fresh layouts; the measure-func part repeats the
   assumption Yoga already makes for leaves.
3. **Fewer edge lookups per node.** Still the top self-time group
   (`compute_margin_for_axis`, `compute_margin`, `compute_padding`,
   `compute_border`, `Edges::compute`, `resolved_min/max_dimension`: ~37%
   self of `fresh` at `371c971`). Yoga's structure calls them many times
   per node per pass with the same arguments; `padding_and_border_for_axis`
   alone is 13% inclusive. Options: per-`Edges` precomputed point values,
   or a node's padding+border resolved once per `calculate_layout_impl` and
   passed down. Expected −10–15% fresh.
4. **Compact `Style` (and `LayoutResults`)** for RN memory, not speed:
   `Style` is 448 B (the resolved-edge tables added 48 B), Yoga's 160 B.
   Splitting `Edges` into f32 values + unit bytes saves 96 B and measured
   neutral; dimensions/gap the same way save ~40 B more. Do it when
   `barre-rn` exists and the blob size matters (docs/RN-SHIM.md §3c).
5. **Cache lookup.** `find_cached_result` / `can_use_cached_measurement`
   are now ~1% of fresh; leave them to T-0010's cache-key work.
6. **Build nodes in place.** `node/tree.rs`, `node_new`:
   `Box::new(Node::new(config))` copies the node from the stack. Use
   `Box::new_uninit()` + field-wise writes. C-API-only (RN keeps
   `yoga::Node`). Expected: `micro/node_new_free` 40 → ~30 ns.
7. **Incremental rounding of moved subtrees.** Cells below an edit move,
   so their subtrees are re-rounded (`text_change` is 0.65× where
   `text_same_size` is 0.41×). A subtree translated by a whole number of
   pixels rounds to the same values translated, but not bit-exactly in
   general (f64 sums), so this needs either a proof for the grid-aligned
   case or acceptance of a difference from Yoga; probably not worth it.
8. **Deep-nesting re-layout amplification** (ticket proposal, beyond
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
9. **FMA policy** (fidelity, not speed): see Finding 3; decided in
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

## Regression check and the faster-than-Yoga metric

The only ratchet that gates anything is conformance
(`conformance/expected-pass.txt`). Speed had none: on 2026-10-17
`micro/style_set_x20` went from barre/yoga 1.36x to 4.49x between CI runs
`1854783423` and `52d96baf8c` (barre 91 -> 305 ns, Yoga flat at ~68 ns) and it
stayed unnoticed for about 12 hours, because `bench.yml` only printed a table.
The same commit improved the overall geomean from 0.898x to 0.662x, so one
aggregate number would have hidden it. Hence per-workload checks.

**Status: report-only (phase 1).** Nothing below can turn CI red. It is
written to the job summary of `bench.yml` and to nothing else (no PR comment:
PRs from forks only get a read-only token). Not yet verified in GitHub: the
workflow steps were written without running them there, and the baseline
fetch (`git fetch origin bench-results` with the job's read-only token, also
on a labelled PR) has only been run from a workstation. The first labelled
PR exercises it for real.

### Two tiers

1. **Coarse, in CI, on ratios.** `.github/scripts/bench_compare.py CURRENT
   BASELINE` compares a run with the previous run on `main` (the newest
   `results/*.json` on the `bench-results` branch; `bench.yml` fetches it
   before the publish job adds the new one). The shared M1 runner is far
   too noisy for absolute times or for 5% effects (load average up to ~30,
   p10-p90 spread often over 100% of the median), but within one run Yoga
   and barre are interleaved, so the ratio barre/yoga survives. It catches
   what the 2026-10-17 miss was: a workload that got a lot worse.
2. **Fine, on a quiet machine, by number.** Changes of a few percent are
   decided by the numeric `check` of a perf ticket (docs/HARNESS.md, "Perf
   tickets: gate the number in `check`"): `bench/run --quick NAME`, then a
   command that exits non-zero unless the ratio is below a threshold. CI
   cannot do that.

### What is compared

Per workload (every key of `results` that has both a `yoga` and a `barre`
entry with a median and no `error`):

- **ratio** = barre median / yoga median, in the current run and the
  baseline; **change** = current ratio / baseline ratio - 1 (positive is
  worse).
- **interval**: barre's p10 and p90 each divided by the *same run's* Yoga
  median. The current and baseline intervals are then compared; "overlap"
  means `cur.lo <= base.hi and base.lo <= cur.hi`. Dividing by the run's own
  Yoga median puts both intervals in "multiples of Yoga" and removes
  runner-wide slowness; Yoga's own spread is not propagated (the check is
  already lenient on this noise). A run without p10/p90 can never produce a
  REGRESSION (the interval test is skipped and the report says so).

Statuses (the constants are at the top of the script):

| status | rule |
|---|---|
| REGRESSION | change > 30% **and** the intervals do not overlap |
| WATCH | change > 10% (including a >30% change whose intervals overlap) |
| IMPROVED | change < -10% |
| steady | otherwise |

Category aggregates (`rn/`, `capture/`, `yoga/`, `micro/`) are geomeans of the
ratios over the workloads in both runs, with their change. A **geomean
regression** is the non-micro geomean getting more than 5% worse. A workload
in only one of the runs is reported as new / missing, an errored workload is
skipped with a note, and a missing or unreadable baseline degrades to the
metric alone.

The script also writes a gate verdict, `would_pass` or `would_fail` with
reasons (any REGRESSION, a geomean regression, or a ratchet failure), in both
the markdown and the JSON. The exit status is 0 regardless; `--enforce`
exits 1 on `would_fail`, and CI does not use it. Promoting the check later
is that one flag.

### Faster than Yoga

For each workload, from the current ratio alone: **faster** below 0.95,
**tie** from 0.95 to 1.05, **slower** above 1.05. The headline is
`faster than Yoga: N of M (excluding micro/)`, with counts per category.
`micro/` is excluded from the headline and has its own line: those workloads
time C API calls (setters, getters, node creation) that Yoga's LTO inlines
into the benchmark and barre's cannot (no cross-language LTO, see "Notes on
fairness"), so barre is ~1.5x there by construction and the number would say
nothing about layout. The README's benchmark block and the `bench-results`
history table carry the headline.

| run | faster than Yoga (excl. `micro/`) | note |
|---|--:|---|
| `9bbc3b4aa9`, `f47ea26fa8`, `300ee03da5`, `1854783423` | 21 of 29 | same barre code; the `*.clean` workloads were ~1.5x |
| `52d96baf8c` | 28 of 29 | only `yoga/huge_nested` (1.07x) not faster |
| `9122b937ed` | 27 of 29 | 1 tie (`rn/feed50/rotate`, 1.00x), 1 slower (`yoga/huge_nested`, 1.18x); `micro/` 0 of 6 |

### The ratchet list

`bench/expected-faster.txt` lists the workloads that comfortably beat Yoga
(ratio <= 0.90 in run `9122b937ed`): 27 non-micro workloads, one per line,
`#` for comments. The report says **would fail** for a listed workload whose
current ratio is above 1.05 (slower than Yoga), and **would fail (marginal)**
between 1.00 and 1.05 (no longer faster). Listed workloads between 0.90 and
1.00 are fine (margin eroded, not flagged). A listed name that is not in the
run (a filtered run, a renamed workload) gets a note. Both verdicts are part
of the gate verdict only.

Update it after a deliberate change, in its own commit:

```
.github/scripts/bench_compare.py RESULT.json --update-list
```

which rewrites the file from a run (every non-micro workload at <= 0.90).
A workload may enter the list as it improves; leaving it needs a waiver.

### Waivers

Some trades are deliberate (layer #17 gave up ~5% on relayout paths for
correctness). Those must be explicit rather than bypassed or silently
absorbed. Convention, **documented only: not enforced and no
`bench/waivers.md` exists yet**. When a gate is enforced, a flagged workload
is excused if `bench/waivers.md` has a row for it naming the commit that
introduced it:

```
| workload | commit | before -> after (barre/yoga, CI run ids) | reason | accepted by |
|---|---|---|---|---|
| `rn/feed50/rotate` | `abc1234` | 0.89x (`52d96baf8c`) -> 1.00x (`9122b937ed`) | relayout fix for yoga#NNNN, see docs/STATUS.md | name, 2026-MM-DD |
```

A row covers the commit's own comparison only: the next baseline is the run
that includes it, so it does not need removing for the check to pass, and a
ratchet entry the commit makes false is removed from
`bench/expected-faster.txt` in the same commit.

### Noise, and the thresholds

`bench_compare.py` was run on the six CI runs on the `bench-results` branch
(`.github/scripts/fixtures/`, trimmed copies; `test_bench_compare.py`
prints these tables). The first four runs measured **identical barre code**
(no change under `crates/`, `bench/` or `vendor/` between their commits), so
comparing them is pure runner noise. Consecutive pairs with the defaults
(REGRESSION > 30% and disjoint, WATCH > 10%):

| pair | code | REGRESSION | WATCH | IMPROVED | non-micro geomean |
|---|---|--:|--:|--:|--:|
| `9bbc3b4aa9` -> `f47ea26fa8` | same | 0 | 1 | 2 | -0.4% |
| `f47ea26fa8` -> `300ee03da5` | same | 0 | 1 | 0 | +0.9% |
| `300ee03da5` -> `1854783423` | same | 0 | 1 | 1 | +0.2% |
| `1854783423` -> `52d96baf8c` | changed | 1 (`micro/style_set_x20`) | 0 | 26 | -33.6% |
| `52d96baf8c` -> `9122b937ed` | changed | 0 | 11 | 0 | +6.8% |

Over all 12 ordered pairs among the four identical-code runs (420
workload comparisons): 16 were worse by more than 10%, 2 by more than 20%,
none by more than 30%; the largest was +24%. So on this sample:

- **30% with disjoint intervals produced no false positive** and caught the
  one real regression (the only REGRESSION in the five pairs). The margin
  is not large (worst noise +24%), and the sample is small (four runs, six
  independent pairs). Keep 30%/disjoint as the REGRESSION definition for
  now, and re-check it on more same-code pairs before it gates (see below);
  if any same-code pair shows a >30% disjoint flag, raise it to 40%.
- **10% (WATCH) is noise-prone**: about one flag per run even with
  identical code (1.3 per ordered pair, 16/12). That is why WATCH is
  information and never part of the gate.
- **The 5% non-micro geomean** moved at most 0.9% across the same-code
  runs, so it is not noise-triggered; it flagged (+6.8%) the last pair,
  which spans the barre-only relayout fixes (`calculate_layout.rs`,
  `layout_results.rs`). Its eleven WATCH flags are mostly the `*.clean`
  early-out workloads (~20 ns) and the relayout workloads (`rotate`, `resize`,
  `text_same_size`), which is where docs/STATUS.md records a locally measured
  cost for those fixes (rn/ geomean 0.52x -> 0.55x, `text_same_size` +11%,
  `rotate` 0.91x -> 0.97x). So that pair is probably a true positive, not noise,
  though a single CI pair cannot prove it.

### Promotion plan

1. **Now:** report-only, summary only. Look at it on every `main` push for a
   few weeks; `bench-results` accumulates the history (one run per push to
   `main`).
2. **Before promoting:** (a) the workflow verified on a labelled PR and on a
   `main` push; (b) at least 20 or so runs with no unexplained flag, in
   particular same-code pairs to re-validate the 30%, 10% and 5% numbers (the
   data above is six runs); (c) `bench/waivers.md` created and the waiver
   lookup added to the script (a few lines); (d) decide how to avoid failing on a
   single noisy run: a gate probably wants the same workload flagged in two
   consecutive runs, or an automatic re-run, before it fails.
3. **Promote:** pass `--enforce` in `bench.yml`, drop `continue-on-error` from
   the compare step, and give the compare its own job name. Then
   `[stack] required_checks` in `harness/orch.toml` (currently
   `["conformance", "fmt, clippy, test"]`) would need that job's name added.
   Two things block that as it stands: `bench.yml`'s job runs only on pushes
   to `main` and on PRs labelled `bench`, and a required check that never
   starts leaves the stacker waiting forever; so either the job runs on every
   stacked PR (a macOS runner per layer, with a 60-minute timeout) or only layers touching `crates/` are required to
   have it (the `stress_paths` / `stress_label` mechanism is the existing hook
   for path-dependent extra CI), and the stacker would add the `bench` label.
   `harness/` is the architect's to edit, not this change's.
   Alternatively keep it a post-merge check on `main` that opens an issue, and
   leave `required_checks` alone.

## Limitations

- Engine overhead only: cheap measure funcs (see Methodology) and C-API
  readback; the RN shim's vtable/blob path (docs/RN-SHIM.md §3c) isn't
  modelled until `barre-rn` exists. It could be added as a third engine.
- macOS/Apple silicon only (`QOS_CLASS_USER_INTERACTIVE`, `-liconv`,
  `dsymutil`/`atos`). Android numbers need an arm64 Linux/Android run.
- The FMA mismatch means a checksum can differ for reasons that are not
  barre bugs; the dump diff says how much and where, read it before filing.
