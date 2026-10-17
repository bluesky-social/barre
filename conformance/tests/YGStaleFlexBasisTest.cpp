/*
 * Regression tests for yoga#2019 (https://github.com/react/yoga/issues/2019):
 * a node's computedFlexBasis survives from one YGNodeCalculateLayout to the
 * next and is only cleared when that node itself is dirtied. When a relayout
 * at a new root size hits an ancestor's measurement cache in the max-content
 * pass, the subtree is skipped and the old basis is never refreshed; a later
 * definite-size pass then keeps it (computeFlexBasisForChild only writes a
 * resolved basis when none is stored). Visible in React Native as a
 * `flex: 1` Text keeping its landscape height after rotating back.
 *
 * The tree and measure func are the issue's minimal reproduction:
 *
 *   root  (exact 400 x 800, then 800 x 400, then 400 x 800 again)
 *    SV   overflow: scroll, flexGrow 1, flexShrink 1   [optional]
 *     CC  plain column
 *      RC plain column                           <- height auto
 *       A  flex: 1
 *        S  height: 24
 *        W1 plain column                         <- wrapper levels: 1..3
 *         W2 plain column
 *          T flex: 1, measure func (text: taller when narrower)
 *
 * with point scale factor 3 and YGErrataAll, as React Native configures it.
 * T is 2000pt of 20pt-high lines: 100 tall at width 400, 60 at width 800.
 *
 * Yoga at the vendor pin (52f6d0b) fails the two-wrapper cases: T comes back
 * at 60 after rotating back. With one wrapper the stale basis is masked (T is
 * stretched to the right height); it is kept as a regression guard. These
 * tests are expected to fail on Yoga: see conformance/divergences.md.
 */

#include <gtest/gtest.h>
#include <yoga/Yoga.h>

#include <cmath>
#include <limits>

#include "util/LayoutInvariant.h"

namespace {

/* 2000pt of text laid out at the available width, 20pt line height. */
YGSize measureText(
    YGNodeConstRef /*node*/,
    float w,
    YGMeasureMode wm,
    float h,
    YGMeasureMode hm) {
  float maxW = (wm == YGMeasureModeUndefined)
      ? std::numeric_limits<float>::infinity()
      : w;
  float lines = std::isfinite(maxW) && maxW > 0 ? std::ceil(2000.0f / maxW)
                                                : 1.0f;
  float natH = lines * 20.0f;
  YGSize out{std::isfinite(maxW) ? maxW : 2000.0f, natH};
  if (hm == YGMeasureModeExactly) {
    out.height = h;
  } else if (hm == YGMeasureModeAtMost && natH > h) {
    out.height = h;
  }
  if (wm == YGMeasureModeExactly) {
    out.width = w;
  }
  return out;
}

struct Tree {
  YGNodeRef root;
  YGNodeRef text;
};

Tree build(YGConfigRef cfg, bool useScrollView, int wrapperLevels) {
  YGNodeRef root = YGNodeNewWithConfig(cfg);
  YGNodeRef parent = root;
  if (useScrollView) {
    YGNodeRef sv = YGNodeNewWithConfig(cfg);
    YGNodeStyleSetFlexGrow(sv, 1);
    YGNodeStyleSetFlexShrink(sv, 1);
    YGNodeStyleSetOverflow(sv, YGOverflowScroll);
    YGNodeInsertChild(root, sv, 0);
    YGNodeRef cc = YGNodeNewWithConfig(cfg);
    YGNodeInsertChild(sv, cc, 0);
    parent = cc;
  }
  YGNodeRef rc = YGNodeNewWithConfig(cfg);
  YGNodeInsertChild(parent, rc, 0);

  YGNodeRef a = YGNodeNewWithConfig(cfg);
  YGNodeStyleSetFlex(a, 1);
  YGNodeInsertChild(rc, a, 0);

  YGNodeRef s = YGNodeNewWithConfig(cfg);
  YGNodeStyleSetHeight(s, 24);
  YGNodeInsertChild(a, s, 0);

  YGNodeRef p = a;
  for (int i = 0; i < wrapperLevels; i++) {
    YGNodeRef m = YGNodeNewWithConfig(cfg);
    YGNodeInsertChild(p, m, YGNodeGetChildCount(p));
    p = m;
  }
  YGNodeRef t = YGNodeNewWithConfig(cfg);
  YGNodeStyleSetFlex(t, 1);
  YGNodeSetMeasureFunc(t, measureText);
  YGNodeInsertChild(p, t, 0);
  return {root, t};
}

// React Native sizes the root by setting its min and max dimensions.
void layoutAt(YGNodeRef root, float w, float h) {
  YGNodeStyleSetMinWidth(root, w);
  YGNodeStyleSetMaxWidth(root, w);
  YGNodeStyleSetMinHeight(root, h);
  YGNodeStyleSetMaxHeight(root, h);
  YGNodeCalculateLayout(root, w, h, YGDirectionLTR);
}

// Portrait, landscape, portrait again. After each layout the whole tree
// matches a fresh tree laid out at the same size, and (with the scroll view)
// T has the height of its text at that width: 100 at 400, 60 at 800.
void rotateAndBack(
    bool useScrollView,
    int wrapperLevels,
    bool markTextDirty = false) {
  YGConfigRef cfg = YGConfigNew();
  YGConfigSetPointScaleFactor(cfg, 3.0f);
  YGConfigSetErrata(cfg, YGErrataAll);

  Tree tree = build(cfg, useScrollView, wrapperLevels);

  struct Step {
    const char* name;
    float w, h, textHeight;
  };
  const Step steps[] = {
      {"portrait #1", 400, 800, 100},
      {"landscape", 800, 400, 60},
      {"portrait #2", 400, 800, 100},
  };
  for (const auto& step : steps) {
    SCOPED_TRACE(step.name);
    if (markTextDirty) {
      YGNodeMarkDirty(tree.text);
    }
    layoutAt(tree.root, step.w, step.h);
    if (useScrollView) {
      EXPECT_EQ(YGNodeLayoutGetHeight(tree.text), step.textHeight);
    }

    Tree fresh = build(cfg, useScrollView, wrapperLevels);
    layoutAt(fresh.root, step.w, step.h);
    if (useScrollView) {
      EXPECT_EQ(YGNodeLayoutGetHeight(fresh.text), step.textHeight);
    }
    barre_test::expectSameLayout(tree.root, fresh.root);
    YGNodeFreeRecursive(fresh.root);
  }

  YGNodeFreeRecursive(tree.root);
  YGConfigFree(cfg);
}

} // namespace

// The issue's reproduction as written (`./minimal`): fails on Yoga.
TEST(YGStaleFlexBasisTest, rotate_and_back_scrollview_two_wrappers) {
  rotateAndBack(/*useScrollView=*/true, /*wrapperLevels=*/2);
}

// `./minimal --wrappers=1`: the wrapper's own cache hit masks the stale basis
// on Yoga (T is stretched to the right height). Regression guard.
TEST(YGStaleFlexBasisTest, rotate_and_back_scrollview_one_wrapper) {
  rotateAndBack(/*useScrollView=*/true, /*wrapperLevels=*/1);
}

// `./minimal --wrappers=3`.
TEST(YGStaleFlexBasisTest, rotate_and_back_scrollview_three_wrappers) {
  rotateAndBack(/*useScrollView=*/true, /*wrapperLevels=*/3);
}

// `./minimal --no-scrollview`: without the scroll view nothing supplies the
// max-content pass that leaves a stale basis behind. T's height is not
// asserted here (in Yoga, a `flex: 1` node in an auto-height column without a
// max-content pass gets a basis of 0 and so a height of 0, in a fresh tree
// too); only relayout == fresh layout is. Regression guard.
TEST(YGStaleFlexBasisTest, rotate_and_back_no_scrollview_two_wrappers) {
  rotateAndBack(/*useScrollView=*/false, /*wrapperLevels=*/2);
}

// Control: the same as rotate_and_back_scrollview_two_wrappers, but T is
// marked dirty before every layout, which clears its computedFlexBasis in
// Yoga. Passes on Yoga, which shows that the failure above is stale per-node
// state, not a wrong layout of the tree.
TEST(
    YGStaleFlexBasisTest,
    rotate_and_back_scrollview_two_wrappers_text_marked_dirty) {
  rotateAndBack(
      /*useScrollView=*/true, /*wrapperLevels=*/2, /*markTextDirty=*/true);
}
