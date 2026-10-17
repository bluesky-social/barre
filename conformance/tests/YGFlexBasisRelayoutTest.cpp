/*
 * Regression tests for yoga#1552 (https://github.com/react/yoga/issues/1552):
 * `flexBasis: '100%'` is not recomputed when the parent's size changes, e.g.
 * when a React Native app rotates. computeFlexBasisForChild only recomputes a
 * resolved (here: percent) flex basis when the child's computedFlexBasis is
 * undefined (or, with YGExperimentalFeatureWebFlexBasis, when the layout
 * generation changed); the child is not dirty (only the root's size
 * changed), so it keeps the basis resolved against the old size.
 *
 * Plus a general oracle for this class of bug: laying out a tree, changing
 * the root's size and laying it out again must give the same layout as a
 * fresh tree laid out at the new size (barre_test::expectSameLayout), over
 * a variety of trees.
 *
 * Tests that fail on Yoga at the vendor pin (52f6d0b) because of this bug are
 * listed in conformance/yoga-divergences.txt; see conformance/divergences.md.
 */

#include <gtest/gtest.h>
#include <yoga/Yoga.h>

#include <cmath>
#include <functional>
#include <limits>
#include <utility>
#include <vector>

#include "util/LayoutInvariant.h"

using barre_test::expectSameLayout;

// The minimal reproduction from the issue thread: a child with a 25% flex
// basis in a column whose height goes from 1000 to 1200.
TEST(YGFlexBasisRelayoutTest, percent_flex_basis_recomputed_when_parent_height_changes) {
  YGNodeRef root = YGNodeNew();
  YGNodeStyleSetWidth(root, 400);
  YGNodeStyleSetHeight(root, 1000);
  YGNodeRef child = YGNodeNew();
  YGNodeStyleSetFlexBasisPercent(child, 25);
  YGNodeInsertChild(root, child, 0);

  YGNodeCalculateLayout(root, YGUndefined, YGUndefined, YGDirectionLTR);
  ASSERT_EQ(YGNodeLayoutGetHeight(child), 250);

  YGNodeStyleSetHeight(root, 1200);
  YGNodeCalculateLayout(root, YGUndefined, YGUndefined, YGDirectionLTR);
  ASSERT_EQ(YGNodeLayoutGetHeight(child), 300);

  YGNodeFreeRecursive(root);
}

// The issue's React Native reproduction, rotating from landscape to portrait:
//
//   <View style={StyleSheet.absoluteFill}>
//     <View style={{flexBasis: '100%'}} />
//   </View>
//
// in a root view sized the way React Native sizes it (min = max = the
// screen), with React Native's point scale factor and errata.
TEST(YGFlexBasisRelayoutTest, flex_basis_100_percent_after_rotation) {
  YGConfigRef config = YGConfigNew();
  YGConfigSetPointScaleFactor(config, 3.0f);
  YGConfigSetErrata(config, YGErrataAll);

  auto build = [&]() {
    YGNodeRef root = YGNodeNewWithConfig(config);
    YGNodeRef fill = YGNodeNewWithConfig(config);
    YGNodeStyleSetPositionType(fill, YGPositionTypeAbsolute);
    YGNodeStyleSetPosition(fill, YGEdgeAll, 0);
    YGNodeInsertChild(root, fill, 0);
    YGNodeRef basis = YGNodeNewWithConfig(config);
    YGNodeStyleSetFlexBasisPercent(basis, 100);
    YGNodeInsertChild(fill, basis, 0);
    return root;
  };
  auto layoutAt = [](YGNodeRef root, float w, float h) {
    YGNodeStyleSetMinWidth(root, w);
    YGNodeStyleSetMaxWidth(root, w);
    YGNodeStyleSetMinHeight(root, h);
    YGNodeStyleSetMaxHeight(root, h);
    YGNodeCalculateLayout(root, w, h, YGDirectionLTR);
  };

  YGNodeRef root = build();
  YGNodeRef basis = YGNodeGetChild(YGNodeGetChild(root, 0), 0);

  layoutAt(root, 800, 400);
  ASSERT_EQ(YGNodeLayoutGetWidth(basis), 800);
  ASSERT_EQ(YGNodeLayoutGetHeight(basis), 400);

  layoutAt(root, 400, 800);
  ASSERT_EQ(YGNodeLayoutGetWidth(basis), 400);
  ASSERT_EQ(YGNodeLayoutGetHeight(basis), 800);

  YGNodeRef fresh = build();
  layoutAt(fresh, 400, 800);
  expectSameLayout(root, fresh);

  YGNodeFreeRecursive(fresh);
  YGNodeFreeRecursive(root);
  YGConfigFree(config);
}

// ---------------------------------------------------------------------------
// relayout == fresh layout, over varied trees

namespace {

/* Text-like: 1200pt of 20pt-high lines, wrapped at the available width. */
YGSize measureText(
    YGNodeConstRef /*node*/,
    float w,
    YGMeasureMode wm,
    float h,
    YGMeasureMode hm) {
  float maxW = (wm == YGMeasureModeUndefined)
      ? std::numeric_limits<float>::infinity()
      : w;
  float width = std::isfinite(maxW) ? std::fmin(maxW, 1200.0f) : 1200.0f;
  float lines = width > 0 ? std::ceil(1200.0f / width) : 1.0f;
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

enum class Resize {
  // YGNodeStyleSetWidth/Height on the root
  RootStyle,
  // the available size passed to YGNodeCalculateLayout (root has no size)
  AvailableSize,
  // min = max = the size, as React Native does
  RootMinMax,
};

const char* resizeName(Resize r) {
  switch (r) {
    case Resize::RootStyle:
      return "root style width/height";
    case Resize::AvailableSize:
      return "available size";
    case Resize::RootMinMax:
      return "root min/max width/height";
  }
  return "?";
}

void layoutAt(YGNodeRef root, Resize resize, float w, float h) {
  switch (resize) {
    case Resize::RootStyle:
      YGNodeStyleSetWidth(root, w);
      YGNodeStyleSetHeight(root, h);
      YGNodeCalculateLayout(root, YGUndefined, YGUndefined, YGDirectionLTR);
      break;
    case Resize::AvailableSize:
      YGNodeCalculateLayout(root, w, h, YGDirectionLTR);
      break;
    case Resize::RootMinMax:
      YGNodeStyleSetMinWidth(root, w);
      YGNodeStyleSetMaxWidth(root, w);
      YGNodeStyleSetMinHeight(root, h);
      YGNodeStyleSetMaxHeight(root, h);
      YGNodeCalculateLayout(root, w, h, YGDirectionLTR);
      break;
  }
}

// Builds a tree (root styled by `build`) with `config`, lays it out at each
// size in turn and checks every layout against a fresh tree, for each way
// of resizing the root.
void expectRelayoutMatchesFresh(
    const std::function<void(YGNodeRef root)>& build,
    float pointScaleFactor = 1.0f,
    YGErrata errata = YGErrataNone) {
  const std::vector<std::pair<float, float>> sizes = {
      {400, 800}, {800, 400}, {400, 800}, {320, 640}, {1000, 500}, {400, 800}};
  for (Resize resize :
       {Resize::RootStyle, Resize::AvailableSize, Resize::RootMinMax}) {
    SCOPED_TRACE(resizeName(resize));
    YGConfigRef config = YGConfigNew();
    YGConfigSetPointScaleFactor(config, pointScaleFactor);
    YGConfigSetErrata(config, errata);

    YGNodeRef root = YGNodeNewWithConfig(config);
    build(root);
    for (size_t i = 0; i < sizes.size(); i++) {
      auto [w, h] = sizes[i];
      SCOPED_TRACE(
          "layout #" + std::to_string(i + 1) + " at " +
          std::to_string(static_cast<int>(w)) + "x" +
          std::to_string(static_cast<int>(h)));
      layoutAt(root, resize, w, h);
      YGNodeRef fresh = YGNodeNewWithConfig(config);
      build(fresh);
      layoutAt(fresh, resize, w, h);
      expectSameLayout(root, fresh);
      YGNodeFreeRecursive(fresh);
    }
    YGNodeFreeRecursive(root);
    YGConfigFree(config);
  }
}

} // namespace

TEST(YGFlexBasisRelayoutTest, relayout_matches_fresh_percent_flex_basis_column) {
  expectRelayoutMatchesFresh([](YGNodeRef root) {
    add(root, [](YGNodeRef n) { YGNodeStyleSetFlexBasisPercent(n, 25); });
    add(root, [](YGNodeRef n) { YGNodeStyleSetFlexBasisPercent(n, 50); });
    add(root, [](YGNodeRef n) { YGNodeStyleSetHeight(n, 40); });
  });
}

TEST(YGFlexBasisRelayoutTest, relayout_matches_fresh_percent_flex_basis_row) {
  expectRelayoutMatchesFresh([](YGNodeRef root) {
    YGNodeStyleSetFlexDirection(root, YGFlexDirectionRow);
    add(root, [](YGNodeRef n) { YGNodeStyleSetFlexBasisPercent(n, 30); });
    add(root, [](YGNodeRef n) { YGNodeStyleSetFlexBasisPercent(n, 100); });
  });
}

TEST(YGFlexBasisRelayoutTest, relayout_matches_fresh_flex_grow_shrink) {
  expectRelayoutMatchesFresh([](YGNodeRef root) {
    YGNodeStyleSetFlexDirection(root, YGFlexDirectionRow);
    add(root, [](YGNodeRef n) {
      YGNodeStyleSetWidthPercent(n, 50);
      YGNodeStyleSetFlexGrow(n, 1);
    });
    add(root, [](YGNodeRef n) {
      YGNodeStyleSetFlexBasis(n, 300);
      YGNodeStyleSetFlexShrink(n, 1);
    });
    add(root, [](YGNodeRef n) {
      YGNodeStyleSetFlexBasisPercent(n, 20);
      YGNodeStyleSetFlexGrow(n, 2);
      YGNodeStyleSetFlexShrink(n, 3);
    });
  });
}

TEST(YGFlexBasisRelayoutTest, relayout_matches_fresh_wrap) {
  expectRelayoutMatchesFresh([](YGNodeRef root) {
    YGNodeStyleSetFlexDirection(root, YGFlexDirectionRow);
    YGNodeStyleSetFlexWrap(root, YGWrapWrap);
    YGNodeStyleSetAlignContent(root, YGAlignStretch);
    for (int i = 0; i < 5; i++) {
      add(root, [](YGNodeRef n) {
        YGNodeStyleSetFlexBasisPercent(n, 40);
        YGNodeStyleSetMinHeight(n, 10);
        YGNodeStyleSetFlexGrow(n, 1);
      });
    }
  });
}

TEST(YGFlexBasisRelayoutTest, relayout_matches_fresh_nested) {
  expectRelayoutMatchesFresh([](YGNodeRef root) {
    YGNodeRef row = add(root, [](YGNodeRef n) {
      YGNodeStyleSetFlexDirection(n, YGFlexDirectionRow);
      YGNodeStyleSetFlexBasisPercent(n, 50);
    });
    YGNodeRef col = add(row, [](YGNodeRef n) {
      YGNodeStyleSetFlexBasisPercent(n, 60);
    });
    add(col, [](YGNodeRef n) { YGNodeStyleSetFlexBasisPercent(n, 50); });
    add(col, [](YGNodeRef n) { YGNodeStyleSetFlexGrow(n, 1); });
    add(row, [](YGNodeRef n) { YGNodeStyleSetFlexGrow(n, 1); });
    add(root, [](YGNodeRef n) { YGNodeStyleSetFlex(n, 1); });
  });
}

TEST(YGFlexBasisRelayoutTest, relayout_matches_fresh_min_max_percent) {
  expectRelayoutMatchesFresh([](YGNodeRef root) {
    add(root, [](YGNodeRef n) {
      YGNodeStyleSetFlexBasisPercent(n, 10);
      YGNodeStyleSetMinHeightPercent(n, 30);
    });
    add(root, [](YGNodeRef n) {
      YGNodeStyleSetFlexGrow(n, 1);
      YGNodeStyleSetMaxHeightPercent(n, 40);
    });
    add(root, [](YGNodeRef n) {
      YGNodeStyleSetFlexBasisPercent(n, 50);
      YGNodeStyleSetAspectRatio(n, 2);
      YGNodeStyleSetAlignSelf(n, YGAlignFlexStart);
    });
  });
}

TEST(YGFlexBasisRelayoutTest, relayout_matches_fresh_measure_funcs) {
  expectRelayoutMatchesFresh([](YGNodeRef root) {
    YGNodeRef row = add(root, [](YGNodeRef n) {
      YGNodeStyleSetFlexDirection(n, YGFlexDirectionRow);
    });
    add(row, [](YGNodeRef n) {
      YGNodeStyleSetFlexShrink(n, 1);
      YGNodeSetMeasureFunc(n, measureText);
    });
    add(row, [](YGNodeRef n) {
      YGNodeStyleSetWidthPercent(n, 25);
    });
    YGNodeRef col = add(root, [](YGNodeRef n) { YGNodeStyleSetFlex(n, 1); });
    add(col, [](YGNodeRef n) {
      YGNodeStyleSetFlex(n, 1);
      YGNodeSetMeasureFunc(n, measureText);
    });
    add(col, [](YGNodeRef n) {
      YGNodeStyleSetFlexBasisPercent(n, 20);
      YGNodeSetMeasureFunc(n, measureText);
    });
  });
}

// A React Native-shaped screen: a scroll view (overflow: scroll, flex: 1)
// whose content is a column of rows, each with a text (measure func) next
// to a percent-wide image, with RN's point scale factor and errata.
TEST(YGFlexBasisRelayoutTest, relayout_matches_fresh_scroll_view_rows) {
  expectRelayoutMatchesFresh(
      [](YGNodeRef root) {
        YGNodeRef sv = add(root, [](YGNodeRef n) {
          YGNodeStyleSetFlexGrow(n, 1);
          YGNodeStyleSetFlexShrink(n, 1);
          YGNodeStyleSetOverflow(n, YGOverflowScroll);
        });
        YGNodeRef content = add(sv, [](YGNodeRef) {});
        for (int i = 0; i < 3; i++) {
          YGNodeRef row = add(content, [](YGNodeRef n) {
            YGNodeStyleSetFlexDirection(n, YGFlexDirectionRow);
            YGNodeStyleSetPadding(n, YGEdgeAll, 8);
          });
          YGNodeRef body = add(row, [](YGNodeRef n) { YGNodeStyleSetFlex(n, 1); });
          add(body, [](YGNodeRef n) { YGNodeStyleSetHeight(n, 18); });
          add(body, [](YGNodeRef n) {
            YGNodeStyleSetFlexShrink(n, 1);
            YGNodeSetMeasureFunc(n, measureText);
          });
          add(row, [](YGNodeRef n) {
            YGNodeStyleSetWidthPercent(n, 30);
            YGNodeStyleSetAspectRatio(n, 1);
          });
        }
      },
      3.0f,
      YGErrataAll);
}
