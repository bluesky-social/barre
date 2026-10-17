//! `YGNodeStyle.h` (`yoga/YGNodeStyle.cpp`).

use barre::debug::assert_fatal::FatalError;
use barre::enums::*;
use barre::node::tree;
use barre::numeric::optional_eq;
use barre::style::{GridLine, GridStyle, GridTrackSize, Style, StyleLength, StyleSizeLength};

use crate::{YGNodeConstRef, YGNodeRef, YGValue, fatal_without_log, from_raw, guard, node};

/// `updateStyle`: applies `update` (which returns whether it changed
/// anything) and marks the node dirty if so.
fn update_style(n: YGNodeRef, update: impl FnOnce(&mut Style) -> bool) {
    let node = node(n);
    // SAFETY: an API entry point; no borrow of this node's style is live.
    let changed = update(unsafe { node.style_mut() });
    if changed {
        node.mark_dirty_and_propagate();
    }
}

/// Sets a property if it differs from `value` (Yoga's `updateStyle`
/// comparison), returning whether it changed.
fn set_if_changed<T: Copy>(
    style: &mut Style,
    value: T,
    get: impl Fn(&Style) -> T,
    eq: impl Fn(T, T) -> bool,
    set: impl FnOnce(&mut Style, T),
) -> bool {
    if eq(get(style), value) {
        false
    } else {
        set(style, value);
        true
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeCopyStyle(dst: YGNodeRef, src: YGNodeConstRef) {
    // SAFETY: an API entry point; no borrow of `dst`'s style is live.
    unsafe { tree::copy_style(node(dst), node(src)) }
}

macro_rules! enum_property {
    ($set:ident, $get:ident, $ty:ident, $getter:ident, $setter:ident) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn $set(n: YGNodeRef, value: u32) {
            let value = from_raw(value, $ty::from_raw);
            update_style(n, |s| {
                set_if_changed(
                    s,
                    value,
                    |s| s.$getter(),
                    |a, b| a == b,
                    |s, v| s.$setter(v),
                )
            });
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn $get(n: YGNodeConstRef) -> u32 {
            node(n).style().$getter() as u32
        }
    };
}

enum_property!(
    YGNodeStyleSetDirection,
    YGNodeStyleGetDirection,
    Direction,
    direction,
    set_direction
);
enum_property!(
    YGNodeStyleSetFlexDirection,
    YGNodeStyleGetFlexDirection,
    FlexDirection,
    flex_direction,
    set_flex_direction
);
enum_property!(
    YGNodeStyleSetJustifyContent,
    YGNodeStyleGetJustifyContent,
    Justify,
    justify_content,
    set_justify_content
);
enum_property!(
    YGNodeStyleSetJustifyItems,
    YGNodeStyleGetJustifyItems,
    Justify,
    justify_items,
    set_justify_items
);
enum_property!(
    YGNodeStyleSetJustifySelf,
    YGNodeStyleGetJustifySelf,
    Justify,
    justify_self,
    set_justify_self
);
enum_property!(
    YGNodeStyleSetAlignContent,
    YGNodeStyleGetAlignContent,
    Align,
    align_content,
    set_align_content
);
enum_property!(
    YGNodeStyleSetAlignItems,
    YGNodeStyleGetAlignItems,
    Align,
    align_items,
    set_align_items
);
enum_property!(
    YGNodeStyleSetAlignSelf,
    YGNodeStyleGetAlignSelf,
    Align,
    align_self,
    set_align_self
);
enum_property!(
    YGNodeStyleSetPositionType,
    YGNodeStyleGetPositionType,
    PositionType,
    position_type,
    set_position_type
);
enum_property!(
    YGNodeStyleSetFlexWrap,
    YGNodeStyleGetFlexWrap,
    Wrap,
    flex_wrap,
    set_flex_wrap
);
enum_property!(
    YGNodeStyleSetOverflow,
    YGNodeStyleGetOverflow,
    Overflow,
    overflow,
    set_overflow
);
enum_property!(
    YGNodeStyleSetBoxSizing,
    YGNodeStyleGetBoxSizing,
    BoxSizing,
    box_sizing,
    set_box_sizing
);

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeStyleSetDisplay(n: YGNodeRef, display: u32) {
    let display = from_raw(display, Display::from_raw);
    let was_contents = node(n).style().display() == Display::Contents;
    update_style(n, |s| {
        set_if_changed(
            s,
            display,
            |s| s.display(),
            |a, b| a == b,
            |s, v| s.set_display(v),
        )
    });
    tree::display_changed(node(n), was_contents);
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeStyleGetDisplay(n: YGNodeConstRef) -> u32 {
    node(n).style().display() as u32
}

macro_rules! number_setter {
    ($set:ident, $getter:ident, $setter:ident) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn $set(n: YGNodeRef, value: f32) {
            update_style(n, |s| {
                set_if_changed(s, value, |s| s.$getter(), optional_eq, |s, v| s.$setter(v))
            });
        }
    };
}

number_setter!(YGNodeStyleSetFlex, flex, set_flex);
number_setter!(YGNodeStyleSetFlexGrow, flex_grow, set_flex_grow);
number_setter!(YGNodeStyleSetFlexShrink, flex_shrink, set_flex_shrink);

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeStyleSetAspectRatio(n: YGNodeRef, value: f32) {
    update_style(n, |s| {
        set_if_changed(
            s,
            value,
            |s| s.aspect_ratio(),
            optional_eq,
            |s, v| s.set_aspect_ratio(v),
        )
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeStyleGetFlex(n: YGNodeConstRef) -> f32 {
    node(n).style().flex()
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeStyleGetFlexGrow(n: YGNodeConstRef) -> f32 {
    let value = node(n).style().flex_grow();
    if value.is_nan() {
        Style::DEFAULT_FLEX_GROW
    } else {
        value
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeStyleGetFlexShrink(n: YGNodeConstRef) -> f32 {
    let node = node(n);
    let value = node.style().flex_shrink();
    if value.is_nan() {
        if node.config().use_web_defaults() {
            Style::WEB_DEFAULT_FLEX_SHRINK
        } else {
            Style::DEFAULT_FLEX_SHRINK
        }
    } else {
        value
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeStyleGetAspectRatio(n: YGNodeConstRef) -> f32 {
    node(n).style().aspect_ratio()
}

fn set_flex_basis(n: YGNodeRef, value: StyleSizeLength) {
    update_style(n, |s| {
        set_if_changed(
            s,
            value,
            |s| s.flex_basis(),
            |a, b| a == b,
            |s, v| s.set_flex_basis(v),
        )
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeStyleSetFlexBasis(n: YGNodeRef, value: f32) {
    set_flex_basis(n, StyleLength::points(value));
}
#[unsafe(no_mangle)]
pub extern "C" fn YGNodeStyleSetFlexBasisPercent(n: YGNodeRef, value: f32) {
    set_flex_basis(n, StyleLength::percent(value));
}
#[unsafe(no_mangle)]
pub extern "C" fn YGNodeStyleSetFlexBasisAuto(n: YGNodeRef) {
    set_flex_basis(n, StyleLength::AUTO);
}
#[unsafe(no_mangle)]
pub extern "C" fn YGNodeStyleSetFlexBasisMaxContent(n: YGNodeRef) {
    set_flex_basis(n, StyleLength::MAX_CONTENT);
}
#[unsafe(no_mangle)]
pub extern "C" fn YGNodeStyleSetFlexBasisFitContent(n: YGNodeRef) {
    set_flex_basis(n, StyleLength::FIT_CONTENT);
}
#[unsafe(no_mangle)]
pub extern "C" fn YGNodeStyleSetFlexBasisStretch(n: YGNodeRef) {
    set_flex_basis(n, StyleLength::STRETCH);
}
#[unsafe(no_mangle)]
pub extern "C" fn YGNodeStyleGetFlexBasis(n: YGNodeConstRef) -> YGValue {
    node(n).style().flex_basis().into()
}

/// Edge-indexed properties (position, margin, padding, border).
#[derive(Copy, Clone)]
enum EdgeProperty {
    Position,
    Margin,
    Padding,
    Border,
}

impl EdgeProperty {
    fn get(self, s: &Style, edge: Edge) -> StyleLength {
        match self {
            EdgeProperty::Position => s.position(edge),
            EdgeProperty::Margin => s.margin(edge),
            EdgeProperty::Padding => s.padding(edge),
            EdgeProperty::Border => s.border(edge),
        }
    }
    fn set(self, s: &mut Style, edge: Edge, value: StyleLength) {
        match self {
            EdgeProperty::Position => s.set_position(edge, value),
            EdgeProperty::Margin => s.set_margin(edge, value),
            EdgeProperty::Padding => s.set_padding(edge, value),
            EdgeProperty::Border => s.set_border(edge, value),
        }
    }
}

fn set_edge(n: YGNodeRef, property: EdgeProperty, edge: u32, value: StyleLength) {
    let edge = from_raw(edge, Edge::from_raw);
    update_style(n, |s| {
        set_if_changed(
            s,
            value,
            |s| property.get(s, edge),
            |a, b| a == b,
            |s, v| property.set(s, edge, v),
        )
    });
}

fn get_edge(n: YGNodeConstRef, property: EdgeProperty, edge: u32) -> YGValue {
    property
        .get(node(n).style(), from_raw(edge, Edge::from_raw))
        .into()
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeStyleSetPosition(n: YGNodeRef, edge: u32, points: f32) {
    set_edge(n, EdgeProperty::Position, edge, StyleLength::points(points));
}
#[unsafe(no_mangle)]
pub extern "C" fn YGNodeStyleSetPositionPercent(n: YGNodeRef, edge: u32, percent: f32) {
    set_edge(
        n,
        EdgeProperty::Position,
        edge,
        StyleLength::percent(percent),
    );
}
#[unsafe(no_mangle)]
pub extern "C" fn YGNodeStyleSetPositionAuto(n: YGNodeRef, edge: u32) {
    set_edge(n, EdgeProperty::Position, edge, StyleLength::AUTO);
}
#[unsafe(no_mangle)]
pub extern "C" fn YGNodeStyleGetPosition(n: YGNodeConstRef, edge: u32) -> YGValue {
    get_edge(n, EdgeProperty::Position, edge)
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeStyleSetMargin(n: YGNodeRef, edge: u32, points: f32) {
    set_edge(n, EdgeProperty::Margin, edge, StyleLength::points(points));
}
#[unsafe(no_mangle)]
pub extern "C" fn YGNodeStyleSetMarginPercent(n: YGNodeRef, edge: u32, percent: f32) {
    set_edge(n, EdgeProperty::Margin, edge, StyleLength::percent(percent));
}
#[unsafe(no_mangle)]
pub extern "C" fn YGNodeStyleSetMarginAuto(n: YGNodeRef, edge: u32) {
    set_edge(n, EdgeProperty::Margin, edge, StyleLength::AUTO);
}
#[unsafe(no_mangle)]
pub extern "C" fn YGNodeStyleGetMargin(n: YGNodeConstRef, edge: u32) -> YGValue {
    get_edge(n, EdgeProperty::Margin, edge)
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeStyleSetPadding(n: YGNodeRef, edge: u32, points: f32) {
    set_edge(n, EdgeProperty::Padding, edge, StyleLength::points(points));
}
#[unsafe(no_mangle)]
pub extern "C" fn YGNodeStyleSetPaddingPercent(n: YGNodeRef, edge: u32, percent: f32) {
    set_edge(
        n,
        EdgeProperty::Padding,
        edge,
        StyleLength::percent(percent),
    );
}
#[unsafe(no_mangle)]
pub extern "C" fn YGNodeStyleGetPadding(n: YGNodeConstRef, edge: u32) -> YGValue {
    get_edge(n, EdgeProperty::Padding, edge)
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeStyleSetBorder(n: YGNodeRef, edge: u32, border: f32) {
    set_edge(n, EdgeProperty::Border, edge, StyleLength::points(border));
}
#[unsafe(no_mangle)]
pub extern "C" fn YGNodeStyleGetBorder(n: YGNodeConstRef, edge: u32) -> f32 {
    let border = node(n).style().border(from_raw(edge, Edge::from_raw));
    if border.is_undefined() || border.is_auto() {
        return f32::NAN;
    }
    border.value()
}

fn set_gap(n: YGNodeRef, gutter: u32, value: StyleLength) {
    let gutter = from_raw(gutter, Gutter::from_raw);
    update_style(n, |s| {
        set_if_changed(
            s,
            value,
            |s| s.gap(gutter),
            |a, b| a == b,
            |s, v| s.set_gap(gutter, v),
        )
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeStyleSetGap(n: YGNodeRef, gutter: u32, gap_length: f32) {
    set_gap(n, gutter, StyleLength::points(gap_length));
}
#[unsafe(no_mangle)]
pub extern "C" fn YGNodeStyleSetGapPercent(n: YGNodeRef, gutter: u32, percent: f32) {
    set_gap(n, gutter, StyleLength::percent(percent));
}
#[unsafe(no_mangle)]
pub extern "C" fn YGNodeStyleGetGap(n: YGNodeConstRef, gutter: u32) -> YGValue {
    node(n)
        .style()
        .gap(from_raw(gutter, Gutter::from_raw))
        .into()
}

/// Dimension-indexed properties (size, min size, max size).
#[derive(Copy, Clone)]
enum SizeProperty {
    Size,
    Min,
    Max,
}

impl SizeProperty {
    fn get(self, s: &Style, dim: Dimension) -> StyleSizeLength {
        match self {
            SizeProperty::Size => s.dimension(dim),
            SizeProperty::Min => s.min_dimension(dim),
            SizeProperty::Max => s.max_dimension(dim),
        }
    }
    fn set(self, s: &mut Style, dim: Dimension, value: StyleSizeLength) {
        match self {
            SizeProperty::Size => s.set_dimension(dim, value),
            SizeProperty::Min => s.set_min_dimension(dim, value),
            SizeProperty::Max => s.set_max_dimension(dim, value),
        }
    }
}

fn set_size(n: YGNodeRef, property: SizeProperty, dim: Dimension, value: StyleSizeLength) {
    update_style(n, |s| {
        set_if_changed(
            s,
            value,
            |s| property.get(s, dim),
            |a, b| a == b,
            |s, v| property.set(s, dim, v),
        )
    });
}

macro_rules! size_property {
    (
        $prop:expr, $dim:expr, $get:ident,
        points: $points:ident, percent: $percent:ident,
        $(auto: $auto:ident,)?
        max_content: $max_content:ident, fit_content: $fit_content:ident, stretch: $stretch:ident
    ) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn $points(n: YGNodeRef, value: f32) {
            set_size(n, $prop, $dim, StyleLength::points(value));
        }
        #[unsafe(no_mangle)]
        pub extern "C" fn $percent(n: YGNodeRef, value: f32) {
            set_size(n, $prop, $dim, StyleLength::percent(value));
        }
        $(
            #[unsafe(no_mangle)]
            pub extern "C" fn $auto(n: YGNodeRef) {
                set_size(n, $prop, $dim, StyleLength::AUTO);
            }
        )?
        #[unsafe(no_mangle)]
        pub extern "C" fn $max_content(n: YGNodeRef) {
            set_size(n, $prop, $dim, StyleLength::MAX_CONTENT);
        }
        #[unsafe(no_mangle)]
        pub extern "C" fn $fit_content(n: YGNodeRef) {
            set_size(n, $prop, $dim, StyleLength::FIT_CONTENT);
        }
        #[unsafe(no_mangle)]
        pub extern "C" fn $stretch(n: YGNodeRef) {
            set_size(n, $prop, $dim, StyleLength::STRETCH);
        }
        #[unsafe(no_mangle)]
        pub extern "C" fn $get(n: YGNodeConstRef) -> YGValue {
            $prop.get(node(n).style(), $dim).into()
        }
    };
}

size_property!(
    SizeProperty::Size, Dimension::Width, YGNodeStyleGetWidth,
    points: YGNodeStyleSetWidth, percent: YGNodeStyleSetWidthPercent,
    auto: YGNodeStyleSetWidthAuto,
    max_content: YGNodeStyleSetWidthMaxContent, fit_content: YGNodeStyleSetWidthFitContent,
    stretch: YGNodeStyleSetWidthStretch
);
size_property!(
    SizeProperty::Size, Dimension::Height, YGNodeStyleGetHeight,
    points: YGNodeStyleSetHeight, percent: YGNodeStyleSetHeightPercent,
    auto: YGNodeStyleSetHeightAuto,
    max_content: YGNodeStyleSetHeightMaxContent, fit_content: YGNodeStyleSetHeightFitContent,
    stretch: YGNodeStyleSetHeightStretch
);
size_property!(
    SizeProperty::Min, Dimension::Width, YGNodeStyleGetMinWidth,
    points: YGNodeStyleSetMinWidth, percent: YGNodeStyleSetMinWidthPercent,
    max_content: YGNodeStyleSetMinWidthMaxContent, fit_content: YGNodeStyleSetMinWidthFitContent,
    stretch: YGNodeStyleSetMinWidthStretch
);
size_property!(
    SizeProperty::Min, Dimension::Height, YGNodeStyleGetMinHeight,
    points: YGNodeStyleSetMinHeight, percent: YGNodeStyleSetMinHeightPercent,
    max_content: YGNodeStyleSetMinHeightMaxContent, fit_content: YGNodeStyleSetMinHeightFitContent,
    stretch: YGNodeStyleSetMinHeightStretch
);
size_property!(
    SizeProperty::Max, Dimension::Width, YGNodeStyleGetMaxWidth,
    points: YGNodeStyleSetMaxWidth, percent: YGNodeStyleSetMaxWidthPercent,
    max_content: YGNodeStyleSetMaxWidthMaxContent, fit_content: YGNodeStyleSetMaxWidthFitContent,
    stretch: YGNodeStyleSetMaxWidthStretch
);
size_property!(
    SizeProperty::Max, Dimension::Height, YGNodeStyleGetMaxHeight,
    points: YGNodeStyleSetMaxHeight, percent: YGNodeStyleSetMaxHeightPercent,
    max_content: YGNodeStyleSetMaxHeightMaxContent, fit_content: YGNodeStyleSetMaxHeightFitContent,
    stretch: YGNodeStyleSetMaxHeightStretch
);

// Grid item placement properties.

/// Updates a grid property through `update`, marking the node dirty if it
/// returns true.
fn update_grid(n: YGNodeRef, update: impl FnOnce(&mut Option<Box<GridStyle>>) -> bool) {
    let node = node(n);
    // SAFETY: an API entry point; no borrow of this node's style is live.
    let changed = update(unsafe { node.grid_mut() });
    if changed {
        node.mark_dirty_and_propagate();
    }
}

fn grid<'a>(n: YGNodeConstRef) -> &'a GridStyle {
    node(n).grid().unwrap_or(&barre::style::DEFAULT_GRID_STYLE)
}

/// `updateStyle` for a grid line: compares with the current value (the
/// defaults if no grid property was set yet), then `ensure()`s the storage.
fn set_grid_line(n: YGNodeRef, value: GridLine, field: fn(&mut GridStyle) -> &mut GridLine) {
    update_grid(n, |storage| {
        let current = match storage {
            Some(g) => *field(g),
            None => *field(&mut GridStyle::default()),
        };
        if current == value {
            return false;
        }
        *field(storage.get_or_insert_with(Default::default)) = value;
        true
    });
}

fn grid_line_value(line: &GridLine) -> i32 {
    if line.is_integer() { line.integer } else { 0 }
}

macro_rules! grid_line_property {
    ($field:ident, $set:ident, $set_auto:ident, $set_span:ident, $get:ident) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn $set(n: YGNodeRef, value: i32) {
            set_grid_line(n, GridLine::from_integer(value), |g| &mut g.$field);
        }
        #[unsafe(no_mangle)]
        pub extern "C" fn $set_auto(n: YGNodeRef) {
            set_grid_line(n, GridLine::auto(), |g| &mut g.$field);
        }
        #[unsafe(no_mangle)]
        pub extern "C" fn $set_span(n: YGNodeRef, span: i32) {
            set_grid_line(n, GridLine::span(span), |g| &mut g.$field);
        }
        #[unsafe(no_mangle)]
        pub extern "C" fn $get(n: YGNodeConstRef) -> i32 {
            grid_line_value(&grid(n).$field)
        }
    };
}

grid_line_property!(
    column_start,
    YGNodeStyleSetGridColumnStart,
    YGNodeStyleSetGridColumnStartAuto,
    YGNodeStyleSetGridColumnStartSpan,
    YGNodeStyleGetGridColumnStart
);
grid_line_property!(
    column_end,
    YGNodeStyleSetGridColumnEnd,
    YGNodeStyleSetGridColumnEndAuto,
    YGNodeStyleSetGridColumnEndSpan,
    YGNodeStyleGetGridColumnEnd
);
grid_line_property!(
    row_start,
    YGNodeStyleSetGridRowStart,
    YGNodeStyleSetGridRowStartAuto,
    YGNodeStyleSetGridRowStartSpan,
    YGNodeStyleGetGridRowStart
);
grid_line_property!(
    row_end,
    YGNodeStyleSetGridRowEnd,
    YGNodeStyleSetGridRowEndAuto,
    YGNodeStyleSetGridRowEndSpan,
    YGNodeStyleGetGridRowEnd
);

// Grid container properties.

/// `gridTrackSizeFromTypeAndValue`.
fn grid_track_size_from_type_and_value(kind: u32, value: f32) -> Result<GridTrackSize, FatalError> {
    match GridTrackType::from_raw(kind) {
        Some(GridTrackType::Points) => Ok(GridTrackSize::length(value)),
        Some(GridTrackType::Percent) => Ok(GridTrackSize::percent(value)),
        Some(GridTrackType::Fr) => Ok(GridTrackSize::fr(value)),
        Some(GridTrackType::Auto) | Some(GridTrackType::Minmax) => Ok(GridTrackSize::auto()),
        None => Err(fatal_without_log("Unknown YGGridTrackType")),
    }
}

/// `styleSizeLengthFromTypeAndValue`.
fn style_size_length_from_type_and_value(
    kind: u32,
    value: f32,
) -> Result<StyleSizeLength, FatalError> {
    match GridTrackType::from_raw(kind) {
        Some(GridTrackType::Points) => Ok(StyleLength::points(value)),
        Some(GridTrackType::Percent) => Ok(StyleLength::percent(value)),
        Some(GridTrackType::Fr) => Ok(StyleLength::stretch(value)),
        Some(GridTrackType::Auto) | Some(GridTrackType::Minmax) => Ok(StyleLength::AUTO),
        None => Err(fatal_without_log("Unknown YGGridTrackType")),
    }
}

/// Grid track list properties, which (unlike other properties) always mark
/// the node dirty.
fn update_tracks(
    n: YGNodeRef,
    list: fn(&mut GridStyle) -> &mut Vec<GridTrackSize>,
    f: impl FnOnce(&mut Vec<GridTrackSize>),
) {
    update_grid(n, |storage| {
        f(list(storage.get_or_insert_with(Default::default)));
        true
    });
}

macro_rules! grid_tracks_property {
    ($field:ident, $set_count:ident, $set:ident, $set_minmax:ident) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn $set_count(n: YGNodeRef, count: usize) {
            update_tracks(
                n,
                |g| &mut g.$field,
                |tracks| tracks.resize(count, GridTrackSize::default()),
            );
        }
        #[unsafe(no_mangle)]
        pub extern "C-unwind" fn $set(n: YGNodeRef, index: usize, kind: u32, value: f32) {
            guard(|| {
                let size = grid_track_size_from_type_and_value(kind, value)?;
                update_tracks(n, |g| &mut g.$field, |tracks| tracks[index] = size);
                Ok(())
            })
        }
        #[unsafe(no_mangle)]
        pub extern "C-unwind" fn $set_minmax(
            n: YGNodeRef,
            index: usize,
            min_kind: u32,
            min_value: f32,
            max_kind: u32,
            max_value: f32,
        ) {
            guard(|| {
                let min = style_size_length_from_type_and_value(min_kind, min_value)?;
                let max = style_size_length_from_type_and_value(max_kind, max_value)?;
                update_tracks(
                    n,
                    |g| &mut g.$field,
                    |tracks| tracks[index] = GridTrackSize::minmax(min, max),
                );
                Ok(())
            })
        }
    };
}

grid_tracks_property!(
    template_columns,
    YGNodeStyleSetGridTemplateColumnsCount,
    YGNodeStyleSetGridTemplateColumn,
    YGNodeStyleSetGridTemplateColumnMinMax
);
grid_tracks_property!(
    template_rows,
    YGNodeStyleSetGridTemplateRowsCount,
    YGNodeStyleSetGridTemplateRow,
    YGNodeStyleSetGridTemplateRowMinMax
);
grid_tracks_property!(
    auto_columns,
    YGNodeStyleSetGridAutoColumnsCount,
    YGNodeStyleSetGridAutoColumn,
    YGNodeStyleSetGridAutoColumnMinMax
);
grid_tracks_property!(
    auto_rows,
    YGNodeStyleSetGridAutoRowsCount,
    YGNodeStyleSetGridAutoRow,
    YGNodeStyleSetGridAutoRowMinMax
);
