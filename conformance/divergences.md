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
