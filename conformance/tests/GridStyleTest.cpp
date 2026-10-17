/*
 * Ported from vendor/yoga/tests/GridStyleTest.cpp at Yoga
 * 52f6d0bd3824296b8e852cb4fcdf3b9821ab2aa5 (the vendor/yoga pin). The
 * original is skipped (conformance/skip.txt) because it tests
 * yoga::Style / GridStyle values directly. Test names are kept for the tests
 * that the C API can express:
 *
 * - Style equality is observable through YGNodeCopyStyle, which dirties the
 *   destination only if the two styles differ (stylesEqual below);
 * - `Style copy = style` is YGNodeCopyStyle into a new node, and
 *   `style = Style{}` is YGNodeCopyStyle from a new node;
 * - grid lines read back through YGNodeStyleGetGrid*, which reads `auto`
 *   (and spans) as 0; grid tracks have no C getters, so track values are
 *   compared with stylesEqual against a node built with the expected tracks.
 *
 * Not ported: moving_a_style_hands_over_the_grid_style (C++ move semantics)
 * and tracks_can_be_sized_then_filled (Style::resizeGridTemplateColumns /
 * gridTemplateColumns() have no observable C counterpart beyond the setters
 * themselves), and the static_assert on sizeof(GridStyleStorage).
 */

#include <gtest/gtest.h>
#include <yoga/Yoga.h>

namespace {

// True iff a and b have equal styles: a clean node with a's style is not
// dirtied by copying b's style into it.
bool stylesEqual(YGNodeConstRef a, YGNodeConstRef b) {
  YGNodeRef probe = YGNodeNew();
  YGNodeCopyStyle(probe, a);
  YGNodeCalculateLayout(probe, YGUndefined, YGUndefined, YGDirectionLTR);
  EXPECT_FALSE(YGNodeIsDirty(probe));
  YGNodeCopyStyle(probe, b);
  bool equal = !YGNodeIsDirty(probe);
  YGNodeFree(probe);
  return equal;
}

} // namespace

TEST(GridStyle, unset_grid_style_reads_as_the_defaults) {
  YGNodeRef style = YGNodeNew();

  // gridTemplateColumns() and gridAutoRows() are empty: equal to a style
  // with zero template columns and zero auto rows.
  YGNodeRef empty = YGNodeNew();
  YGNodeStyleSetGridTemplateColumnsCount(empty, 0);
  YGNodeStyleSetGridAutoRowsCount(empty, 0);
  ASSERT_TRUE(stylesEqual(style, empty));
  ASSERT_EQ(YGNodeStyleGetGridColumnStart(style), 0);
  ASSERT_EQ(YGNodeStyleGetGridRowEnd(style), 0);

  YGNodeFree(empty);
  YGNodeFree(style);
}

TEST(GridStyle, unset_grid_style_equals_grid_style_set_to_the_defaults) {
  YGNodeRef unset = YGNodeNew();

  YGNodeRef setToDefaults = YGNodeNew();
  YGNodeStyleSetGridColumnStartAuto(setToDefaults);

  ASSERT_TRUE(stylesEqual(unset, setToDefaults));
  ASSERT_TRUE(stylesEqual(setToDefaults, unset));

  YGNodeFree(setToDefaults);
  YGNodeFree(unset);
}

TEST(GridStyle, styles_with_different_grid_values_are_not_equal) {
  YGNodeRef style = YGNodeNew();
  YGNodeRef withColumnStart = YGNodeNew();
  YGNodeStyleSetGridColumnStart(withColumnStart, 2);

  ASSERT_FALSE(stylesEqual(style, withColumnStart));
  ASSERT_FALSE(stylesEqual(withColumnStart, style));

  YGNodeFree(withColumnStart);
  YGNodeFree(style);
}

TEST(GridStyle, copies_do_not_share_grid_style) {
  auto make = [](float column0) {
    YGNodeRef n = YGNodeNew();
    YGNodeStyleSetGridTemplateColumnsCount(n, 1);
    YGNodeStyleSetGridTemplateColumn(n, 0, YGGridTrackTypePoints, column0);
    YGNodeStyleSetGridRowEndSpan(n, 3);
    return n;
  };
  YGNodeRef style = make(10.0f);
  YGNodeRef columns10 = make(10.0f);
  YGNodeRef columns20 = make(20.0f);

  YGNodeRef copy = YGNodeNew();
  YGNodeCopyStyle(copy, style);

  ASSERT_TRUE(stylesEqual(copy, style));
  // copy.gridTemplateColumns()[0] == length(10), copy.gridRowEnd() == span(3)
  ASSERT_TRUE(stylesEqual(copy, columns10));

  YGNodeStyleSetGridTemplateColumn(copy, 0, YGGridTrackTypePoints, 20.0f);
  ASSERT_TRUE(stylesEqual(copy, columns20));

  // style.gridTemplateColumns()[0] is still length(10)
  ASSERT_TRUE(stylesEqual(style, columns10));
  ASSERT_FALSE(stylesEqual(copy, style));

  YGNodeFree(copy);
  YGNodeFree(columns20);
  YGNodeFree(columns10);
  YGNodeFree(style);
}

TEST(GridStyle, assigning_an_unset_style_clears_a_set_one) {
  YGNodeRef style = YGNodeNew();
  YGNodeStyleSetGridTemplateColumnsCount(style, 1);
  YGNodeStyleSetGridTemplateColumn(style, 0, YGGridTrackTypePoints, 10.0f);
  YGNodeStyleSetGridRowStartSpan(style, 2);

  YGNodeRef unset = YGNodeNew();
  YGNodeCopyStyle(style, unset);

  // gridTemplateColumns() is empty and gridRowStart() is auto
  YGNodeRef unset2 = YGNodeNew();
  ASSERT_TRUE(stylesEqual(style, unset2));
  ASSERT_EQ(YGNodeStyleGetGridRowStart(style), 0);

  YGNodeFree(unset2);
  YGNodeFree(unset);
  YGNodeFree(style);
}
