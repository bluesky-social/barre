/*
 * Helpers for barre's relayout tests (conformance/tests/YGStaleFlexBasisTest,
 * YGFlexBasisRelayoutTest).
 *
 * The invariant: laying out a tree, changing its inputs and laying it out
 * again must give the same layout as laying out a freshly built tree with
 * the new inputs. Caches may make the relayout cheaper, never different.
 */

#pragma once

#include <gtest/gtest.h>
#include <yoga/Yoga.h>

#include <cmath>
#include <string>

namespace barre_test {

inline bool sameFloat(float a, float b) {
  if (std::isnan(a) || std::isnan(b)) {
    return std::isnan(a) && std::isnan(b);
  }
  return std::fabs(a - b) <= 1e-3f;
}

// Expects `actual` and `expected` to have the same shape and, at every node,
// the same left, top, width and height. Failures name the node by its path
// of child indices from the root (e.g. "root/0/1").
inline void expectSameLayout(
    YGNodeRef actual,
    YGNodeRef expected,
    const std::string& path = "root") {
  struct Field {
    const char* name;
    float (*get)(YGNodeConstRef);
  };
  static const Field fields[] = {
      {"left", YGNodeLayoutGetLeft},
      {"top", YGNodeLayoutGetTop},
      {"width", YGNodeLayoutGetWidth},
      {"height", YGNodeLayoutGetHeight},
  };
  for (const auto& f : fields) {
    float a = f.get(actual);
    float e = f.get(expected);
    EXPECT_TRUE(sameFloat(a, e))
        << path << " " << f.name << ": relayout gives " << a
        << ", a fresh layout gives " << e;
  }
  size_t n = YGNodeGetChildCount(actual);
  ASSERT_EQ(n, YGNodeGetChildCount(expected)) << path << ": child count";
  for (size_t i = 0; i < n; i++) {
    expectSameLayout(
        YGNodeGetChild(actual, i),
        YGNodeGetChild(expected, i),
        path + "/" + std::to_string(i));
  }
}

} // namespace barre_test
