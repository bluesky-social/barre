/*
 * Ported from vendor/yoga/tests/StyleTest.cpp at Yoga
 * 52f6d0bd3824296b8e852cb4fcdf3b9821ab2aa5 (the vendor/yoga pin). The
 * original is skipped (conformance/skip.txt) because it tests yoga::Style's
 * computed values directly. Through the C API those computed values are what
 * layout uses and reports: the computed padding, border and margin are
 * YGNodeLayoutGetPadding/Border/Margin after layout, and the computed gap is
 * the space layout leaves between two children. Test names and asserted
 * values are kept (the original's static_assert on sizeof(Style) is about
 * Yoga's memory layout and is dropped).
 */

#include <gtest/gtest.h>
#include <yoga/Yoga.h>

TEST(Style, computed_padding_is_floored) {
  YGNodeRef node = YGNodeNew();
  YGNodeStyleSetPadding(node, YGEdgeAll, -1.0f);
  YGNodeCalculateLayout(node, YGUndefined, YGUndefined, YGDirectionLTR);
  // inline-start padding of a row, LTR
  auto paddingStart = YGNodeLayoutGetPadding(node, YGEdgeLeft);
  ASSERT_EQ(paddingStart, 0.0f);
  YGNodeFree(node);
}

TEST(Style, computed_border_is_floored) {
  YGNodeRef node = YGNodeNew();
  YGNodeStyleSetBorder(node, YGEdgeAll, -1.0f);
  YGNodeCalculateLayout(node, YGUndefined, YGUndefined, YGDirectionLTR);
  // inline-start border of a row, LTR
  auto borderStart = YGNodeLayoutGetBorder(node, YGEdgeLeft);
  ASSERT_EQ(borderStart, 0.0f);
  YGNodeFree(node);
}

TEST(Style, computed_gap_is_floored) {
  YGNodeRef root = YGNodeNew();
  YGNodeStyleSetFlexDirection(root, YGFlexDirectionRow);
  YGNodeStyleSetGap(root, YGGutterColumn, -1.0f);
  YGNodeRef first = YGNodeNew();
  YGNodeStyleSetWidth(first, 10);
  YGNodeInsertChild(root, first, 0);
  YGNodeRef second = YGNodeNew();
  YGNodeStyleSetWidth(second, 10);
  YGNodeInsertChild(root, second, 1);
  YGNodeCalculateLayout(root, YGUndefined, YGUndefined, YGDirectionLTR);
  // the gap between columns of a row
  auto gapBetweenColumns = YGNodeLayoutGetLeft(second) -
      (YGNodeLayoutGetLeft(first) + YGNodeLayoutGetWidth(first));
  ASSERT_EQ(gapBetweenColumns, 0.0f);
  YGNodeFreeRecursive(root);
}

TEST(Style, computed_margin_is_not_floored) {
  YGNodeRef root = YGNodeNew();
  YGNodeRef node = YGNodeNew();
  YGNodeStyleSetMargin(node, YGEdgeAll, -1.0f);
  YGNodeInsertChild(root, node, 0);
  YGNodeCalculateLayout(root, YGUndefined, YGUndefined, YGDirectionLTR);
  // inline-start margin of a row, LTR
  auto marginStart = YGNodeLayoutGetMargin(node, YGEdgeLeft);
  ASSERT_EQ(marginStart, -1.0f);
  YGNodeFreeRecursive(root);
}
