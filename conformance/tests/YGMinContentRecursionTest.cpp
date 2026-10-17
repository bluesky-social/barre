/*
 * Regression tests for yoga#2007 (https://github.com/react/yoga/issues/2007):
 * computeMinContentMainSize is a per-axis function, so its container branch
 * recurses into every in-flow child twice (once for the container's main
 * axis, once for its cross axis), re-walking the child's whole subtree each
 * time: T(d) = 2 * T(d - 1), i.e. O(2^depth). Nothing memoizes it, and it
 * bypasses the layout cache. It is reached from computeAutoMinMainSize for
 * every in-flow flex item whenever YGErrataMinSizeUndefinedInsteadOfAuto is
 * not set (CSS Flexbox 4.5 automatic minimum size), e.g. with YGErrataNone.
 *
 * The bug is correctness-neutral: the layout is right, it just takes
 * exponential time. The tree is the issue's repro: a chain of plain
 * `flexDirection: row` containers under an 800x600 row root, with one 20x20
 * leaf at the bottom. Here the leaf is a measure func returning 20x20
 * (instead of width/height 20 styles), so the walk is observable: a leaf with
 * a measure func is measured once per axis per visit of the min-content
 * walk, so the number of measure calls is a deterministic stand-in for time.
 *
 * Expected layout (hand computation, Yoga's non-web defaults): every chain
 * node is a flex item of a row with no size and flexShrink 0, so its flex
 * basis is its max-content width, 20 (the leaf's); its height is stretched
 * (alignItems: stretch) to the root's definite 600, which makes it definite
 * for the next level down. So every chain node is 20 x 600 at (0, 0), and so
 * is the measured leaf (stretched to 600). With the issue's styled 20x20
 * leaf, the leaf is 20 x 20.
 *
 * On Yoga at the vendor pin these fail by design (measure calls grow as
 * 2^depth; the deep test times out). See conformance/divergences.md.
 */

#include <gtest/gtest.h>
#include <yoga/Yoga.h>

#include <vector>

namespace {

long gMeasureCalls = 0;

YGSize measure20(
    YGNodeConstRef /*node*/,
    float /*w*/,
    YGMeasureMode /*wm*/,
    float /*h*/,
    YGMeasureMode /*hm*/) {
  gMeasureCalls++;
  return YGSize{20.0f, 20.0f};
}

struct Chain {
  YGConfigRef config;
  YGNodeRef root;
  std::vector<YGNodeRef> nodes; // chain nodes below the root, top down
  YGNodeRef leaf;
};

Chain buildChain(int depth, bool measuredLeaf) {
  Chain c{};
  c.config = YGConfigNew();
  YGConfigSetErrata(c.config, YGErrataNone);

  c.root = YGNodeNewWithConfig(c.config);
  YGNodeStyleSetFlexDirection(c.root, YGFlexDirectionRow);
  YGNodeStyleSetWidth(c.root, 800);
  YGNodeStyleSetHeight(c.root, 600);

  YGNodeRef parent = c.root;
  for (int i = 0; i < depth; i++) {
    YGNodeRef n = YGNodeNewWithConfig(c.config);
    YGNodeStyleSetFlexDirection(n, YGFlexDirectionRow);
    YGNodeInsertChild(parent, n, 0);
    c.nodes.push_back(n);
    parent = n;
  }
  c.leaf = YGNodeNewWithConfig(c.config);
  if (measuredLeaf) {
    YGNodeSetMeasureFunc(c.leaf, measure20);
  } else {
    YGNodeStyleSetWidth(c.leaf, 20);
    YGNodeStyleSetHeight(c.leaf, 20);
  }
  YGNodeInsertChild(parent, c.leaf, 0);
  return c;
}

void freeChain(Chain& c) {
  YGNodeFreeRecursive(c.root);
  YGConfigFree(c.config);
}

void expectChainLayout(const Chain& c, float leafHeight) {
  EXPECT_EQ(YGNodeLayoutGetWidth(c.root), 800);
  EXPECT_EQ(YGNodeLayoutGetHeight(c.root), 600);
  for (size_t i = 0; i < c.nodes.size(); i++) {
    SCOPED_TRACE("chain node " + std::to_string(i + 1));
    EXPECT_EQ(YGNodeLayoutGetLeft(c.nodes[i]), 0);
    EXPECT_EQ(YGNodeLayoutGetTop(c.nodes[i]), 0);
    EXPECT_EQ(YGNodeLayoutGetWidth(c.nodes[i]), 20);
    EXPECT_EQ(YGNodeLayoutGetHeight(c.nodes[i]), 600);
  }
  EXPECT_EQ(YGNodeLayoutGetLeft(c.leaf), 0);
  EXPECT_EQ(YGNodeLayoutGetTop(c.leaf), 0);
  EXPECT_EQ(YGNodeLayoutGetWidth(c.leaf), 20);
  EXPECT_EQ(YGNodeLayoutGetHeight(c.leaf), leafHeight);
}

// Lays out a chain of `depth` rows over a measured leaf and returns the
// number of measure calls the layout made.
long measureCallsAtDepth(int depth) {
  Chain c = buildChain(depth, /*measuredLeaf=*/true);
  gMeasureCalls = 0;
  YGNodeCalculateLayout(c.root, YGUndefined, YGUndefined, YGDirectionLTR);
  long calls = gMeasureCalls;
  {
    SCOPED_TRACE("depth " + std::to_string(depth));
    expectChainLayout(c, /*leafHeight=*/600);
  }
  freeChain(c);
  return calls;
}

} // namespace

// Small chain: the layout itself is right (on Yoga too; this is a sanity
// check of the expected values above, not a divergence).
TEST(YGMinContentRecursionTest, shallow_chain_layout) {
  for (bool measured : {false, true}) {
    SCOPED_TRACE(measured ? "measured leaf" : "20x20 leaf");
    Chain c = buildChain(6, measured);
    YGNodeCalculateLayout(c.root, YGUndefined, YGUndefined, YGDirectionLTR);
    expectChainLayout(c, measured ? 600 : 20);
    freeChain(c);
  }
}

// The deterministic assertion: the number of measure calls grows at most
// polynomially with depth. With the double recursion, each extra level
// doubles the min-content walk below it, so doubling the depth from 10 to 20
// multiplies the calls by ~2^10; a single walk per subtree (the issue's
// proposed fix) keeps it polynomial (each level's auto-min walks its subtree
// once: O(depth^2) at worst). The bound allows cubic growth, ratio 2^3, with
// slack: 16.
TEST(YGMinContentRecursionTest, measure_calls_polynomial_in_depth) {
  long at10 = measureCallsAtDepth(10);
  long at20 = measureCallsAtDepth(20);
  ASSERT_GT(at10, 0);
  EXPECT_LE(at20, 16 * at10)
      << "measure calls: " << at10 << " at depth 10, " << at20
      << " at depth 20";
}

// An absolute bound at depth 20: a fixed walk makes O(depth^2) measure calls
// (a few per level per auto-min walk), far below 2^20.
TEST(YGMinContentRecursionTest, measure_calls_bounded_at_depth_20) {
  const int depth = 20;
  long calls = measureCallsAtDepth(depth);
  EXPECT_LE(calls, 16L * depth * depth) << "measure calls at depth " << depth;
}

// The issue's repro as written (styled 20x20 leaf, no measure func) at depth
// 26, where the issue measured 4.2 s on Yoga at -O2 (and ~2x more per extra
// level). The conformance runner's per-test timeout (10 s by default) turns
// the exponential walk into a failure; a linear walk finishes in well under
// a millisecond.
TEST(YGMinContentRecursionTest, deep_chain_completes_with_right_layout) {
  Chain c = buildChain(26, /*measuredLeaf=*/false);
  YGNodeCalculateLayout(c.root, YGUndefined, YGUndefined, YGDirectionLTR);
  expectChainLayout(c, /*leafHeight=*/20);
  freeChain(c);
}
