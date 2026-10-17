# Divergences from Yoga

Behaviour where barre deliberately differs from Yoga because Yoga is wrong.
Each divergence has tests under `conformance/tests/` that assert the correct
behaviour, and so fail on real Yoga by design. Their IDs are listed in
`conformance/yoga-divergences.txt`, which `./conf --yoga` reads to report them
as `xfail` instead of failures (see docs/CONFORMANCE.md). Everything below was
checked against Yoga at the vendor pin, 52f6d0bd (`./conf --yoga barre/`).

Both bugs below are the class barre's design is meant to rule out
(docs/PLAN.md): layout state cached on a node (`computedFlexBasis`) without a
record of which layout, under which constraints, produced it.

## yoga#2019: stale `computedFlexBasis` reused across layouts

<https://github.com/react/yoga/issues/2019>

`layout.computedFlexBasis` survives from one `YGNodeCalculateLayout` to the
next and is cleared only when that very node is dirtied
(`Node::markDirtyAndPropagate`). Resizing the root dirties only the root. If
a relayout at a new root size hits an ancestor's measurement cache during a
max-content pass, the subtree is skipped and the child's basis is not
refreshed; a later definite-size pass then keeps it, because
`computeFlexBasisForChild` only writes a resolved basis when none is stored
(the stored value's `computedFlexBasisGeneration` is only consulted with
`WebFlexBasis`). In React Native: a `flex: 1` Text keeps its landscape height
after rotating back to portrait.

Tests (`conformance/tests/YGStaleFlexBasisTest.cpp`, the issue's minimal
reproduction: point scale factor 3, `YGErrataAll`, root resized through
min/max like React Native, 400x800 -> 800x400 -> 400x800; at every step T has
its text's height and the whole tree equals a fresh tree laid out at that
size):

| test | Yoga | why |
|---|---|---|
| `rotate_and_back_scrollview_two_wrappers` | fails | T and its wrapper W2 are 60 after rotating back; a fresh tree gives 100 |
| `rotate_and_back_scrollview_three_wrappers` | fails | same |
| `rotate_and_back_scrollview_one_wrapper` | passes | the stale basis is masked (W1's cache hit stretches T to the right height); regression guard |
| `rotate_and_back_no_scrollview_two_wrappers` | passes | no max-content pass, so no stale basis; regression guard |
| `rotate_and_back_scrollview_two_wrappers_text_marked_dirty` | passes | control: `YGNodeMarkDirty(T)` before each layout clears T's basis in Yoga and fixes it, so the failure is stale per-node state, not a wrong layout |

## yoga#1552: percent flex basis not recomputed when the parent's size changes

<https://github.com/react/yoga/issues/1552>

Same root cause through the other branch of `computeFlexBasisForChild`: a
child with a resolvable basis (`flexBasis: '25%'`, or `flexBasis: auto` with
a percent width or height) keeps the basis it resolved in the previous
layout, because it is not dirty (only the root's size changed) and its
`computedFlexBasis` is defined. So the percentage stays resolved against the
old parent size. In React Native: `flexBasis: '100%'` keeps the landscape
size after rotating to portrait. With `YGExperimentalFeatureWebFlexBasis`
Yoga recomputes the basis in each layout and gets these right.

Tests (`conformance/tests/YGFlexBasisRelayoutTest.cpp`):

| test | Yoga | why |
|---|---|---|
| `percent_flex_basis_recomputed_when_parent_height_changes` | fails | the thread's minimal reproduction: a 25% basis stays 250 when the parent grows from 1000 to 1200 (fresh: 300) |
| `flex_basis_100_percent_after_rotation` | fails | the issue's React Native snack (`absoluteFill` > `flexBasis: '100%'`): 400 tall instead of 800 after rotating to portrait |
| `relayout_matches_fresh_percent_flex_basis_column` | fails | |
| `relayout_matches_fresh_percent_flex_basis_row` | fails | |
| `relayout_matches_fresh_flex_grow_shrink` | fails | the stale basis comes from a percent `width` |
| `relayout_matches_fresh_wrap` | fails | |
| `relayout_matches_fresh_nested` | fails | |
| `relayout_matches_fresh_min_max_percent` | fails | |
| `relayout_matches_fresh_measure_funcs` | fails | |
| `relayout_matches_fresh_scroll_view_rows` | passes | regression guard |

The `relayout_matches_fresh_*` tests are a general oracle: lay a tree out at
a sequence of root sizes (resizing the root through its style width/height,
through the available size passed to `YGNodeCalculateLayout`, and through
min/max as React Native does) and require every layout to equal a fresh
tree's at the same size. Every Yoga failure among them was confirmed to be
this bug: each passes on Yoga when, before each relayout, only
`computedFlexBasis` is cleared on every node (without dirtying anything),
and each passes with `WebFlexBasis` enabled.

## Bugs from the Yoga issue tracker audit (docs/YOGA-ISSUES.md)

Regression tests for audited Yoga bugs, one file per issue, each citing the
source of its expected values (a browser result quoted in the issue, the
issue's hand computation, or one shown in the test). Where the correct
numbers are not known, the test is the "relayout == fresh layout"
invariant (`conformance/tests/util/LayoutInvariant.h`). Tests not listed as
failing pass on Yoga (controls and must-preserve behaviour).

### yoga#2038: flexShrink runaway from an FMA residual

<https://github.com/react/yoga/issues/2038>. The issue's 4-node repro: a
row's only shrinkable item has an `aspectRatio` basis (21.083334) and
`flexShrink: 3`. Freezing it at 0 in `distributeFreeSpaceFirstPass` should
cancel `totalFlexShrinkScaledFactors` to 0, but the subtraction is
contracted into an FMA that uses the unrounded product, leaving 1.9e-6:
just above the `1e-6f` guard from #1974, so `distributeFreeSpaceSecondPass`
divides the remaining free space by it. Reproduces in the conformance
runner's libyoga too (Apple clang, arm64, even at -O0). barre computes in
Rust, which never contracts, so it gets 0 x 0; the tests guard that.

`conformance/tests/YGFlexShrinkResidualTest.cpp`:

| test | Yoga | why |
|---|---|---|
| `aspect_ratio_box_flex_shrink_3` | fails | box 8.29e9 x 9.04e9; expected 0 x 0 (the issue) |
| `aspect_ratio_box_flex_shrink_table` | fails | the issue's table of flexShrink values |
| `shrinking_never_grows` | fails | flexShrink 0.25..250: a shrinking item never exceeds its basis; first fails at 2.25 |

### yoga#2035: free space computed from min-clamped flex bases

<https://github.com/react/yoga/issues/2035>. `calculateFlexLine` sums the
*clamped* bases into `sizeConsumed` and both distribution passes start from
the clamped basis, so an item raised by its `minWidth` gets its minimum plus
a full share. CSS 9.7 uses the flex base sizes. Expected values are Chrome's
(and Firefox's, Safari's), quoted in the issue: 40 / 80 and 200 / 50 / 50;
Yoga gives 60 / 60 and 233 / 34 / 33, in rows and columns, with the default
config, `YGErrataNone` and `useWebDefaults` (not errata-gated).

`conformance/tests/YGMinClampedFreeSpaceTest.cpp`: both tests fail on Yoga.

### yoga#1932: baseline of an unmeasured child

<https://github.com/react/yoga/issues/1932>. In an `alignItems: baseline`
row that is measured before it is laid out, an item with a definite size
is not laid out, so its baseline child's measured height is undefined and
`calculateBaseline` returns NaN; the line's height comes out 0. The issue's
tree: a 200-wide, padding-10 baseline row holding a 100x100 row with one
empty child. Expected (by hand, shown in the test; Chrome/Safari lay it out
normally per the issue's screenshots): row 200 x 120, item at (10, 10).
Yoga: row 20 tall (50 with a second, 30-tall item).

`conformance/tests/YGBaselineUnmeasuredChildTest.cpp`:

| test | Yoga | why |
|---|---|---|
| `baseline_row_in_content_sized_column` | fails | the issue's tree |
| `baseline_row_in_content_sized_column_with_available_size` | fails | |
| `baseline_row_in_fixed_size_column` | fails | the row's own height is auto, so it is still measured first |
| `baseline_row_two_items_in_content_sized_column` | fails | |
| `baseline_row_item_without_children` | passes | control: no unmeasured grandchild, no bug |

### yoga#1659: `YGNodeFree` must dirty its former parent (fixed in Yoga)

<https://github.com/react/yoga/issues/1659>, closed. Must-preserve tests
(`conformance/tests/YGNodeFreeDirtiesParentTest.cpp`): the issue's sequence
(`YGNodeFree` of a leaf, `YGNodeFreeRecursive` of a leaf and of a subtree
each dirty the root), propagation to all ancestors with the dirtied
callback, and relayout after freeing == a fresh tree. All pass on Yoga.

### yoga#1626: percentage padding stale after resize

<https://github.com/react/yoga/issues/1626>. Resizing a parent dirties only
the parent; the child, offered the same available size and mode, hits its
measurement cache, whose key does not include the size percentages resolve
against, and keeps padding (and margin) resolved against the old width.
Expected values are the issue's: 50% of 10 = 5, then 50% of 5 = 2.5.

`conformance/tests/YGPercentPaddingRelayoutTest.cpp`:

| test | Yoga | why |
|---|---|---|
| `percent_padding_updates_when_root_resized` | fails | the issue's repro: padding stays 5 |
| `percent_padding_updates_when_inner_parent_resized` | fails | the repro from the issue's comments |
| `percent_padding_moves_grandchild_after_resize` | fails | the stale padding is visible in positions: grandchild at 100, not 50 |
| `relayout_matches_fresh_percent_padding_margin` | fails | relayout == fresh, comparing padding and margin too |
| `percent_padding_updates_when_child_also_dirtied` | passes | control: the issue's workaround (dirty the child) |

### yoga#1784: measure func sees a stale available width after a resize

<https://github.com/react/yoga/issues/1784>. No minimal repro in the issue;
the tests build its described shape (a panel resized 1pt at a time,
140 -> 180 -> 140, holding a 35%-wide container with fixed paddings, a
fixed spacer and wrapping text) and check relayout == fresh at every step.
On Yoga they pass with no pixel rounding (point scale factor 0) and fail
with rounding (scale 1: first at panel width 141 in the issue's shape): the
text keeps a measurement from the previous size. Marking the texts dirty
before each relayout (the reporter's workaround) makes it pass, so it is
stale cached state, the same family as #1626 and #1003.

`conformance/tests/YGMeasureAvailableWidthRelayoutTest.cpp`:

| test | Yoga | why |
|---|---|---|
| `relayout_matches_fresh_issue_shape` | fails | root 1340 tall after relayout at 141, 1300 fresh |
| `relayout_matches_fresh_nested_percent_widths` | fails | nested % widths with fixed paddings (the reporter's follow-up) |
| `relayout_matches_fresh_row_with_flex` | fails | |
| `relayout_matches_fresh_issue_shape_texts_marked_dirty` | passes | control: texts dirtied before each relayout |
| `text_size_tracks_panel_width` | passes | hand-computed text size at scale 0 |

### yoga#1003: cached measurement ignores min/max

<https://github.com/react/yoga/issues/1003>. `canUseCachedMeasurement` reuses
a size measured against an undefined height when the new AtMost height
still fits it, though the child's percentage min/max height now clamp
differently. The issue's repro (all three of its cases, with its config
and without): child height 10, minHeight 40%, maxHeight 60%; expected 12 at
owner height 30 (the issue's computation: max(12, min(10, 18))).

`conformance/tests/YGCachedMeasurementMinMaxTest.cpp`:

| test | Yoga | why |
|---|---|---|
| `measure_then_layout_at_30` | fails | the issue's case 2: cached 10 reused |
| `inner_parent_height_from_auto_to_30` | fails | same through a non-root parent's style height |
| `layout_at_30`, `measure_then_layout_at_10_then_30`, `measure_undefined_height` | pass | the issue's cases 1 and 3, and the unclamped measurement |

### yoga#2006: first-pass running totals (fixed in Yoga, errata-gated)

<https://github.com/react/yoga/issues/2006>, closed: the fix divides by the
original flex totals; `YGErrataFlexFirstPassUsesRunningTotals` (set on new
configs by default) keeps the legacy result. Both are contract.
`conformance/tests/YGFirstPassRunningTotalsTest.cpp` pins the issue's
playground tree with the bit cleared (180 / 180 / 180 wherever the larger
minimum is) and set (60 / 30 / 30, Yoga's legacy result, hand-checked), and
the grow/max and shrink/min paths with the bit cleared for every item
order (CSS by hand). All pass on Yoga.

### yoga#2007: `computeMinContentMainSize` recurses twice per child

<https://github.com/react/yoga/issues/2007>. Correct layout, exponential
time: the container branch calls itself once per axis for every child, so a
chain of `depth` rows costs O(2^depth); reached from
`computeAutoMinMainSize` whenever `YGErrataMinSizeUndefinedInsteadOfAuto`
is not set. The issue's chain of rows (`YGErrataNone`), with a measured
leaf so the walk can be counted: Yoga makes exactly 2^(depth+1) measure
calls (2048 at depth 10, 2097152 at 20).

`conformance/tests/YGMinContentRecursionTest.cpp`:

| test | Yoga | why |
|---|---|---|
| `measure_calls_polynomial_in_depth` | fails | calls(20) <= 16 * calls(10) |
| `measure_calls_bounded_at_depth_20` | fails | calls <= 16 * depth^2 |
| `deep_chain_completes_with_right_layout` | times out | the issue's repro at depth 26 (10 s timeout) |
| `shallow_chain_layout` | passes | the expected layout at depth 6 |

## YGStaleFlexBasisSiblingsTest (yoga#2019 family)

Two `flex: 1` text siblings (2000pt and 800pt of text) inside two plain wrappers under a scroll
view, laid out at 400x800, 800x400, then 400x800 again. On Yoga at the pin, the first text comes
back 90 tall instead of the fresh layout's 100 after rotating back, and its sibling is placed
accordingly: the same stale `computedFlexBasis` as YGStaleFlexBasisTest. Found by the T-0006
fuzzer; it also rejects the shortcut fix of writing the resolved basis when the stored one is
stale. Asserts relayout == fresh layout at every step, with Yoga's default config and with RN's
(point scale 3, YGErrataAll).
