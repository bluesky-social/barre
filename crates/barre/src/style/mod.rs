//! Node style (`yoga/style/Style.h`, `StyleLength.h`, `StyleSizeLength.h`,
//! `Grid*.h`).
//!
//! [`Style`] is a flat, `Copy`, pointer-free `repr(C)` struct: the snapshot
//! the layout algorithm reads (a host with its own style representation
//! flattens into it). Grid properties are not part of it: no layout reads
//! them yet, and they need heap storage. barre's own nodes keep them next to
//! the style ([`GridStyle`]).
//!
//! Yoga stores style values as 16-bit handles into a per-style value pool.
//! barre stores them inline as [`StyleLength`] (`f32` + unit byte). The
//! observable behaviour of the pool is kept: non-finite values become
//! undefined, and `-0.0` is stored as `0.0` (the pool packs it as the integer
//! 0). Shrinking this representation is later perf work.

use crate::algorithm::flex_direction::{
    flex_end_edge, flex_start_edge, inline_end_edge, inline_start_edge, is_row,
};
use crate::enums::*;
use crate::numeric::{inexact_equals, is_defined, is_undefined, max_or_defined, optional_eq};

/// `yoga::StyleLength` and `yoga::StyleSizeLength`: a length with a unit.
///
/// One type serves both: the C API only ever stores the keyword units
/// (`max-content`, `fit-content`, `stretch`) in size properties.
#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub struct StyleLength {
    value: f32,
    unit: Unit,
}

/// `yoga::StyleSizeLength` (same representation as [`StyleLength`]).
pub type StyleSizeLength = StyleLength;

impl StyleLength {
    pub const UNDEFINED: StyleLength = StyleLength {
        value: f32::NAN,
        unit: Unit::Undefined,
    };
    pub const AUTO: StyleLength = StyleLength {
        value: f32::NAN,
        unit: Unit::Auto,
    };
    pub const MAX_CONTENT: StyleLength = StyleLength {
        value: f32::NAN,
        unit: Unit::MaxContent,
    };
    pub const FIT_CONTENT: StyleLength = StyleLength {
        value: f32::NAN,
        unit: Unit::FitContent,
    };
    pub const STRETCH: StyleLength = StyleLength {
        value: f32::NAN,
        unit: Unit::Stretch,
    };

    #[inline]
    fn valued(value: f32, unit: Unit) -> StyleLength {
        if value.is_nan() || value.is_infinite() {
            StyleLength::UNDEFINED
        } else {
            StyleLength { value, unit }
        }
    }

    /// `StyleLength::points`.
    #[inline]
    pub fn points(value: f32) -> StyleLength {
        Self::valued(value, Unit::Point)
    }

    /// `StyleLength::percent`.
    #[inline]
    pub fn percent(value: f32) -> StyleLength {
        Self::valued(value, Unit::Percent)
    }

    /// `StyleSizeLength::stretch(fraction)` (grid `fr` tracks).
    #[inline]
    pub fn stretch(fraction: f32) -> StyleLength {
        Self::valued(fraction, Unit::Stretch)
    }

    #[inline]
    pub fn unit(self) -> Unit {
        self.unit
    }
    /// The raw value (NaN for keyword units).
    #[inline]
    pub fn value(self) -> f32 {
        self.value
    }
    #[inline]
    pub fn is_auto(self) -> bool {
        self.unit == Unit::Auto
    }
    #[inline]
    pub fn is_undefined(self) -> bool {
        self.unit == Unit::Undefined
    }
    #[inline]
    pub fn is_defined(self) -> bool {
        self.unit != Unit::Undefined
    }
    #[inline]
    pub fn is_points(self) -> bool {
        self.unit == Unit::Point
    }
    #[inline]
    pub fn is_percent(self) -> bool {
        self.unit == Unit::Percent
    }
    #[inline]
    pub fn is_max_content(self) -> bool {
        self.unit == Unit::MaxContent
    }
    #[inline]
    pub fn is_fit_content(self) -> bool {
        self.unit == Unit::FitContent
    }
    #[inline]
    pub fn is_stretch(self) -> bool {
        self.unit == Unit::Stretch
    }

    /// `StyleLength::resolve`: points as-is, percent of `reference_length`,
    /// anything else undefined.
    #[inline]
    pub fn resolve(self, reference_length: f32) -> f32 {
        match self.unit {
            Unit::Point => self.value,
            Unit::Percent => self.value * reference_length * 0.01f32,
            _ => f32::NAN,
        }
    }

    /// `StyleLength::inexactEquals`.
    #[inline]
    pub fn inexact_equals(self, other: StyleLength) -> bool {
        self.unit == other.unit && inexact_equals(self.value, other.value)
    }

    /// What the style pool would hand back for this value: `-0.0` becomes
    /// `0.0` (it is packed as the integer 0).
    #[inline]
    fn stored(self) -> StyleLength {
        StyleLength {
            value: normalize_zero(self.value),
            unit: self.unit,
        }
    }
}

impl PartialEq for StyleLength {
    /// `StyleLength::operator==`.
    fn eq(&self, other: &StyleLength) -> bool {
        optional_eq(self.value, other.value) && self.unit == other.unit
    }
}

#[inline]
fn normalize_zero(value: f32) -> f32 {
    if value == 0.0 { 0.0 } else { value }
}

/// `yoga::GridLineType`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum GridLineType {
    #[default]
    Auto,
    Integer,
    Span,
}

/// `yoga::GridLine`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub struct GridLine {
    pub kind: GridLineType,
    pub integer: i32,
}

impl GridLine {
    pub fn auto() -> GridLine {
        GridLine {
            kind: GridLineType::Auto,
            integer: 0,
        }
    }
    pub fn from_integer(value: i32) -> GridLine {
        GridLine {
            kind: GridLineType::Integer,
            integer: value,
        }
    }
    pub fn span(value: i32) -> GridLine {
        GridLine {
            kind: GridLineType::Span,
            integer: value,
        }
    }
    pub fn is_integer(&self) -> bool {
        self.kind == GridLineType::Integer
    }
}

/// `yoga::GridTrackSize` (the style part; the algorithm-state fields Yoga
/// keeps on it are not style).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct GridTrackSize {
    pub min_sizing_function: StyleSizeLength,
    pub max_sizing_function: StyleSizeLength,
}

impl Default for GridTrackSize {
    fn default() -> Self {
        GridTrackSize::auto()
    }
}

impl GridTrackSize {
    pub fn auto() -> GridTrackSize {
        GridTrackSize {
            min_sizing_function: StyleLength::AUTO,
            max_sizing_function: StyleLength::AUTO,
        }
    }
    pub fn length(points: f32) -> GridTrackSize {
        let len = StyleLength::points(points);
        GridTrackSize {
            min_sizing_function: len,
            max_sizing_function: len,
        }
    }
    pub fn fr(fraction: f32) -> GridTrackSize {
        GridTrackSize {
            min_sizing_function: StyleLength::AUTO,
            max_sizing_function: StyleLength::stretch(fraction),
        }
    }
    pub fn percent(percentage: f32) -> GridTrackSize {
        let len = StyleLength::percent(percentage);
        GridTrackSize {
            min_sizing_function: len,
            max_sizing_function: len,
        }
    }
    pub fn minmax(min: StyleSizeLength, max: StyleSizeLength) -> GridTrackSize {
        GridTrackSize {
            min_sizing_function: min,
            max_sizing_function: max,
        }
    }
}

/// `yoga::GridStyle`.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct GridStyle {
    pub template_columns: Vec<GridTrackSize>,
    pub template_rows: Vec<GridTrackSize>,
    pub auto_columns: Vec<GridTrackSize>,
    pub auto_rows: Vec<GridTrackSize>,
    pub column_start: GridLine,
    pub column_end: GridLine,
    pub row_start: GridLine,
    pub row_end: GridLine,
}

/// `GridStyleStorage::defaults`.
pub static DEFAULT_GRID_STYLE: GridStyle = GridStyle {
    template_columns: Vec::new(),
    template_rows: Vec::new(),
    auto_columns: Vec::new(),
    auto_rows: Vec::new(),
    column_start: GridLine {
        kind: GridLineType::Auto,
        integer: 0,
    },
    column_end: GridLine {
        kind: GridLineType::Auto,
        integer: 0,
    },
    row_start: GridLine {
        kind: GridLineType::Auto,
        integer: 0,
    },
    row_end: GridLine {
        kind: GridLineType::Auto,
        integer: 0,
    },
};

type Edges = [StyleLength; 9];

/// `yoga::Style` (without the grid properties; see the module docs).
#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub struct Style {
    direction: Direction,
    flex_direction: FlexDirection,
    justify_content: Justify,
    justify_items: Justify,
    justify_self: Justify,
    align_content: Align,
    align_items: Align,
    align_self: Align,
    position_type: PositionType,
    flex_wrap: Wrap,
    overflow: Overflow,
    display: Display,
    box_sizing: BoxSizing,
    flex: f32,
    flex_grow: f32,
    flex_shrink: f32,
    aspect_ratio: f32,
    flex_basis: StyleSizeLength,
    margin: Edges,
    position: Edges,
    padding: Edges,
    border: Edges,
    gap: [StyleLength; 3],
    dimensions: [StyleSizeLength; 2],
    min_dimensions: [StyleSizeLength; 2],
    max_dimensions: [StyleSizeLength; 2],
}

impl Default for Style {
    fn default() -> Self {
        Style::new()
    }
}

macro_rules! enum_property {
    ($get:ident, $set:ident, $field:ident, $ty:ty) => {
        #[inline]
        pub fn $get(&self) -> $ty {
            self.$field
        }
        #[inline]
        pub fn $set(&mut self, value: $ty) {
            self.$field = value;
        }
    };
}

macro_rules! number_property {
    ($get:ident, $set:ident, $field:ident) => {
        /// Undefined (NaN) when unset.
        #[inline]
        pub fn $get(&self) -> f32 {
            self.$field
        }
        #[inline]
        pub fn $set(&mut self, value: f32) {
            self.$field = normalize_zero(value);
        }
    };
}

impl Style {
    /// `Style::DefaultFlexGrow`.
    pub const DEFAULT_FLEX_GROW: f32 = 0.0;
    /// `Style::DefaultFlexShrink`.
    pub const DEFAULT_FLEX_SHRINK: f32 = 0.0;
    /// `Style::WebDefaultFlexShrink`.
    pub const WEB_DEFAULT_FLEX_SHRINK: f32 = 1.0;

    /// A style with Yoga's defaults.
    pub fn new() -> Style {
        Style {
            direction: Direction::Inherit,
            flex_direction: FlexDirection::Column,
            justify_content: Justify::FlexStart,
            justify_items: Justify::Stretch,
            justify_self: Justify::Auto,
            align_content: Align::FlexStart,
            align_items: Align::Stretch,
            align_self: Align::Auto,
            position_type: PositionType::Relative,
            flex_wrap: Wrap::NoWrap,
            overflow: Overflow::Visible,
            display: Display::Flex,
            box_sizing: BoxSizing::BorderBox,
            flex: f32::NAN,
            flex_grow: f32::NAN,
            flex_shrink: f32::NAN,
            aspect_ratio: f32::NAN,
            flex_basis: StyleLength::AUTO,
            margin: [StyleLength::UNDEFINED; 9],
            position: [StyleLength::UNDEFINED; 9],
            padding: [StyleLength::UNDEFINED; 9],
            border: [StyleLength::UNDEFINED; 9],
            gap: [StyleLength::UNDEFINED; 3],
            dimensions: [StyleLength::AUTO; 2],
            min_dimensions: [StyleLength::UNDEFINED; 2],
            max_dimensions: [StyleLength::UNDEFINED; 2],
        }
    }

    enum_property!(direction, set_direction, direction, Direction);
    enum_property!(
        flex_direction,
        set_flex_direction,
        flex_direction,
        FlexDirection
    );
    enum_property!(
        justify_content,
        set_justify_content,
        justify_content,
        Justify
    );
    enum_property!(justify_items, set_justify_items, justify_items, Justify);
    enum_property!(justify_self, set_justify_self, justify_self, Justify);
    enum_property!(align_content, set_align_content, align_content, Align);
    enum_property!(align_items, set_align_items, align_items, Align);
    enum_property!(align_self, set_align_self, align_self, Align);
    enum_property!(
        position_type,
        set_position_type,
        position_type,
        PositionType
    );
    enum_property!(flex_wrap, set_flex_wrap, flex_wrap, Wrap);
    enum_property!(overflow, set_overflow, overflow, Overflow);
    enum_property!(display, set_display, display, Display);
    enum_property!(box_sizing, set_box_sizing, box_sizing, BoxSizing);

    number_property!(flex, set_flex, flex);
    number_property!(flex_grow, set_flex_grow, flex_grow);
    number_property!(flex_shrink, set_flex_shrink, flex_shrink);

    #[inline]
    pub fn flex_basis(&self) -> StyleSizeLength {
        self.flex_basis
    }
    #[inline]
    pub fn set_flex_basis(&mut self, value: StyleSizeLength) {
        self.flex_basis = value.stored();
    }

    #[inline]
    pub fn margin(&self, edge: Edge) -> StyleLength {
        self.margin[edge as usize]
    }
    #[inline]
    pub fn set_margin(&mut self, edge: Edge, value: StyleLength) {
        self.margin[edge as usize] = value.stored();
    }
    #[inline]
    pub fn position(&self, edge: Edge) -> StyleLength {
        self.position[edge as usize]
    }
    #[inline]
    pub fn set_position(&mut self, edge: Edge, value: StyleLength) {
        self.position[edge as usize] = value.stored();
    }
    #[inline]
    pub fn padding(&self, edge: Edge) -> StyleLength {
        self.padding[edge as usize]
    }
    #[inline]
    pub fn set_padding(&mut self, edge: Edge, value: StyleLength) {
        self.padding[edge as usize] = value.stored();
    }
    #[inline]
    pub fn border(&self, edge: Edge) -> StyleLength {
        self.border[edge as usize]
    }
    #[inline]
    pub fn set_border(&mut self, edge: Edge, value: StyleLength) {
        self.border[edge as usize] = value.stored();
    }
    #[inline]
    pub fn gap(&self, gutter: Gutter) -> StyleLength {
        self.gap[gutter as usize]
    }
    #[inline]
    pub fn set_gap(&mut self, gutter: Gutter, value: StyleLength) {
        self.gap[gutter as usize] = value.stored();
    }
    #[inline]
    pub fn dimension(&self, axis: Dimension) -> StyleSizeLength {
        self.dimensions[axis as usize]
    }
    #[inline]
    pub fn set_dimension(&mut self, axis: Dimension, value: StyleSizeLength) {
        self.dimensions[axis as usize] = value.stored();
    }
    #[inline]
    pub fn min_dimension(&self, axis: Dimension) -> StyleSizeLength {
        self.min_dimensions[axis as usize]
    }
    #[inline]
    pub fn set_min_dimension(&mut self, axis: Dimension, value: StyleSizeLength) {
        self.min_dimensions[axis as usize] = value.stored();
    }
    #[inline]
    pub fn max_dimension(&self, axis: Dimension) -> StyleSizeLength {
        self.max_dimensions[axis as usize]
    }
    #[inline]
    pub fn set_max_dimension(&mut self, axis: Dimension, value: StyleSizeLength) {
        self.max_dimensions[axis as usize] = value.stored();
    }

    /// Undefined (NaN) when unset.
    #[inline]
    pub fn aspect_ratio(&self) -> f32 {
        self.aspect_ratio
    }
    /// `Style::setAspectRatio`: degenerate aspect ratios act as auto.
    /// See <https://drafts.csswg.org/css-sizing-4/#valdef-aspect-ratio-ratio>.
    #[inline]
    pub fn set_aspect_ratio(&mut self, value: f32) {
        self.aspect_ratio = if value == 0.0 || value.is_infinite() {
            f32::NAN
        } else {
            value
        };
    }

    /// `Style::resolvedMinDimension`.
    pub fn resolved_min_dimension(
        &self,
        direction: Direction,
        axis: Dimension,
        reference_length: f32,
        owner_width: f32,
    ) -> f32 {
        let length = self.min_dimensions[axis as usize];
        if length.is_undefined() {
            return f32::NAN;
        }
        self.add_box_sizing(
            direction,
            axis,
            length.resolve(reference_length),
            owner_width,
        )
    }

    /// `Style::resolvedMaxDimension`.
    pub fn resolved_max_dimension(
        &self,
        direction: Direction,
        axis: Dimension,
        reference_length: f32,
        owner_width: f32,
    ) -> f32 {
        let length = self.max_dimensions[axis as usize];
        if length.is_undefined() {
            return f32::NAN;
        }
        self.add_box_sizing(
            direction,
            axis,
            length.resolve(reference_length),
            owner_width,
        )
    }

    /// Shared tail of `resolvedMin/MaxDimension` and
    /// `Node::getResolvedDimension`: content-box sizes get padding and border
    /// added.
    #[inline]
    pub(crate) fn add_box_sizing(
        &self,
        direction: Direction,
        axis: Dimension,
        value: f32,
        owner_width: f32,
    ) -> f32 {
        if self.box_sizing == BoxSizing::BorderBox || is_undefined(value) {
            return value;
        }
        let padding_and_border =
            self.compute_padding_and_border_for_dimension(direction, axis, owner_width);
        value
            + if is_defined(padding_and_border) {
                padding_and_border
            } else {
                0.0
            }
    }

    /// `Style::horizontalInsetsDefined`.
    pub fn horizontal_insets_defined(&self) -> bool {
        self.position[Edge::Left as usize].is_defined()
            || self.position[Edge::Right as usize].is_defined()
            || self.position[Edge::All as usize].is_defined()
            || self.position[Edge::Horizontal as usize].is_defined()
            || self.position[Edge::Start as usize].is_defined()
            || self.position[Edge::End as usize].is_defined()
    }

    /// `Style::verticalInsetsDefined`.
    pub fn vertical_insets_defined(&self) -> bool {
        self.position[Edge::Top as usize].is_defined()
            || self.position[Edge::Bottom as usize].is_defined()
            || self.position[Edge::All as usize].is_defined()
            || self.position[Edge::Vertical as usize].is_defined()
    }

    /// `Style::isFlexStartPositionDefined`.
    pub fn is_flex_start_position_defined(
        &self,
        axis: FlexDirection,
        direction: Direction,
    ) -> bool {
        self.compute_position(flex_start_edge(axis), direction)
            .is_defined()
    }
    /// `Style::isFlexStartPositionAuto`.
    pub fn is_flex_start_position_auto(&self, axis: FlexDirection, direction: Direction) -> bool {
        self.compute_position(flex_start_edge(axis), direction)
            .is_auto()
    }
    /// `Style::isInlineStartPositionDefined`.
    pub fn is_inline_start_position_defined(
        &self,
        axis: FlexDirection,
        direction: Direction,
    ) -> bool {
        self.compute_position(inline_start_edge(axis, direction), direction)
            .is_defined()
    }
    /// `Style::isInlineStartPositionAuto`.
    pub fn is_inline_start_position_auto(&self, axis: FlexDirection, direction: Direction) -> bool {
        self.compute_position(inline_start_edge(axis, direction), direction)
            .is_auto()
    }
    /// `Style::isFlexEndPositionDefined`.
    pub fn is_flex_end_position_defined(&self, axis: FlexDirection, direction: Direction) -> bool {
        self.compute_position(flex_end_edge(axis), direction)
            .is_defined()
    }
    /// `Style::isFlexEndPositionAuto`.
    pub fn is_flex_end_position_auto(&self, axis: FlexDirection, direction: Direction) -> bool {
        self.compute_position(flex_end_edge(axis), direction)
            .is_auto()
    }
    /// `Style::isInlineEndPositionDefined`.
    pub fn is_inline_end_position_defined(
        &self,
        axis: FlexDirection,
        direction: Direction,
    ) -> bool {
        self.compute_position(inline_end_edge(axis, direction), direction)
            .is_defined()
    }
    /// `Style::isInlineEndPositionAuto`.
    pub fn is_inline_end_position_auto(&self, axis: FlexDirection, direction: Direction) -> bool {
        self.compute_position(inline_end_edge(axis, direction), direction)
            .is_auto()
    }

    /// `Style::computeFlexStartPosition`.
    pub fn compute_flex_start_position(
        &self,
        axis: FlexDirection,
        direction: Direction,
        axis_size: f32,
    ) -> f32 {
        or_zero(
            self.compute_position(flex_start_edge(axis), direction)
                .resolve(axis_size),
        )
    }
    /// `Style::computeInlineStartPosition`.
    pub fn compute_inline_start_position(
        &self,
        axis: FlexDirection,
        direction: Direction,
        axis_size: f32,
    ) -> f32 {
        or_zero(
            self.compute_position(inline_start_edge(axis, direction), direction)
                .resolve(axis_size),
        )
    }
    /// `Style::computeFlexEndPosition`.
    pub fn compute_flex_end_position(
        &self,
        axis: FlexDirection,
        direction: Direction,
        axis_size: f32,
    ) -> f32 {
        or_zero(
            self.compute_position(flex_end_edge(axis), direction)
                .resolve(axis_size),
        )
    }
    /// `Style::computeInlineEndPosition`.
    pub fn compute_inline_end_position(
        &self,
        axis: FlexDirection,
        direction: Direction,
        axis_size: f32,
    ) -> f32 {
        or_zero(
            self.compute_position(inline_end_edge(axis, direction), direction)
                .resolve(axis_size),
        )
    }

    /// `Style::computeFlexStartMargin`.
    pub fn compute_flex_start_margin(
        &self,
        axis: FlexDirection,
        direction: Direction,
        width_size: f32,
    ) -> f32 {
        or_zero(
            self.compute_margin(flex_start_edge(axis), direction)
                .resolve(width_size),
        )
    }
    /// `Style::computeInlineStartMargin`.
    pub fn compute_inline_start_margin(
        &self,
        axis: FlexDirection,
        direction: Direction,
        width_size: f32,
    ) -> f32 {
        or_zero(
            self.compute_margin(inline_start_edge(axis, direction), direction)
                .resolve(width_size),
        )
    }
    /// `Style::computeFlexEndMargin`.
    pub fn compute_flex_end_margin(
        &self,
        axis: FlexDirection,
        direction: Direction,
        width_size: f32,
    ) -> f32 {
        or_zero(
            self.compute_margin(flex_end_edge(axis), direction)
                .resolve(width_size),
        )
    }
    /// `Style::computeInlineEndMargin`.
    pub fn compute_inline_end_margin(
        &self,
        axis: FlexDirection,
        direction: Direction,
        width_size: f32,
    ) -> f32 {
        or_zero(
            self.compute_margin(inline_end_edge(axis, direction), direction)
                .resolve(width_size),
        )
    }

    /// `Style::computeFlexStartBorder`.
    pub fn compute_flex_start_border(&self, axis: FlexDirection, direction: Direction) -> f32 {
        max_or_defined(
            self.compute_border(flex_start_edge(axis), direction)
                .resolve(0.0),
            0.0,
        )
    }
    /// `Style::computeInlineStartBorder`.
    pub fn compute_inline_start_border(&self, axis: FlexDirection, direction: Direction) -> f32 {
        max_or_defined(
            self.compute_border(inline_start_edge(axis, direction), direction)
                .resolve(0.0),
            0.0,
        )
    }
    /// `Style::computeFlexEndBorder`.
    pub fn compute_flex_end_border(&self, axis: FlexDirection, direction: Direction) -> f32 {
        max_or_defined(
            self.compute_border(flex_end_edge(axis), direction)
                .resolve(0.0),
            0.0,
        )
    }
    /// `Style::computeInlineEndBorder`.
    pub fn compute_inline_end_border(&self, axis: FlexDirection, direction: Direction) -> f32 {
        max_or_defined(
            self.compute_border(inline_end_edge(axis, direction), direction)
                .resolve(0.0),
            0.0,
        )
    }

    /// `Style::computeFlexStartPadding`.
    pub fn compute_flex_start_padding(
        &self,
        axis: FlexDirection,
        direction: Direction,
        width_size: f32,
    ) -> f32 {
        max_or_defined(
            self.compute_padding(flex_start_edge(axis), direction)
                .resolve(width_size),
            0.0,
        )
    }
    /// `Style::computeInlineStartPadding`.
    pub fn compute_inline_start_padding(
        &self,
        axis: FlexDirection,
        direction: Direction,
        width_size: f32,
    ) -> f32 {
        max_or_defined(
            self.compute_padding(inline_start_edge(axis, direction), direction)
                .resolve(width_size),
            0.0,
        )
    }
    /// `Style::computeFlexEndPadding`.
    pub fn compute_flex_end_padding(
        &self,
        axis: FlexDirection,
        direction: Direction,
        width_size: f32,
    ) -> f32 {
        max_or_defined(
            self.compute_padding(flex_end_edge(axis), direction)
                .resolve(width_size),
            0.0,
        )
    }
    /// `Style::computeInlineEndPadding`.
    pub fn compute_inline_end_padding(
        &self,
        axis: FlexDirection,
        direction: Direction,
        width_size: f32,
    ) -> f32 {
        max_or_defined(
            self.compute_padding(inline_end_edge(axis, direction), direction)
                .resolve(width_size),
            0.0,
        )
    }

    /// `Style::computeInlineStartPaddingAndBorder`.
    pub fn compute_inline_start_padding_and_border(
        &self,
        axis: FlexDirection,
        direction: Direction,
        width_size: f32,
    ) -> f32 {
        self.compute_inline_start_padding(axis, direction, width_size)
            + self.compute_inline_start_border(axis, direction)
    }
    /// `Style::computeFlexStartPaddingAndBorder`.
    pub fn compute_flex_start_padding_and_border(
        &self,
        axis: FlexDirection,
        direction: Direction,
        width_size: f32,
    ) -> f32 {
        self.compute_flex_start_padding(axis, direction, width_size)
            + self.compute_flex_start_border(axis, direction)
    }
    /// `Style::computeInlineEndPaddingAndBorder`.
    pub fn compute_inline_end_padding_and_border(
        &self,
        axis: FlexDirection,
        direction: Direction,
        width_size: f32,
    ) -> f32 {
        self.compute_inline_end_padding(axis, direction, width_size)
            + self.compute_inline_end_border(axis, direction)
    }
    /// `Style::computeFlexEndPaddingAndBorder`.
    pub fn compute_flex_end_padding_and_border(
        &self,
        axis: FlexDirection,
        direction: Direction,
        width_size: f32,
    ) -> f32 {
        self.compute_flex_end_padding(axis, direction, width_size)
            + self.compute_flex_end_border(axis, direction)
    }

    /// `Style::computePaddingAndBorderForDimension`.
    pub fn compute_padding_and_border_for_dimension(
        &self,
        direction: Direction,
        dimension: Dimension,
        width_size: f32,
    ) -> f32 {
        let flex_direction_for_dimension = if dimension == Dimension::Width {
            FlexDirection::Row
        } else {
            FlexDirection::Column
        };
        self.compute_flex_start_padding_and_border(
            flex_direction_for_dimension,
            direction,
            width_size,
        ) + self.compute_flex_end_padding_and_border(
            flex_direction_for_dimension,
            direction,
            width_size,
        )
    }

    /// `Style::computeBorderForAxis`.
    pub fn compute_border_for_axis(&self, axis: FlexDirection) -> f32 {
        self.compute_inline_start_border(axis, Direction::LTR)
            + self.compute_inline_end_border(axis, Direction::LTR)
    }

    /// `Style::computeMarginForAxis`. The total margin for a given axis does
    /// not depend on the direction, so LTR is used.
    pub fn compute_margin_for_axis(&self, axis: FlexDirection, width_size: f32) -> f32 {
        self.compute_inline_start_margin(axis, Direction::LTR, width_size)
            + self.compute_inline_end_margin(axis, Direction::LTR, width_size)
    }

    /// `Style::computeGapForAxis`.
    pub fn compute_gap_for_axis(&self, axis: FlexDirection, owner_size: f32) -> f32 {
        let gap = if is_row(axis) {
            self.compute_column_gap()
        } else {
            self.compute_row_gap()
        };
        max_or_defined(gap.resolve(owner_size), 0.0)
    }

    /// `Style::computeGapForDimension`.
    pub fn compute_gap_for_dimension(&self, dimension: Dimension, owner_size: f32) -> f32 {
        let gap = if dimension == Dimension::Width {
            self.compute_column_gap()
        } else {
            self.compute_row_gap()
        };
        max_or_defined(gap.resolve(owner_size), 0.0)
    }

    /// `Style::flexStartMarginIsAuto`.
    pub fn flex_start_margin_is_auto(&self, axis: FlexDirection, direction: Direction) -> bool {
        self.compute_margin(flex_start_edge(axis), direction)
            .is_auto()
    }
    /// `Style::flexEndMarginIsAuto`.
    pub fn flex_end_margin_is_auto(&self, axis: FlexDirection, direction: Direction) -> bool {
        self.compute_margin(flex_end_edge(axis), direction)
            .is_auto()
    }
    /// `Style::inlineStartMarginIsAuto`.
    pub fn inline_start_margin_is_auto(&self, axis: FlexDirection, direction: Direction) -> bool {
        self.compute_margin(inline_start_edge(axis, direction), direction)
            .is_auto()
    }
    /// `Style::inlineEndMarginIsAuto`.
    pub fn inline_end_margin_is_auto(&self, axis: FlexDirection, direction: Direction) -> bool {
        self.compute_margin(inline_end_edge(axis, direction), direction)
            .is_auto()
    }

    /// `Style::computeColumnGap`.
    fn compute_column_gap(&self) -> StyleLength {
        if self.gap[Gutter::Column as usize].is_defined() {
            self.gap[Gutter::Column as usize]
        } else {
            self.gap[Gutter::All as usize]
        }
    }

    /// `Style::computeRowGap`.
    fn compute_row_gap(&self) -> StyleLength {
        if self.gap[Gutter::Row as usize].is_defined() {
            self.gap[Gutter::Row as usize]
        } else {
            self.gap[Gutter::All as usize]
        }
    }

    /// `Style::computePosition`.
    #[inline]
    pub fn compute_position(&self, edge: PhysicalEdge, direction: Direction) -> StyleLength {
        compute_edge(&self.position, edge, direction)
    }
    /// `Style::computeMargin`.
    #[inline]
    pub fn compute_margin(&self, edge: PhysicalEdge, direction: Direction) -> StyleLength {
        compute_edge(&self.margin, edge, direction)
    }
    /// `Style::computePadding`.
    #[inline]
    pub fn compute_padding(&self, edge: PhysicalEdge, direction: Direction) -> StyleLength {
        compute_edge(&self.padding, edge, direction)
    }
    /// `Style::computeBorder`.
    #[inline]
    pub fn compute_border(&self, edge: PhysicalEdge, direction: Direction) -> StyleLength {
        compute_edge(&self.border, edge, direction)
    }
}

impl PartialEq for Style {
    /// `Style::operator==` (minus the grid properties).
    fn eq(&self, other: &Style) -> bool {
        self.direction == other.direction
            && self.flex_direction == other.flex_direction
            && self.justify_content == other.justify_content
            && self.justify_items == other.justify_items
            && self.justify_self == other.justify_self
            && self.align_content == other.align_content
            && self.align_items == other.align_items
            && self.align_self == other.align_self
            && self.position_type == other.position_type
            && self.flex_wrap == other.flex_wrap
            && self.overflow == other.overflow
            && self.display == other.display
            && self.box_sizing == other.box_sizing
            && optional_eq(self.flex, other.flex)
            && optional_eq(self.flex_grow, other.flex_grow)
            && optional_eq(self.flex_shrink, other.flex_shrink)
            && self.flex_basis == other.flex_basis
            && self.margin == other.margin
            && self.position == other.position
            && self.padding == other.padding
            && self.border == other.border
            && self.gap == other.gap
            && self.dimensions == other.dimensions
            && self.min_dimensions == other.min_dimensions
            && self.max_dimensions == other.max_dimensions
            && optional_eq(self.aspect_ratio, other.aspect_ratio)
    }
}

#[inline]
fn or_zero(value: f32) -> f32 {
    if is_undefined(value) { 0.0 } else { value }
}

/// `Style::computeLeftEdge` / `computeTopEdge` / `computeRightEdge` /
/// `computeBottomEdge`: the style value that applies to a physical edge,
/// falling back from the specific edge through the shorthands.
#[inline]
fn compute_edge(edges: &Edges, edge: PhysicalEdge, direction: Direction) -> StyleLength {
    let e = |e: Edge| edges[e as usize];
    match edge {
        PhysicalEdge::Left => {
            if direction == Direction::LTR && e(Edge::Start).is_defined() {
                e(Edge::Start)
            } else if direction == Direction::RTL && e(Edge::End).is_defined() {
                e(Edge::End)
            } else if e(Edge::Left).is_defined() {
                e(Edge::Left)
            } else if e(Edge::Horizontal).is_defined() {
                e(Edge::Horizontal)
            } else {
                e(Edge::All)
            }
        }
        PhysicalEdge::Top => {
            if e(Edge::Top).is_defined() {
                e(Edge::Top)
            } else if e(Edge::Vertical).is_defined() {
                e(Edge::Vertical)
            } else {
                e(Edge::All)
            }
        }
        PhysicalEdge::Right => {
            if direction == Direction::LTR && e(Edge::End).is_defined() {
                e(Edge::End)
            } else if direction == Direction::RTL && e(Edge::Start).is_defined() {
                e(Edge::Start)
            } else if e(Edge::Right).is_defined() {
                e(Edge::Right)
            } else if e(Edge::Horizontal).is_defined() {
                e(Edge::Horizontal)
            } else {
                e(Edge::All)
            }
        }
        PhysicalEdge::Bottom => {
            if e(Edge::Bottom).is_defined() {
                e(Edge::Bottom)
            } else if e(Edge::Vertical).is_defined() {
                e(Edge::Vertical)
            } else {
                e(Edge::All)
            }
        }
    }
}
