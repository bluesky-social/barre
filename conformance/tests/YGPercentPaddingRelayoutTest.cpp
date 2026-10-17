/*
 * Regression tests for yoga#1626 (https://github.com/react/yoga/issues/1626):
 * percentage padding is not updated when the parent's size changes, unless
 * the child is separately dirtied.
 *
 * Percentages of padding (and margin) resolve against the containing
 * block's width. Resizing the parent dirties only the parent; when the child
 * is then laid out with the same available size and measure modes as last
 * time, its measurement cache (canUseCachedMeasurement, keyed by available
 * size and mode, not by the owner size percentages resolve against) is hit
 * and the child keeps the padding resolved against the old width. A
 * maintainer comment in the issue agrees the cache key should include the
 * percentage-resolution size. Same family as yoga#1784 and yoga#1003.
 *
 * Expected values are the issue's hand computation: 50% of 10 = 5, then
 * 50% of 5 = 2.5. The issue's JS repro sets point scale factor 0 (no
 * rounding); so do these.
 *
 * Fail on Yoga at the vendor pin by design (except the controls): see
 * conformance/divergences.md.
 */

#include <gtest/gtest.h>
#include <yoga/Yoga.h>

#include <functional>
#include <string>

#include "util/LayoutInvariant.h"

namespace {

// The issue's first repro: the parent is the root.
void issueRepro(bool dirtyChildBeforeRelayout) {
  YGConfigRef config = YGConfigNew();
  YGConfigSetPointScaleFactor(config, 0);

  YGNodeRef parent = YGNodeNewWithConfig(config);
  YGNodeStyleSetWidth(parent, 10);
  YGNodeRef child = YGNodeNewWithConfig(config);
  YGNodeStyleSetWidth(child, 5);
  YGNodeStyleSetPaddingPercent(child, YGEdgeLeft, 50);
  YGNodeInsertChild(parent, child, 0);

  YGNodeCalculateLayout(parent, 10, 10, YGDirectionLTR);
  EXPECT_EQ(YGNodeLayoutGetPadding(child, YGEdgeLeft), 5);

  YGNodeStyleSetWidth(parent, 5);
  if (dirtyChildBeforeRelayout) {
    // The issue's workaround: an arbitrary style change dirties the child.
    YGNodeStyleSetBorder(child, YGEdgeTop, 0);
  }
  YGNodeCalculateLayout(parent, 10, 10, YGDirectionLTR);
  EXPECT_EQ(YGNodeLayoutGetPadding(child, YGEdgeLeft), 2.5f);

  YGNodeFreeRecursive(parent);
  YGConfigFree(config);
}

} // namespace

TEST(YGPercentPaddingRelayoutTest, percent_padding_updates_when_root_resized) {
  issueRepro(/*dirtyChildBeforeRelayout=*/false);
}

// Control: with the child dirtied too, Yoga gets 2.5 (the issue's third
// step). Passes on Yoga.
TEST(YGPercentPaddingRelayoutTest, percent_padding_updates_when_child_also_dirtied) {
  issueRepro(/*dirtyChildBeforeRelayout=*/true);
}

// The issue's second repro (from the comments): the resized parent is not
// the root.
TEST(YGPercentPaddingRelayoutTest, percent_padding_updates_when_inner_parent_resized) {
  YGConfigRef config = YGConfigNew();
  YGConfigSetPointScaleFactor(config, 0);

  YGNodeRef root = YGNodeNewWithConfig(config);
  YGNodeStyleSetWidth(root, 10);
  YGNodeRef parent = YGNodeNewWithConfig(config);
  YGNodeStyleSetWidth(parent, 10);
  YGNodeInsertChild(root, parent, 0);
  YGNodeRef child = YGNodeNewWithConfig(config);
  YGNodeStyleSetWidth(child, 5);
  YGNodeStyleSetPaddingPercent(child, YGEdgeLeft, 50);
  YGNodeInsertChild(parent, child, 0);

  YGNodeCalculateLayout(root, 10, 10, YGDirectionLTR);
  EXPECT_EQ(YGNodeLayoutGetPadding(child, YGEdgeLeft), 5);

  YGNodeStyleSetWidth(parent, 5);
  YGNodeCalculateLayout(root, 10, 10, YGDirectionLTR);
  EXPECT_EQ(YGNodeLayoutGetPadding(child, YGEdgeLeft), 2.5f);

  YGNodeFreeRecursive(root);
  YGConfigFree(config);
}

// The stale padding is visible in the layout, not just in the padding
// getter: a grandchild is placed at the child's left padding. Parent 200 ->
// 100 wide; the child (width 50, paddingLeft 50%) puts its 10x10 grandchild
// at left 100, then 50 (hand computation: 50% of the parent's width).
TEST(YGPercentPaddingRelayoutTest, percent_padding_moves_grandchild_after_resize) {
  YGNodeRef root = YGNodeNew();
  YGNodeStyleSetWidth(root, 400);
  YGNodeStyleSetHeight(root, 400);
  YGNodeRef parent = YGNodeNew();
  YGNodeStyleSetWidth(parent, 200);
  YGNodeInsertChild(root, parent, 0);
  YGNodeRef child = YGNodeNew();
  YGNodeStyleSetWidth(child, 50);
  YGNodeStyleSetPaddingPercent(child, YGEdgeLeft, 50);
  YGNodeInsertChild(parent, child, 0);
  YGNodeRef grandchild = YGNodeNew();
  YGNodeStyleSetWidth(grandchild, 10);
  YGNodeStyleSetHeight(grandchild, 10);
  YGNodeInsertChild(child, grandchild, 0);

  YGNodeCalculateLayout(root, YGUndefined, YGUndefined, YGDirectionLTR);
  EXPECT_EQ(YGNodeLayoutGetLeft(grandchild), 100);

  YGNodeStyleSetWidth(parent, 100);
  YGNodeCalculateLayout(root, YGUndefined, YGUndefined, YGDirectionLTR);
  EXPECT_EQ(YGNodeLayoutGetPadding(child, YGEdgeLeft), 50);
  EXPECT_EQ(YGNodeLayoutGetLeft(grandchild), 50);

  YGNodeFreeRecursive(root);
}

namespace {

// Like barre_test::expectSameLayout, plus the computed padding and margin on
// every edge (percentages of both resolve against the containing block).
void expectSameBoxes(
    YGNodeRef actual,
    YGNodeRef expected,
    const std::string& path = "root") {
  for (YGEdge edge : {YGEdgeLeft, YGEdgeTop, YGEdgeRight, YGEdgeBottom}) {
    EXPECT_TRUE(barre_test::sameFloat(
        YGNodeLayoutGetPadding(actual, edge),
        YGNodeLayoutGetPadding(expected, edge)))
        << path << " padding " << YGEdgeToString(edge) << ": relayout "
        << YGNodeLayoutGetPadding(actual, edge) << ", fresh "
        << YGNodeLayoutGetPadding(expected, edge);
    EXPECT_TRUE(barre_test::sameFloat(
        YGNodeLayoutGetMargin(actual, edge),
        YGNodeLayoutGetMargin(expected, edge)))
        << path << " margin " << YGEdgeToString(edge) << ": relayout "
        << YGNodeLayoutGetMargin(actual, edge) << ", fresh "
        << YGNodeLayoutGetMargin(expected, edge);
  }
  size_t n = YGNodeGetChildCount(actual);
  ASSERT_EQ(n, YGNodeGetChildCount(expected));
  for (size_t i = 0; i < n; i++) {
    expectSameBoxes(
        YGNodeGetChild(actual, i),
        YGNodeGetChild(expected, i),
        path + "/" + std::to_string(i));
  }
}

// root (500 x 500) > panel (style width w: the only node resized, so the
// only one dirtied) > subtree built by `build`. Laid out at each width in
// turn; every layout must equal a fresh tree's at that width.
void expectRelayoutMatchesFresh(const std::function<void(YGNodeRef)>& build) {
  auto make = [&](float w) {
    YGNodeRef root = YGNodeNew();
    YGNodeStyleSetWidth(root, 500);
    YGNodeStyleSetHeight(root, 500);
    YGNodeRef panel = YGNodeNew();
    YGNodeStyleSetWidth(panel, w);
    YGNodeInsertChild(root, panel, 0);
    build(panel);
    return root;
  };
  const float widths[] = {200, 100, 300, 150, 200, 260};
  YGNodeRef root = make(widths[0]);
  for (float w : widths) {
    SCOPED_TRACE("panel width " + std::to_string(static_cast<int>(w)));
    YGNodeStyleSetWidth(YGNodeGetChild(root, 0), w);
    YGNodeCalculateLayout(root, YGUndefined, YGUndefined, YGDirectionLTR);
    YGNodeRef fresh = make(w);
    YGNodeCalculateLayout(fresh, YGUndefined, YGUndefined, YGDirectionLTR);
    barre_test::expectSameLayout(root, fresh);
    expectSameBoxes(root, fresh);
    YGNodeFreeRecursive(fresh);
  }
  YGNodeFreeRecursive(root);
}

} // namespace

// Fixed-size children with percent padding and margin: their own available
// size doesn't change when the panel is resized, only the percentage base.
TEST(YGPercentPaddingRelayoutTest, relayout_matches_fresh_percent_padding_margin) {
  expectRelayoutMatchesFresh([](YGNodeRef panel) {
    YGNodeRef a = YGNodeNew();
    YGNodeStyleSetWidth(a, 40);
    YGNodeStyleSetHeight(a, 20);
    YGNodeStyleSetPaddingPercent(a, YGEdgeLeft, 10);
    YGNodeStyleSetPaddingPercent(a, YGEdgeTop, 5);
    YGNodeInsertChild(panel, a, 0);
    YGNodeRef aChild = YGNodeNew();
    YGNodeStyleSetHeight(aChild, 5);
    YGNodeInsertChild(a, aChild, 0);

    YGNodeRef b = YGNodeNew();
    YGNodeStyleSetWidth(b, 30);
    YGNodeStyleSetHeight(b, 10);
    YGNodeStyleSetMarginPercent(b, YGEdgeLeft, 20);
    YGNodeStyleSetMarginPercent(b, YGEdgeTop, 10);
    YGNodeInsertChild(panel, b, 1);

    YGNodeRef row = YGNodeNew();
    YGNodeStyleSetFlexDirection(row, YGFlexDirectionRow);
    YGNodeStyleSetHeight(row, 30);
    YGNodeInsertChild(panel, row, 2);
    YGNodeRef c = YGNodeNew();
    YGNodeStyleSetWidth(c, 20);
    YGNodeStyleSetPaddingPercent(c, YGEdgeAll, 4);
    YGNodeInsertChild(row, c, 0);
    YGNodeRef cChild = YGNodeNew();
    YGNodeStyleSetWidth(cChild, 4);
    YGNodeStyleSetHeight(cChild, 4);
    YGNodeInsertChild(c, cChild, 0);
  });
}
