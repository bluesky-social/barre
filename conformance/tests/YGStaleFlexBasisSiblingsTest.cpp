/*
 * yoga#2019 family: two `flex: 1` text siblings of different lengths inside
 * two plain wrappers under a scroll view, laid out portrait -> landscape ->
 * portrait. Relayout must equal a fresh layout at every step.
 *
 * Found by a random-tree fuzzer while designing T-0006 (the flex-basis anchor
 * fix). A fix that writes the resolved basis when the stored one is stale
 * passes the single-text repro in YGStaleFlexBasisTest but fails this one:
 * the siblings' bases have to be what the skipped max-content pass would have
 * stored, not the resolved 0. Expected values are the fresh layouts, so the
 * test asserts the invariant rather than numbers.
 */

#include <gtest/gtest.h>
#include <yoga/Yoga.h>
#include <cmath>
#include <cstdint>
#include <limits>
#include "util/LayoutInvariant.h"

namespace {

// Text-like: `len` points of 20pt-high lines, taller when narrower. The
// length is stored in the node's context.
YGSize measureText(
    YGNodeConstRef node,
    float width,
    YGMeasureMode widthMode,
    float height,
    YGMeasureMode heightMode) {
  float len = static_cast<float>(
      reinterpret_cast<intptr_t>(YGNodeGetContext(node)));
  float maxW = widthMode == YGMeasureModeUndefined
      ? std::numeric_limits<float>::infinity()
      : width;
  float lines =
      std::isfinite(maxW) && maxW > 0 ? std::ceil(len / maxW) : 1.0f;
  YGSize out{std::isfinite(maxW) ? maxW : len, lines * 20.0f};
  if (heightMode == YGMeasureModeExactly) {
    out.height = height;
  } else if (heightMode == YGMeasureModeAtMost && out.height > height) {
    out.height = height;
  }
  if (widthMode == YGMeasureModeExactly) {
    out.width = width;
  }
  return out;
}

YGNodeRef text(YGConfigRef config, intptr_t len) {
  YGNodeRef t = YGNodeNewWithConfig(config);
  YGNodeStyleSetFlex(t, 1);
  YGNodeSetContext(t, reinterpret_cast<void*>(len));
  YGNodeSetMeasureFunc(t, measureText);
  return t;
}

YGNodeRef build(YGConfigRef config) {
  YGNodeRef root = YGNodeNewWithConfig(config);
  YGNodeRef sv = YGNodeNewWithConfig(config);
  YGNodeStyleSetFlexGrow(sv, 1);
  YGNodeStyleSetFlexShrink(sv, 1);
  YGNodeStyleSetOverflow(sv, YGOverflowScroll);
  YGNodeInsertChild(root, sv, 0);
  YGNodeRef cc = YGNodeNewWithConfig(config);
  YGNodeInsertChild(sv, cc, 0);
  YGNodeRef w1 = YGNodeNewWithConfig(config);
  YGNodeInsertChild(cc, w1, 0);
  YGNodeRef w2 = YGNodeNewWithConfig(config);
  YGNodeInsertChild(w1, w2, 0);
  YGNodeInsertChild(w2, text(config, 2000), 0);
  YGNodeInsertChild(w2, text(config, 800), 1);
  return root;
}

void layoutAt(YGNodeRef root, float w, float h) {
  YGNodeStyleSetMinWidth(root, w);
  YGNodeStyleSetMaxWidth(root, w);
  YGNodeStyleSetMinHeight(root, h);
  YGNodeStyleSetMaxHeight(root, h);
  YGNodeCalculateLayout(root, w, h, YGDirectionLTR);
}

void rotateAndBack(YGConfigRef config) {
  YGNodeRef root = build(config);
  const float sizes[3][2] = {{400, 800}, {800, 400}, {400, 800}};
  for (const auto& s : sizes) {
    layoutAt(root, s[0], s[1]);
    YGNodeRef fresh = build(config);
    layoutAt(fresh, s[0], s[1]);
    barre_test::expectSameLayout(root, fresh);
    YGNodeFreeRecursive(fresh);
  }
  YGNodeFreeRecursive(root);
}

} // namespace

TEST(YGStaleFlexBasisSiblingsTest, two_texts_rotate_and_back) {
  YGConfigRef config = YGConfigNew();
  rotateAndBack(config);
  YGConfigFree(config);
}

// As React Native configures Yoga.
TEST(YGStaleFlexBasisSiblingsTest, two_texts_rotate_and_back_rn_config) {
  YGConfigRef config = YGConfigNew();
  YGConfigSetPointScaleFactor(config, 3.0f);
  YGConfigSetErrata(config, YGErrataAll);
  rotateAndBack(config);
  YGConfigFree(config);
}
