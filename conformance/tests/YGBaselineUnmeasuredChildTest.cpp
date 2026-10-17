/*
 * Regression tests for yoga#1932 (https://github.com/react/yoga/issues/1932):
 * calculateBaseline(node) may return NaN when the node's baseline child has
 * not been measured.
 *
 * calculateBaseline (yoga/algorithm/Baseline.cpp) falls back to
 * `baselineChild->getLayout().measuredDimension(Dimension::Height)` plus
 * the child's position. When a row with `alignItems: baseline` is itself
 * only *measured* (its parent is sized by content, so the first pass over it
 * is a measure-only pass) and one of its items has a definite size, that
 * item's own children are never laid out in that pass; its baseline child's
 * measured height is still undefined (NaN), the line's max ascent/descent
 * become NaN and the row's measured cross size comes out wrong.
 *
 * The issue's tree (playground, useWebDefaults: false):
 *
 *   <Node style={{flexDirection: 'column'}}>                        outer
 *     <Node style={{flexDirection: 'row', width: 200, padding: 10,
 *                   alignItems: 'baseline'}}>                         row
 *       <Node style={{flexDirection: 'row', width: 100, height: 100}}> box
 *         <Node />                                                    leaf
 *
 * Yoga gives the baseline row a height (line height) of 0 + padding; Chrome
 * and Safari lay it out normally (screenshots in the issue). Expected values
 * by hand, with Yoga's border-box sizing: the row has a single item, so
 * baseline alignment puts it at the cross-start content edge, (10, 10), and
 * the row's height is padding + the item's 100 = 120; the outer column is
 * sized by content, 200 x 120. The leaf has no size and is stretched by the
 * box's row (alignItems: stretch) to 0 x 100 at (0, 0).
 *
 * Fails on Yoga at the vendor pin by design: see conformance/divergences.md.
 */

#include <gtest/gtest.h>
#include <yoga/Yoga.h>

#include <cmath>

namespace {

struct Tree {
  YGNodeRef outer, row, box, leaf;
};

Tree build(YGConfigRef config) {
  Tree t{};
  t.outer = YGNodeNewWithConfig(config);
  YGNodeStyleSetFlexDirection(t.outer, YGFlexDirectionColumn);

  t.row = YGNodeNewWithConfig(config);
  YGNodeStyleSetFlexDirection(t.row, YGFlexDirectionRow);
  YGNodeStyleSetWidth(t.row, 200);
  YGNodeStyleSetPadding(t.row, YGEdgeAll, 10);
  YGNodeStyleSetAlignItems(t.row, YGAlignBaseline);
  YGNodeInsertChild(t.outer, t.row, 0);

  t.box = YGNodeNewWithConfig(config);
  YGNodeStyleSetFlexDirection(t.box, YGFlexDirectionRow);
  YGNodeStyleSetWidth(t.box, 100);
  YGNodeStyleSetHeight(t.box, 100);
  YGNodeInsertChild(t.row, t.box, 0);

  t.leaf = YGNodeNewWithConfig(config);
  YGNodeInsertChild(t.box, t.leaf, 0);
  return t;
}

void expectRect(YGNodeRef n, float l, float tp, float w, float h) {
  EXPECT_EQ(YGNodeLayoutGetLeft(n), l);
  EXPECT_EQ(YGNodeLayoutGetTop(n), tp);
  EXPECT_EQ(YGNodeLayoutGetWidth(n), w);
  EXPECT_EQ(YGNodeLayoutGetHeight(n), h);
}

void expectIssueLayout(const Tree& t) {
  {
    SCOPED_TRACE("outer");
    expectRect(t.outer, 0, 0, 200, 120);
  }
  {
    SCOPED_TRACE("row");
    expectRect(t.row, 0, 0, 200, 120);
  }
  {
    SCOPED_TRACE("box");
    expectRect(t.box, 10, 10, 100, 100);
  }
  {
    SCOPED_TRACE("leaf");
    expectRect(t.leaf, 0, 0, 0, 100);
  }
}

} // namespace

// The issue's tree, laid out with no available size (the playground's
// default): the row is measured first because the outer column is sized by
// its content.
TEST(YGBaselineUnmeasuredChildTest, baseline_row_in_content_sized_column) {
  YGConfigRef config = YGConfigNew();
  Tree t = build(config);
  YGNodeCalculateLayout(t.outer, YGUndefined, YGUndefined, YGDirectionLTR);
  expectIssueLayout(t);
  YGNodeFreeRecursive(t.outer);
  YGConfigFree(config);
}

// Same, with an available size passed in (the outer column is still sized
// by content along its main axis).
TEST(YGBaselineUnmeasuredChildTest, baseline_row_in_content_sized_column_with_available_size) {
  YGConfigRef config = YGConfigNew();
  Tree t = build(config);
  YGNodeCalculateLayout(t.outer, 500, YGUndefined, YGDirectionLTR);
  EXPECT_EQ(YGNodeLayoutGetHeight(t.row), 120);
  EXPECT_EQ(YGNodeLayoutGetLeft(t.box), 10);
  EXPECT_EQ(YGNodeLayoutGetTop(t.box), 10);
  EXPECT_FALSE(std::isnan(YGNodeLayoutGetHeight(t.outer)));
  EXPECT_EQ(YGNodeLayoutGetHeight(t.outer), 120);
  YGNodeFreeRecursive(t.outer);
  YGConfigFree(config);
}

// Two items, so baseline alignment actually moves something. The box's
// baseline is its first child's: the leaf is stretched to the box's 100
// height and has no baseline of its own, so its baseline is its bottom edge
// (Yoga: a node without children or baseline func has baseline = its
// height), 100. The second item is a 50 x 30 leaf: its baseline is 30. The
// max ascent is 100 and the max descent 0, so the line is 100 tall (row
// 200 x 120, as before) and the second item's top is 10 + 100 - 30 = 80,
// its left 10 + 100 = 110.
TEST(YGBaselineUnmeasuredChildTest, baseline_row_two_items_in_content_sized_column) {
  YGConfigRef config = YGConfigNew();
  Tree t = build(config);
  YGNodeRef small = YGNodeNewWithConfig(config);
  YGNodeStyleSetWidth(small, 50);
  YGNodeStyleSetHeight(small, 30);
  YGNodeInsertChild(t.row, small, 1);

  YGNodeCalculateLayout(t.outer, YGUndefined, YGUndefined, YGDirectionLTR);
  expectIssueLayout(t);
  {
    SCOPED_TRACE("small");
    expectRect(small, 110, 80, 50, 30);
  }
  YGNodeFreeRecursive(t.outer);
  YGConfigFree(config);
}

// The row's parent has a definite size, but the row's height is auto, so it
// is still measured before it is laid out: same bug.
TEST(YGBaselineUnmeasuredChildTest, baseline_row_in_fixed_size_column) {
  YGConfigRef config = YGConfigNew();
  Tree t = build(config);
  YGNodeStyleSetHeight(t.outer, 120);
  YGNodeStyleSetWidth(t.outer, 200);
  YGNodeCalculateLayout(t.outer, YGUndefined, YGUndefined, YGDirectionLTR);
  expectIssueLayout(t);
  YGNodeFreeRecursive(t.outer);
  YGConfigFree(config);
}

// Control: without the leaf, the box is its own baseline child-less item
// (baseline = its own height, which is definite), nothing unmeasured is
// read, and Yoga gets it right. Shows the failure above is the unmeasured
// grandchild, not baseline alignment as such.
TEST(YGBaselineUnmeasuredChildTest, baseline_row_item_without_children) {
  YGConfigRef config = YGConfigNew();
  Tree t = build(config);
  YGNodeRemoveChild(t.box, t.leaf);
  YGNodeFree(t.leaf);
  YGNodeCalculateLayout(t.outer, YGUndefined, YGUndefined, YGDirectionLTR);
  expectRect(t.outer, 0, 0, 200, 120);
  expectRect(t.row, 0, 0, 200, 120);
  expectRect(t.box, 10, 10, 100, 100);
  YGNodeFreeRecursive(t.outer);
  YGConfigFree(config);
}
