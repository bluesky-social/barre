/*
 * Regression tests for yoga#2035 (https://github.com/react/yoga/issues/2035):
 * free space is computed from min-clamped flex bases, so a growing item with
 * a minWidth larger than its basis gets its minimum *plus* a full share.
 *
 * calculateFlexLine adds boundAxisWithinMinAndMax(computedFlexBasis) (the
 * clamped basis) to sizeConsumed, and distributeFreeSpaceFirstPass /
 * SecondPass start each item from that clamped basis. CSS Flexbox 9.7
 * (https://drafts.csswg.org/css-flexbox-1/#resolve-flexible-lengths)
 * computes the initial free space from the unfrozen items' flex *base* sizes,
 * and step 3 only freezes an item up front at its hypothetical (clamped)
 * size when it is inflexible or, when growing, when its base size is
 * greater than its hypothetical size; an item raised by its min-width stays
 * flexible from its base size, and is frozen at its minimum only if its
 * share would violate it.
 *
 * The issue's two rows (items: flexGrow g, flexShrink 1, flexBasis 0,
 * optional minWidth; container height 10):
 *
 *   width 120: [grow 1, min 30] [grow 2]
 *     Chrome, Firefox, Safari: 40 / 80 (quoted in the issue). By hand: free
 *     space 120, shared 1:2 = 40 / 80; 40 does not violate the 30 minimum.
 *     Yoga: 60 / 60.
 *   width 300: [grow 1, min 200] [grow 1] [grow 1]
 *     Chrome, Firefox, Safari: 200 / 50 / 50 (quoted in the issue). By hand:
 *     300 shared 100 each; the first violates its 200 minimum and is frozen
 *     at 200; the remaining 100 goes 50 / 50. Yoga: 233 / 34 / 33.
 *
 * The issue reports the same numbers with the default config and with
 * useWebDefaults; the bug is not errata-gated, so every config here expects
 * the CSS result. The column variants are the same layouts transposed.
 *
 * These fail on Yoga at the vendor pin by design: see
 * conformance/divergences.md.
 */

#include <gtest/gtest.h>
#include <yoga/Yoga.h>

#include <vector>

namespace {

struct Item {
  float grow;
  float minMain; // 0: none
};

enum class Cfg { Default, ErrataNone, WebDefaults };

const char* cfgName(Cfg c) {
  switch (c) {
    case Cfg::Default:
      return "default config";
    case Cfg::ErrataNone:
      return "YGErrataNone";
    case Cfg::WebDefaults:
      return "useWebDefaults";
  }
  return "?";
}

std::vector<float> layoutLine(
    Cfg cfg,
    bool row,
    float containerMain,
    const std::vector<Item>& items) {
  YGConfigRef config = YGConfigNew();
  if (cfg == Cfg::ErrataNone) {
    YGConfigSetErrata(config, YGErrataNone);
  } else if (cfg == Cfg::WebDefaults) {
    YGConfigSetUseWebDefaults(config, true);
  }
  YGNodeRef root = YGNodeNewWithConfig(config);
  YGNodeStyleSetFlexDirection(
      root, row ? YGFlexDirectionRow : YGFlexDirectionColumn);
  if (row) {
    YGNodeStyleSetWidth(root, containerMain);
    YGNodeStyleSetHeight(root, 10);
  } else {
    YGNodeStyleSetHeight(root, containerMain);
    YGNodeStyleSetWidth(root, 10);
  }
  for (size_t i = 0; i < items.size(); i++) {
    YGNodeRef n = YGNodeNewWithConfig(config);
    YGNodeStyleSetFlexGrow(n, items[i].grow);
    YGNodeStyleSetFlexShrink(n, 1);
    YGNodeStyleSetFlexBasis(n, 0);
    if (items[i].minMain > 0) {
      if (row) {
        YGNodeStyleSetMinWidth(n, items[i].minMain);
      } else {
        YGNodeStyleSetMinHeight(n, items[i].minMain);
      }
    }
    YGNodeInsertChild(root, n, i);
  }
  YGNodeCalculateLayout(root, YGUndefined, YGUndefined, YGDirectionLTR);
  std::vector<float> sizes;
  for (size_t i = 0; i < items.size(); i++) {
    YGNodeRef n = YGNodeGetChild(root, i);
    sizes.push_back(row ? YGNodeLayoutGetWidth(n) : YGNodeLayoutGetHeight(n));
  }
  YGNodeFreeRecursive(root);
  YGConfigFree(config);
  return sizes;
}

void expectLine(
    float containerMain,
    const std::vector<Item>& items,
    const std::vector<float>& expected) {
  for (bool row : {true, false}) {
    for (Cfg cfg : {Cfg::Default, Cfg::ErrataNone, Cfg::WebDefaults}) {
      SCOPED_TRACE(std::string(row ? "row, " : "column, ") + cfgName(cfg));
      std::vector<float> sizes = layoutLine(cfg, row, containerMain, items);
      ASSERT_EQ(sizes.size(), expected.size());
      for (size_t i = 0; i < sizes.size(); i++) {
        EXPECT_NEAR(sizes[i], expected[i], 1e-3f) << "item " << i;
      }
    }
  }
}

} // namespace

// <div style="display: flex; width: 120px; height: 10px">
//   <div style="flex: 1; min-width: 30px"></div>
//   <div style="flex: 2"></div>
// </div>
TEST(YGMinClampedFreeSpaceTest, min_below_share_is_not_added_to_share) {
  expectLine(120, {{1, 30}, {2, 0}}, {40, 80});
}

// <div style="display: flex; width: 300px; height: 10px">
//   <div style="flex: 1; min-width: 200px"></div>
//   <div style="flex: 1"></div>
//   <div style="flex: 1"></div>
// </div>
TEST(YGMinClampedFreeSpaceTest, min_above_share_freezes_at_min) {
  expectLine(300, {{1, 200}, {1, 0}, {1, 0}}, {200, 50, 50});
}
