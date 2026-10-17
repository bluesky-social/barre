//! The node-access interface the layout algorithm is written against.
//!
//! The algorithm (`crate::algorithm`) never touches barre's [`Node`] storage
//! directly: it is generic over [`LayoutNode`], a `Copy` handle that exposes
//! exactly what Yoga's algorithm reads and writes on a `yoga::Node`. barre's
//! own nodes implement it for `&Node`; the React Native shim
//! (`docs/RN-SHIM.md`) implements it over RN's C++ `yoga::Node`s. The
//! algorithm is monomorphised per implementation, so this costs nothing.
//!
//! [`NodeExt`] holds the derived node operations from `yoga/node/Node.cpp`
//! (`resolveFlexGrow`, `setPosition`, ...), written once on top of
//! [`LayoutNode`].

use crate::algorithm::flex_direction::{
    dimension, inline_end_edge, inline_start_edge, is_row, resolve_cross_direction,
    resolve_direction,
};
use crate::config::Config;
use crate::enums::*;
use crate::node::layout_results::LayoutResults;
use crate::node::layoutable_children::LayoutableChildren;
use crate::node::{Node, NodePtr, Size};
use crate::numeric::is_defined;
use crate::style::{Style, StyleLength, StyleSizeLength};

/// A handle to a node, as the layout algorithm sees it.
///
/// `'a` is the lifetime of the tree being laid out: styles, configs and
/// layout results borrowed through a handle live that long. Layout state is
/// written through `&'a LayoutResults` (it is made of `Cell`s).
pub trait LayoutNode<'a>: Copy + 'a {
    /// The node's style (`Node::style`).
    fn style(self) -> &'a Style;
    /// The node's layout results and cached layout state
    /// (`Node::getLayout`).
    fn layout(self) -> &'a LayoutResults;
    /// The node's config (`Node::getConfig`).
    fn config(self) -> &'a Config;

    /// `Node::getChildCount` (all children, including `display: contents`
    /// and `display: none` ones).
    fn child_count(self) -> usize;
    /// `Node::getChild`.
    fn child(self, index: usize) -> Self;
    /// `Node::getOwner`.
    fn owner(self) -> Option<Self>;
    /// Whether two handles refer to the same node.
    fn is_same(self, other: Self) -> bool;
    /// `Node::hasContentsChildren`.
    fn has_contents_children(self) -> bool;
    /// `Node::cloneChildrenIfNeeded`: makes every child owned by this node
    /// (cloning shared ones) before layout writes to them.
    fn clone_children_if_needed(self);
    /// `Node::cloneContentsChildrenIfNeeded`.
    fn clone_contents_children_if_needed(self);

    /// `Node::isDirty`.
    fn is_dirty(self) -> bool;
    /// `Node::setDirty` (calls the dirtied callback when becoming dirty).
    fn set_dirty(self, dirty: bool);
    /// `Node::getHasNewLayout`.
    fn has_new_layout(self) -> bool;
    /// `Node::setHasNewLayout`.
    fn set_has_new_layout(self, has_new_layout: bool);

    /// `Node::getNodeType`.
    fn node_type(self) -> NodeType;
    /// `Node::isReferenceBaseline`.
    fn is_reference_baseline(self) -> bool;
    /// `Node::alwaysFormsContainingBlock`.
    fn always_forms_containing_block(self) -> bool;

    /// `Node::hasMeasureFunc`.
    fn has_measure_func(self) -> bool;
    /// `Node::measure` (including its clamping of invalid results).
    fn measure(
        self,
        available_width: f32,
        width_mode: MeasureMode,
        available_height: f32,
        height_mode: MeasureMode,
    ) -> Size;
    /// `Node::hasMinContentMeasureFunc`.
    fn has_min_content_measure_func(self) -> bool;
    /// `Node::measureMinContent`.
    fn measure_min_content(
        self,
        available_width: f32,
        width_mode: MeasureMode,
        available_height: f32,
        height_mode: MeasureMode,
    ) -> Size;
    /// `Node::getMinContentWidth` (undefined when unset).
    fn min_content_width(self) -> f32;
    /// `Node::getMinContentHeight` (undefined when unset).
    fn min_content_height(self) -> f32;
    /// `Node::hasBaselineFunc`.
    fn has_baseline_func(self) -> bool;
    /// `Node::baseline`.
    fn baseline(self, width: f32, height: f32) -> f32;
}

impl<'a> LayoutNode<'a> for &'a Node {
    #[inline]
    fn style(self) -> &'a Style {
        Node::style(self)
    }
    #[inline]
    fn layout(self) -> &'a LayoutResults {
        Node::layout(self)
    }
    #[inline]
    fn config(self) -> &'a Config {
        Node::config(self)
    }
    #[inline]
    fn child_count(self) -> usize {
        Node::child_count(self)
    }
    #[inline]
    fn child(self, index: usize) -> Self {
        Node::child(self, index)
    }
    #[inline]
    fn owner(self) -> Option<Self> {
        Node::owner(self)
    }
    #[inline]
    fn is_same(self, other: Self) -> bool {
        std::ptr::eq(self, other)
    }
    #[inline]
    fn has_contents_children(self) -> bool {
        Node::has_contents_children(self)
    }
    #[inline]
    fn clone_children_if_needed(self) {
        Node::clone_children_if_needed(self)
    }
    #[inline]
    fn clone_contents_children_if_needed(self) {
        Node::clone_contents_children_if_needed(self)
    }
    #[inline]
    fn is_dirty(self) -> bool {
        Node::is_dirty(self)
    }
    #[inline]
    fn set_dirty(self, dirty: bool) {
        Node::set_dirty(self, dirty)
    }
    #[inline]
    fn has_new_layout(self) -> bool {
        Node::has_new_layout(self)
    }
    #[inline]
    fn set_has_new_layout(self, has_new_layout: bool) {
        Node::set_has_new_layout(self, has_new_layout)
    }
    #[inline]
    fn node_type(self) -> NodeType {
        Node::node_type(self)
    }
    #[inline]
    fn is_reference_baseline(self) -> bool {
        Node::is_reference_baseline(self)
    }
    #[inline]
    fn always_forms_containing_block(self) -> bool {
        Node::always_forms_containing_block(self)
    }
    #[inline]
    fn has_measure_func(self) -> bool {
        Node::has_measure_func(self)
    }
    #[inline]
    fn measure(
        self,
        available_width: f32,
        width_mode: MeasureMode,
        available_height: f32,
        height_mode: MeasureMode,
    ) -> Size {
        Node::measure(
            self,
            available_width,
            width_mode,
            available_height,
            height_mode,
        )
    }
    #[inline]
    fn has_min_content_measure_func(self) -> bool {
        Node::has_min_content_measure_func(self)
    }
    #[inline]
    fn measure_min_content(
        self,
        available_width: f32,
        width_mode: MeasureMode,
        available_height: f32,
        height_mode: MeasureMode,
    ) -> Size {
        Node::measure_min_content(
            self,
            available_width,
            width_mode,
            available_height,
            height_mode,
        )
    }
    #[inline]
    fn min_content_width(self) -> f32 {
        Node::min_content_width(self)
    }
    #[inline]
    fn min_content_height(self) -> f32 {
        Node::min_content_height(self)
    }
    #[inline]
    fn has_baseline_func(self) -> bool {
        Node::has_baseline_func(self)
    }
    #[inline]
    fn baseline(self, width: f32, height: f32) -> f32 {
        Node::baseline(self, width, height)
    }
}

impl NodePtr {
    /// The handle the algorithm uses for this node.
    #[inline]
    pub fn as_layout_node<'a>(self) -> &'a Node {
        self.live()
    }
}

/// The derived node operations from `yoga/node/Node.cpp`, available on every
/// [`LayoutNode`].
pub trait NodeExt<'a>: LayoutNode<'a> {
    /// `Node::getLayoutChildren`: the children that take part in layout.
    #[inline]
    fn layout_children(self) -> LayoutableChildren<'a, Self> {
        LayoutableChildren::begin(self)
    }

    /// `Node::getLayoutChildCount`.
    fn layout_child_count(self) -> usize {
        if !self.has_contents_children() {
            self.child_count()
        } else {
            self.layout_children().count()
        }
    }

    /// `Node::hasErrata`.
    #[inline]
    fn has_errata(self, errata: Errata) -> bool {
        self.config().has_errata(errata)
    }

    /// `Node::getLineIndex`.
    #[inline]
    fn line_index(self) -> usize {
        self.layout().line_index.get() as usize
    }

    /// `Node::setLineIndex`.
    #[inline]
    fn set_line_index(self, line_index: usize) {
        self.layout().line_index.set(line_index as u32);
    }

    /// `Node::getProcessedDimension`.
    #[inline]
    fn processed_dimension(self, dimension: Dimension) -> StyleSizeLength {
        self.layout().processed_dimensions[dimension as usize].get()
    }

    /// `Node::getResolvedDimension`.
    fn resolved_dimension(
        self,
        direction: Direction,
        dimension: Dimension,
        reference_length: f32,
        owner_width: f32,
    ) -> f32 {
        let value = self
            .processed_dimension(dimension)
            .resolve(reference_length);
        let style = self.style();
        if style.box_sizing() == BoxSizing::BorderBox {
            return value;
        }
        let padding_and_border =
            style.compute_padding_and_border_for_dimension(direction, dimension, owner_width);
        value
            + if is_defined(padding_and_border) {
                padding_and_border
            } else {
                0.0
            }
    }

    /// `Node::hasDefiniteLength`: whether the node has a "definite length"
    /// along the given axis. <https://www.w3.org/TR/css-sizing-3/#definite>
    #[inline]
    fn has_definite_length(self, dimension: Dimension, owner_size: f32) -> bool {
        let used_value = self.processed_dimension(dimension).resolve(owner_size);
        is_defined(used_value) && used_value >= 0.0
    }

    /// `Node::dimensionWithMargin`.
    #[inline]
    fn dimension_with_margin(self, axis: FlexDirection, width_size: f32) -> f32 {
        self.layout().measured_dimension(dimension(axis))
            + self.style().compute_margin_for_axis(axis, width_size)
    }

    /// `Node::isLayoutDimensionDefined`.
    #[inline]
    fn is_layout_dimension_defined(self, axis: FlexDirection) -> bool {
        let value = self.layout().measured_dimension(dimension(axis));
        is_defined(value) && value >= 0.0
    }

    /// `Node::setLayoutComputedFlexBasis`.
    #[inline]
    fn set_layout_computed_flex_basis(self, computed_flex_basis: f32) {
        self.layout().computed_flex_basis.set(computed_flex_basis);
    }
    /// `Node::setLayoutComputedFlexBasisGeneration`.
    #[inline]
    fn set_layout_computed_flex_basis_generation(self, generation: u32) {
        self.layout().computed_flex_basis_generation.set(generation);
    }
    /// `Node::setLayoutMeasuredDimension`.
    #[inline]
    fn set_layout_measured_dimension(self, measured_dimension: f32, dimension: Dimension) {
        self.layout()
            .set_measured_dimension(dimension, measured_dimension);
    }
    /// `Node::setLayoutHadOverflow`.
    #[inline]
    fn set_layout_had_overflow(self, had_overflow: bool) {
        self.layout().set_had_overflow(had_overflow);
    }
    /// `Node::setLayoutDimension`: sets both the dimension and the raw
    /// (unrounded) dimension.
    #[inline]
    fn set_layout_dimension(self, length_value: f32, dimension: Dimension) {
        self.layout().set_dimension(dimension, length_value);
        self.layout().set_raw_dimension(dimension, length_value);
    }
    /// `Node::setLayoutDirection`.
    #[inline]
    fn set_layout_direction(self, direction: Direction) {
        self.layout().set_direction(direction);
    }
    /// `Node::setLayoutMargin`.
    #[inline]
    fn set_layout_margin(self, margin: f32, edge: PhysicalEdge) {
        self.layout().set_margin(edge, margin);
    }
    /// `Node::setLayoutBorder`.
    #[inline]
    fn set_layout_border(self, border: f32, edge: PhysicalEdge) {
        self.layout().set_border(edge, border);
    }
    /// `Node::setLayoutPadding`.
    #[inline]
    fn set_layout_padding(self, padding: f32, edge: PhysicalEdge) {
        self.layout().set_padding(edge, padding);
    }
    /// `Node::setLayoutPosition`.
    #[inline]
    fn set_layout_position(self, position: f32, edge: PhysicalEdge) {
        self.layout().set_position(edge, position);
    }

    /// `Node::relativePosition`: if both left and right are defined, then use
    /// left. Otherwise return +left or -right depending on which is defined.
    /// Ignore statically positioned nodes as insets do not apply to them.
    fn relative_position(self, axis: FlexDirection, direction: Direction, axis_size: f32) -> f32 {
        let style = self.style();
        if style.position_type() == PositionType::Static {
            return 0.0;
        }
        if style.is_inline_start_position_defined(axis, direction)
            && !style.is_inline_start_position_auto(axis, direction)
        {
            return style.compute_inline_start_position(axis, direction, axis_size);
        }
        -style.compute_inline_end_position(axis, direction, axis_size)
    }

    /// `Node::setPosition`.
    fn set_position(self, direction: Direction, owner_width: f32, owner_height: f32) {
        // Root nodes should always be laid out as LTR, so we don't return
        // negative values.
        let direction_respecting_root = if self.owner().is_some() {
            direction
        } else {
            Direction::LTR
        };
        let style = self.style();
        let main_axis = resolve_direction(style.flex_direction(), direction_respecting_root);
        let cross_axis = resolve_cross_direction(main_axis, direction_respecting_root);

        // In the case of position static these are just 0. See:
        // https://www.w3.org/TR/css-position-3/#valdef-position-static
        let relative_position_main = self.relative_position(
            main_axis,
            direction_respecting_root,
            if is_row(main_axis) {
                owner_width
            } else {
                owner_height
            },
        );
        let relative_position_cross = self.relative_position(
            cross_axis,
            direction_respecting_root,
            if is_row(main_axis) {
                owner_height
            } else {
                owner_width
            },
        );

        let main_axis_leading_edge = inline_start_edge(main_axis, direction);
        let main_axis_trailing_edge = inline_end_edge(main_axis, direction);
        let cross_axis_leading_edge = inline_start_edge(cross_axis, direction);
        let cross_axis_trailing_edge = inline_end_edge(cross_axis, direction);

        self.set_layout_position(
            style.compute_inline_start_margin(main_axis, direction, owner_width)
                + relative_position_main,
            main_axis_leading_edge,
        );
        self.set_layout_position(
            style.compute_inline_end_margin(main_axis, direction, owner_width)
                + relative_position_main,
            main_axis_trailing_edge,
        );
        self.set_layout_position(
            style.compute_inline_start_margin(cross_axis, direction, owner_width)
                + relative_position_cross,
            cross_axis_leading_edge,
        );
        self.set_layout_position(
            style.compute_inline_end_margin(cross_axis, direction, owner_width)
                + relative_position_cross,
            cross_axis_trailing_edge,
        );
    }

    /// `Node::processFlexBasis`.
    fn process_flex_basis(self) -> StyleSizeLength {
        let style = self.style();
        let flex_basis = style.flex_basis();
        if !flex_basis.is_auto() && !flex_basis.is_undefined() {
            return flex_basis;
        }
        if is_defined(style.flex()) && style.flex() > 0.0 {
            return if self.config().use_web_defaults() {
                StyleLength::AUTO
            } else {
                StyleLength::points(0.0)
            };
        }
        StyleLength::AUTO
    }

    /// `Node::resolveFlexBasis`.
    fn resolve_flex_basis(
        self,
        direction: Direction,
        flex_direction: FlexDirection,
        reference_length: f32,
        owner_width: f32,
    ) -> f32 {
        let value = self.process_flex_basis().resolve(reference_length);
        let style = self.style();
        if style.box_sizing() == BoxSizing::BorderBox {
            return value;
        }
        let dim = dimension(flex_direction);
        let padding_and_border =
            style.compute_padding_and_border_for_dimension(direction, dim, owner_width);
        value
            + if is_defined(padding_and_border) {
                padding_and_border
            } else {
                0.0
            }
    }

    /// `Node::processDimensions`.
    fn process_dimensions(self) {
        let style = self.style();
        for dim in [Dimension::Width, Dimension::Height] {
            let processed = if style.max_dimension(dim).is_defined()
                && style
                    .max_dimension(dim)
                    .inexact_equals(style.min_dimension(dim))
            {
                style.max_dimension(dim)
            } else {
                style.dimension(dim)
            };
            self.layout().processed_dimensions[dim as usize].set(processed);
        }
    }

    /// `Node::resolveDirection`.
    fn resolve_direction(self, owner_direction: Direction) -> Direction {
        let direction = self.style().direction();
        if direction == Direction::Inherit {
            if owner_direction != Direction::Inherit {
                owner_direction
            } else {
                Direction::LTR
            }
        } else {
            direction
        }
    }

    /// `Node::resolveFlexGrow`.
    fn resolve_flex_grow(self) -> f32 {
        // Root nodes flexGrow should always be 0
        if self.owner().is_none() {
            return 0.0;
        }
        let style = self.style();
        if is_defined(style.flex_grow()) {
            return style.flex_grow();
        }
        if is_defined(style.flex()) && style.flex() > 0.0 {
            return style.flex();
        }
        Style::DEFAULT_FLEX_GROW
    }

    /// `Node::resolveFlexShrink`.
    fn resolve_flex_shrink(self) -> f32 {
        if self.owner().is_none() {
            return 0.0;
        }
        let style = self.style();
        if is_defined(style.flex_shrink()) {
            return style.flex_shrink();
        }
        let web_defaults = self.config().use_web_defaults();
        if !web_defaults && is_defined(style.flex()) && style.flex() < 0.0 {
            return -style.flex();
        }
        if web_defaults {
            Style::WEB_DEFAULT_FLEX_SHRINK
        } else {
            Style::DEFAULT_FLEX_SHRINK
        }
    }

    /// `Node::isNodeFlexible`.
    fn is_node_flexible(self) -> bool {
        self.style().position_type() != PositionType::Absolute
            && (self.resolve_flex_grow() != 0.0 || self.resolve_flex_shrink() != 0.0)
    }
}

impl<'a, N: LayoutNode<'a>> NodeExt<'a> for N {}
