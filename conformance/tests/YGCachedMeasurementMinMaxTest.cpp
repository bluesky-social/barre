/*
 * Regression tests for yoga#1003 (https://github.com/react/yoga/issues/1003):
 * an incorrect cached child size is used because canUseCachedMeasurement
 * (yoga/algorithm/Cache.cpp) does not respect min/max dimensions.
 *
 * canUseCachedMeasurement only compares available sizes and measure modes
 * (and the last computed size). When a child is first measured against an
 * undefined available height and then offered an AtMost height that its
 * last computed height fits in, the cached size is reused, even though the
 * child's percentage min/max height resolve against the owner's height and
 * now clamp it differently.
 *
 * The issue's repro (pastebin linked from the issue), all three cases: a row
 * (alignItems/alignContent flex-start) holding a child with height 10,
 * minHeight 40%, maxHeight 60%, width 50 and a measure func returning 0x0;
 * config with WebFlexBasis and point scale factor 3. Expected child height
 * in every case, from the issue's hand computation at owner height 30:
 * max(40% * 30, min(10, 60% * 30)) = max(12, min(10, 18)) = 12 (and the
 * issue's case 1, a fresh layout at 30, does give 12 on Yoga).
 *
 * Fails on Yoga at the vendor pin by design (except the controls): see
 * conformance/divergences.md.
 */

#include <gtest/gtest.h>
#include <yoga/Yoga.h>

#include <cmath>
#include <initializer_list>
#include <string>

namespace {

YGSize measureZero(
    YGNodeConstRef /*node*/,
    float /*w*/,
    YGMeasureMode /*wm*/,
    float /*h*/,
    YGMeasureMode /*hm*/) {
  return YGSize{0, 0};
}

struct Variant {
  bool issueConfig; // WebFlexBasis + point scale factor 3, as in the issue
  bool measureFunc; // child has the issue's 0x0 measure func
};

struct Tree {
  YGConfigRef config;
  YGNodeRef parent, child;
};

Tree build(Variant s) {
  Tree t{};
  t.config = YGConfigNew();
  if (s.issueConfig) {
    YGConfigSetExperimentalFeatureEnabled(
        t.config, YGExperimentalFeatureWebFlexBasis, true);
    YGConfigSetPointScaleFactor(t.config, 3.f);
  }
  t.parent = YGNodeNewWithConfig(t.config);
  YGNodeStyleSetFlexDirection(t.parent, YGFlexDirectionRow);
  YGNodeStyleSetAlignItems(t.parent, YGAlignFlexStart);
  YGNodeStyleSetAlignContent(t.parent, YGAlignFlexStart);
  t.child = YGNodeNewWithConfig(t.config);
  YGNodeStyleSetMinHeightPercent(t.child, 40.f);
  YGNodeStyleSetMaxHeightPercent(t.child, 60.f);
  YGNodeStyleSetHeight(t.child, 10.f);
  YGNodeStyleSetWidth(t.child, 50.f);
  if (s.measureFunc) {
    YGNodeSetMeasureFunc(t.child, measureZero);
  }
  YGNodeInsertChild(t.parent, t.child, 0);
  return t;
}

void freeTree(Tree& t) {
  YGNodeFreeRecursive(t.parent);
  YGConfigFree(t.config);
}

const Variant kVariants[] = {
    {true, true}, // the issue's exact setup
    {true, false},
    {false, true},
    {false, false},
};

std::string variantName(Variant s) {
  return std::string(s.issueConfig ? "WebFlexBasis + scale 3" : "default config") +
      (s.measureFunc ? ", measure func" : ", no measure func");
}

// Lays out at each available height in turn (the parent is the root, its
// width is 50 throughout) and expects the child's final height.
void layoutSequence(
    std::initializer_list<float> heights,
    float expectedChildHeight) {
  for (Variant s : kVariants) {
    SCOPED_TRACE(variantName(s));
    Tree t = build(s);
    for (float h : heights) {
      YGNodeCalculateLayout(t.parent, 50.f, h, YGDirectionLTR);
    }
    EXPECT_EQ(YGNodeLayoutGetHeight(t.child), expectedChildHeight);
    freeTree(t);
  }
}

} // namespace

// Case 1, "layout": a fresh layout at height 30. Control: passes on Yoga.
TEST(YGCachedMeasurementMinMaxTest, layout_at_30) {
  layoutSequence({30.f}, 12.f);
}

// Sanity of the clamp in the other cases: measured against an undefined
// height the percentages don't resolve and the child is its 10.
TEST(YGCachedMeasurementMinMaxTest, measure_undefined_height) {
  layoutSequence({NAN}, 10.f);
}

// Case 2, "measure & layout": the bug. Measured against an undefined height
// (child 10), then laid out at 30: the cached 10 is reused instead of 12.
TEST(YGCachedMeasurementMinMaxTest, measure_then_layout_at_30) {
  layoutSequence({NAN, 30.f}, 12.f);
}

// Case 3, "measure, layout, re-layout": a layout at 10 in between (child:
// max(4, min(10, 6)) = 6) invalidates the cache. Control: passes on Yoga.
TEST(YGCachedMeasurementMinMaxTest, measure_then_layout_at_10_then_30) {
  layoutSequence({NAN, 10.f, 30.f}, 12.f);
}

// The same with the parent not being the root: the parent's height goes
// from auto to 30 through its own style (only the parent is dirtied).
TEST(YGCachedMeasurementMinMaxTest, inner_parent_height_from_auto_to_30) {
  for (Variant s : kVariants) {
    SCOPED_TRACE(variantName(s));
    Tree t = build(s);
    YGNodeRef root = YGNodeNewWithConfig(t.config);
    YGNodeStyleSetWidth(root, 50.f);
    YGNodeStyleSetAlignItems(root, YGAlignFlexStart);
    YGNodeInsertChild(root, t.parent, 0);
    // (No assertion on this first layout: with an auto-height parent, Yoga
    // resolves the percentages against the parent's content height, which
    // CSS would not; that is not what this test is about.)
    YGNodeCalculateLayout(root, YGUndefined, YGUndefined, YGDirectionLTR);
    YGNodeStyleSetHeight(t.parent, 30.f);
    YGNodeCalculateLayout(root, YGUndefined, YGUndefined, YGDirectionLTR);
    EXPECT_EQ(YGNodeLayoutGetHeight(t.child), 12.f);
    YGNodeFreeRecursive(root);
    YGConfigFree(t.config);
  }
}
