/*
 * Regression tests for yoga#1659 (https://github.com/react/yoga/issues/1659):
 * YGNodeFree must mark its (former) parent dirty.
 *
 * Fixed in Yoga (the issue is closed): YGNodeFree now does
 * `owner->removeChild(node); node->setOwner(nullptr);
 * owner->markDirtyAndPropagate();` whenever the node has an owner, and
 * YGNodeFreeRecursive frees through it. Before the fix, YGNodeFree never
 * dirtied the parent and YGNodeFreeRecursive only did so incidentally (via
 * YGNodeRemoveChild, when the freed node had children), so the next
 * YGNodeCalculateLayout could skip the parent and keep the removed child's
 * space. These pin the fixed behaviour; they pass on Yoga.
 */

#include <gtest/gtest.h>
#include <yoga/Yoga.h>

#include "util/LayoutInvariant.h"

namespace {

int gDirtied = 0;

void onDirtied(YGNodeConstRef /*node*/) {
  gDirtied++;
}

// The issue's tree: root with child0, child1, child2 (which has child3).
// Each child is 10 tall so freeing one visibly moves the others.
struct Tree {
  YGNodeRef root, child0, child1, child2, child3;
};

Tree build() {
  Tree t{};
  t.root = YGNodeNew();
  YGNodeStyleSetWidth(t.root, 100);
  t.child0 = YGNodeNew();
  YGNodeStyleSetHeight(t.child0, 10);
  YGNodeInsertChild(t.root, t.child0, 0);
  t.child1 = YGNodeNew();
  YGNodeStyleSetHeight(t.child1, 10);
  YGNodeInsertChild(t.root, t.child1, 1);
  t.child2 = YGNodeNew();
  YGNodeInsertChild(t.root, t.child2, 2);
  t.child3 = YGNodeNew();
  YGNodeStyleSetHeight(t.child3, 10);
  YGNodeInsertChild(t.child2, t.child3, 0);
  return t;
}

} // namespace

// The issue's sequence, step by step.
TEST(YGNodeFreeDirtiesParentTest, issue_sequence) {
  Tree t = build();
  YGNodeCalculateLayout(t.root, YGUndefined, YGUndefined, YGDirectionLTR);
  ASSERT_FALSE(YGNodeIsDirty(t.root));

  YGNodeFree(t.child0);
  EXPECT_TRUE(YGNodeIsDirty(t.root)) << "YGNodeFree of a leaf";
  EXPECT_EQ(YGNodeGetChildCount(t.root), 2u);

  YGNodeCalculateLayout(t.root, YGUndefined, YGUndefined, YGDirectionLTR);
  ASSERT_FALSE(YGNodeIsDirty(t.root));
  YGNodeFreeRecursive(t.child1);
  EXPECT_TRUE(YGNodeIsDirty(t.root)) << "YGNodeFreeRecursive of a leaf";
  EXPECT_EQ(YGNodeGetChildCount(t.root), 1u);

  YGNodeCalculateLayout(t.root, YGUndefined, YGUndefined, YGDirectionLTR);
  ASSERT_FALSE(YGNodeIsDirty(t.root));
  YGNodeFreeRecursive(t.child2);
  EXPECT_TRUE(YGNodeIsDirty(t.root)) << "YGNodeFreeRecursive of a subtree";
  EXPECT_EQ(YGNodeGetChildCount(t.root), 0u);

  YGNodeFreeRecursive(t.root);
}

// Dirtiness propagates through the ancestors, and the root's dirtied
// callback fires.
TEST(YGNodeFreeDirtiesParentTest, free_dirties_all_ancestors) {
  YGNodeRef root = YGNodeNew();
  YGNodeRef mid = YGNodeNew();
  YGNodeInsertChild(root, mid, 0);
  YGNodeRef leaf = YGNodeNew();
  YGNodeStyleSetHeight(leaf, 10);
  YGNodeInsertChild(mid, leaf, 0);
  YGNodeCalculateLayout(root, YGUndefined, YGUndefined, YGDirectionLTR);
  ASSERT_FALSE(YGNodeIsDirty(root));
  ASSERT_FALSE(YGNodeIsDirty(mid));

  gDirtied = 0;
  YGNodeSetDirtiedFunc(root, onDirtied);
  YGNodeFree(leaf);
  EXPECT_TRUE(YGNodeIsDirty(mid));
  EXPECT_TRUE(YGNodeIsDirty(root));
  EXPECT_EQ(gDirtied, 1);

  YGNodeCalculateLayout(root, YGUndefined, YGUndefined, YGDirectionLTR);
  EXPECT_EQ(YGNodeLayoutGetHeight(mid), 0);
  EXPECT_EQ(YGNodeLayoutGetHeight(root), 0);
  YGNodeFreeRecursive(root);
}

// The point of dirtying: the next layout reflects the removal. After freeing
// child0 (YGNodeFree) and child1 (YGNodeFreeRecursive), the layout equals a
// fresh tree that only ever had child2 > child3.
TEST(YGNodeFreeDirtiesParentTest, relayout_after_free_matches_fresh) {
  Tree t = build();
  YGNodeCalculateLayout(t.root, YGUndefined, YGUndefined, YGDirectionLTR);
  ASSERT_EQ(YGNodeLayoutGetTop(t.child2), 20);
  ASSERT_EQ(YGNodeLayoutGetHeight(t.root), 30);

  YGNodeFree(t.child0);
  YGNodeCalculateLayout(t.root, YGUndefined, YGUndefined, YGDirectionLTR);
  EXPECT_EQ(YGNodeLayoutGetTop(t.child1), 0);
  EXPECT_EQ(YGNodeLayoutGetTop(t.child2), 10);
  EXPECT_EQ(YGNodeLayoutGetHeight(t.root), 20);

  YGNodeFreeRecursive(t.child1);
  YGNodeCalculateLayout(t.root, YGUndefined, YGUndefined, YGDirectionLTR);
  EXPECT_EQ(YGNodeLayoutGetTop(t.child2), 0);
  EXPECT_EQ(YGNodeLayoutGetHeight(t.root), 10);

  YGNodeRef fresh = YGNodeNew();
  YGNodeStyleSetWidth(fresh, 100);
  YGNodeRef c2 = YGNodeNew();
  YGNodeInsertChild(fresh, c2, 0);
  YGNodeRef c3 = YGNodeNew();
  YGNodeStyleSetHeight(c3, 10);
  YGNodeInsertChild(c2, c3, 0);
  YGNodeCalculateLayout(fresh, YGUndefined, YGUndefined, YGDirectionLTR);
  barre_test::expectSameLayout(t.root, fresh);

  YGNodeFreeRecursive(fresh);
  YGNodeFreeRecursive(t.root);
}
