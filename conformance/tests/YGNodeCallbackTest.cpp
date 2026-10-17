/*
 * Ported from vendor/yoga/tests/YGNodeCallbackTest.cpp at Yoga
 * 52f6d0bd3824296b8e852cb4fcdf3b9821ab2aa5 (the vendor/yoga pin). The
 * original is skipped (conformance/skip.txt) because it tests yoga::Node
 * directly. Test names are kept; the yoga::Node calls become their C API
 * equivalents:
 *
 * - Node::hasMeasureFunc / setMeasureFunc / hasBaselineFunc /
 *   setBaselineFunc are YGNodeHasMeasureFunc, YGNodeSetMeasureFunc, ...;
 * - Node::measure(w, wm, h, hm) and Node::baseline(w, h) have no C API: the
 *   C API reaches them through layout, so those tests lay out a tree in
 *   which layout calls the callback with the original arguments, assert that
 *   it did, and assert that the layout used the callback's result.
 */

#include <gtest/gtest.h>
#include <yoga/Yoga.h>

inline bool operator==(const YGSize& lhs, const YGSize& rhs) {
  return lhs.width == rhs.width && lhs.height == rhs.height;
}

TEST(Node, hasMeasureFunc_initial) {
  YGNodeRef n = YGNodeNew();
  ASSERT_FALSE(YGNodeHasMeasureFunc(n));
  YGNodeFree(n);
}

TEST(Node, hasMeasureFunc_with_measure_fn) {
  YGNodeRef n = YGNodeNew();
  YGNodeSetMeasureFunc(
      n, [](YGNodeConstRef, float, YGMeasureMode, float, YGMeasureMode) {
        return YGSize{};
      });
  ASSERT_TRUE(YGNodeHasMeasureFunc(n));
  YGNodeFree(n);
}

namespace {
struct MeasureCall {
  int calls = 0;
  float w = 0;
  YGMeasureMode wm = YGMeasureModeUndefined;
  float h = 0;
  YGMeasureMode hm = YGMeasureModeUndefined;
};
} // namespace

TEST(Node, measure_with_measure_fn) {
  YGNodeRef n = YGNodeNew();
  MeasureCall call;
  YGNodeSetContext(n, &call);

  YGNodeSetMeasureFunc(
      n,
      [](YGNodeConstRef node,
         float w,
         YGMeasureMode wm,
         float h,
         YGMeasureMode hm) {
        auto* c = static_cast<MeasureCall*>(YGNodeGetContext(node));
        *c = MeasureCall{c->calls + 1, w, wm, h, hm};
        return YGSize{w * static_cast<float>(wm), h / static_cast<float>(hm)};
      });

  // Node::measure(23, Exactly, 24, AtMost): a definite width of 23 and a
  // max-height of 24 (no height) make layout measure the node with exactly
  // these arguments, and size it to the result.
  YGNodeStyleSetWidth(n, 23);
  YGNodeStyleSetMaxHeight(n, 24);
  YGNodeCalculateLayout(n, YGUndefined, YGUndefined, YGDirectionLTR);

  ASSERT_EQ(call.calls, 1);
  ASSERT_EQ(call.w, 23);
  ASSERT_EQ(call.wm, YGMeasureModeExactly);
  ASSERT_EQ(call.h, 24);
  ASSERT_EQ(call.hm, YGMeasureModeAtMost);

  ASSERT_EQ(
      (YGSize{YGNodeLayoutGetWidth(n), YGNodeLayoutGetHeight(n)}),
      (YGSize{23, 12}));

  YGNodeFree(n);
}

TEST(Node, hasMeasureFunc_after_unset) {
  YGNodeRef n = YGNodeNew();
  YGNodeSetMeasureFunc(
      n, [](YGNodeConstRef, float, YGMeasureMode, float, YGMeasureMode) {
        return YGSize{};
      });

  YGNodeSetMeasureFunc(n, nullptr);
  ASSERT_FALSE(YGNodeHasMeasureFunc(n));
  YGNodeFree(n);
}

TEST(Node, hasBaselineFunc_initial) {
  YGNodeRef n = YGNodeNew();
  ASSERT_FALSE(YGNodeHasBaselineFunc(n));
  YGNodeFree(n);
}

TEST(Node, hasBaselineFunc_with_baseline_fn) {
  YGNodeRef n = YGNodeNew();
  YGNodeSetBaselineFunc(n, [](YGNodeConstRef, float, float) { return 0.0f; });
  ASSERT_TRUE(YGNodeHasBaselineFunc(n));
  YGNodeFree(n);
}

namespace {
struct BaselineCall {
  int calls = 0;
  float w = 0;
  float h = 0;
};
} // namespace

TEST(Node, baseline_with_baseline_fn) {
  // No pixel-grid rounding, so the fractional sizes and positions survive.
  YGConfigRef config = YGConfigNew();
  YGConfigSetPointScaleFactor(config, 0);

  // Node::baseline(1.25, 2.5): a node of that size, baseline-aligned in a
  // row next to a 10pt-tall sibling (whose baseline is its height, 10), has
  // its baseline func called with its size, and is placed so that its
  // baseline lines up with the sibling's: top = 10 - baseline.
  YGNodeRef root = YGNodeNewWithConfig(config);
  YGNodeStyleSetFlexDirection(root, YGFlexDirectionRow);
  YGNodeStyleSetAlignItems(root, YGAlignBaseline);

  YGNodeRef sibling = YGNodeNewWithConfig(config);
  YGNodeStyleSetWidth(sibling, 10);
  YGNodeStyleSetHeight(sibling, 10);
  YGNodeInsertChild(root, sibling, 0);

  YGNodeRef n = YGNodeNewWithConfig(config);
  YGNodeStyleSetWidth(n, 1.25f);
  YGNodeStyleSetHeight(n, 2.5f);
  BaselineCall call;
  YGNodeSetContext(n, &call);
  YGNodeSetBaselineFunc(n, [](YGNodeConstRef node, float w, float h) {
    auto* c = static_cast<BaselineCall*>(YGNodeGetContext(node));
    c->calls++;
    c->w = w;
    c->h = h;
    return w + h;
  });
  YGNodeInsertChild(root, n, 1);

  YGNodeCalculateLayout(root, YGUndefined, YGUndefined, YGDirectionLTR);

  ASSERT_GE(call.calls, 1);
  ASSERT_EQ(call.w, 1.25f);
  ASSERT_EQ(call.h, 2.5f);
  ASSERT_EQ(10.0f - YGNodeLayoutGetTop(n), 3.75f);

  YGNodeFreeRecursive(root);
  YGConfigFree(config);
}

TEST(Node, hasBaselineFunc_after_unset) {
  YGNodeRef n = YGNodeNew();
  YGNodeSetBaselineFunc(n, [](YGNodeConstRef, float, float) { return 0.0f; });

  YGNodeSetBaselineFunc(n, nullptr);
  ASSERT_FALSE(YGNodeHasBaselineFunc(n));
  YGNodeFree(n);
}
