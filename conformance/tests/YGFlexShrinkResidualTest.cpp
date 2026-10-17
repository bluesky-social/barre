/*
 * Regression tests for yoga#2038 (https://github.com/react/yoga/issues/2038):
 * flexShrink runaway (~8e9) when the sole shrinkable item of a line has an
 * aspectRatio-derived basis.
 *
 * distributeFreeSpaceFirstPass freezes the item at 0 and subtracts its
 * scaled shrink factor (-flexShrink * computedFlexBasis) from
 * totalFlexShrinkScaledFactors, which calculateFlexLine accumulated from the
 * same product. When the compiler contracts that subtraction into an FMA, it
 * uses the unrounded product (3 * 21.083334f = 63.2500019, a tie that
 * rounds to 63.25 as a float), so the total ends at 2^-19 = 1.9e-6 instead
 * of 0: just above the 1e-6f guard (#1974) in distributeFreeSpaceSecondPass,
 * which then divides the remaining free space (-250) by it. The issue
 * reproduces with Apple clang at -O2 (-ffp-contract=on) and not with
 * -ffp-contract=off. Any residual of that size is possible, since it scales
 * with flexShrink * flexBasis: no absolute epsilon is right; the issue's fix
 * (#2039) counts the shrinkable items left on the line instead.
 *
 * Expected values (from the issue): the box's basis is 23 * 22/24 =
 * 21.083334 (its height is stretched to the row's 23, the sibling's height,
 * and the aspect ratio gives the width); the line overflows by
 * 21.08 + 600 - 350 = 271 and the box is the only shrinkable item, so it
 * shrinks to 0 (no automatic minimum under Yoga's default errata), and its
 * aspect ratio makes its height 0 too: box 0 x 0. The sibling keeps 600 x 23
 * at left 0.
 *
 * Whether real Yoga shows the bug depends on how libyoga was compiled (FMA
 * contraction); see conformance/divergences.md for the result at the
 * conformance runner's flags.
 */

#include <gtest/gtest.h>
#include <yoga/Yoga.h>

#include <cmath>

namespace {

struct Result {
  float boxWidth, boxHeight, siblingLeft, siblingWidth, siblingHeight;
};

// The issue's 4-node reproduction, default config.
Result layoutRepro(float flexShrink) {
  YGNodeRef root = YGNodeNew();
  YGNodeStyleSetWidth(root, 350);
  YGNodeStyleSetHeight(root, 350);

  YGNodeRef row = YGNodeNew();
  YGNodeStyleSetFlexDirection(row, YGFlexDirectionRow);
  YGNodeInsertChild(root, row, 0);

  YGNodeRef box = YGNodeNew();
  YGNodeStyleSetAspectRatio(box, 22.0f / 24.0f); // basis = 23 * 22/24
  YGNodeStyleSetFlexShrink(box, flexShrink);
  YGNodeInsertChild(row, box, 0);

  YGNodeRef sibling = YGNodeNew(); // overflows the row; flexShrink stays 0
  YGNodeStyleSetWidth(sibling, 600);
  YGNodeStyleSetHeight(sibling, 23);
  YGNodeInsertChild(row, sibling, 1);

  YGNodeCalculateLayout(root, YGUndefined, YGUndefined, YGDirectionLTR);
  Result r{
      YGNodeLayoutGetWidth(box),
      YGNodeLayoutGetHeight(box),
      YGNodeLayoutGetLeft(sibling),
      YGNodeLayoutGetWidth(sibling),
      YGNodeLayoutGetHeight(sibling)};
  YGNodeFreeRecursive(root);
  return r;
}

void expectShrunkToZero(float flexShrink) {
  SCOPED_TRACE("flexShrink " + std::to_string(flexShrink));
  Result r = layoutRepro(flexShrink);
  EXPECT_EQ(r.boxWidth, 0);
  EXPECT_EQ(r.boxHeight, 0);
  EXPECT_EQ(r.siblingLeft, 0);
  EXPECT_EQ(r.siblingWidth, 600);
  EXPECT_EQ(r.siblingHeight, 23);
}

} // namespace

// The issue's reproduction as written (flexShrink 3).
TEST(YGFlexShrinkResidualTest, aspect_ratio_box_flex_shrink_3) {
  expectShrunkToZero(3);
}

// The issue's table: 1 and 2 already clamp to 0 on Yoga; 3, 6, 9, 11, 12, 19,
// 22 and 999 explode there (with FMA contraction).
TEST(YGFlexShrinkResidualTest, aspect_ratio_box_flex_shrink_table) {
  for (float shrink : {1.0f, 2.0f, 3.0f, 6.0f, 9.0f, 11.0f, 12.0f, 19.0f,
                       22.0f, 999.0f}) {
    expectShrunkToZero(shrink);
  }
}

// Weaker but compiler-independent: whatever flexShrink, the box never ends
// up larger than its flex basis when its line overflows (shrinking can only
// shrink), and its size is finite.
TEST(YGFlexShrinkResidualTest, shrinking_never_grows) {
  const float basis = 23.0f * 22.0f / 24.0f;
  for (int i = 1; i <= 1000; i++) {
    float shrink = static_cast<float>(i) * 0.25f;
    Result r = layoutRepro(shrink);
    ASSERT_TRUE(std::isfinite(r.boxWidth)) << "flexShrink " << shrink;
    ASSERT_GE(r.boxWidth, 0) << "flexShrink " << shrink;
    ASSERT_LE(r.boxWidth, basis + 1e-3f) << "flexShrink " << shrink;
  }
}
