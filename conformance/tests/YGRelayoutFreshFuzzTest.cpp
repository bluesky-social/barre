/*
 * Relayout must equal a fresh layout (docs/PLAN.md): trees found by the
 * relayout == fresh fuzzer (docs/FUZZ.md, `fuzzer/cases/`) on which barre's
 * relayout differed from a fresh layout of the same tree while real Yoga's
 * did not. Each test is one mechanism in barre's own cross-layout cache
 * logic (T-0006's flex-basis anchor, the yoga#1932 baseline layout), traced
 * from the minimised case it names. The last test guards the fix of the
 * second against a regression the fuzzer found while it was being made.
 *
 * Every tree is laid out at each size in turn and compared after every step
 * with a freshly built tree laid out directly at that size, at point scales
 * 0 and 1, with all errata on (as the fuzzer runs it). Expected values are
 * the fresh layouts, so the tests assert the invariant rather than numbers.
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

// The root pinned through min/max width and height, then laid out at that
// size, as React Native does (the fuzzer's mode 0).
void layoutPinned(YGNodeRef root, float width, float height) {
  YGNodeStyleSetMinWidth(root, width);
  YGNodeStyleSetMaxWidth(root, width);
  YGNodeStyleSetMinHeight(root, height);
  YGNodeStyleSetMaxHeight(root, height);
  YGNodeCalculateLayout(root, width, height, YGDirectionLTR);
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

// root > row scroll view > shrinking scroll view (column) > column >
// text with `flexBasis: 1`.
YGNodeRef buildEarlierEntryHitTwice(YGConfigRef config) {
  YGNodeRef root = YGNodeNewWithConfig(config);
  YGNodeRef row = child(root, config);
  YGNodeStyleSetFlexDirection(row, YGFlexDirectionRow);
  YGNodeStyleSetOverflow(row, YGOverflowScroll);
  YGNodeRef scroll = child(row, config);
  YGNodeStyleSetFlexShrink(scroll, 1);
  YGNodeStyleSetOverflow(scroll, YGOverflowScroll);
  YGNodeRef column = child(scroll, config);
  YGNodeRef text = child(column, config);
  YGNodeStyleSetFlexBasis(text, 1);
  setText(text, 300);
  return root;
}

// root > row (align-items: flex-start) > row > [text 250, column with
// `flexBasis: 1` and `height: 1%` > text 1 with `flexBasis: 1`].
YGNodeRef buildMaxContentWithDefiniteSize(YGConfigRef config) {
  YGNodeRef root = YGNodeNewWithConfig(config);
  YGNodeRef outer = child(root, config);
  YGNodeStyleSetFlexDirection(outer, YGFlexDirectionRow);
  YGNodeStyleSetAlignItems(outer, YGAlignFlexStart);
  YGNodeRef row = child(outer, config);
  YGNodeStyleSetFlexDirection(row, YGFlexDirectionRow);
  setText(child(row, config), 250);
  YGNodeRef column = child(row, config);
  YGNodeStyleSetFlexBasis(column, 1);
  YGNodeStyleSetHeightPercent(column, 1);
  YGNodeRef text = child(column, config);
  YGNodeStyleSetFlexBasis(text, 1);
  setText(text, 1);
  return root;
}

// root > row > row > baseline-aligned column (`height: 1%`, `maxHeight: 1`)
// > child with `flexBasis: 1`.
YGNodeRef buildBaselineChildLaidOutDuringMeasure(YGConfigRef config) {
  YGNodeRef root = YGNodeNewWithConfig(config);
  YGNodeRef outer = child(root, config);
  YGNodeStyleSetFlexDirection(outer, YGFlexDirectionRow);
  YGNodeRef row = child(outer, config);
  YGNodeStyleSetFlexDirection(row, YGFlexDirectionRow);
  YGNodeRef item = child(row, config);
  YGNodeStyleSetHeightPercent(item, 1);
  YGNodeStyleSetMaxHeight(item, 1);
  YGNodeStyleSetAlignSelf(item, YGAlignBaseline);
  YGNodeStyleSetFlexBasis(child(item, config), 1);
  return root;
}

// root > row > column (`height: 1`) > [`flex: 1`, column > child with
// `flexGrow: 1`, `flexShrink: 1`, `flexBasis: 1`].
YGNodeRef buildSingleFlexChildFromEarlierLayout(YGConfigRef config) {
  YGNodeRef root = YGNodeNewWithConfig(config);
  YGNodeRef row = child(root, config);
  YGNodeStyleSetFlexDirection(row, YGFlexDirectionRow);
  YGNodeRef column = child(row, config);
  YGNodeStyleSetHeight(column, 1);
  YGNodeStyleSetFlex(child(column, config), 1);
  YGNodeRef inner = child(column, config);
  YGNodeRef flexible = child(inner, config);
  YGNodeStyleSetFlexGrow(flexible, 1);
  YGNodeStyleSetFlexShrink(flexible, 1);
  YGNodeStyleSetFlexBasis(flexible, 1);
  return root;
}

// root > wrapping row > [`flexBasis: 1`, `minHeight: 90`; `flexBasis: 100`,
// `aspectRatio: 2` > child with `flexBasis: 2%`].
YGNodeRef buildFixedSizeMeasureFirst(YGConfigRef config) {
  YGNodeRef root = YGNodeNewWithConfig(config);
  YGNodeRef row = child(root, config);
  YGNodeStyleSetFlexDirection(row, YGFlexDirectionRow);
  YGNodeStyleSetFlexWrap(row, YGWrapWrap);
  YGNodeRef first = child(row, config);
  YGNodeStyleSetFlexBasis(first, 1);
  YGNodeStyleSetMinHeight(first, 90);
  YGNodeRef second = child(row, config);
  YGNodeStyleSetFlexBasis(second, 100);
  YGNodeStyleSetAspectRatio(second, 2);
  YGNodeStyleSetFlexBasisPercent(child(second, config), 2);
  return root;
}

} // namespace

// fuzzer/cases/barre-only-basis-mode2.case. A container's call answered from
// an earlier layout's cache entry, then asked again in the same layout: a
// fresh layout ran the first call and the second hits what it stored, so the
// second must not replay the container's flex-basis anchor again (the
// replay rewrote the text's basis to its unconstrained content size).
TEST(YGRelayoutFreshFuzzTest, earlier_layout_entry_hit_twice) {
  expectRelayoutMatchesFresh(
      buildEarlierEntryHitTwice, layoutAtStyleWidth, {{100, 800}, {200, 640}});
}

// fuzzer/cases/slots-barre-only.case. A call whose main-axis sizing mode is
// max-content but whose available main size is definite keeps its
// children's bases (the resolved-basis branch only needs a definite size),
// so it is not a flex-basis anchor: replaying it alone loses what the
// container's earlier, unconstrained call wrote.
TEST(YGRelayoutFreshFuzzTest, max_content_main_axis_with_definite_size) {
  expectRelayoutMatchesFresh(
      buildMaxContentWithDefiniteSize,
      layoutAtStyleWidth,
      {{300, 800}, {100, 640}, {200, 500}, {100, 800}});
}

// fuzzer/cases/barre-only-baseline.case. A baseline child laid out during
// its owner's measure-only pass (the yoga#1932 divergence), where the
// owner's layout pass is then answered from an earlier layout's cache: a
// fresh layout runs that pass and lays the child out again.
TEST(YGRelayoutFreshFuzzTest, baseline_child_laid_out_during_measure) {
  expectRelayoutMatchesFresh(
      buildBaselineChildLaidOutDuringMeasure,
      layoutAtStyleWidth,
      {{100, 800}, {200, 400}});
}

// fuzzer/cases/barre-only-basis-mode0.case. A call with a stretch-fit main
// axis answered from an earlier layout's cache: a fresh layout ran it, and
// `computeFlexBasisForChildren` gave the container's single flex child a
// basis of 0, after the container's flex-basis anchor.
TEST(YGRelayoutFreshFuzzTest, single_flex_child_call_from_earlier_layout) {
  expectRelayoutMatchesFresh(
      buildSingleFlexChildFromEarlierLayout,
      layoutPinned,
      {{400, 100}, {100, 100}, {400, 100}});
}

// Minimised from fuzzer seed 5580 (mode 2). A container's first call in a
// layout is a measurement at a fixed size, which returns before computing
// any flex basis; in a fresh layout the first call that writes its child's
// percent basis (resolved against that call's height) is a later one. Not a
// failure of `stack` at the time it was found: it guards the flex-basis
// anchor rule against treating the fixed-size measurement as the anchor.
TEST(YGRelayoutFreshFuzzTest, fixed_size_measurement_is_not_an_anchor) {
  expectRelayoutMatchesFresh(
      buildFixedSizeMeasureFirst, layoutAtStyleWidth, {{100, 800}, {200, 400}});
}
