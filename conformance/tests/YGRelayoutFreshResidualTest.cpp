/*
 * Relayout must equal a fresh layout (docs/PLAN.md): fuzzer finds
 * (docs/FUZZ.md) on which barre's relayout still differs from a fresh layout
 * while real Yoga's does not, through residuals of T-0006's flex-basis
 * anchor design that need a design decision rather than a local fix
 * (docs/FUZZ.md, "Known failure classes"). These fail on barre; they are
 * targets, not regressions. Same harness as YGRelayoutFreshFuzzTest.
 */

#include <gtest/gtest.h>
#include <yoga/Yoga.h>
#include <cmath>
#include <cstdint>
#include <limits>
#include <vector>
#include "util/LayoutInvariant.h"

namespace {

// The fuzzer's text-like measure function: `len` points of text (the node's
// context) in 20-point lines, wrapped to the available width.
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
  float w = std::isfinite(maxW) ? std::fmin(maxW, len) : len;
  float lines = w > 0 ? std::ceil(len / w) : 1.0f;
  YGSize out{w, lines * 20.0f};
  if (widthMode == YGMeasureModeExactly) {
    out.width = width;
  }
  if (heightMode == YGMeasureModeExactly) {
    out.height = height;
  } else if (heightMode == YGMeasureModeAtMost && out.height > height) {
    out.height = height;
  }
  return out;
}

void setText(YGNodeRef node, intptr_t len) {
  YGNodeSetContext(node, reinterpret_cast<void*>(len));
  YGNodeSetMeasureFunc(node, measureText);
}

YGNodeRef child(YGNodeRef owner, YGConfigRef config) {
  YGNodeRef node = YGNodeNewWithConfig(config);
  YGNodeInsertChild(owner, node, YGNodeGetChildCount(owner));
  return node;
}

// The root's width as its style width, laid out with an undefined available
// size (the fuzzer's mode 2).
void layoutAtStyleWidth(YGNodeRef root, float width, float /* height */) {
  YGNodeStyleSetWidth(root, width);
  YGNodeCalculateLayout(root, YGUndefined, YGUndefined, YGDirectionLTR);
}

struct Size {
  float width;
  float height;
};

void expectRelayoutMatchesFresh(
    YGNodeRef (*build)(YGConfigRef),
    void (*layoutAt)(YGNodeRef, float, float),
    const std::vector<Size>& sizes) {
  for (float scale : {0.0f, 1.0f}) {
    SCOPED_TRACE(::testing::Message() << "point scale " << scale);
    YGConfigRef config = YGConfigNew();
    YGConfigSetPointScaleFactor(config, scale);
    YGConfigSetErrata(config, YGErrataAll);
    YGNodeRef root = build(config);
    for (const Size& s : sizes) {
      SCOPED_TRACE(
          ::testing::Message() << "at " << s.width << "x" << s.height);
      layoutAt(root, s.width, s.height);
      YGNodeRef fresh = build(config);
      layoutAt(fresh, s.width, s.height);
      barre_test::expectSameLayout(root, fresh);
      YGNodeFreeRecursive(fresh);
    }
    YGNodeFreeRecursive(root);
    YGConfigFree(config);
  }
}

// root > row scroll view > growing row > column > text with `flexBasis: 1`.
YGNodeRef buildSupersededAnchor(YGConfigRef config) {
  YGNodeRef root = YGNodeNewWithConfig(config);
  YGNodeRef scroll = child(root, config);
  YGNodeStyleSetFlexDirection(scroll, YGFlexDirectionRow);
  YGNodeStyleSetOverflow(scroll, YGOverflowScroll);
  YGNodeRef row = child(scroll, config);
  YGNodeStyleSetFlexDirection(row, YGFlexDirectionRow);
  YGNodeStyleSetFlexGrow(row, 1);
  YGNodeRef column = child(row, config);
  YGNodeRef text = child(column, config);
  YGNodeStyleSetFlexBasis(text, 1);
  setText(text, 1);
  return root;
}

// root > row (align-items: flex-start) > row > [`aspectRatio: 1` > text 200,
// column > [`flex: 1`, text 500 with `flexBasis: 1`]].
YGNodeRef buildSkippedNonAnchorCall(YGConfigRef config) {
  YGNodeRef root = YGNodeNewWithConfig(config);
  YGNodeRef outer = child(root, config);
  YGNodeStyleSetFlexDirection(outer, YGFlexDirectionRow);
  YGNodeStyleSetAlignItems(outer, YGAlignFlexStart);
  YGNodeRef row = child(outer, config);
  YGNodeStyleSetFlexDirection(row, YGFlexDirectionRow);
  YGNodeRef square = child(row, config);
  YGNodeStyleSetAspectRatio(square, 1);
  setText(child(square, config), 200);
  YGNodeRef column = child(row, config);
  YGNodeStyleSetFlex(child(column, config), 1);
  YGNodeRef text = child(column, config);
  YGNodeStyleSetFlexBasis(text, 1);
  setText(text, 500);
  return root;
}

} // namespace

// Minimised from fuzzer seed 8266 (mode 2); seeds 259, 2422, 5766, 6496,
// 6723, 7046 and 7441 minimise to the same mechanism. In one layout the
// growing row gets two flex-basis anchor calls (main size undefined) that
// are both answered from an earlier layout's cache before its next real run.
// Only the second is replayed. In a fresh layout the first one ran too, and
// it made the column's first call of the layout (an anchor of the column,
// which measured the text's basis as its content height); the replay of the
// second makes the column's first call one with a definite main size, which
// writes the resolved basis instead.
TEST(YGRelayoutFreshResidualTest, superseded_anchor_not_replayed) {
  expectRelayoutMatchesFresh(
      buildSupersededAnchor,
      layoutAtStyleWidth,
      {{400, 800}, {100, 400}, {400, 800}});
}

// Minimised from fuzzer seed 254 (mode 2). A call of the inner row that is
// not a flex-basis anchor (its main size is definite) is answered from an
// earlier layout's cache. In a fresh layout it ran, and its run made anchor
// calls (undefined main size) into the column, which measured the text's
// basis at that width; nothing replays them, so the column keeps a basis
// measured at another width.
TEST(YGRelayoutFreshResidualTest, skipped_non_anchor_call_made_child_anchors) {
  expectRelayoutMatchesFresh(
      buildSkippedNonAnchorCall,
      layoutAtStyleWidth,
      {{400, 800}, {300, 400}, {400, 800}});
}
