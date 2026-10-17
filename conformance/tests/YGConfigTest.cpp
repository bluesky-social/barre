/*
 * Ported from vendor/yoga/tests/YGConfigTest.cpp at Yoga
 * 52f6d0bd3824296b8e852cb4fcdf3b9821ab2aa5 (the vendor/yoga pin). The
 * original is skipped (conformance/skip.txt) because its fixture and some
 * tests use yoga::Config / yoga::Node directly. Every test here goes through
 * Yoga.h only; test names and assertions are kept, setup that used internals
 * is rewritten through the C API (see the comment on each such test).
 */

#include <gtest/gtest.h>
#include <yoga/Yoga.h>

#include <stdexcept>

namespace {

// Layout with this measure func counts its calls in the node's context; a
// relayout that re-measures is a relayout whose cached results were
// invalidated.
YGSize countingMeasure(
    YGNodeConstRef node,
    float /*width*/,
    YGMeasureMode /*widthMode*/,
    float /*height*/,
    YGMeasureMode /*heightMode*/) {
  (*static_cast<int*>(YGNodeGetContext(node)))++;
  return YGSize{10, 10};
}

} // namespace

// The original fixture calls yoga::Config::cloneNode(node, owner, 0) directly.
// Through the C API, the config's clone callback is invoked when a parent lays
// out a child it shares with another parent (a child whose owner is not the
// parent being laid out): the parent replaces the child with the callback's
// result, falling back to a plain copy of the child if it returns null.
struct ConfigCloningTest : public ::testing::Test {
  YGConfigRef config = nullptr;
  void SetUp() override;
  void TearDown() override;

  // Lays out a parent that shares `node` with its owner, and returns the
  // child the parent ends up with.
  YGNodeRef cloneThroughLayout();

  YGNodeRef owner = nullptr;
  YGNodeRef owner2 = nullptr;
  YGNodeRef node = nullptr;

  static YGNodeRef clonedNode;
  static YGNodeRef cloneNode(
      YGNodeConstRef /*unused*/,
      YGNodeConstRef /*unused*/,
      size_t /*unused*/) {
    return clonedNode;
  }
  static YGNodeRef doNotClone(
      YGNodeConstRef /*unused*/,
      YGNodeConstRef /*unused*/,
      size_t /*unused*/) {
    return nullptr;
  }
};

YGNodeRef ConfigCloningTest::clonedNode = nullptr;

void ConfigCloningTest::SetUp() {
  config = YGConfigNew();
  clonedNode = YGNodeNewWithConfig(config);
}

void ConfigCloningTest::TearDown() {
  bool clonedNodeAdopted = owner2 != nullptr &&
      YGNodeGetChildCount(owner2) > 0 && YGNodeGetChild(owner2, 0) == clonedNode;
  if (owner2 != nullptr) {
    YGNodeFreeRecursive(owner2);
  }
  if (owner != nullptr) {
    YGNodeFreeRecursive(owner);
  }
  if (!clonedNodeAdopted) {
    YGNodeFree(clonedNode);
  }
  clonedNode = nullptr;
  YGConfigFree(config);
}

YGNodeRef ConfigCloningTest::cloneThroughLayout() {
  owner = YGNodeNewWithConfig(config);
  node = YGNodeNewWithConfig(config);
  YGNodeInsertChild(owner, node, 0);
  // The clone shares `node`, which stays owned by `owner`.
  owner2 = YGNodeClone(owner);
  YGNodeCalculateLayout(owner2, YGUndefined, YGUndefined, YGDirectionLTR);
  return YGNodeGetChild(owner2, 0);
}

TEST_F(ConfigCloningTest, uses_values_provided_by_cloning_callback) {
  YGConfigSetCloneNodeFunc(config, cloneNode);

  auto clone = cloneThroughLayout();

  ASSERT_EQ(clone, clonedNode);
}

TEST_F(
    ConfigCloningTest,
    falls_back_to_regular_cloning_if_callback_returns_null) {
  YGConfigSetCloneNodeFunc(config, doNotClone);

  auto clone = cloneThroughLayout();

  ASSERT_NE(clone, nullptr);
  // (the original frees the clone here; TearDown frees it with owner2)
  ASSERT_NE(clone, node);
}

TEST(YogaTest, config_point_scale_factor_negative_throws) {
  YGConfigRef config = YGConfigNew();

  // Zero is explicitly allowed per the API contract
  YGConfigSetPointScaleFactor(config, 0.0f);
  ASSERT_EQ(YGConfigGetPointScaleFactor(config), 0.0f);

  // Negative values should trigger a fatal assertion (throws std::logic_error)
  ASSERT_THROW(YGConfigSetPointScaleFactor(config, -1.0f), std::logic_error);

  // Verify the value was not changed after the failed set
  ASSERT_EQ(YGConfigGetPointScaleFactor(config), 0.0f);

  YGConfigFree(config);
}

TEST(YogaTest, config_set_logger_null_resets_to_default_logger) {
  YGConfigRef config = YGConfigNew();

  // Track whether our custom logger was called
  bool customLoggerCalled = false;
  YGConfigSetContext(config, &customLoggerCalled);

  YGLogger customLogger = [](YGConfigConstRef cfg,
                             YGNodeConstRef /*node*/,
                             YGLogLevel /*level*/,
                             const char* /*format*/,
                             va_list /*args*/) -> int {
    bool* called = static_cast<bool*>(YGConfigGetContext(cfg));
    *called = true;
    return 0;
  };

  // Set custom logger and verify it's invoked via layout warning
  YGConfigSetLogger(config, customLogger);
  YGNodeRef node = YGNodeNewWithConfig(config);
  YGNodeCalculateLayout(node, YGUndefined, YGUndefined, YGDirectionLTR);
  // Trigger a log by setting a negative point scale factor (which logs before
  // throwing)
  customLoggerCalled = false;
  ASSERT_THROW(YGConfigSetPointScaleFactor(config, -1.0f), std::logic_error);
  ASSERT_TRUE(customLoggerCalled);

  // Reset logger to null — should fall back to default logger
  YGConfigSetLogger(config, nullptr);
  customLoggerCalled = false;
  ASSERT_THROW(YGConfigSetPointScaleFactor(config, -2.0f), std::logic_error);
  // Custom logger should NOT have been called since we reset to default
  ASSERT_FALSE(customLoggerCalled);

  YGNodeFree(node);
  YGConfigFree(config);
}

// The original reads yoga::Config::getVersion(). The version exists only to
// invalidate cached layouts of nodes using the config (Yoga's
// calculateLayoutInternal revisits a node whose layout was computed under
// another config version). Through the C API, "the version incremented" is
// observed as "a relayout of a clean tree re-measures", and "did not
// increment" as "it does not".
TEST(YogaTest, config_version_increments_only_on_actual_changes) {
  YGConfigRef config = YGConfigNew();
  YGNodeRef node = YGNodeNewWithConfig(config);
  int measures = 0;
  YGNodeSetContext(node, &measures);
  YGNodeSetMeasureFunc(node, countingMeasure);
  YGNodeCalculateLayout(node, YGUndefined, YGUndefined, YGDirectionLTR);

  int last = measures;
  auto versionIncremented = [&]() {
    YGNodeCalculateLayout(node, YGUndefined, YGUndefined, YGDirectionLTR);
    bool remeasured = measures != last;
    last = measures;
    return remeasured;
  };

  // A relayout with nothing changed reuses the cached layout
  ASSERT_FALSE(versionIncremented());

  // Changing point scale factor should increment version
  YGConfigSetPointScaleFactor(config, 2.0f);
  ASSERT_TRUE(versionIncremented());

  // Setting the same value again should NOT increment version
  YGConfigSetPointScaleFactor(config, 2.0f);
  ASSERT_FALSE(versionIncremented());

  // Changing errata should increment version
  YGConfigSetErrata(config, YGErrataStretchFlexBasis);
  ASSERT_TRUE(versionIncremented());

  // Setting the same errata again should NOT increment version
  YGConfigSetErrata(config, YGErrataStretchFlexBasis);
  ASSERT_FALSE(versionIncremented());

  // Enabling an experimental feature should increment version
  YGConfigSetExperimentalFeatureEnabled(
      config, YGExperimentalFeatureWebFlexBasis, true);
  ASSERT_TRUE(versionIncremented());

  // Enabling the same feature again should NOT increment version
  YGConfigSetExperimentalFeatureEnabled(
      config, YGExperimentalFeatureWebFlexBasis, true);
  ASSERT_FALSE(versionIncremented());

  YGNodeFree(node);
  YGConfigFree(config);
}

// The original calls yoga::configUpdateInvalidatesLayout(config1, config2)
// directly. Through the C API that predicate decides whether
// YGNodeSetConfig(node, config2) on a node laid out under config1 invalidates
// its layout: "invalidates" is observed as "the next relayout re-measures".
//
// One step of the original cannot be expressed: a node may not move to a
// config with a different useWebDefaults (YGNodeSetConfig throws), so the
// "changing useWebDefaults should invalidate" check is dropped; making the
// two configs match again on useWebDefaults (both true) is kept.
TEST(YogaTest, config_update_invalidates_layout_detects_each_property) {
  YGConfigRef config1 = YGConfigNew();
  YGConfigRef config2 = YGConfigNew();
  YGNodeRef node = YGNodeNewWithConfig(config1);
  int measures = 0;
  YGNodeSetContext(node, &measures);
  YGNodeSetMeasureFunc(node, countingMeasure);

  auto configUpdateInvalidatesLayout = [&]() {
    YGNodeSetConfig(node, config1);
    YGNodeCalculateLayout(node, YGUndefined, YGUndefined, YGDirectionLTR);
    int before = measures;
    YGNodeSetConfig(node, config2);
    YGNodeCalculateLayout(node, YGUndefined, YGUndefined, YGDirectionLTR);
    return measures != before;
  };

  // Two identical configs should not invalidate layout
  ASSERT_FALSE(configUpdateInvalidatesLayout());

  // Changing errata on one config should invalidate
  YGConfigSetErrata(config2, YGErrataStretchFlexBasis);
  ASSERT_TRUE(configUpdateInvalidatesLayout());

  // Make them match again
  YGConfigSetErrata(config1, YGErrataStretchFlexBasis);
  ASSERT_FALSE(configUpdateInvalidatesLayout());

  // Changing point scale factor should invalidate
  YGConfigSetPointScaleFactor(config2, 3.0f);
  ASSERT_TRUE(configUpdateInvalidatesLayout());

  // Make them match again
  YGConfigSetPointScaleFactor(config1, 3.0f);
  ASSERT_FALSE(configUpdateInvalidatesLayout());

  // (Changing useWebDefaults should invalidate: not expressible, see above.)

  // Make them match again
  YGConfigSetUseWebDefaults(config1, true);
  YGConfigSetUseWebDefaults(config2, true);
  ASSERT_FALSE(configUpdateInvalidatesLayout());

  // Changing experimental features should invalidate
  YGConfigSetExperimentalFeatureEnabled(
      config2, YGExperimentalFeatureWebFlexBasis, true);
  ASSERT_TRUE(configUpdateInvalidatesLayout());

  YGNodeFree(node);
  YGConfigFree(config1);
  YGConfigFree(config2);
}

// The original uses yoga::Config::addErrata/removeErrata/hasErrata. Through
// the C API the same bitmask operations are YGConfigSetErrata of
// YGConfigGetErrata combined with the flag.
TEST(YogaTest, config_errata_bitmask_add_remove_operations) {
  YGConfigRef config = YGConfigNew();
  auto hasErrata = [&](YGErrata errata) {
    return (YGConfigGetErrata(config) & errata) != YGErrataNone;
  };
  auto addErrata = [&](YGErrata errata) {
    YGConfigSetErrata(config, YGConfigGetErrata(config) | errata);
  };
  auto removeErrata = [&](YGErrata errata) {
    YGConfigSetErrata(config, YGConfigGetErrata(config) & ~errata);
  };

  // Default configs carry the errata that preserve legacy geometry
  // (MinSizeUndefinedInsteadOfAuto for CSS §4.5 auto-min,
  // FlexFirstPassUsesRunningTotals for free-space distribution). Clear them
  // for this test so we can assert exact equality at the end.
  removeErrata(YGErrataMinSizeUndefinedInsteadOfAuto);
  removeErrata(YGErrataFlexFirstPassUsesRunningTotals);

  // Initially no errata
  ASSERT_FALSE(hasErrata(YGErrataStretchFlexBasis));
  ASSERT_FALSE(hasErrata(YGErrataAbsolutePositionWithoutInsetsExcludesPadding));

  // Add one errata flag
  addErrata(YGErrataStretchFlexBasis);
  ASSERT_TRUE(hasErrata(YGErrataStretchFlexBasis));
  ASSERT_FALSE(hasErrata(YGErrataAbsolutePositionWithoutInsetsExcludesPadding));

  // Add another errata flag — first should still be set
  addErrata(YGErrataAbsolutePositionWithoutInsetsExcludesPadding);
  ASSERT_TRUE(hasErrata(YGErrataStretchFlexBasis));
  ASSERT_TRUE(hasErrata(YGErrataAbsolutePositionWithoutInsetsExcludesPadding));

  // Remove only the first flag — second should remain
  removeErrata(YGErrataStretchFlexBasis);
  ASSERT_FALSE(hasErrata(YGErrataStretchFlexBasis));
  ASSERT_TRUE(hasErrata(YGErrataAbsolutePositionWithoutInsetsExcludesPadding));

  // Remove the second flag
  removeErrata(YGErrataAbsolutePositionWithoutInsetsExcludesPadding);
  ASSERT_EQ(YGConfigGetErrata(config), YGErrataNone);

  YGConfigFree(config);
}
