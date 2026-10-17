/*
 * Ported from vendor/yoga/tests/CacheTest.cpp at Yoga
 * 52f6d0bd3824296b8e852cb4fcdf3b9821ab2aa5 (the vendor/yoga pin). The
 * original is skipped (conformance/skip.txt) because it calls the internal
 * facebook::yoga::canUseCachedMeasurement with yoga::SizingMode values and
 * yoga::Config. Yoga exports the same function in its C API as the
 * (deprecated) YGNodeCanUseCachedMeasurement, which converts each
 * YGMeasureMode to its SizingMode (Exactly = StretchFit, AtMost =
 * FitContent, Undefined = MaxContent) and calls it. Test names, arguments
 * and assertions are kept, with that mapping applied.
 */

#include <gtest/gtest.h>
#include <yoga/Yoga.h>

#pragma clang diagnostic ignored "-Wdeprecated-declarations"
#pragma GCC diagnostic ignored "-Wdeprecated-declarations"

class CacheTest : public ::testing::Test {
 protected:
  void SetUp() override {
    config_ = YGConfigNew();
  }

  void TearDown() override {
    YGConfigFree(config_);
  }

  YGConfigRef config_ = nullptr;
};

TEST_F(CacheTest, negativeCachedMeasurementDisablesCache) {
  // Cached values with defined-but-negative extents are treated as invalid;
  // both axes must independently invalidate the cache.
  const bool negativeWidth = YGNodeCanUseCachedMeasurement(
      YGMeasureModeExactly,
      100.0f,
      YGMeasureModeExactly,
      50.0f,
      YGMeasureModeExactly,
      100.0f,
      YGMeasureModeExactly,
      50.0f,
      /*lastComputedWidth=*/-1.0f,
      /*lastComputedHeight=*/50.0f,
      0.0f,
      0.0f,
      config_);
  EXPECT_FALSE(negativeWidth);

  const bool negativeHeight = YGNodeCanUseCachedMeasurement(
      YGMeasureModeExactly,
      100.0f,
      YGMeasureModeExactly,
      50.0f,
      YGMeasureModeExactly,
      100.0f,
      YGMeasureModeExactly,
      50.0f,
      /*lastComputedWidth=*/100.0f,
      /*lastComputedHeight=*/-0.5f,
      0.0f,
      0.0f,
      config_);
  EXPECT_FALSE(negativeHeight);
}

TEST_F(CacheTest, identicalSpecsAllowCacheReuse) {
  // Primary cache-hit path: identical mode + available size on both axes
  // means the previously computed measurement can be reused verbatim.
  const bool canUse = YGNodeCanUseCachedMeasurement(
      YGMeasureModeExactly,
      100.0f,
      YGMeasureModeAtMost,
      50.0f,
      YGMeasureModeExactly,
      100.0f,
      YGMeasureModeAtMost,
      50.0f,
      /*lastComputedWidth=*/95.0f,
      /*lastComputedHeight=*/45.0f,
      0.0f,
      0.0f,
      config_);

  EXPECT_TRUE(canUse);
}

TEST_F(CacheTest, incompatibleSpecsDisableCache) {
  // Previous run used MaxContent with a computed size of 95; the new run
  // stretches to 200. StretchFit only reuses a cached size when the new
  // inner extent equals the last computed size, which does not hold here,
  // so no compatibility branch applies.
  const bool canUse = YGNodeCanUseCachedMeasurement(
      YGMeasureModeExactly,
      200.0f,
      YGMeasureModeExactly,
      200.0f,
      YGMeasureModeUndefined,
      YGUndefined,
      YGMeasureModeUndefined,
      YGUndefined,
      /*lastComputedWidth=*/95.0f,
      /*lastComputedHeight=*/95.0f,
      0.0f,
      0.0f,
      config_);

  EXPECT_FALSE(canUse);
}

TEST_F(
    CacheTest,
    stretchFitMatchingComputedSizeReusesCacheAccountingForMargin) {
  // Under StretchFit, the requested content size is (availableWidth -
  // marginRow) = 90, which matches the last computed width of 90. This
  // hits the sizeIsExactAndMatchesOldMeasuredSize branch and also verifies
  // that marginRow is subtracted before the comparison (a bare 100 would
  // not match 90).
  const bool canUse = YGNodeCanUseCachedMeasurement(
      YGMeasureModeExactly,
      /*availableWidth=*/100.0f,
      YGMeasureModeExactly,
      /*availableHeight=*/60.0f,
      YGMeasureModeAtMost,
      /*lastAvailableWidth=*/90.0f,
      YGMeasureModeExactly,
      /*lastAvailableHeight=*/60.0f,
      /*lastComputedWidth=*/90.0f,
      /*lastComputedHeight=*/60.0f,
      /*marginRow=*/10.0f,
      /*marginColumn=*/0.0f,
      config_);

  EXPECT_TRUE(canUse);
}

TEST_F(CacheTest, maxContentResultStillFitsFitContentRequest) {
  // Cached MaxContent width of 80 fits inside a FitContent request with 120
  // available space, so the previous result remains valid (the max-content
  // size cannot overflow the looser constraint).
  const bool canUse = YGNodeCanUseCachedMeasurement(
      YGMeasureModeAtMost,
      /*availableWidth=*/120.0f,
      YGMeasureModeExactly,
      /*availableHeight=*/50.0f,
      YGMeasureModeUndefined,
      /*lastAvailableWidth=*/YGUndefined,
      YGMeasureModeExactly,
      /*lastAvailableHeight=*/50.0f,
      /*lastComputedWidth=*/80.0f,
      /*lastComputedHeight=*/50.0f,
      0.0f,
      0.0f,
      config_);

  EXPECT_TRUE(canUse);
}

TEST_F(CacheTest, tighterFitContentReusesCacheWhenPreviousResultStillFits) {
  // Both runs use FitContent. The new available width (100) is stricter than
  // the previous one (150), but the previously computed width (80) still
  // fits inside the new constraint, so the cached measurement is reusable.
  const bool canUse = YGNodeCanUseCachedMeasurement(
      YGMeasureModeAtMost,
      /*availableWidth=*/100.0f,
      YGMeasureModeExactly,
      /*availableHeight=*/50.0f,
      YGMeasureModeAtMost,
      /*lastAvailableWidth=*/150.0f,
      YGMeasureModeExactly,
      /*lastAvailableHeight=*/50.0f,
      /*lastComputedWidth=*/80.0f,
      /*lastComputedHeight=*/50.0f,
      0.0f,
      0.0f,
      config_);

  EXPECT_TRUE(canUse);
}

TEST_F(
    CacheTest,
    tighterFitContentInvalidatesCacheWhenPreviousResultOverflows) {
  // Same shape as the previous test but the cached width (120) is larger
  // than the tightened constraint (100). Reusing the measurement would
  // silently overflow the container, so the cache must be discarded.
  const bool canUse = YGNodeCanUseCachedMeasurement(
      YGMeasureModeAtMost,
      /*availableWidth=*/100.0f,
      YGMeasureModeExactly,
      /*availableHeight=*/50.0f,
      YGMeasureModeAtMost,
      /*lastAvailableWidth=*/150.0f,
      YGMeasureModeExactly,
      /*lastAvailableHeight=*/50.0f,
      /*lastComputedWidth=*/120.0f,
      /*lastComputedHeight=*/50.0f,
      0.0f,
      0.0f,
      config_);

  EXPECT_FALSE(canUse);
}

TEST_F(CacheTest, pixelGridRoundingMakesSubPixelSpecsEquivalent) {
  // At a point scale factor of 2, both 10.25 and 10.5 snap to 10.5 on the
  // physical pixel grid. Specs that differ by less than one device pixel
  // must be treated as identical so we can reuse the cache; without grid
  // rounding, inexactEquals(10.25, 10.5) is false and the cache would miss.
  YGConfigSetPointScaleFactor(config_, 2.0f);

  const bool canUse = YGNodeCanUseCachedMeasurement(
      YGMeasureModeExactly,
      /*availableWidth=*/10.25f,
      YGMeasureModeExactly,
      /*availableHeight=*/20.0f,
      YGMeasureModeExactly,
      /*lastAvailableWidth=*/10.5f,
      YGMeasureModeExactly,
      /*lastAvailableHeight=*/20.0f,
      /*lastComputedWidth=*/10.5f,
      /*lastComputedHeight=*/20.0f,
      0.0f,
      0.0f,
      config_);

  EXPECT_TRUE(canUse);
}
