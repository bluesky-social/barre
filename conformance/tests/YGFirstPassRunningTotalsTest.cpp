/*
 * Regression tests for yoga#2006 (https://github.com/react/yoga/issues/2006):
 * a minWidth difference on the first child breaks even distribution.
 *
 * Fixed in Yoga, behind an errata bit: distributeFreeSpaceFirstPass used to
 * compute each item's tentative size against the *running* flex totals,
 * which it reduces as items freeze, so items after a frozen one got an
 * inflated share, froze at their max, and drained the remaining free space
 * before the second pass. The fix divides by the original totals; the old
 * behaviour is kept behind YGErrataFlexFirstPassUsesRunningTotals, which new
 * configs set by default. So both are contract: the fixed behaviour with the
 * bit cleared, the legacy behaviour with it set. (Yoga's own
 * tests/YGFirstChildMinWidthTest.cpp pins the flat case; these add the
 * issue's playground tree, the shrink and max-clamp paths, and an order
 * invariant.)
 *
 * Expected values:
 *  - errata cleared: CSS Flexbox 9.7 by hand (each case below shows it);
 *  - errata set: Yoga's legacy result, which by definition is what the bit
 *    preserves; for the issue's row it is 60 / 30 / 30, pinned by Yoga's
 *    YGFirstChildMinWidth.errata_preserves_prefix_geometry_by_default and
 *    checked by hand below.
 *
 * Pass on Yoga (the fix is in at the vendor pin).
 */

#include <gtest/gtest.h>
#include <yoga/Yoga.h>

#include <algorithm>
#include <array>
#include <string>
#include <vector>

namespace {

YGConfigRef makeConfig(bool runningTotals) {
  YGConfigRef config = YGConfigNew();
  YGErrata errata = YGConfigGetErrata(config);
  errata = runningTotals
      ? static_cast<YGErrata>(errata | YGErrataFlexFirstPassUsesRunningTotals)
      : static_cast<YGErrata>(
            errata & (YGErrataAll ^ YGErrataFlexFirstPassUsesRunningTotals));
  YGConfigSetErrata(config, errata);
  return config;
}

// The issue's playground tree:
//   <Node style={{width: 540, flexDirection: "row"}}>
//     <Node style={{width: 540, flexDirection: "row"}}>
//       <Node style={{flexGrow: 1, flexShrink: 1, maxWidth: 180, height: 30,
//                     minWidth: 60}} />      (then minWidth 30, 30)
std::array<float, 3> playground(YGConfigRef config, std::array<float, 3> mins) {
  YGNodeRef outer = YGNodeNewWithConfig(config);
  YGNodeStyleSetWidth(outer, 540);
  YGNodeStyleSetFlexDirection(outer, YGFlexDirectionRow);
  YGNodeRef inner = YGNodeNewWithConfig(config);
  YGNodeStyleSetWidth(inner, 540);
  YGNodeStyleSetFlexDirection(inner, YGFlexDirectionRow);
  YGNodeInsertChild(outer, inner, 0);
  for (size_t i = 0; i < 3; i++) {
    YGNodeRef n = YGNodeNewWithConfig(config);
    YGNodeStyleSetFlexGrow(n, 1);
    YGNodeStyleSetFlexShrink(n, 1);
    YGNodeStyleSetMaxWidth(n, 180);
    YGNodeStyleSetHeight(n, 30);
    YGNodeStyleSetMinWidth(n, mins[i]);
    YGNodeInsertChild(inner, n, i);
  }
  YGNodeCalculateLayout(outer, YGUndefined, YGUndefined, YGDirectionLTR);
  std::array<float, 3> widths{};
  for (size_t i = 0; i < 3; i++) {
    widths[i] = YGNodeLayoutGetWidth(YGNodeGetChild(inner, i));
  }
  YGNodeFreeRecursive(outer);
  return widths;
}

void expectWidths(
    const std::array<float, 3>& actual,
    const std::array<float, 3>& expected) {
  for (size_t i = 0; i < 3; i++) {
    EXPECT_NEAR(actual[i], expected[i], 1e-3f) << "item " << i;
  }
}

} // namespace

// Errata cleared. By hand (CSS 9.7): the items' flex base sizes are 0 (no
// width, flexBasis auto, empty), so the free space is 540, 180 each; 180 is
// within every item's [min, 180]. Wherever the 60 minimum is.
TEST(YGFirstPassRunningTotalsTest, playground_tree_fixed) {
  YGConfigRef config = makeConfig(/*runningTotals=*/false);
  for (auto mins : {std::array<float, 3>{60, 30, 30},
                    std::array<float, 3>{30, 60, 30},
                    std::array<float, 3>{30, 30, 60},
                    std::array<float, 3>{30, 30, 30}}) {
    SCOPED_TRACE(
        "minWidths " + std::to_string(mins[0]) + ", " +
        std::to_string(mins[1]) + ", " + std::to_string(mins[2]));
    expectWidths(playground(config, mins), {180, 180, 180});
  }
  YGConfigFree(config);
}

// Errata set (the default for new configs): the legacy collapse. By hand,
// Yoga's legacy algorithm: bases clamped to the minimums (60, 30, 30), free
// space 540 - 120 = 420, grow total 3. First pass with running totals:
// item 0: 60 + 420/3 = 200 -> max 180, frozen, total 2; item 1: 30 + 420/2 =
// 240 -> 180, frozen, total 1; item 2: 30 + 420/1 = 450 -> 180, frozen; the
// frozen deltas 120 + 150 + 150 use up all 420, so the second pass has
// nothing left to distribute and every item keeps its clamped basis.
TEST(YGFirstPassRunningTotalsTest, playground_tree_legacy_errata) {
  YGConfigRef config = YGConfigNew();
  ASSERT_NE(YGConfigGetErrata(config) & YGErrataFlexFirstPassUsesRunningTotals, 0)
      << "new configs set the errata bit by default";
  expectWidths(playground(config, {60, 30, 30}), {60, 30, 30});
  YGConfigFree(config);

  // Setting the bit explicitly on an otherwise errata-free config does the
  // same: the bit alone selects the legacy distribution.
  YGConfigRef only = YGConfigNew();
  YGConfigSetErrata(only, YGErrataFlexFirstPassUsesRunningTotals);
  expectWidths(playground(only, {60, 30, 30}), {60, 30, 30});
  YGConfigFree(only);
}

namespace {

struct Item {
  float basis;
  float minW; // 0: none
  float maxW; // 0: none
};

// A 300-wide row of items with flexGrow 1, flexShrink 1, height 10.
std::vector<float> row300(YGConfigRef config, const std::vector<Item>& items) {
  YGNodeRef root = YGNodeNewWithConfig(config);
  YGNodeStyleSetFlexDirection(root, YGFlexDirectionRow);
  YGNodeStyleSetWidth(root, 300);
  YGNodeStyleSetHeight(root, 10);
  for (size_t i = 0; i < items.size(); i++) {
    YGNodeRef n = YGNodeNewWithConfig(config);
    YGNodeStyleSetFlexGrow(n, 1);
    YGNodeStyleSetFlexShrink(n, 1);
    YGNodeStyleSetFlexBasis(n, items[i].basis);
    if (items[i].minW > 0) {
      YGNodeStyleSetMinWidth(n, items[i].minW);
    }
    if (items[i].maxW > 0) {
      YGNodeStyleSetMaxWidth(n, items[i].maxW);
    }
    YGNodeInsertChild(root, n, i);
  }
  YGNodeCalculateLayout(root, YGUndefined, YGUndefined, YGDirectionLTR);
  std::vector<float> widths;
  for (size_t i = 0; i < items.size(); i++) {
    widths.push_back(YGNodeLayoutGetWidth(YGNodeGetChild(root, i)));
  }
  YGNodeFreeRecursive(root);
  return widths;
}

// Expects `expected` for `items` and for every rotation of `items` (rotated
// the same way): where the constrained item sits must not matter.
void expectAllRotations(
    YGConfigRef config,
    std::vector<Item> items,
    std::vector<float> expected) {
  for (size_t r = 0; r < items.size(); r++) {
    SCOPED_TRACE("rotation " + std::to_string(r));
    std::vector<float> widths = row300(config, items);
    for (size_t i = 0; i < items.size(); i++) {
      EXPECT_NEAR(widths[i], expected[i], 1e-3f) << "item " << i;
    }
    std::rotate(items.begin(), items.begin() + 1, items.end());
    std::rotate(expected.begin(), expected.begin() + 1, expected.end());
  }
}

} // namespace

// Grow with a max clamp, errata cleared. By hand: bases 0, free space 300,
// 100 each; the item with max 50 violates it and is frozen at 50; the other
// two share the remaining 250: 125 each.
TEST(YGFirstPassRunningTotalsTest, grow_max_clamp_any_position_fixed) {
  YGConfigRef config = makeConfig(/*runningTotals=*/false);
  expectAllRotations(
      config, {{0, 0, 50}, {0, 0, 0}, {0, 0, 0}}, {50, 125, 125});
  YGConfigFree(config);
}

// Shrink with a min clamp, errata cleared. By hand: bases 200 each, free
// space 300 - 600 = -300, equal scaled shrink factors (1 * 200), so -100
// each: 100; the item with min 150 violates it and is frozen at 150; the
// other two (bases 200 + 200) absorb 300 - 150 - 400 = -250 in equal
// shares, -125 each: 75 each.
TEST(YGFirstPassRunningTotalsTest, shrink_min_clamp_any_position_fixed) {
  YGConfigRef config = makeConfig(/*runningTotals=*/false);
  expectAllRotations(
      config, {{200, 150, 0}, {200, 0, 0}, {200, 0, 0}}, {150, 75, 75});
  YGConfigFree(config);
}
