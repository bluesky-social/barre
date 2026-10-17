/*
 * Ported from vendor/yoga/tests/YGDirtiedTest.cpp at Yoga
 * 52f6d0bd3824296b8e852cb4fcdf3b9821ab2aa5 (the vendor/yoga pin). The
 * original is skipped (conformance/skip.txt) because it dirties nodes through
 * yoga::Node::setDirty / markDirtyAndPropagate. Test names and assertions are
 * kept; the dirtying goes through the C API instead:
 *
 * - explicitly dirtying a leaf is YGNodeMarkDirty, which requires the leaf to
 *   have a measure func, so the dirtied leaves get one (they also have a fixed
 *   width and height, so it is never called and the layout is unchanged);
 * - markDirtyAndPropagate on a node with children is what Yoga's style
 *   setters do when the value changes, so it is a style change.
 */

#include <gtest/gtest.h>
#include <yoga/Yoga.h>

static void _dirtied(YGNodeConstRef node) {
  int* dirtiedCount = (int*)YGNodeGetContext(node);
  (*dirtiedCount)++;
}

static YGSize _measure(
    YGNodeConstRef /*node*/,
    float /*width*/,
    YGMeasureMode /*widthMode*/,
    float /*height*/,
    YGMeasureMode /*heightMode*/) {
  return YGSize{0, 0};
}

TEST(YogaTest, dirtied) {
  YGNodeRef root = YGNodeNew();
  YGNodeStyleSetAlignItems(root, YGAlignFlexStart);
  YGNodeStyleSetWidth(root, 100);
  YGNodeStyleSetHeight(root, 100);
  YGNodeSetMeasureFunc(root, _measure);

  YGNodeCalculateLayout(root, YGUndefined, YGUndefined, YGDirectionLTR);

  int dirtiedCount = 0;
  YGNodeSetContext(root, &dirtiedCount);
  YGNodeSetDirtiedFunc(root, _dirtied);

  ASSERT_EQ(0, dirtiedCount);

  // `_dirtied` MUST be called in case of explicit dirtying.
  YGNodeMarkDirty(root);
  ASSERT_EQ(1, dirtiedCount);

  // `_dirtied` MUST be called ONCE.
  YGNodeMarkDirty(root);
  ASSERT_EQ(1, dirtiedCount);

  YGNodeFreeRecursive(root);
}

TEST(YogaTest, dirtied_propagation) {
  YGNodeRef root = YGNodeNew();
  YGNodeStyleSetAlignItems(root, YGAlignFlexStart);
  YGNodeStyleSetWidth(root, 100);
  YGNodeStyleSetHeight(root, 100);

  YGNodeRef root_child0 = YGNodeNew();
  YGNodeStyleSetWidth(root_child0, 50);
  YGNodeStyleSetHeight(root_child0, 20);
  YGNodeSetMeasureFunc(root_child0, _measure);
  YGNodeInsertChild(root, root_child0, 0);

  YGNodeRef root_child1 = YGNodeNew();
  YGNodeStyleSetWidth(root_child1, 50);
  YGNodeStyleSetHeight(root_child1, 20);
  YGNodeInsertChild(root, root_child1, 1);

  YGNodeCalculateLayout(root, YGUndefined, YGUndefined, YGDirectionLTR);

  int dirtiedCount = 0;
  YGNodeSetContext(root, &dirtiedCount);
  YGNodeSetDirtiedFunc(root, _dirtied);

  ASSERT_EQ(0, dirtiedCount);

  // `_dirtied` MUST be called for the first time.
  YGNodeMarkDirty(root_child0);
  ASSERT_EQ(1, dirtiedCount);

  // `_dirtied` must NOT be called for the second time.
  YGNodeMarkDirty(root_child0);
  ASSERT_EQ(1, dirtiedCount);

  YGNodeFreeRecursive(root);
}

TEST(YogaTest, dirtied_hierarchy) {
  YGNodeRef root = YGNodeNew();
  YGNodeStyleSetAlignItems(root, YGAlignFlexStart);
  YGNodeStyleSetWidth(root, 100);
  YGNodeStyleSetHeight(root, 100);

  YGNodeRef root_child0 = YGNodeNew();
  YGNodeStyleSetWidth(root_child0, 50);
  YGNodeStyleSetHeight(root_child0, 20);
  YGNodeSetMeasureFunc(root_child0, _measure);
  YGNodeInsertChild(root, root_child0, 0);

  YGNodeRef root_child1 = YGNodeNew();
  YGNodeStyleSetWidth(root_child1, 50);
  YGNodeStyleSetHeight(root_child1, 20);
  YGNodeSetMeasureFunc(root_child1, _measure);
  YGNodeInsertChild(root, root_child1, 1);

  YGNodeCalculateLayout(root, YGUndefined, YGUndefined, YGDirectionLTR);

  int dirtiedCount = 0;
  YGNodeSetContext(root_child0, &dirtiedCount);
  YGNodeSetDirtiedFunc(root_child0, _dirtied);

  ASSERT_EQ(0, dirtiedCount);

  // `_dirtied` must NOT be called for descendants.
  // (root has children: dirty it with a style change)
  YGNodeStyleSetWidth(root, 110);
  ASSERT_TRUE(YGNodeIsDirty(root));
  ASSERT_EQ(0, dirtiedCount);

  // `_dirtied` must NOT be called for the sibling node.
  YGNodeMarkDirty(root_child1);
  ASSERT_EQ(0, dirtiedCount);

  // `_dirtied` MUST be called in case of explicit dirtying.
  YGNodeMarkDirty(root_child0);
  ASSERT_EQ(1, dirtiedCount);

  YGNodeFreeRecursive(root);
}
