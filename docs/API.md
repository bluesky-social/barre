# barre: code layout and conventions

Read CLAUDE.md first. This is the map of the code: what lives where, the
design decisions behind it, and the pieces left as tickets.

## Crates

```
crates/barre/          the engine (pure Rust; `unsafe` only in node storage and callback calls)
  src/lib.rs           not_implemented! macro, re-exports
  src/enums.rs         Yoga's enums (yoga/YGEnums.h, yoga/enums/*), C values, ToString names
  src/numeric.rs       yoga/numeric/*: NaN-as-undefined helpers (maxOrDefined, inexactEquals, ...)
  src/config.rs        yoga/config/Config.*
  src/debug/           yoga/debug/*: logging, fatal assertions (FatalError)
  src/style/mod.rs     yoga/style/*: StyleLength, Style (flat, Copy), grid style types
  src/node/mod.rs      yoga/node/Node.*: barre's own node storage + its mutation API
  src/node/layout_node.rs     the LayoutNode trait (what the algorithm sees) + NodeExt
  src/node/layout_results.rs  yoga/node/LayoutResults.h, CachedMeasurement.h
  src/node/layoutable_children.rs  yoga/node/LayoutableChildren.h
  src/node/children.rs        a node's child list
  src/node/tree.rs     yoga/YGNode.cpp semantics: new/clone/free, insert/remove/swap/set children
  src/algorithm/       yoga/algorithm/*, one file per Yoga file:
    calculate_layout.rs  CalculateLayout.cpp      flex_line.rs     FlexLine.cpp
    pixel_grid.rs        PixelGrid.cpp            bound_axis.rs    BoundAxis.h
    align.rs             Align.h                  flex_direction.rs FlexDirection.h
    trailing_position.rs TrailingPosition.h       sizing_mode.rs   SizingMode.h
    absolute_layout.rs   AbsoluteLayout.cpp (stub)  baseline.rs    Baseline.cpp (stub)
    cache.rs             Cache.cpp (stub)
crates/barre-ffi/      Yoga's C API (Yoga.h) over barre; staticlib + rlib
  src/{config,node,layout,style,value,enums}.rs   YGConfig.h, YGNode.h, YGNodeLayout.h,
                                                  YGNodeStyle.h, YGValue.h+YGPixelGrid.h, YG*ToString
  shim/shim.cpp        calls YGLogger (va_list) and throws std::logic_error
  check-symbols.sh     every YG function/global declared in the headers is exported
```

## Nodes: storage and handles

**Decision: each node is its own heap allocation (`Box<Node>`), and
`YGNodeRef` is a pointer to it.** PLAN.md sketched an arena owned by a
tree; the C API leaves no owner for one:

- `YGNodeNew()` takes no tree or context, nodes are freed one at a time, and
  move between trees with insert/remove;
- persistent cloning shares one child between several parents
  (`YGNodeClone` is shallow; the owner pointer says which tree may mutate it);
- one tree can mix nodes from several configs, and `YGNodeSetConfig` moves a
  node between configs.

An arena would have to be process-global (forbidden: no global state) or
support moving subtrees and cross-arena references, which is the same
pointer graph with more bookkeeping. Yoga allocates nodes individually too;
the layout loop's cost is not allocation but how much node data it touches.
Later perf work (milestone 4) can change *where* nodes are allocated (a
per-thread pool) or split hot/cold node data without changing semantics.

**Interior mutability.** Every field of `Node` is a `Cell` or sits behind an
`UnsafeCell` with narrow accessors (style, grid, child list). The engine
only ever holds `&Node`; no `&mut Node` exists anywhere. This is what makes
the C API's callback model sound in Rust: layout writes results through
`&Node` while a measure/baseline/dirtied/clone callback may read the same
node through its `YGNodeConstRef`, and pointers handed back to C (owner,
children) may be used to mutate later. Style is mutated only through
`unsafe fn Node::style_mut`, from C API entry points (never during layout).

**Liveness.** A node is alive until `tree::node_free*` (or
`YGNodeFree*`/`YGNodeFinalize`). Nodes reachable from a node in use (its
children, its owner) are assumed alive (`NodePtr::live`); freeing a node that
is still in use is undefined behaviour, exactly as in Yoga. Configs must
outlive their nodes (also as in Yoga). The default config
(`Config::get_default`, used by `YGNodeNew`) is an immutable static.

## The algorithm sees nodes only through `LayoutNode`

`algorithm/` is generic over `LayoutNode<'a>` (`node/layout_node.rs`), a
`Copy` handle exposing what Yoga's algorithm reads and writes on a
`yoga::Node`: style, layout results, config, children, owner,
clone-children-if-needed, dirty/has-new-layout flags, measure/min-content/
baseline callbacks, node type and the two baseline/containing-block flags.
`NodeExt` adds Yoga's derived node operations (`resolve_flex_grow`,
`set_position`, `process_dimensions`, ...) on top. barre's own nodes
implement it for `&'a Node`; monomorphisation makes that free.

This is for the React Native integration (docs/RN-SHIM.md §3): RN keeps its
C++ `yoga::Node`s and calls barre's algorithm through a host
implementation of the same trait. Rules that keep that possible:

- **Per-node layout state is a fixed-size, pointer-free blob.**
  `LayoutResults` is `repr(C)`, made of `Cell`s of plain data (no `Drop`, no
  pointers), and holds everything that persists on a node between layouts:
  results, the measure cache, computed flex basis + generation, auto-min
  size, plus Yoga's `lineIndex_` and `processedDimensions_`. A host can embed
  it as raw bytes and memcpy it when a node is cloned.
- **Style is a flat snapshot.** `Style` is `Copy` and `repr(C)`; grid
  properties are not in it (no layout reads them, and they need the heap), so
  a host can flatten its own style into it.
- **No globals beyond the generation counter** (`CURRENT_GENERATION_COUNT` in
  `calculate_layout.rs`, mirroring Yoga's `gCurrentGenerationCount`).
  Per-pass scratch lives in `LayoutData`, passed down the recursion.

Do not add algorithm code that touches `barre::Node` directly.

## The C ABI (`barre-ffi`)

- Every function and global declared by `vendor/yoga/yoga/Yoga.h` and the
  headers it includes is exported with its C name and signature
  (`crates/barre-ffi/check-symbols.sh` verifies it). `YGNodeRef` is
  `*mut barre::Node`, `YGConfigRef` is `*mut barre::Config`.
- Enums cross as their C integer values (`u32`) and are converted with
  `Enum::from_raw`; out-of-range values abort (Yoga's behaviour there is
  undefined). `YGValue` is `repr(C)`.
- Callbacks (`MeasureFunc`, `BaselineFunc`, `DirtiedFunc`,
  `CloneNodeFunc`) are stored as the C function pointers they are.
- **Errors.** Yoga's fatal assertions log at `Fatal` and throw
  `std::logic_error` (the suite checks this with `ASSERT_THROW`). barre's
  core returns `Err(FatalError)` from the same places (after logging); the
  exports that can fail are `extern "C-unwind"`, and `guard` turns the error
  into a `std::logic_error` thrown by `shim/shim.cpp`. This is why the
  profiles use `panic = "unwind"`.
- **Panics abort.** `not_implemented!` and any other panic abort the
  process: plain `extern "C"` exports abort when a panic reaches them, and
  `guard` aborts on panics in the `C-unwind` ones. A panic never unwinds into
  C. Internal invariants of the algorithm (Yoga's assertions inside
  `calculateLayoutImpl`) are plain panics.
- **Logging.** A config's logger is a `barre::debug::log::Logger` (a Rust fn
  plus an opaque `usize`). `YGConfigSetLogger` stores the C `YGLogger` there,
  and the shim calls it with format `"%s"` and the formatted message (Rust
  cannot build a `va_list`). Without a logger, messages go to stdout/stderr
  like Yoga's default logger.
- Yoga's event API (`yoga/event/`) is C++-only; barre has no events.

## Conventions

- **Names.** A Yoga function `fooBar` becomes `foo_bar`, in the file that
  mirrors Yoga's, with a doc comment naming it (`/// \`fooBar\`.`). Member
  functions of `yoga::Node` used by the algorithm live in `NodeExt` or
  `LayoutNode`; `yoga::Style` members on `Style`.
- **Undefined values.** Yoga's `FloatOptional` is a plain `f32` with NaN as
  undefined; `numeric.rs` has its helpers. FloatOptional's `==` (both
  undefined compare equal) is `optional_eq`; its `<`/`>` are plain `f32`
  comparisons (false if either side is NaN).
- **Float arithmetic** follows Yoga operation for operation: same order of
  operations, `f32` where Yoga uses `float`, `f64` where it uses `double`
  (pixel-grid rounding). Generated tests compare with `ASSERT_FLOAT_EQ`.
- **`not_implemented!("yogaName")`** for anything missing. Never return a
  plausible value instead.
- Stubs are cut so a ticket touches one file. Each stub file's module doc
  says what to port, from where, and which tests it should turn green.

## Stubs left for tickets

| ticket | file | stub | reached when | target tests |
|---|---|---|---|---|
| absolute layout | `algorithm/absolute_layout.rs` | `layout_absolute_child` (+ Yoga's static helpers) | a tree has a `position: absolute` node | `generated/YGAbsolutePositionTest`, `generated/YGStaticPositionTest`, absolute cases elsewhere (~110 tests) |
| baseline | `algorithm/baseline.rs` | `calculate_baseline` | baseline alignment is used (`is_baseline_layout` is implemented) | `YGAlignBaselineTest`, `YGBaselineFuncTest`, `align_baseline*` (~31) |
| display: contents | `node/layoutable_children.rs` | `skip_contents_nodes` + backtracking in `advance` | a `display: contents` child is iterated | `generated/YGDisplayContentsTest`, `YGContentsNodeHasNewLayoutTest`, contents cases (~14) |
| measure cache | `algorithm/cache.rs` | `can_use_cached_measurement` (not_implemented) and `find_cached_result` (returns `None`) | always: the spine never reuses a cached measurement or layout; results are still recorded in `LayoutResults` exactly as Yoga records them | `YGMeasureCacheTest`, measure-count checks in `YGMeasureModeTest`, `YGFlexBasisFitContentTest`, `YGScaleChangeTest`, plus `YGNodeCanUseCachedMeasurement` |

The cache stub is the one stub that does not abort: recomputing is correct,
only slower, and without it every test that lays a tree out twice would
crash. Its absence shows up as extra measure-callback calls.

Implemented around the stubs (so the stub boundary is clean):
`layout_absolute_descendants` (the walk to absolute descendants through
static children), `is_baseline_layout`, `cleanup_contents_nodes_recursively`,
`clone_contents_children_if_needed`, the `display: contents` child counts,
and the recording side of the cache.

## Status

At the spine commit, `./conf`: 584 of 752 in-scope tests pass. Every crash is
one of the stubs above (absolute 110, baseline 31, contents 14); the 13
failures are all measure-callback counts that need the cache.

## Known gaps and perf notes

- Style values are stored inline as `f32` + unit byte (8 bytes each, ~380
  bytes per style) rather than Yoga's 2-byte handles into a value pool.
  Behaviour matches the pool (non-finite values become undefined, `-0.0`
  is stored as `0.0`); shrinking it is perf work.
- `calculate_flex_line` allocates a `Vec` per line, as Yoga does; a reusable
  buffer in `LayoutData` would avoid it.
- Clang contracts `a * b + c` into FMA by default on arm64 and Rust never
  does, so barre and Yoga can differ in the last bits before pixel-grid
  rounding. No test has shown it yet.
