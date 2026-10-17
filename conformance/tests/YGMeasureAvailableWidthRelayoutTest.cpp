/*
 * Regression tests for yoga#1784 (https://github.com/react/yoga/issues/1784):
 * a wrapping text node (measure func) is measured with a stale available
 * width when an ancestor is resized slowly.
 *
 * The issue has no minimal repro; it describes the shape: a panel resized
 * 1pt at a time from 140 to 180 wide (only the panel is dirtied), containing
 * a container X with a percentage width (35%) and fixed paddings, holding a
 * fixed-width spacer followed by wrapping text. Occasionally the text's
 * measure func is called with the available width of an earlier size (at
 * 161: 121 instead of 141), so it wraps for the wrong width. The reporter's
 * fixes all force a revisit below a resized node and/or reset
 * computedFlexBasis; a maintainer comment points at the measurement cache
 * being keyed by available size and mode but not by the containing block
 * size percentages resolve against. Same family as yoga#1626, yoga#1003 and
 * the flex-basis staleness bugs (yoga#1552, yoga#2019).
 *
 * The exact numbers are unknown, so the oracle is mostly the invariant
 * "relayout == fresh layout" (util/LayoutInvariant.h) at every step of the
 * slow resize, over the issue's shape and variants with nested percentages
 * and paddings. For the issue's shape, the text's size also follows from a
 * hand computation (see text_size_tracks_panel_width).
 *
 * Text: 600pt of 20pt-high lines wrapped at the available width, so its
 * height reveals the width it was measured with.
 */

#include <gtest/gtest.h>
#include <yoga/Yoga.h>

#include <cmath>
#include <functional>
#include <limits>
#include <string>
#include <vector>

#include "util/LayoutInvariant.h"

namespace {

YGSize measureText(
    YGNodeConstRef /*node*/,
    float w,
    YGMeasureMode wm,
    float h,
    YGMeasureMode hm) {
  float maxW = (wm == YGMeasureModeUndefined)
      ? std::numeric_limits<float>::infinity()
      : w;
  float width = std::isfinite(maxW) ? std::fmin(maxW, 600.0f) : 600.0f;
  float lines = width > 0 ? std::ceil(600.0f / width) : 1.0f;
  YGSize out{width, lines * 20.0f};
  if (wm == YGMeasureModeExactly) {
    out.width = w;
  }
  if (hm == YGMeasureModeExactly) {
    out.height = h;
  } else if (hm == YGMeasureModeAtMost && out.height > h) {
    out.height = h;
  }
  return out;
}

YGNodeRef add(YGNodeRef parent, const std::function<void(YGNodeRef)>& style) {
  YGNodeRef n = YGNodeNewWithConfig(YGNodeGetConfig(parent));
  style(n);
  YGNodeInsertChild(parent, n, YGNodeGetChildCount(parent));
  return n;
}

// The issue's shape inside `panel`: X (row, width 35%, horizontal padding
// 10) > [spacer (width 20), text (flexShrink 1, measure func)].
YGNodeRef buildIssueShape(YGNodeRef panel) {
  YGNodeRef x = add(panel, [](YGNodeRef n) {
    YGNodeStyleSetFlexDirection(n, YGFlexDirectionRow);
    YGNodeStyleSetWidthPercent(n, 35);
    YGNodeStyleSetPadding(n, YGEdgeHorizontal, 10);
  });
  add(x, [](YGNodeRef n) { YGNodeStyleSetWidth(n, 20); });
  return add(x, [](YGNodeRef n) {
    YGNodeStyleSetFlexShrink(n, 1);
    YGNodeSetMeasureFunc(n, measureText);
  });
}

// root (width 1000, height by content, so the text's height is never
// capped) > panel (style width w, the only node resized) > `build(panel)`.
YGNodeRef makeRoot(
    YGConfigRef config,
    float w,
    const std::function<void(YGNodeRef)>& build) {
  YGNodeRef root = YGNodeNewWithConfig(config);
  YGNodeStyleSetWidth(root, 1000);
  YGNodeRef panel = YGNodeNewWithConfig(config);
  YGNodeStyleSetWidth(panel, w);
  YGNodeInsertChild(root, panel, 0);
  build(panel);
  return root;
}

// 140 -> 180 -> 140 in 1pt steps.
std::vector<float> slowResize() {
  std::vector<float> widths;
  for (int w = 140; w <= 180; w++) {
    widths.push_back(static_cast<float>(w));
  }
  for (int w = 179; w >= 140; w--) {
    widths.push_back(static_cast<float>(w));
  }
  return widths;
}

void markMeasuredNodesDirty(YGNodeRef node) {
  if (YGNodeHasMeasureFunc(node)) {
    YGNodeMarkDirty(node);
  }
  for (size_t i = 0; i < YGNodeGetChildCount(node); i++) {
    markMeasuredNodesDirty(YGNodeGetChild(node, i));
  }
}

// With `dirtyMeasuredNodes`, every node with a measure func is marked dirty
// before each relayout (the reporter's workaround), which drops their
// measurement caches.
void expectRelayoutMatchesFresh(
    const std::function<void(YGNodeRef)>& build,
    float pointScaleFactor,
    bool dirtyMeasuredNodes = false) {
  YGConfigRef config = YGConfigNew();
  YGConfigSetPointScaleFactor(config, pointScaleFactor);
  std::vector<float> widths = slowResize();
  YGNodeRef root = makeRoot(config, widths[0], build);
  const bool failedBefore = ::testing::Test::HasFailure();
  for (float w : widths) {
    SCOPED_TRACE("panel width " + std::to_string(static_cast<int>(w)));
    YGNodeStyleSetWidth(YGNodeGetChild(root, 0), w);
    if (dirtyMeasuredNodes) {
      markMeasuredNodesDirty(root);
    }
    YGNodeCalculateLayout(root, YGUndefined, YGUndefined, YGDirectionLTR);
    YGNodeRef fresh = makeRoot(config, w, build);
    YGNodeCalculateLayout(fresh, YGUndefined, YGUndefined, YGDirectionLTR);
    barre_test::expectSameLayout(root, fresh);
    YGNodeFreeRecursive(fresh);
    if (!failedBefore && ::testing::Test::HasFailure()) {
      break; // one step's diff is enough
    }
  }
  YGNodeFreeRecursive(root);
  YGConfigFree(config);
}

} // namespace

// The issue's shape, no rounding. Hand computation at panel width w: X is
// 0.35 * w wide; its content box is 0.35 * w - 20; the spacer takes 20, so
// the text (flexShrink 1, max-content 600) shrinks to 0.35 * w - 40 and
// wraps there: height ceil(600 / (0.35 * w - 40)) * 20. (At w = 140 the text
// is 9 wide and 67 lines tall; at 180 it is 23 wide and 27 lines tall.)
TEST(YGMeasureAvailableWidthRelayoutTest, text_size_tracks_panel_width) {
  YGConfigRef config = YGConfigNew();
  YGConfigSetPointScaleFactor(config, 0);
  YGNodeRef text = nullptr;
  YGNodeRef root = makeRoot(
      config, 140, [&](YGNodeRef panel) { text = buildIssueShape(panel); });
  for (float w : slowResize()) {
    SCOPED_TRACE("panel width " + std::to_string(static_cast<int>(w)));
    YGNodeStyleSetWidth(YGNodeGetChild(root, 0), w);
    YGNodeCalculateLayout(root, YGUndefined, YGUndefined, YGDirectionLTR);
    float textWidth = 0.35f * w - 40.0f;
    EXPECT_NEAR(YGNodeLayoutGetWidth(text), textWidth, 1e-3f);
    EXPECT_NEAR(
        YGNodeLayoutGetHeight(text),
        std::ceil(600.0f / YGNodeLayoutGetWidth(text)) * 20.0f,
        1e-3f);
  }
  YGNodeFreeRecursive(root);
  YGConfigFree(config);
}

TEST(YGMeasureAvailableWidthRelayoutTest, relayout_matches_fresh_issue_shape) {
  for (float scale : {0.0f, 1.0f, 3.0f}) {
    SCOPED_TRACE("point scale factor " + std::to_string(scale));
    expectRelayoutMatchesFresh(
        [](YGNodeRef panel) { buildIssueShape(panel); }, scale);
  }
}

// The reporter's follow-up: "between that ancestor and descendant are
// multiple nodes with % width dimensions and fixed paddings".
TEST(YGMeasureAvailableWidthRelayoutTest, relayout_matches_fresh_nested_percent_widths) {
  for (float scale : {0.0f, 1.0f, 3.0f}) {
    SCOPED_TRACE("point scale factor " + std::to_string(scale));
    expectRelayoutMatchesFresh(
        [](YGNodeRef panel) {
          YGNodeStyleSetPadding(panel, YGEdgeAll, 10);
          YGNodeRef a = add(panel, [](YGNodeRef n) {
            YGNodeStyleSetWidthPercent(n, 80);
            YGNodeStyleSetPadding(n, YGEdgeHorizontal, 6);
          });
          YGNodeRef b = add(a, [](YGNodeRef n) {
            YGNodeStyleSetWidthPercent(n, 90);
            YGNodeStyleSetPadding(n, YGEdgeLeft, 4);
          });
          buildIssueShape(b);
          // a sibling text directly under the percent containers
          add(b, [](YGNodeRef n) { YGNodeSetMeasureFunc(n, measureText); });
        },
        scale);
  }
}

// The same nesting in a row panel with the percent container flexing, so
// the text's width also depends on flex distribution.
TEST(YGMeasureAvailableWidthRelayoutTest, relayout_matches_fresh_row_with_flex) {
  for (float scale : {0.0f, 1.0f, 3.0f}) {
    SCOPED_TRACE("point scale factor " + std::to_string(scale));
    expectRelayoutMatchesFresh(
        [](YGNodeRef panel) {
          YGNodeStyleSetFlexDirection(panel, YGFlexDirectionRow);
          add(panel, [](YGNodeRef n) { YGNodeStyleSetWidth(n, 30); });
          YGNodeRef col = add(panel, [](YGNodeRef n) {
            YGNodeStyleSetFlexGrow(n, 1);
            YGNodeStyleSetFlexShrink(n, 1);
            YGNodeStyleSetPadding(n, YGEdgeHorizontal, 5);
          });
          buildIssueShape(col);
          add(col, [](YGNodeRef n) {
            YGNodeStyleSetWidthPercent(n, 50);
            YGNodeSetMeasureFunc(n, measureText);
          });
        },
        scale);
  }
}

// Control: the issue's shape again, with the reporter's workaround of
// marking every wrapping text dirty before each relayout. If this passes
// where relayout_matches_fresh_issue_shape fails, the failure is a stale
// measurement of the text, not a wrong layout of the tree.
TEST(YGMeasureAvailableWidthRelayoutTest, relayout_matches_fresh_issue_shape_texts_marked_dirty) {
  for (float scale : {0.0f, 1.0f, 3.0f}) {
    SCOPED_TRACE("point scale factor " + std::to_string(scale));
    expectRelayoutMatchesFresh(
        [](YGNodeRef panel) { buildIssueShape(panel); },
        scale,
        /*dirtyMeasuredNodes=*/true);
  }
}
