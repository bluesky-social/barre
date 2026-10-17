//! The flexbox algorithm (`yoga/algorithm/CalculateLayout.cpp`).

use std::sync::atomic::{AtomicU32, Ordering};

use crate::algorithm::absolute_layout::layout_absolute_descendants;
use crate::algorithm::align::{
    fallback_alignment, fallback_justification, resolve_child_alignment,
};
use crate::algorithm::baseline::{calculate_baseline, is_baseline_layout};
use crate::algorithm::bound_axis::{
    bound_axis, bound_axis_within_min_and_max, padding_and_border_for_axis,
};
use crate::algorithm::cache::{CachedResult, find_cached_result};
use crate::algorithm::flex_direction::{
    dimension, flex_start_edge, is_column, is_row, resolve_cross_direction, resolve_direction,
};
use crate::algorithm::flex_line::{FlexLine, calculate_flex_line};
use crate::algorithm::pixel_grid::round_layout_results_to_pixel_grid;
use crate::algorithm::sizing_mode::{SizingMode, measure_mode};
use crate::algorithm::trailing_position::{needs_trailing_position, set_child_trailing_position};
use crate::enums::*;
use crate::node::layout_results::{CachedMeasurement, FlexBasisAnchor, MAX_CACHED_MEASUREMENTS};
use crate::node::{LayoutNode, NodeExt};
use crate::numeric::{inexact_equals, is_defined, is_undefined, max_or_defined, min_or_defined};
use crate::style::{Style, StyleLength};

/// `gCurrentGenerationCount`: a process-wide, monotonically increasing id for
/// layout passes. A node visited in the current pass is not visited again
/// (unless its constraints change); a process-wide counter keeps passes over
/// different trees that share nodes (persistent cloning) from colliding.
/// This is the only global in barre, and it is not state about any tree.
static CURRENT_GENERATION_COUNT: AtomicU32 = AtomicU32::new(0);

/// `yoga::LayoutPassReason`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum LayoutPassReason {
    Initial = 0,
    AbsLayout = 1,
    Stretch = 2,
    MultilineStretch = 3,
    FlexLayout = 4,
    MeasureChild = 5,
    AbsMeasureChild = 6,
    FlexMeasure = 7,
    GridLayout = 8,
}

/// `LayoutPassReason::COUNT`.
pub const LAYOUT_PASS_REASON_COUNT: usize = 9;

/// `yoga::LayoutData`: per-pass statistics. This is the per-layout scratch
/// context passed down the recursion.
#[derive(Clone, Debug, Default)]
pub struct LayoutData {
    pub layouts: i32,
    pub measures: i32,
    pub max_measure_cache: u32,
    pub cached_layouts: i32,
    pub cached_measures: i32,
    pub measure_callbacks: i32,
    pub measure_callback_reasons_count: [i32; LAYOUT_PASS_REASON_COUNT],
}

/// `hasAutoHorizontalMargin`.
fn has_auto_horizontal_margin(style: &Style) -> bool {
    for direction in [Direction::LTR, Direction::RTL] {
        if style.flex_start_margin_is_auto(FlexDirection::Row, direction)
            || style.flex_end_margin_is_auto(FlexDirection::Row, direction)
        {
            return true;
        }
    }
    false
}

/// `isColumnStretchEdge`.
fn is_column_stretch_edge<'a, N: LayoutNode<'a>>(owner: N, child: N) -> bool {
    let owner_style = owner.style();
    let child_style = child.style();
    let child_width = child.processed_dimension(Dimension::Width);
    owner_style.display() == Display::Flex
        && is_column(owner_style.flex_direction())
        && owner_style.flex_wrap() == Wrap::NoWrap
        && child_style.position_type() != PositionType::Absolute
        && !is_defined(child_style.aspect_ratio())
        && (child_width.is_auto() || child_width.is_undefined())
        && !has_auto_horizontal_margin(child_style)
        && resolve_child_alignment(owner, child) == Align::Stretch
}

/// `isInColumnStretchScrollSubtree`.
fn is_in_column_stretch_scroll_subtree<'a, N: LayoutNode<'a>>(node: N) -> bool {
    let mut current = node;
    loop {
        let mut owner = current.owner();
        while let Some(o) = owner {
            if o.style().display() != Display::Contents {
                break;
            }
            owner = o.owner();
        }
        let Some(owner) = owner else {
            return false;
        };
        if !is_column_stretch_edge(owner, current) {
            return false;
        }
        if owner.style().overflow() == Overflow::Scroll {
            return true;
        }
        current = owner;
    }
}

/// `isNonZeroLength`.
fn is_non_zero_length(length: StyleLength) -> bool {
    length.is_auto() || (is_defined(length.value()) && length.value() != 0.0)
}

/// `hasNonZeroVerticalSpacing`.
fn has_non_zero_vertical_spacing(style: &Style) -> bool {
    for edge in [Edge::Top, Edge::Bottom, Edge::Vertical, Edge::All] {
        if is_non_zero_length(style.margin(edge))
            || is_non_zero_length(style.padding(edge))
            || is_non_zero_length(style.border(edge))
        {
            return true;
        }
    }
    false
}

/// `hasPercentageLength`.
fn has_percentage_length(style: &Style) -> bool {
    for &edge in Edge::ALL {
        if style.margin(edge).is_percent()
            || style.position(edge).is_percent()
            || style.padding(edge).is_percent()
            || style.border(edge).is_percent()
        {
            return true;
        }
    }
    for dimension in [Dimension::Width, Dimension::Height] {
        if style.dimension(dimension).is_percent()
            || style.min_dimension(dimension).is_percent()
            || style.max_dimension(dimension).is_percent()
        {
            return true;
        }
    }
    style.flex_basis().is_percent()
        || style.gap(Gutter::Column).is_percent()
        || style.gap(Gutter::Row).is_percent()
        || style.gap(Gutter::All).is_percent()
}

/// `hasNonZeroFlex`.
fn has_non_zero_flex<'a, N: LayoutNode<'a>>(node: N) -> bool {
    let style = node.style();
    let flex = style.flex();
    let flex_grow = style.flex_grow();
    let flex_shrink = style.flex_shrink();
    let can_grow = if is_defined(flex_grow) {
        flex_grow != 0.0
    } else {
        is_defined(flex) && flex > 0.0
    };
    let can_shrink = if is_defined(flex_shrink) {
        flex_shrink != 0.0
    } else {
        node.config().use_web_defaults() || (is_defined(flex) && flex < 0.0)
    };
    can_grow || can_shrink
}

/// `isHeightFitContentIndependent`.
fn is_height_fit_content_independent<'a, N: LayoutNode<'a>>(node: N) -> bool {
    let style = node.style();
    let height = style.dimension(Dimension::Height);
    let flex_basis = style.flex_basis();
    let has_relative_percent_position = style.position(Edge::Top).is_percent()
        || style.position(Edge::Bottom).is_percent()
        || style.position(Edge::Vertical).is_percent()
        || style.position(Edge::All).is_percent();
    !node.has_measure_func()
        && !node.has_min_content_measure_func()
        && !node.has_baseline_func()
        && !node.is_reference_baseline()
        && (height.is_auto() || height.is_undefined())
        && style.min_dimension(Dimension::Height).is_undefined()
        && style.max_dimension(Dimension::Height).is_undefined()
        && (flex_basis.is_auto() || flex_basis.is_undefined())
        && !has_non_zero_flex(node)
        && style.box_sizing() == BoxSizing::BorderBox
        && !is_defined(style.aspect_ratio())
        && style.position_type() != PositionType::Absolute
        && style.overflow() != Overflow::Scroll
        && style.display() == Display::Flex
        && is_column(style.flex_direction())
        && style.align_items() == Align::Stretch
        && (style.align_self() == Align::Auto || style.align_self() == Align::Stretch)
        && style.justify_content() == Justify::FlexStart
        && style.flex_wrap() == Wrap::NoWrap
        && !style.gap(Gutter::All).is_defined()
        && !style.gap(Gutter::Row).is_defined()
        && !has_relative_percent_position
        && !has_non_zero_vertical_spacing(style)
        && !has_percentage_length(style)
}

/// `canSkipHeightFitContent`.
fn can_skip_height_fit_content<'a, N: LayoutNode<'a>>(root: N) -> bool {
    const MAX_PENDING_NODES: usize = 64;
    let mut stack: [Option<N>; MAX_PENDING_NODES] = [None; MAX_PENDING_NODES];
    stack[0] = Some(root);
    let mut stack_size = 1;
    while stack_size > 0 {
        stack_size -= 1;
        let Some(node) = stack[stack_size] else {
            return false;
        };
        if !is_height_fit_content_independent(node) {
            return false;
        }
        for child in node.layout_children() {
            if stack_size == stack.len() {
                return false;
            }
            stack[stack_size] = Some(child);
            stack_size += 1;
        }
    }
    true
}

/// `constrainMaxSizeForMode`.
pub fn constrain_max_size_for_mode<'a, N: LayoutNode<'a>>(
    node: N,
    direction: Direction,
    axis: FlexDirection,
    owner_axis_size: f32,
    owner_width: f32,
    mode: &mut SizingMode,
    size: &mut f32,
) {
    let max_size = node.style().resolved_max_dimension(
        direction,
        dimension(axis),
        owner_axis_size,
        owner_width,
    ) + node.style().compute_margin_for_axis(axis, owner_width);
    match *mode {
        SizingMode::StretchFit | SizingMode::FitContent => {
            *size = if is_undefined(max_size) || *size < max_size {
                *size
            } else {
                max_size
            };
        }
        SizingMode::MaxContent => {
            if is_defined(max_size) {
                *mode = SizingMode::FitContent;
                *size = max_size;
            }
        }
    }
}

/// `computeFlexBasisForChild`.
fn compute_flex_basis_for_child<'a, N: LayoutNode<'a>>(
    node: N,
    child: N,
    width: f32,
    width_mode: SizingMode,
    height: f32,
    owner_width: f32,
    owner_height: f32,
    height_mode: SizingMode,
    direction: Direction,
    layout_marker_data: &mut LayoutData,
    depth: u32,
    generation_count: u32,
) {
    let main_axis = resolve_direction(node.style().flex_direction(), direction);
    let is_main_axis_row = is_row(main_axis);
    let main_axis_size = if is_main_axis_row { width } else { height };
    let main_axis_owner_size = if is_main_axis_row {
        owner_width
    } else {
        owner_height
    };

    let mut child_width = f32::NAN;
    let mut child_height = f32::NAN;

    let resolved_flex_basis =
        child.resolve_flex_basis(direction, main_axis, main_axis_owner_size, owner_width);
    let is_row_style_dim_defined = child.has_definite_length(Dimension::Width, owner_width);
    let is_column_style_dim_defined = child.has_definite_length(Dimension::Height, owner_height);

    let fix_flex_basis_fit_content = node
        .config()
        .is_experimental_feature_enabled(ExperimentalFeature::FixFlexBasisFitContent);

    let use_resolved_flex_basis = is_defined(resolved_flex_basis) && is_defined(main_axis_size);

    if use_resolved_flex_basis {
        // Deliberately differs from Yoga (yoga#1552): the `WebFlexBasis`
        // gate is dropped, so a basis from an earlier layout is always
        // rewritten. With the flex-basis anchor replay and cache stamps in
        // `calculate_layout_internal` (yoga#2019), a stale stamp here means
        // no run of `node` in this layout (real or skipped) has written the
        // child's basis, which is exactly when a fresh layout writes the
        // resolved value. So this is the fresh-layout value, not a stand-in.
        if is_undefined(child.layout().computed_flex_basis.get())
            || child.layout().computed_flex_basis_generation.get() != generation_count
        {
            let padding_and_border =
                padding_and_border_for_axis(child, main_axis, direction, owner_width);
            child.set_layout_computed_flex_basis(max_or_defined(
                resolved_flex_basis,
                padding_and_border,
            ));
        }
    } else if is_main_axis_row && is_row_style_dim_defined {
        // The width is definite, so use that as the flex basis.
        let padding_and_border =
            padding_and_border_for_axis(child, FlexDirection::Row, direction, owner_width);

        child.set_layout_computed_flex_basis(max_or_defined(
            child.resolved_dimension(direction, Dimension::Width, owner_width, owner_width),
            padding_and_border,
        ));
    } else if !is_main_axis_row && is_column_style_dim_defined {
        // The height is definite, so use that as the flex basis.
        let padding_and_border =
            padding_and_border_for_axis(child, FlexDirection::Column, direction, owner_width);
        child.set_layout_computed_flex_basis(max_or_defined(
            child.resolved_dimension(direction, Dimension::Height, owner_height, owner_width),
            padding_and_border,
        ));
    } else {
        // Compute the flex basis and hypothetical main size (i.e. the clamped
        // flex basis).
        let mut child_width_sizing_mode = SizingMode::MaxContent;
        let mut child_height_sizing_mode = SizingMode::MaxContent;

        let margin_row = child
            .style()
            .compute_margin_for_axis(FlexDirection::Row, owner_width);
        let margin_column = child
            .style()
            .compute_margin_for_axis(FlexDirection::Column, owner_width);

        if is_row_style_dim_defined {
            child_width =
                child.resolved_dimension(direction, Dimension::Width, owner_width, owner_width)
                    + margin_row;
            child_width_sizing_mode = SizingMode::StretchFit;
        }
        if is_column_style_dim_defined {
            child_height =
                child.resolved_dimension(direction, Dimension::Height, owner_height, owner_width)
                    + margin_column;
            child_height_sizing_mode = SizingMode::StretchFit;
        }

        // The W3C spec doesn't say anything about the 'overflow' property,
        // but all major browsers appear to implement the following logic.
        if (!is_main_axis_row && node.style().overflow() == Overflow::Scroll)
            || node.style().overflow() != Overflow::Scroll
        {
            if is_undefined(child_width) && is_defined(width) {
                child_width = width;
                child_width_sizing_mode = SizingMode::FitContent;
            }
        }

        // A zero-intrinsic-height column subtree has the same layout with an
        // unbounded height, allowing its measurement cache to survive
        // unrelated size changes elsewhere in a vertical scroll subtree.
        let parent_does_not_scroll = node.style().overflow() != Overflow::Scroll;
        let mut apply_height_fit_content = is_main_axis_row || parent_does_not_scroll;
        if fix_flex_basis_fit_content {
            let child_had_overflow = child.is_dirty() && child.layout().had_overflow();
            let has_height_independent_subtree = !is_main_axis_row
                && parent_does_not_scroll
                && is_undefined(child_height)
                && is_defined(height)
                && is_column_stretch_edge(node, child)
                && is_in_column_stretch_scroll_subtree(node)
                && can_skip_height_fit_content(child);
            if has_height_independent_subtree && child_had_overflow {
                child.set_layout_had_overflow(false);
            }
            if has_height_independent_subtree {
                apply_height_fit_content = false;
            }
        }
        if apply_height_fit_content && is_undefined(child_height) && is_defined(height) {
            child_height = height;
            child_height_sizing_mode = SizingMode::FitContent;
        }

        let child_style = child.style();
        let aspect_ratio = child_style.aspect_ratio();
        if is_defined(aspect_ratio) {
            if !is_main_axis_row && child_width_sizing_mode == SizingMode::StretchFit {
                child_height = margin_column + (child_width - margin_row) / aspect_ratio;
                child_height_sizing_mode = SizingMode::StretchFit;
            } else if is_main_axis_row && child_height_sizing_mode == SizingMode::StretchFit {
                child_width = margin_row + (child_height - margin_column) * aspect_ratio;
                child_width_sizing_mode = SizingMode::StretchFit;
            }
        }

        // If child has no defined size in the cross axis and is set to
        // stretch, set the cross axis to be measured exactly with the
        // available inner width

        let has_exact_width = is_defined(width) && width_mode == SizingMode::StretchFit;
        let child_width_stretch = resolve_child_alignment(node, child) == Align::Stretch
            && child_width_sizing_mode != SizingMode::StretchFit;
        if !is_main_axis_row && !is_row_style_dim_defined && has_exact_width && child_width_stretch
        {
            child_width = width;
            child_width_sizing_mode = SizingMode::StretchFit;
            if is_defined(aspect_ratio) {
                child_height = (child_width - margin_row) / aspect_ratio;
                child_height_sizing_mode = SizingMode::StretchFit;
            }
        }

        let has_exact_height = is_defined(height) && height_mode == SizingMode::StretchFit;
        let child_height_stretch = resolve_child_alignment(node, child) == Align::Stretch
            && child_height_sizing_mode != SizingMode::StretchFit;
        if is_main_axis_row
            && !is_column_style_dim_defined
            && has_exact_height
            && child_height_stretch
        {
            child_height = height;
            child_height_sizing_mode = SizingMode::StretchFit;

            if is_defined(aspect_ratio) {
                child_width = (child_height - margin_column) * aspect_ratio;
                child_width_sizing_mode = SizingMode::StretchFit;
            }
        }

        constrain_max_size_for_mode(
            child,
            direction,
            FlexDirection::Row,
            owner_width,
            owner_width,
            &mut child_width_sizing_mode,
            &mut child_width,
        );
        constrain_max_size_for_mode(
            child,
            direction,
            FlexDirection::Column,
            owner_height,
            owner_width,
            &mut child_height_sizing_mode,
            &mut child_height,
        );

        // Measure the child
        calculate_layout_internal(
            child,
            child_width,
            child_height,
            direction,
            child_width_sizing_mode,
            child_height_sizing_mode,
            owner_width,
            owner_height,
            false,
            LayoutPassReason::MeasureChild,
            layout_marker_data,
            depth,
            generation_count,
        );

        child.set_layout_computed_flex_basis(max_or_defined(
            child.layout().measured_dimension(dimension(main_axis)),
            padding_and_border_for_axis(child, main_axis, direction, owner_width),
        ));
    }
    child.set_layout_computed_flex_basis_generation(generation_count);
}

/// `measureNodeWithMeasureFunc`.
fn measure_node_with_measure_func<'a, N: LayoutNode<'a>>(
    node: N,
    direction: Direction,
    mut available_width: f32,
    mut available_height: f32,
    width_sizing_mode: SizingMode,
    height_sizing_mode: SizingMode,
    owner_width: f32,
    owner_height: f32,
    layout_marker_data: &mut LayoutData,
    reason: LayoutPassReason,
) {
    assert!(
        node.has_measure_func(),
        "Expected node to have custom measure function"
    );

    if width_sizing_mode == SizingMode::MaxContent {
        available_width = f32::NAN;
    }
    if height_sizing_mode == SizingMode::MaxContent {
        available_height = f32::NAN;
    }

    let layout = node.layout();
    let padding_and_border_axis_row = layout.padding(PhysicalEdge::Left)
        + layout.padding(PhysicalEdge::Right)
        + layout.border(PhysicalEdge::Left)
        + layout.border(PhysicalEdge::Right);
    let padding_and_border_axis_column = layout.padding(PhysicalEdge::Top)
        + layout.padding(PhysicalEdge::Bottom)
        + layout.border(PhysicalEdge::Top)
        + layout.border(PhysicalEdge::Bottom);

    // We want to make sure we don't call measure with negative size
    let inner_width = if is_undefined(available_width) {
        available_width
    } else {
        max_or_defined(0.0, available_width - padding_and_border_axis_row)
    };
    let inner_height = if is_undefined(available_height) {
        available_height
    } else {
        max_or_defined(0.0, available_height - padding_and_border_axis_column)
    };

    if width_sizing_mode == SizingMode::StretchFit && height_sizing_mode == SizingMode::StretchFit {
        // Don't bother sizing the text if both dimensions are already
        // defined.
        node.set_layout_measured_dimension(
            bound_axis(
                node,
                FlexDirection::Row,
                direction,
                available_width,
                owner_width,
                owner_width,
            ),
            Dimension::Width,
        );
        node.set_layout_measured_dimension(
            bound_axis(
                node,
                FlexDirection::Column,
                direction,
                available_height,
                owner_height,
                owner_width,
            ),
            Dimension::Height,
        );
    } else {
        // Measure the text under the current constraints.
        let measured_size = node.measure(
            inner_width,
            measure_mode(width_sizing_mode),
            inner_height,
            measure_mode(height_sizing_mode),
        );

        layout_marker_data.measure_callbacks += 1;
        layout_marker_data.measure_callback_reasons_count[reason as usize] += 1;

        node.set_layout_measured_dimension(
            bound_axis(
                node,
                FlexDirection::Row,
                direction,
                if width_sizing_mode == SizingMode::MaxContent
                    || width_sizing_mode == SizingMode::FitContent
                {
                    measured_size.width + padding_and_border_axis_row
                } else {
                    available_width
                },
                owner_width,
                owner_width,
            ),
            Dimension::Width,
        );

        node.set_layout_measured_dimension(
            bound_axis(
                node,
                FlexDirection::Column,
                direction,
                if height_sizing_mode == SizingMode::MaxContent
                    || height_sizing_mode == SizingMode::FitContent
                {
                    measured_size.height + padding_and_border_axis_column
                } else {
                    available_height
                },
                owner_height,
                owner_width,
            ),
            Dimension::Height,
        );
    }
}

/// `measureNodeWithoutChildren`: for nodes with no children, use the
/// available values if they were provided, or the minimum size as indicated
/// by the padding and border sizes.
fn measure_node_without_children<'a, N: LayoutNode<'a>>(
    node: N,
    direction: Direction,
    available_width: f32,
    available_height: f32,
    width_sizing_mode: SizingMode,
    height_sizing_mode: SizingMode,
    owner_width: f32,
    owner_height: f32,
) {
    let layout = node.layout();

    let mut width = available_width;
    if width_sizing_mode == SizingMode::MaxContent || width_sizing_mode == SizingMode::FitContent {
        width = layout.padding(PhysicalEdge::Left)
            + layout.padding(PhysicalEdge::Right)
            + layout.border(PhysicalEdge::Left)
            + layout.border(PhysicalEdge::Right);
    }
    node.set_layout_measured_dimension(
        bound_axis(
            node,
            FlexDirection::Row,
            direction,
            width,
            owner_width,
            owner_width,
        ),
        Dimension::Width,
    );

    let mut height = available_height;
    if height_sizing_mode == SizingMode::MaxContent || height_sizing_mode == SizingMode::FitContent
    {
        height = layout.padding(PhysicalEdge::Top)
            + layout.padding(PhysicalEdge::Bottom)
            + layout.border(PhysicalEdge::Top)
            + layout.border(PhysicalEdge::Bottom);
    }
    node.set_layout_measured_dimension(
        bound_axis(
            node,
            FlexDirection::Column,
            direction,
            height,
            owner_height,
            owner_width,
        ),
        Dimension::Height,
    );
}

/// `isFixedSize`.
#[inline]
fn is_fixed_size(dim: f32, sizing_mode: SizingMode) -> bool {
    sizing_mode == SizingMode::StretchFit
        || (is_defined(dim) && sizing_mode == SizingMode::FitContent && dim <= 0.0)
}

/// `measureNodeWithFixedSize`.
fn measure_node_with_fixed_size<'a, N: LayoutNode<'a>>(
    node: N,
    direction: Direction,
    available_width: f32,
    available_height: f32,
    width_sizing_mode: SizingMode,
    height_sizing_mode: SizingMode,
    owner_width: f32,
    owner_height: f32,
) -> bool {
    if is_fixed_size(available_width, width_sizing_mode)
        && is_fixed_size(available_height, height_sizing_mode)
    {
        node.set_layout_measured_dimension(
            bound_axis(
                node,
                FlexDirection::Row,
                direction,
                if is_undefined(available_width)
                    || (width_sizing_mode == SizingMode::FitContent && available_width < 0.0)
                {
                    0.0
                } else {
                    available_width
                },
                owner_width,
                owner_width,
            ),
            Dimension::Width,
        );

        node.set_layout_measured_dimension(
            bound_axis(
                node,
                FlexDirection::Column,
                direction,
                if is_undefined(available_height)
                    || (height_sizing_mode == SizingMode::FitContent && available_height < 0.0)
                {
                    0.0
                } else {
                    available_height
                },
                owner_height,
                owner_width,
            ),
            Dimension::Height,
        );
        return true;
    }

    false
}

/// `zeroOutLayoutRecursively`.
pub fn zero_out_layout_recursively<'a, N: LayoutNode<'a>>(node: N) {
    node.layout().reset();
    node.set_layout_dimension(0.0, Dimension::Width);
    node.set_layout_dimension(0.0, Dimension::Height);
    node.set_has_new_layout(true);

    node.clone_children_if_needed();
    for i in 0..node.child_count() {
        zero_out_layout_recursively(node.child(i));
    }
}

/// `cleanupContentsNodesRecursively`.
pub fn cleanup_contents_nodes_recursively<'a, N: LayoutNode<'a>>(
    node: N,
    did_perform_layout: bool,
) {
    if node.has_contents_children() {
        node.clone_contents_children_if_needed();
        for i in 0..node.child_count() {
            let child = node.child(i);
            if child.style().display() == Display::Contents {
                child.layout().reset();
                child.set_layout_dimension(0.0, Dimension::Width);
                child.set_layout_dimension(0.0, Dimension::Height);
                if did_perform_layout {
                    child.set_has_new_layout(true);
                }
                child.set_dirty(false);
                child.clone_children_if_needed();

                cleanup_contents_nodes_recursively(child, did_perform_layout);
            }
        }
    }
}

/// `calculateAvailableInnerDimension`.
pub fn calculate_available_inner_dimension<'a, N: LayoutNode<'a>>(
    node: N,
    direction: Direction,
    dimension: Dimension,
    available_dim: f32,
    padding_and_border: f32,
    owner_dim: f32,
    owner_width: f32,
) -> f32 {
    let mut available_inner_dim = available_dim - padding_and_border;
    // Max dimension overrides predefined dimension value; Min dimension in
    // turn overrides both of the above
    if is_defined(available_inner_dim) {
        // We want to make sure our available height does not violate min and
        // max constraints
        let min_dimension_optional =
            node.style()
                .resolved_min_dimension(direction, dimension, owner_dim, owner_width);
        let min_inner_dim = if is_undefined(min_dimension_optional) {
            0.0
        } else {
            min_dimension_optional - padding_and_border
        };

        let max_dimension_optional =
            node.style()
                .resolved_max_dimension(direction, dimension, owner_dim, owner_width);

        let max_inner_dim = if is_undefined(max_dimension_optional) {
            f32::MAX
        } else {
            max_dimension_optional - padding_and_border
        };
        available_inner_dim = max_or_defined(
            min_or_defined(available_inner_dim, max_inner_dim),
            min_inner_dim,
        );
    }

    available_inner_dim
}

/// `computeFlexBasisForChildren`.
fn compute_flex_basis_for_children<'a, N: LayoutNode<'a>>(
    node: N,
    available_inner_width: f32,
    available_inner_height: f32,
    owner_width: f32,
    owner_height: f32,
    width_sizing_mode: SizingMode,
    height_sizing_mode: SizingMode,
    direction: Direction,
    main_axis: FlexDirection,
    perform_layout: bool,
    layout_marker_data: &mut LayoutData,
    depth: u32,
    generation_count: u32,
) -> f32 {
    let mut total_outer_flex_basis = 0.0f32;
    let mut single_flex_child: Option<N> = None;
    let sizing_mode_main_dim = if is_row(main_axis) {
        width_sizing_mode
    } else {
        height_sizing_mode
    };
    // If there is only one child with flexGrow + flexShrink it means we can
    // set the computedFlexBasis to 0 instead of measuring and shrinking /
    // flexing the child to exactly match the remaining space
    if sizing_mode_main_dim == SizingMode::StretchFit {
        for child in node.layout_children() {
            if child.is_node_flexible() {
                if single_flex_child.is_some()
                    || inexact_equals(child.resolve_flex_grow(), 0.0)
                    || inexact_equals(child.resolve_flex_shrink(), 0.0)
                {
                    // There is already a flexible child, or this flexible
                    // child doesn't have flexGrow and flexShrink, abort
                    single_flex_child = None;
                    break;
                } else {
                    single_flex_child = Some(child);
                }
            }
        }
    }

    for child in node.layout_children() {
        child.process_dimensions();
        if child.style().display() == Display::None {
            // Only mutate display: none children during layout passes.
            // Zeroing them out during measure-only passes contributes nothing
            // to the measurement, but sets `hasNewLayout` on nodes the
            // parent's layout pass may never visit.
            if perform_layout {
                zero_out_layout_recursively(child);
                child.set_has_new_layout(true);
                child.set_dirty(false);
            }
            continue;
        }
        if perform_layout {
            // Set the initial position (relative to the owner).
            let child_direction = child.resolve_direction(direction);
            child.set_position(
                child_direction,
                available_inner_width,
                available_inner_height,
            );
        }

        if child.style().position_type() == PositionType::Absolute {
            continue;
        }
        if single_flex_child.is_some_and(|single| single.is_same(child)) {
            child.set_layout_computed_flex_basis_generation(generation_count);
            child.set_layout_computed_flex_basis(0.0);
        } else {
            compute_flex_basis_for_child(
                node,
                child,
                available_inner_width,
                width_sizing_mode,
                available_inner_height,
                owner_width,
                owner_height,
                height_sizing_mode,
                direction,
                layout_marker_data,
                depth,
                generation_count,
            );
        }

        total_outer_flex_basis += child.layout().computed_flex_basis.get()
            + child
                .style()
                .compute_margin_for_axis(main_axis, available_inner_width);
    }

    total_outer_flex_basis
}

/// `computeMinContentMainSize`: the min-content size of `node` along
/// `requested_axis`, used by CSS Flexbox §4.5 automatic minimum sizing.
///
/// Projects the pair from `compute_min_content_sizes` onto `requested_axis`;
/// only the requested axis is measured at this level.
fn compute_min_content_main_size<'a, N: LayoutNode<'a>>(
    node: N,
    requested_axis: FlexDirection,
    owner_direction: Direction,
    owner_width: f32,
    owner_height: f32,
) -> f32 {
    let want_row = is_row(requested_axis);
    let (row_axis, column_axis) = if want_row {
        (Some(requested_axis), None)
    } else {
        (None, Some(requested_axis))
    };
    let sizes = compute_min_content_sizes(
        node,
        row_axis,
        column_axis,
        owner_direction,
        owner_width,
        owner_height,
    );
    if want_row { sizes.row } else { sizes.column }
}

/// The min-content size of a node on the row and the column axis.
#[derive(Copy, Clone, Debug)]
struct MinContentSizes {
    row: f32,
    column: f32,
}

/// `computeMinContentMainSize` for up to two axes at once.
///
/// `row_axis` and `column_axis` are the exact `FlexDirection` Yoga would pass
/// for that slot (they select the leaf's padding and border, which depends on
/// the reverse directions too). `None` means the slot is not needed and is not
/// computed; its field in the result is left as the node's static value or
/// NaN.
///
/// This deliberately differs from Yoga's structure, not its results (yoga#2007).
/// Yoga's container branch calls itself twice per child, once per axis, so
/// each child's subtree is walked twice per level (O(2^depth)). Here each
/// subtree is walked once: the container branch reads both the main and the
/// cross axis of a child from the same call. Leaves still measure exactly the
/// axes Yoga would measure.
///
/// * Leaf with measure function: invoke it with `AtMost 0` on the requested
///   axis and `Undefined` on the other.
/// * Empty leaf: 0.
/// * Container: iterate in-flow children; sum their min-content along the
///   container's main axis and take the max along its cross axis (plus
///   margins), add the container's padding and border, and project onto each
///   axis.
fn compute_min_content_sizes<'a, N: LayoutNode<'a>>(
    node: N,
    row_axis: Option<FlexDirection>,
    column_axis: Option<FlexDirection>,
    owner_direction: Direction,
    owner_width: f32,
    owner_height: f32,
) -> MinContentSizes {
    // 1. Static value wins for any node (leaf or container), per axis.
    let mut sizes = MinContentSizes {
        row: node.min_content_width(),
        column: node.min_content_height(),
    };
    // Keep only the slots that are needed and still undefined.
    let row_axis = row_axis.filter(|_| is_undefined(sizes.row));
    let column_axis = column_axis.filter(|_| is_undefined(sizes.column));
    if row_axis.is_none() && column_axis.is_none() {
        return sizes;
    }

    if node.has_measure_func() {
        // 2. Dynamic min-content callback if set. Otherwise fall back to the
        // regular measure function with `AtMost 0`, which text measurers
        // naturally answer with longest-word width.
        if let Some(axis) = row_axis {
            sizes.row = leaf_min_content_on_axis(node, axis, owner_direction, owner_width);
        }
        if let Some(axis) = column_axis {
            sizes.column = leaf_min_content_on_axis(node, axis, owner_direction, owner_width);
        }
        return sizes;
    }

    if node.child_count() == 0 {
        if row_axis.is_some() {
            sizes.row = 0.0;
        }
        if column_axis.is_some() {
            sizes.column = 0.0;
        }
        return sizes;
    }

    let direction = node.resolve_direction(owner_direction);
    let node_main_axis = resolve_direction(node.style().flex_direction(), direction);
    let node_cross_axis = resolve_cross_direction(node_main_axis, direction);
    // The row and column slots of each child, as the exact axes Yoga's main
    // and cross calls request.
    let (child_row_axis, child_column_axis) = if is_row(node_main_axis) {
        (node_main_axis, node_cross_axis)
    } else {
        (node_cross_axis, node_main_axis)
    };

    let mut main_total = 0.0f32;
    let mut cross_max = 0.0f32;

    for i in 0..node.child_count() {
        let child = node.child(i);
        if child.style().display() == Display::None
            || child.style().position_type() == PositionType::Absolute
        {
            continue;
        }

        let child_sizes = compute_min_content_sizes(
            child,
            Some(child_row_axis),
            Some(child_column_axis),
            direction,
            owner_width,
            owner_height,
        );
        let (child_main, child_cross) = if is_row(node_main_axis) {
            (child_sizes.row, child_sizes.column)
        } else {
            (child_sizes.column, child_sizes.row)
        };
        let child_main = child_main
            + child
                .style()
                .compute_margin_for_axis(node_main_axis, owner_width);
        let child_cross = child_cross
            + child
                .style()
                .compute_margin_for_axis(node_cross_axis, owner_width);

        main_total += child_main;
        // std::max(crossMax, childCross)
        cross_max = if cross_max < child_cross {
            child_cross
        } else {
            cross_max
        };
    }

    main_total +=
        node.style()
            .compute_flex_start_padding_and_border(node_main_axis, direction, owner_width)
            + node.style().compute_flex_end_padding_and_border(
                node_main_axis,
                direction,
                owner_width,
            );
    cross_max +=
        node.style()
            .compute_flex_start_padding_and_border(node_cross_axis, direction, owner_width)
            + node.style().compute_flex_end_padding_and_border(
                node_cross_axis,
                direction,
                owner_width,
            );

    let node_main_is_row = is_row(node_main_axis);
    let width_min = if node_main_is_row {
        main_total
    } else {
        cross_max
    };
    let height_min = if node_main_is_row {
        cross_max
    } else {
        main_total
    };
    if row_axis.is_some() {
        sizes.row = width_min;
    }
    if column_axis.is_some() {
        sizes.column = height_min;
    }
    sizes
}

/// The measure-func part of `computeMinContentMainSize`'s leaf branch, for one
/// axis: the leaf's measured min-content size on `requested_axis` (any row or
/// column direction, as Yoga passed it), plus its own padding and border.
fn leaf_min_content_on_axis<'a, N: LayoutNode<'a>>(
    node: N,
    requested_axis: FlexDirection,
    owner_direction: Direction,
    owner_width: f32,
) -> f32 {
    let want_row = is_row(requested_axis);
    let (width, width_mode, height, height_mode) = if want_row {
        (0.0, MeasureMode::AtMost, f32::NAN, MeasureMode::Undefined)
    } else {
        (f32::NAN, MeasureMode::Undefined, 0.0, MeasureMode::AtMost)
    };
    let size = if node.has_min_content_measure_func() {
        node.measure_min_content(width, width_mode, height, height_mode)
    } else {
        node.measure(width, width_mode, height, height_mode)
    };
    // Add the leaf's own padding and border, like the container branch.
    let leaf_direction = node.resolve_direction(owner_direction);
    let padding_and_border = node.style().compute_flex_start_padding_and_border(
        requested_axis,
        leaf_direction,
        owner_width,
    ) + node.style().compute_flex_end_padding_and_border(
        requested_axis,
        leaf_direction,
        owner_width,
    );
    (if want_row { size.width } else { size.height }) + padding_and_border
}

/// `computeAutoMinMainSize`: the CSS Flexbox §4.5 automatic minimum
/// main-axis size for `child`. Undefined when no auto-min applies (errata
/// set, explicit `min-{w,h}`, or `display: none`); 0 when the item's own
/// `overflow != visible`; otherwise a concrete floor.
///
/// Floor = min(content-size, specified-size) capped by max-size, with the
/// transferred (aspect-ratio × cross-size) suggestion replacing the
/// specified-size leg when the item has an aspect ratio but no specified main
/// size. See <https://www.w3.org/TR/css-flexbox-1/#min-size-auto>.
fn compute_auto_min_main_size<'a, N: LayoutNode<'a>>(
    child: N,
    main_axis: FlexDirection,
    direction: Direction,
    owner_main_axis_size: f32,
    owner_width: f32,
    owner_height: f32,
) -> f32 {
    if child.has_errata(Errata::MIN_SIZE_UNDEFINED_INSTEAD_OF_AUTO) {
        return f32::NAN;
    }
    if child.style().display() == Display::None {
        return f32::NAN;
    }
    // Explicit `min-{w,h}` (including `0`) wins over auto.
    if child
        .style()
        .min_dimension(dimension(main_axis))
        .is_defined()
    {
        return f32::NAN;
    }
    // A flex item whose own `overflow` is not `visible` gets auto-min = 0.
    if child.style().overflow() != Overflow::Visible {
        return 0.0;
    }

    let main_dim = dimension(main_axis);
    let cross_dim = if is_row(main_axis) {
        Dimension::Height
    } else {
        Dimension::Width
    };
    let is_main_axis_row = is_row(main_axis);

    // Specified size suggestion: the resolved main-axis style dimension.
    let specified_main =
        child.resolved_dimension(direction, main_dim, owner_main_axis_size, owner_width);

    // Transferred size suggestion: cross × aspect-ratio, if both are
    // definite.
    let mut transferred_main = f32::NAN;
    let aspect_ratio = child.style().aspect_ratio();
    if is_defined(aspect_ratio) {
        let cross_owner = if is_main_axis_row {
            owner_height
        } else {
            owner_width
        };
        let cross_resolved =
            child.resolved_dimension(direction, cross_dim, cross_owner, owner_width);
        if is_defined(cross_resolved) {
            let ratio = aspect_ratio;
            let cross_value = cross_resolved;
            transferred_main = if is_main_axis_row {
                cross_value * ratio
            } else {
                cross_value / ratio
            };
        }
    }

    // Content size suggestion: probe via min-content recursion.
    let content_main =
        compute_min_content_main_size(child, main_axis, direction, owner_width, owner_height);

    // Combine per §4.5.
    let mut floor = content_main;
    if is_defined(specified_main) {
        if is_undefined(floor) || specified_main < floor {
            floor = specified_main;
        }
    } else if is_defined(transferred_main) && (is_undefined(floor) || transferred_main < floor) {
        floor = transferred_main;
    }

    // §4.5: cap by the max main size.
    let max_main = child.style().resolved_max_dimension(
        direction,
        main_dim,
        owner_main_axis_size,
        owner_width,
    );
    if is_defined(max_main) && floor > max_main {
        floor = max_main;
    }

    if is_undefined(floor) || floor < 0.0 {
        floor = 0.0;
    }
    floor
}

/// `boundAxisWithAutoMin`: [`bound_axis`] with an additional lower bound from
/// `child`'s cached `computedAutoMinMainSize`, applied on the main axis only.
fn bound_axis_with_auto_min<'a, N: LayoutNode<'a>>(
    child: N,
    axis: FlexDirection,
    direction: Direction,
    value: f32,
    axis_size: f32,
    width_size: f32,
) -> f32 {
    let mut bounded = bound_axis(child, axis, direction, value, axis_size, width_size);
    let auto_min = child.layout().computed_auto_min_main_size.get();
    if is_defined(auto_min) && bounded < auto_min {
        bounded = auto_min;
    }
    bounded
}

/// `distributeFreeSpaceSecondPass`: distributes the free space to the
/// flexible items and ensures that the size of the flex items abide the min
/// and max constraints. At the end of this function the child nodes have
/// their proper size. `distribute_free_space_first_pass` must run first.
///
/// Deliberately differs from Yoga (yoga#2035): each item's share is added to
/// its CSS 9.7 start size (`start_sizes`, the unclamped flex basis unless the
/// item was frozen up front) instead of its min/max-clamped flex basis, and
/// items frozen up front (`frozen`) keep their clamped size. An item no
/// branch applies to also keeps its clamped size, as in Yoga.
fn distribute_free_space_second_pass<'a, N: LayoutNode<'a>>(
    flex_line: &mut FlexLine<N>,
    start_sizes: &[f32],
    frozen: &[bool],
    node: N,
    main_axis: FlexDirection,
    cross_axis: FlexDirection,
    direction: Direction,
    owner_width: f32,
    main_axis_owner_size: f32,
    available_inner_main_dim: f32,
    available_inner_cross_dim: f32,
    available_inner_width: f32,
    available_inner_height: f32,
    main_axis_overflows: bool,
    sizing_mode_cross_dim: SizingMode,
    perform_layout: bool,
    layout_marker_data: &mut LayoutData,
    depth: u32,
    generation_count: u32,
) -> f32 {
    let mut delta_free_space = 0.0f32;
    let is_main_axis_row = is_row(main_axis);
    let is_node_flex_wrap = node.style().flex_wrap() != Wrap::NoWrap;

    for (i, &current_line_child) in flex_line.items_in_flow.iter().enumerate() {
        // Deliberate divergence from Yoga (yoga#2035): distribution starts
        // from the item's CSS 9.7 start size, not its clamped flex basis.
        let child_flex_basis = start_sizes[i];
        let mut updated_main_size = bound_axis_within_min_and_max(
            current_line_child,
            direction,
            main_axis,
            current_line_child.layout().computed_flex_basis.get(),
            main_axis_owner_size,
            owner_width,
        );

        let remaining_free_space = flex_line.layout.remaining_free_space;
        if frozen[i] {
            // Frozen up front at its hypothetical (clamped) size.
        } else if is_defined(remaining_free_space) && remaining_free_space < 0.0 {
            let flex_shrink_scaled_factor =
                -current_line_child.resolve_flex_shrink() * child_flex_basis;
            // Is this child able to shrink?
            if flex_shrink_scaled_factor != 0.0 {
                // Use a relative epsilon guard instead of exact equality:
                // after the first pass removes all constrained items,
                // totalFlexShrinkScaledFactors may be near-zero rather than
                // exactly 0 due to floating-point cancellation. Dividing by a
                // near-zero value produces a gigantic childSize that
                // overwhelms the min/max clamp.
                let total = flex_line.layout.total_flex_shrink_scaled_factors;
                let shrink_factor_magnitude = total.abs();
                let child_size = if is_defined(total) && shrink_factor_magnitude < 1e-6f32 {
                    child_flex_basis + flex_shrink_scaled_factor
                } else {
                    child_flex_basis + (remaining_free_space / total) * flex_shrink_scaled_factor
                };

                updated_main_size = bound_axis_with_auto_min(
                    current_line_child,
                    main_axis,
                    direction,
                    child_size,
                    available_inner_main_dim,
                    available_inner_width,
                );
            }
        } else if is_defined(remaining_free_space) && remaining_free_space > 0.0 {
            let flex_grow_factor = current_line_child.resolve_flex_grow();

            // Is this child able to grow?
            if !flex_grow_factor.is_nan() && flex_grow_factor != 0.0 {
                updated_main_size = bound_axis_with_auto_min(
                    current_line_child,
                    main_axis,
                    direction,
                    child_flex_basis
                        + remaining_free_space / flex_line.layout.total_flex_grow_factors
                            * flex_grow_factor,
                    available_inner_main_dim,
                    available_inner_width,
                );
            }
        }

        delta_free_space += updated_main_size - child_flex_basis;

        let child_style = current_line_child.style();
        let margin_main = child_style.compute_margin_for_axis(main_axis, available_inner_width);
        let margin_cross = child_style.compute_margin_for_axis(cross_axis, available_inner_width);

        let mut child_cross_size;
        let mut child_main_size = updated_main_size + margin_main;
        let mut child_cross_sizing_mode;
        let mut child_main_sizing_mode = SizingMode::StretchFit;

        let aspect_ratio = child_style.aspect_ratio();
        if is_defined(aspect_ratio) {
            child_cross_size = if is_main_axis_row {
                (child_main_size - margin_main) / aspect_ratio
            } else {
                (child_main_size - margin_main) * aspect_ratio
            };
            child_cross_sizing_mode = SizingMode::StretchFit;

            child_cross_size += margin_cross;
        } else if !available_inner_cross_dim.is_nan()
            && !current_line_child
                .has_definite_length(dimension(cross_axis), available_inner_cross_dim)
            && sizing_mode_cross_dim == SizingMode::StretchFit
            && !(is_node_flex_wrap && main_axis_overflows)
            && resolve_child_alignment(node, current_line_child) == Align::Stretch
            && !child_style.flex_start_margin_is_auto(cross_axis, direction)
            && !child_style.flex_end_margin_is_auto(cross_axis, direction)
        {
            child_cross_size = available_inner_cross_dim;
            child_cross_sizing_mode = SizingMode::StretchFit;
        } else if !current_line_child
            .has_definite_length(dimension(cross_axis), available_inner_cross_dim)
        {
            child_cross_size = available_inner_cross_dim;
            child_cross_sizing_mode = if is_undefined(child_cross_size) {
                SizingMode::MaxContent
            } else {
                SizingMode::FitContent
            };
        } else {
            child_cross_size = current_line_child.resolved_dimension(
                direction,
                dimension(cross_axis),
                available_inner_cross_dim,
                available_inner_width,
            ) + margin_cross;
            let is_loose_percentage_measurement = current_line_child
                .processed_dimension(dimension(cross_axis))
                .is_percent()
                && sizing_mode_cross_dim != SizingMode::StretchFit;
            child_cross_sizing_mode =
                if is_undefined(child_cross_size) || is_loose_percentage_measurement {
                    SizingMode::MaxContent
                } else {
                    SizingMode::StretchFit
                };
        }

        constrain_max_size_for_mode(
            current_line_child,
            direction,
            main_axis,
            available_inner_main_dim,
            available_inner_width,
            &mut child_main_sizing_mode,
            &mut child_main_size,
        );
        constrain_max_size_for_mode(
            current_line_child,
            direction,
            cross_axis,
            available_inner_cross_dim,
            available_inner_width,
            &mut child_cross_sizing_mode,
            &mut child_cross_size,
        );

        let requires_stretch_layout = !current_line_child
            .has_definite_length(dimension(cross_axis), available_inner_cross_dim)
            && resolve_child_alignment(node, current_line_child) == Align::Stretch
            && !child_style.flex_start_margin_is_auto(cross_axis, direction)
            && !child_style.flex_end_margin_is_auto(cross_axis, direction);

        let child_width = if is_main_axis_row {
            child_main_size
        } else {
            child_cross_size
        };
        let child_height = if !is_main_axis_row {
            child_main_size
        } else {
            child_cross_size
        };

        let child_width_sizing_mode = if is_main_axis_row {
            child_main_sizing_mode
        } else {
            child_cross_sizing_mode
        };
        let child_height_sizing_mode = if !is_main_axis_row {
            child_main_sizing_mode
        } else {
            child_cross_sizing_mode
        };

        let is_layout_pass = perform_layout && !requires_stretch_layout;
        // Recursively call the layout algorithm for this child with the
        // updated main size.
        calculate_layout_internal(
            current_line_child,
            child_width,
            child_height,
            node.layout().direction(),
            child_width_sizing_mode,
            child_height_sizing_mode,
            available_inner_width,
            available_inner_height,
            is_layout_pass,
            if is_layout_pass {
                LayoutPassReason::FlexLayout
            } else {
                LayoutPassReason::FlexMeasure
            },
            layout_marker_data,
            depth,
            generation_count,
        );
        node.set_layout_had_overflow(
            node.layout().had_overflow() || current_line_child.layout().had_overflow(),
        );
    }
    delta_free_space
}

/// `distributeFreeSpaceFirstPass`: distributes the free space to the
/// flexible items. For those flexible items whose min and max constraints
/// are triggered, that flex item's clamped size is removed from the
/// remainingFreeSpace.
///
/// Deliberately differs from Yoga (yoga#2035): each item's tentative size is
/// its CSS 9.7 start size (`start_sizes`, the unclamped flex basis unless the
/// item was frozen up front) plus its share, rather than its min/max-clamped
/// flex basis plus its share, and items frozen up front (`frozen`) are
/// skipped, as their factors are already out of the totals.
fn distribute_free_space_first_pass<'a, N: LayoutNode<'a>>(
    node: N,
    flex_line: &mut FlexLine<N>,
    start_sizes: &[f32],
    frozen: &[bool],
    direction: Direction,
    main_axis: FlexDirection,
    available_inner_main_dim: f32,
    available_inner_width: f32,
) {
    let mut delta_free_space = 0.0f32;

    // The first pass performs a single distribution of the free space over
    // all of the line's flexible items, so every item's tentative size must
    // be computed against the *original* totals. The totals are still
    // reduced as items get frozen below (so the second pass can
    // redistribute), but those reduced values must not feed back into the
    // fair-share calculation for the remaining items (see
    // https://github.com/react/yoga/issues/2006).
    //
    // Dividing by the running totals is the pre-fix behavior, preserved for
    // existing layouts behind an errata bit that is set on new configs by
    // default.
    let use_running_totals = node.has_errata(Errata::FLEX_FIRST_PASS_USES_RUNNING_TOTALS);
    let original_total_flex_grow_factors = flex_line.layout.total_flex_grow_factors;
    let original_total_flex_shrink_scaled_factors =
        flex_line.layout.total_flex_shrink_scaled_factors;

    for (i, &current_line_child) in flex_line.items_in_flow.iter().enumerate() {
        // Deliberate divergence from Yoga (yoga#2035): start from the CSS 9.7
        // start size; items frozen up front take no part in this pass.
        if frozen[i] {
            continue;
        }
        let child_flex_basis = start_sizes[i];

        if flex_line.layout.remaining_free_space < 0.0 {
            let flex_shrink_scaled_factor =
                -current_line_child.resolve_flex_shrink() * child_flex_basis;

            // Is this child able to shrink?
            if is_defined(flex_shrink_scaled_factor) && flex_shrink_scaled_factor != 0.0 {
                let base_main_size = child_flex_basis
                    + flex_line.layout.remaining_free_space
                        / (if use_running_totals {
                            flex_line.layout.total_flex_shrink_scaled_factors
                        } else {
                            original_total_flex_shrink_scaled_factors
                        })
                        * flex_shrink_scaled_factor;
                let bound_main_size = bound_axis_with_auto_min(
                    current_line_child,
                    main_axis,
                    direction,
                    base_main_size,
                    available_inner_main_dim,
                    available_inner_width,
                );
                if is_defined(base_main_size)
                    && is_defined(bound_main_size)
                    && base_main_size != bound_main_size
                {
                    // By excluding this item's size and flex factor from
                    // remaining, this item's min/max constraints should also
                    // trigger in the second pass resulting in the item's size
                    // calculation being identical in the first and second
                    // passes.
                    delta_free_space += bound_main_size - child_flex_basis;
                    flex_line.layout.total_flex_shrink_scaled_factors -= -current_line_child
                        .resolve_flex_shrink()
                        * current_line_child.layout().computed_flex_basis.get();
                }
            }
        } else if is_defined(flex_line.layout.remaining_free_space)
            && flex_line.layout.remaining_free_space > 0.0
        {
            let flex_grow_factor = current_line_child.resolve_flex_grow();

            // Is this child able to grow?
            if is_defined(flex_grow_factor) && flex_grow_factor != 0.0 {
                let base_main_size = child_flex_basis
                    + flex_line.layout.remaining_free_space
                        / (if use_running_totals {
                            flex_line.layout.total_flex_grow_factors
                        } else {
                            original_total_flex_grow_factors
                        })
                        * flex_grow_factor;
                let bound_main_size = bound_axis(
                    current_line_child,
                    main_axis,
                    direction,
                    base_main_size,
                    available_inner_main_dim,
                    available_inner_width,
                );

                if is_defined(base_main_size)
                    && is_defined(bound_main_size)
                    && base_main_size != bound_main_size
                {
                    // By excluding this item's size and flex factor from
                    // remaining, this item's min/max constraints should also
                    // trigger in the second pass resulting in the item's size
                    // calculation being identical in the first and second
                    // passes.
                    delta_free_space += bound_main_size - child_flex_basis;
                    flex_line.layout.total_flex_grow_factors -= flex_grow_factor;
                }
            }
        }
    }
    flex_line.layout.remaining_free_space -= delta_free_space;
}

/// `resolveFlexibleLength`: two passes over the flex items to figure out how
/// to distribute the remaining space.
///
/// The first pass finds the items whose min/max constraints trigger, freezes
/// them at those sizes, and excludes those sizes from the remaining space.
///
/// The second pass sets the size of each flexible item. It distributes the
/// remaining space amongst the items whose min/max constraints didn't trigger
/// in the first pass. For the other items, it sets their sizes by forcing
/// their min/max constraints to trigger again.
///
/// This two pass approach for resolving min/max constraints deviates from
/// the spec (<https://www.w3.org/TR/CSS-flexbox-1/#resolve-flexible-lengths>),
/// which describes a process that needs to be repeated a variable number of
/// times.
///
/// Deliberately differs from Yoga (yoga#2035): before the two passes, items
/// are frozen up front and the initial free space is recomputed from the
/// unclamped flex base sizes, per CSS 9.7 steps 1-4 (see the comment in the
/// body), since Yoga computes it from the min/max-clamped bases.
fn resolve_flexible_length<'a, N: LayoutNode<'a>>(
    node: N,
    flex_line: &mut FlexLine<N>,
    main_axis: FlexDirection,
    cross_axis: FlexDirection,
    direction: Direction,
    owner_width: f32,
    main_axis_owner_size: f32,
    available_inner_main_dim: f32,
    available_inner_cross_dim: f32,
    available_inner_width: f32,
    available_inner_height: f32,
    main_axis_overflows: bool,
    sizing_mode_cross_dim: SizingMode,
    perform_layout: bool,
    layout_marker_data: &mut LayoutData,
    depth: u32,
    generation_count: u32,
) {
    let original_free_space = flex_line.layout.remaining_free_space;

    // CSS Flexbox §4.5: compute each item's automatic minimum main-axis size
    // up front so the bounding helpers below can floor shrunk values.
    if !node.has_errata(Errata::MIN_SIZE_UNDEFINED_INSTEAD_OF_AUTO) {
        for &current_line_child in &flex_line.items_in_flow {
            current_line_child.layout().computed_auto_min_main_size.set(
                compute_auto_min_main_size(
                    current_line_child,
                    main_axis,
                    direction,
                    main_axis_owner_size,
                    available_inner_width,
                    available_inner_height,
                ),
            );
        }
    } else {
        for &current_line_child in &flex_line.items_in_flow {
            current_line_child
                .layout()
                .computed_auto_min_main_size
                .set(f32::NAN);
        }
    }

    // Deliberate divergence from Yoga (yoga#2035), CSS Flexbox 9.7 steps 1-4.
    // The flex factor in use comes from the hypothetical (clamped) sizes,
    // i.e. the free space calculateFlexLine's sizeConsumed gives: grow if it
    // is positive, shrink otherwise. Items with a zero factor, or whose
    // clamping goes against that direction (a max lowering a growing item, a
    // min raising a shrinking one) are frozen at their clamped size and their
    // factors removed from the totals. Every other item starts from its
    // unclamped flex base size, and the initial free space is recomputed from
    // those start sizes. When the free space is undefined nothing is
    // distributed, and items start from their clamped size as in Yoga.
    let mut start_sizes = Vec::with_capacity(flex_line.items_in_flow.len());
    let mut frozen = Vec::with_capacity(flex_line.items_in_flow.len());
    let mut free_space_correction = 0.0f32;
    for &current_line_child in &flex_line.items_in_flow {
        let flex_basis = current_line_child.layout().computed_flex_basis.get();
        let clamped = bound_axis_within_min_and_max(
            current_line_child,
            direction,
            main_axis,
            flex_basis,
            main_axis_owner_size,
            owner_width,
        );
        if !is_defined(original_free_space) {
            start_sizes.push(clamped);
            frozen.push(false);
            continue;
        }
        let growing = original_free_space > 0.0;
        let freeze = if growing {
            current_line_child.resolve_flex_grow() == 0.0 || flex_basis > clamped
        } else {
            current_line_child.resolve_flex_shrink() == 0.0 || flex_basis < clamped
        };
        if freeze {
            if current_line_child.is_node_flexible() {
                if growing {
                    flex_line.layout.total_flex_grow_factors -=
                        current_line_child.resolve_flex_grow();
                } else {
                    flex_line.layout.total_flex_shrink_scaled_factors -=
                        -current_line_child.resolve_flex_shrink() * flex_basis;
                }
            }
            start_sizes.push(clamped);
        } else {
            free_space_correction += clamped - flex_basis;
            start_sizes.push(flex_basis);
        }
        frozen.push(freeze);
    }
    flex_line.layout.remaining_free_space += free_space_correction;
    let original_free_space = flex_line.layout.remaining_free_space;

    // First pass: detect the flex items whose min/max constraints trigger
    distribute_free_space_first_pass(
        node,
        flex_line,
        &start_sizes,
        &frozen,
        direction,
        main_axis,
        available_inner_main_dim,
        available_inner_width,
    );

    // Second pass: resolve the sizes of the flexible items
    let distributed_free_space = distribute_free_space_second_pass(
        flex_line,
        &start_sizes,
        &frozen,
        node,
        main_axis,
        cross_axis,
        direction,
        owner_width,
        main_axis_owner_size,
        available_inner_main_dim,
        available_inner_cross_dim,
        available_inner_width,
        available_inner_height,
        main_axis_overflows,
        sizing_mode_cross_dim,
        perform_layout,
        layout_marker_data,
        depth,
        generation_count,
    );

    flex_line.layout.remaining_free_space = original_free_space - distributed_free_space;
}

/// `justifyMainAxis`.
///
/// Deliberate divergence from Yoga (yoga#1932): the baseline of a
/// baseline-aligned child is read through `calculate_baseline_of_child`, which
/// lays the child out first, so the extra arguments are passed through to it.
fn justify_main_axis<'a, N: LayoutNode<'a>>(
    node: N,
    flex_line: &mut FlexLine<N>,
    main_axis: FlexDirection,
    cross_axis: FlexDirection,
    direction: Direction,
    sizing_mode_main_dim: SizingMode,
    sizing_mode_cross_dim: SizingMode,
    main_axis_owner_size: f32,
    owner_width: f32,
    available_inner_main_dim: f32,
    available_inner_cross_dim: f32,
    available_inner_width: f32,
    available_inner_height: f32,
    perform_layout: bool,
    reason: LayoutPassReason,
    layout_marker_data: &mut LayoutData,
    depth: u32,
    generation_count: u32,
) {
    let style = node.style();

    let leading_padding_and_border_main =
        style.compute_flex_start_padding_and_border(main_axis, direction, owner_width);
    let trailing_padding_and_border_main =
        style.compute_flex_end_padding_and_border(main_axis, direction, owner_width);

    let gap = style.compute_gap_for_axis(main_axis, available_inner_main_dim);
    // If we are using "at most" rules in the main axis, make sure that
    // remainingFreeSpace is 0 when min main dimension is not given
    if sizing_mode_main_dim == SizingMode::FitContent && flex_line.layout.remaining_free_space > 0.0
    {
        if style.min_dimension(dimension(main_axis)).is_defined()
            && is_defined(style.resolved_min_dimension(
                direction,
                dimension(main_axis),
                main_axis_owner_size,
                owner_width,
            ))
        {
            // This condition makes sure that if the size of main
            // dimension(after considering child nodes main dim, leading and
            // trailing padding etc) falls below min dimension, then the
            // remainingFreeSpace is reassigned considering the min dimension

            // `minAvailableMainDim` denotes minimum available space in which
            // child can be laid out, it will exclude space consumed by
            // padding and border.
            let min_available_main_dim = style.resolved_min_dimension(
                direction,
                dimension(main_axis),
                main_axis_owner_size,
                owner_width,
            ) - leading_padding_and_border_main
                - trailing_padding_and_border_main;
            let occupied_space_by_child_nodes =
                available_inner_main_dim - flex_line.layout.remaining_free_space;
            flex_line.layout.remaining_free_space =
                max_or_defined(0.0, min_available_main_dim - occupied_space_by_child_nodes);
        } else {
            flex_line.layout.remaining_free_space = 0.0;
        }
    }

    // In order to position the elements in the main axis, we have two
    // controls. The space between the beginning and the first element and
    // the space between each two elements.
    let mut leading_main_dim = 0.0f32;
    let mut between_main_dim = gap;
    let justify_content = if flex_line.layout.remaining_free_space >= 0.0 {
        style.justify_content()
    } else {
        fallback_justification(style.justify_content())
    };

    let item_count = flex_line.items_in_flow.len();
    if flex_line.number_of_auto_margins == 0 {
        match justify_content {
            Justify::Start | Justify::End | Justify::Auto => {
                // No-Op
            }
            Justify::Stretch => {
                // No-Op
            }
            Justify::Center => {
                leading_main_dim = flex_line.layout.remaining_free_space / 2.0;
            }
            Justify::FlexEnd => {
                leading_main_dim = flex_line.layout.remaining_free_space;
            }
            Justify::SpaceBetween => {
                if item_count > 1 {
                    between_main_dim +=
                        flex_line.layout.remaining_free_space / (item_count - 1) as f32;
                }
            }
            Justify::SpaceEvenly => {
                // Space is distributed evenly across all elements
                leading_main_dim = flex_line.layout.remaining_free_space / (item_count + 1) as f32;
                between_main_dim += leading_main_dim;
            }
            Justify::SpaceAround => {
                // Space on the edges is half of the space between elements
                leading_main_dim =
                    0.5f32 * flex_line.layout.remaining_free_space / item_count as f32;
                between_main_dim += leading_main_dim * 2.0;
            }
            Justify::FlexStart => {}
        }
    }

    flex_line.layout.main_dim = leading_padding_and_border_main + leading_main_dim;
    flex_line.layout.cross_dim = 0.0;

    let mut max_ascent_for_current_line = 0.0f32;
    let mut max_descent_for_current_line = 0.0f32;
    let is_node_baseline_layout = is_baseline_layout(node);
    let last_item = flex_line.items_in_flow.last().copied();
    for &child in &flex_line.items_in_flow {
        let child_layout = child.layout();
        let child_style = child.style();
        if child_style.flex_start_margin_is_auto(main_axis, direction)
            && flex_line.layout.remaining_free_space > 0.0
        {
            flex_line.layout.main_dim +=
                flex_line.layout.remaining_free_space / flex_line.number_of_auto_margins as f32;
        }

        if perform_layout {
            child.set_layout_position(
                child_layout.position(flex_start_edge(main_axis)) + flex_line.layout.main_dim,
                flex_start_edge(main_axis),
            );
        }

        if !last_item.is_some_and(|last| last.is_same(child)) {
            flex_line.layout.main_dim += between_main_dim;
        }

        if child_style.flex_end_margin_is_auto(main_axis, direction)
            && flex_line.layout.remaining_free_space > 0.0
        {
            flex_line.layout.main_dim +=
                flex_line.layout.remaining_free_space / flex_line.number_of_auto_margins as f32;
        }
        let can_skip_flex = !perform_layout && sizing_mode_cross_dim == SizingMode::StretchFit;
        if can_skip_flex {
            // If we skipped the flex step, then we can't rely on the
            // measuredDims because they weren't computed. This means we can't
            // call dimensionWithMargin.
            flex_line.layout.main_dim += child_style
                .compute_margin_for_axis(main_axis, available_inner_width)
                + bound_axis_within_min_and_max(
                    child,
                    direction,
                    main_axis,
                    child_layout.computed_flex_basis.get(),
                    main_axis_owner_size,
                    owner_width,
                );
            flex_line.layout.cross_dim = available_inner_cross_dim;
        } else {
            // The main dimension is the sum of all the elements dimension
            // plus the spacing.
            flex_line.layout.main_dim +=
                child.dimension_with_margin(main_axis, available_inner_width);

            if is_node_baseline_layout {
                // If the child is baseline aligned then the cross dimension
                // is calculated by adding maxAscent and maxDescent from the
                // baseline.
                let ascent = calculate_baseline_of_child(
                    child,
                    direction,
                    available_inner_width,
                    available_inner_height,
                    reason,
                    layout_marker_data,
                    depth,
                    generation_count,
                ) + child_style.compute_flex_start_margin(
                    FlexDirection::Column,
                    direction,
                    available_inner_width,
                );
                let descent = child.layout().measured_dimension(Dimension::Height)
                    + child_style
                        .compute_margin_for_axis(FlexDirection::Column, available_inner_width)
                    - ascent;

                max_ascent_for_current_line = max_or_defined(max_ascent_for_current_line, ascent);
                max_descent_for_current_line =
                    max_or_defined(max_descent_for_current_line, descent);
            } else {
                // The cross dimension is the max of the elements dimension
                // since there can only be one element in that cross dimension
                // in the case when the items are not baseline aligned
                flex_line.layout.cross_dim = max_or_defined(
                    flex_line.layout.cross_dim,
                    child.dimension_with_margin(cross_axis, available_inner_width),
                );
            }
        }
    }
    flex_line.layout.main_dim += trailing_padding_and_border_main;

    if is_node_baseline_layout {
        flex_line.layout.cross_dim = max_ascent_for_current_line + max_descent_for_current_line;
    }
}

/// `calculateBaseline` of a baseline-aligned `child`, with the child's
/// subtree laid out first.
///
/// Deliberate divergence from Yoga (yoga#1932): when a node's own width and
/// height are already known, a measure-only pass takes the
/// `measure_node_with_fixed_size` shortcut in `calculate_layout_internal` and
/// never recurses into the node's children. Their measured dimensions are then
/// never written, and `calculate_baseline` can read them as NaN. So a baseline
/// child with children is laid out again at its measured size (StretchFit,
/// `perform_layout`), which writes real measured dimensions down its subtree
/// before its baseline is read. The child's measured size is unchanged, so the
/// owner's flex results do not change. The shortcut itself is not changed.
fn calculate_baseline_of_child<'a, N: LayoutNode<'a>>(
    child: N,
    direction: Direction,
    available_inner_width: f32,
    available_inner_height: f32,
    reason: LayoutPassReason,
    layout_marker_data: &mut LayoutData,
    depth: u32,
    generation_count: u32,
) -> f32 {
    if child.layout_child_count() > 0 {
        let child_style = child.style();
        let child_width = child.layout().measured_dimension(Dimension::Width)
            + child_style.compute_margin_for_axis(FlexDirection::Row, available_inner_width);
        let child_height = child.layout().measured_dimension(Dimension::Height)
            + child_style.compute_margin_for_axis(FlexDirection::Column, available_inner_width);
        calculate_layout_internal(
            child,
            child_width,
            child_height,
            direction,
            SizingMode::StretchFit,
            SizingMode::StretchFit,
            available_inner_width,
            available_inner_height,
            true,
            reason,
            layout_marker_data,
            depth,
            generation_count,
        );
    }
    calculate_baseline(child)
}

/// `calculateLayoutImpl`: the main routine, implementing a subset of the
/// flexbox layout algorithm described in the W3C CSS documentation
/// (<https://www.w3.org/TR/CSS3-flexbox/>). See the long comment above
/// `calculateLayoutImpl` in Yoga for its limitations and deviations.
///
/// It uses the information in `node.style`, which is treated as a read-only
/// input. It is responsible for setting the layout direction and measured
/// dimensions of the node, and the positions and line indices of its
/// children. The measured dimensions include any border or padding of the
/// node but not its margins.
///
/// When calling `calculate_layout_impl` and `calculate_layout_internal`, if
/// the caller passes an available size of undefined then it must also pass a
/// sizing mode of `MaxContent` in that dimension.
fn calculate_layout_impl<'a, N: LayoutNode<'a>>(
    node: N,
    available_width: f32,
    available_height: f32,
    owner_direction: Direction,
    width_sizing_mode: SizingMode,
    height_sizing_mode: SizingMode,
    owner_width: f32,
    owner_height: f32,
    perform_layout: bool,
    reason: LayoutPassReason,
    layout_marker_data: &mut LayoutData,
    depth: u32,
    generation_count: u32,
) {
    assert!(
        !is_undefined(available_width) || width_sizing_mode == SizingMode::MaxContent,
        "availableWidth is indefinite so widthSizingMode must be SizingMode::MaxContent"
    );
    assert!(
        !is_undefined(available_height) || height_sizing_mode == SizingMode::MaxContent,
        "availableHeight is indefinite so heightSizingMode must be SizingMode::MaxContent"
    );

    if perform_layout {
        layout_marker_data.layouts += 1;
    } else {
        layout_marker_data.measures += 1;
    }

    // Set the resolved resolution in the node's layout.
    let direction = node.resolve_direction(owner_direction);
    node.set_layout_direction(direction);
    let fix_flex_basis_fit_content = node
        .config()
        .is_experimental_feature_enabled(ExperimentalFeature::FixFlexBasisFitContent);
    if fix_flex_basis_fit_content && perform_layout {
        node.set_layout_had_overflow(false);
    }

    let flex_row_direction = resolve_direction(FlexDirection::Row, direction);
    let flex_column_direction = resolve_direction(FlexDirection::Column, direction);

    let start_edge = if direction == Direction::LTR {
        PhysicalEdge::Left
    } else {
        PhysicalEdge::Right
    };
    let end_edge = if direction == Direction::LTR {
        PhysicalEdge::Right
    } else {
        PhysicalEdge::Left
    };

    let style = node.style();

    let margin_row_leading =
        style.compute_inline_start_margin(flex_row_direction, direction, owner_width);
    node.set_layout_margin(margin_row_leading, start_edge);
    let margin_row_trailing =
        style.compute_inline_end_margin(flex_row_direction, direction, owner_width);
    node.set_layout_margin(margin_row_trailing, end_edge);
    let margin_column_leading =
        style.compute_inline_start_margin(flex_column_direction, direction, owner_width);
    node.set_layout_margin(margin_column_leading, PhysicalEdge::Top);
    let margin_column_trailing =
        style.compute_inline_end_margin(flex_column_direction, direction, owner_width);
    node.set_layout_margin(margin_column_trailing, PhysicalEdge::Bottom);

    let margin_axis_row = margin_row_leading + margin_row_trailing;
    let margin_axis_column = margin_column_leading + margin_column_trailing;

    node.set_layout_border(
        style.compute_inline_start_border(flex_row_direction, direction),
        start_edge,
    );
    node.set_layout_border(
        style.compute_inline_end_border(flex_row_direction, direction),
        end_edge,
    );
    node.set_layout_border(
        style.compute_inline_start_border(flex_column_direction, direction),
        PhysicalEdge::Top,
    );
    node.set_layout_border(
        style.compute_inline_end_border(flex_column_direction, direction),
        PhysicalEdge::Bottom,
    );

    node.set_layout_padding(
        style.compute_inline_start_padding(flex_row_direction, direction, owner_width),
        start_edge,
    );
    node.set_layout_padding(
        style.compute_inline_end_padding(flex_row_direction, direction, owner_width),
        end_edge,
    );
    node.set_layout_padding(
        style.compute_inline_start_padding(flex_column_direction, direction, owner_width),
        PhysicalEdge::Top,
    );
    node.set_layout_padding(
        style.compute_inline_end_padding(flex_column_direction, direction, owner_width),
        PhysicalEdge::Bottom,
    );

    if node.has_measure_func() {
        measure_node_with_measure_func(
            node,
            direction,
            available_width - margin_axis_row,
            available_height - margin_axis_column,
            width_sizing_mode,
            height_sizing_mode,
            owner_width,
            owner_height,
            layout_marker_data,
            reason,
        );

        // Clean and update all display: contents nodes with a direct path to
        // the current node as they will not be traversed
        cleanup_contents_nodes_recursively(node, perform_layout);
        return;
    }

    let child_count = node.layout_child_count();
    if child_count == 0 {
        measure_node_without_children(
            node,
            direction,
            available_width - margin_axis_row,
            available_height - margin_axis_column,
            width_sizing_mode,
            height_sizing_mode,
            owner_width,
            owner_height,
        );

        // Clean and update all display: contents nodes with a direct path to
        // the current node as they will not be traversed
        cleanup_contents_nodes_recursively(node, perform_layout);
        return;
    }

    // If we're not being asked to perform a full layout we can skip the
    // algorithm if we already know the size
    if !perform_layout
        && measure_node_with_fixed_size(
            node,
            direction,
            available_width - margin_axis_row,
            available_height - margin_axis_column,
            width_sizing_mode,
            height_sizing_mode,
            owner_width,
            owner_height,
        )
    {
        // Clean and update all display: contents nodes with a direct path to
        // the current node as they will not be traversed
        cleanup_contents_nodes_recursively(node, false);
        return;
    }

    // At this point we know we're going to perform work. Ensure that each
    // child has a mutable copy.
    node.clone_children_if_needed();
    if !fix_flex_basis_fit_content || !perform_layout {
        node.set_layout_had_overflow(false);
    }
    // Clean and update all display: contents nodes with a direct path to the
    // current node as they will not be traversed
    cleanup_contents_nodes_recursively(node, perform_layout);

    // STEP 1: CALCULATE VALUES FOR REMAINDER OF ALGORITHM
    let main_axis = resolve_direction(style.flex_direction(), direction);
    let cross_axis = resolve_cross_direction(main_axis, direction);
    let is_main_axis_row = is_row(main_axis);
    let is_node_flex_wrap = style.flex_wrap() != Wrap::NoWrap;

    let main_axis_owner_size = if is_main_axis_row {
        owner_width
    } else {
        owner_height
    };
    let cross_axis_owner_size = if is_main_axis_row {
        owner_height
    } else {
        owner_width
    };

    let padding_and_border_axis_main =
        padding_and_border_for_axis(node, main_axis, direction, owner_width);
    let padding_and_border_axis_cross =
        padding_and_border_for_axis(node, cross_axis, direction, owner_width);
    let leading_padding_and_border_cross =
        style.compute_flex_start_padding_and_border(cross_axis, direction, owner_width);

    let mut sizing_mode_main_dim = if is_main_axis_row {
        width_sizing_mode
    } else {
        height_sizing_mode
    };
    let sizing_mode_cross_dim = if is_main_axis_row {
        height_sizing_mode
    } else {
        width_sizing_mode
    };

    let padding_and_border_axis_row = if is_main_axis_row {
        padding_and_border_axis_main
    } else {
        padding_and_border_axis_cross
    };
    let padding_and_border_axis_column = if is_main_axis_row {
        padding_and_border_axis_cross
    } else {
        padding_and_border_axis_main
    };

    // STEP 2: DETERMINE AVAILABLE SIZE IN MAIN AND CROSS DIRECTIONS

    let available_inner_width = calculate_available_inner_dimension(
        node,
        direction,
        Dimension::Width,
        available_width - margin_axis_row,
        padding_and_border_axis_row,
        owner_width,
        owner_width,
    );
    let available_inner_height = calculate_available_inner_dimension(
        node,
        direction,
        Dimension::Height,
        available_height - margin_axis_column,
        padding_and_border_axis_column,
        owner_height,
        owner_width,
    );

    let mut available_inner_main_dim = if is_main_axis_row {
        available_inner_width
    } else {
        available_inner_height
    };
    let available_inner_cross_dim = if is_main_axis_row {
        available_inner_height
    } else {
        available_inner_width
    };

    // STEP 3: DETERMINE FLEX BASIS FOR EACH ITEM

    // Computed basis + margins + gap
    let mut total_main_dim = 0.0f32;
    total_main_dim += compute_flex_basis_for_children(
        node,
        available_inner_width,
        available_inner_height,
        available_inner_width,
        available_inner_height,
        width_sizing_mode,
        height_sizing_mode,
        direction,
        main_axis,
        perform_layout,
        layout_marker_data,
        depth,
        generation_count,
    );

    if child_count > 1 {
        total_main_dim += style.compute_gap_for_axis(main_axis, available_inner_main_dim)
            * (child_count - 1) as f32;
    }

    let main_axis_overflows =
        sizing_mode_main_dim != SizingMode::MaxContent && total_main_dim > available_inner_main_dim;

    if is_node_flex_wrap && main_axis_overflows && sizing_mode_main_dim == SizingMode::FitContent {
        sizing_mode_main_dim = SizingMode::StretchFit;
    }
    // STEP 4: COLLECT FLEX ITEMS INTO FLEX LINES

    // Iterator representing the beginning of the current line
    let mut start_of_line_iterator = node.layout_children();

    // Number of lines.
    let mut line_count: usize = 0;

    // Accumulated cross dimensions of all lines so far.
    let mut total_line_cross_dim = 0.0f32;

    let cross_axis_gap = style.compute_gap_for_axis(cross_axis, available_inner_cross_dim);

    // Max main dimension of all the lines.
    let mut max_line_main_dim = 0.0f32;
    while !start_of_line_iterator.is_end() {
        let mut flex_line = calculate_flex_line(
            node,
            owner_direction,
            owner_width,
            main_axis_owner_size,
            available_inner_width,
            available_inner_main_dim,
            &mut start_of_line_iterator,
            line_count,
        );

        // If we don't need to measure the cross axis, we can skip the entire
        // flex step.
        let can_skip_flex = !perform_layout && sizing_mode_cross_dim == SizingMode::StretchFit;

        // STEP 5: RESOLVING FLEXIBLE LENGTHS ON MAIN AXIS
        // Calculate the remaining available space that needs to be
        // allocated. If the main dimension size isn't known, it is computed
        // based on the line length, so there's no more space left to
        // distribute.

        let mut size_based_on_content = false;
        // If we don't measure with exact main dimension we want to ensure we
        // don't violate min and max
        if sizing_mode_main_dim != SizingMode::StretchFit {
            let min_inner_width =
                style.resolved_min_dimension(direction, Dimension::Width, owner_width, owner_width)
                    - padding_and_border_axis_row;
            let max_inner_width =
                style.resolved_max_dimension(direction, Dimension::Width, owner_width, owner_width)
                    - padding_and_border_axis_row;
            let min_inner_height = style.resolved_min_dimension(
                direction,
                Dimension::Height,
                owner_height,
                owner_width,
            ) - padding_and_border_axis_column;
            let max_inner_height = style.resolved_max_dimension(
                direction,
                Dimension::Height,
                owner_height,
                owner_width,
            ) - padding_and_border_axis_column;

            let min_inner_main_dim = if is_main_axis_row {
                min_inner_width
            } else {
                min_inner_height
            };
            let max_inner_main_dim = if is_main_axis_row {
                max_inner_width
            } else {
                max_inner_height
            };

            if is_defined(min_inner_main_dim) && flex_line.size_consumed < min_inner_main_dim {
                available_inner_main_dim = min_inner_main_dim;
            } else if is_defined(max_inner_main_dim) && flex_line.size_consumed > max_inner_main_dim
            {
                available_inner_main_dim = max_inner_main_dim;
            } else {
                let use_legacy_stretch_behaviour = node.has_errata(Errata::STRETCH_FLEX_BASIS);

                if !use_legacy_stretch_behaviour
                    && ((is_defined(flex_line.layout.total_flex_grow_factors)
                        && flex_line.layout.total_flex_grow_factors == 0.0)
                        || (is_defined(node.resolve_flex_grow())
                            && node.resolve_flex_grow() == 0.0))
                {
                    // If we don't have any children to flex or we can't flex
                    // the node itself, space we've used is all space we need.
                    // Root node also should be shrunk to minimum
                    available_inner_main_dim = flex_line.size_consumed;
                }

                size_based_on_content = !use_legacy_stretch_behaviour;
            }
        }

        if !size_based_on_content && is_defined(available_inner_main_dim) {
            flex_line.layout.remaining_free_space =
                available_inner_main_dim - flex_line.size_consumed;
        } else if flex_line.size_consumed < 0.0 {
            // availableInnerMainDim is indefinite which means the node is
            // being sized based on its content. sizeConsumed is negative
            // which means the node will allocate 0 points for its content.
            // Consequently, remainingFreeSpace is 0 - sizeConsumed.
            flex_line.layout.remaining_free_space = -flex_line.size_consumed;
        }

        if !can_skip_flex {
            resolve_flexible_length(
                node,
                &mut flex_line,
                main_axis,
                cross_axis,
                direction,
                owner_width,
                main_axis_owner_size,
                available_inner_main_dim,
                available_inner_cross_dim,
                available_inner_width,
                available_inner_height,
                main_axis_overflows,
                sizing_mode_cross_dim,
                perform_layout,
                layout_marker_data,
                depth,
                generation_count,
            );
        }

        node.set_layout_had_overflow(
            node.layout().had_overflow() || (flex_line.layout.remaining_free_space < 0.0),
        );

        // STEP 6: MAIN-AXIS JUSTIFICATION & CROSS-AXIS SIZE DETERMINATION

        // At this point, all the children have their dimensions set in the
        // main axis. Their dimensions are also set in the cross axis with the
        // exception of items that are aligned "stretch". We need to compute
        // these stretch values and set the final positions.

        justify_main_axis(
            node,
            &mut flex_line,
            main_axis,
            cross_axis,
            direction,
            sizing_mode_main_dim,
            sizing_mode_cross_dim,
            main_axis_owner_size,
            owner_width,
            available_inner_main_dim,
            available_inner_cross_dim,
            available_inner_width,
            available_inner_height,
            perform_layout,
            reason,
            layout_marker_data,
            depth,
            generation_count,
        );

        let mut container_cross_axis = available_inner_cross_dim;
        if sizing_mode_cross_dim == SizingMode::MaxContent
            || sizing_mode_cross_dim == SizingMode::FitContent
        {
            // Compute the cross axis from the max cross dimension of the
            // children.
            container_cross_axis = bound_axis(
                node,
                cross_axis,
                direction,
                flex_line.layout.cross_dim + padding_and_border_axis_cross,
                cross_axis_owner_size,
                owner_width,
            ) - padding_and_border_axis_cross;
        }

        // If there's no flex wrap, the cross dimension is defined by the
        // container.
        if !is_node_flex_wrap && sizing_mode_cross_dim == SizingMode::StretchFit {
            flex_line.layout.cross_dim = available_inner_cross_dim;
        }

        // As-per https://www.w3.org/TR/css-flexbox-1/#cross-sizing, the
        // cross-size of the line within a single-line container should be
        // bound to min/max constraints before alignment within the line. In a
        // multi-line container, affecting alignment between the lines.
        if !is_node_flex_wrap {
            flex_line.layout.cross_dim = bound_axis(
                node,
                cross_axis,
                direction,
                flex_line.layout.cross_dim + padding_and_border_axis_cross,
                cross_axis_owner_size,
                owner_width,
            ) - padding_and_border_axis_cross;
        }

        // STEP 7: CROSS-AXIS ALIGNMENT
        // We can skip child alignment if we're just measuring the container.
        if perform_layout {
            for &child in &flex_line.items_in_flow {
                let mut leading_cross_dim = leading_padding_and_border_cross;
                let child_style = child.style();

                // For a relative children, we're either using alignItems
                // (owner) or alignSelf (child) in order to determine the
                // position in the cross axis
                let align_item = resolve_child_alignment(node, child);

                // If the child uses align stretch, we need to lay it out one
                // more time, this time forcing the cross-axis size to be the
                // computed cross size for the current line.
                if align_item == Align::Stretch
                    && !child_style.flex_start_margin_is_auto(cross_axis, direction)
                    && !child_style.flex_end_margin_is_auto(cross_axis, direction)
                {
                    // If the child defines a definite size for its cross
                    // axis, there's no need to stretch.
                    if !child.has_definite_length(dimension(cross_axis), available_inner_cross_dim)
                    {
                        let mut child_main_size =
                            child.layout().measured_dimension(dimension(main_axis));
                        let aspect_ratio = child_style.aspect_ratio();
                        let mut child_cross_size = if is_defined(aspect_ratio) {
                            child_style.compute_margin_for_axis(cross_axis, available_inner_width)
                                + if is_main_axis_row {
                                    child_main_size / aspect_ratio
                                } else {
                                    child_main_size * aspect_ratio
                                }
                        } else {
                            flex_line.layout.cross_dim
                        };

                        child_main_size +=
                            child_style.compute_margin_for_axis(main_axis, available_inner_width);

                        let mut child_main_sizing_mode = SizingMode::StretchFit;
                        let mut child_cross_sizing_mode = SizingMode::StretchFit;
                        constrain_max_size_for_mode(
                            child,
                            direction,
                            main_axis,
                            available_inner_main_dim,
                            available_inner_width,
                            &mut child_main_sizing_mode,
                            &mut child_main_size,
                        );
                        constrain_max_size_for_mode(
                            child,
                            direction,
                            cross_axis,
                            available_inner_cross_dim,
                            available_inner_width,
                            &mut child_cross_sizing_mode,
                            &mut child_cross_size,
                        );

                        let child_width = if is_main_axis_row {
                            child_main_size
                        } else {
                            child_cross_size
                        };
                        let child_height = if !is_main_axis_row {
                            child_main_size
                        } else {
                            child_cross_size
                        };

                        let align_content = style.align_content();
                        let cross_axis_does_not_grow =
                            align_content != Align::Stretch && is_node_flex_wrap;
                        let child_width_sizing_mode = if is_undefined(child_width)
                            || (!is_main_axis_row && cross_axis_does_not_grow)
                        {
                            SizingMode::MaxContent
                        } else {
                            SizingMode::StretchFit
                        };
                        let child_height_sizing_mode = if is_undefined(child_height)
                            || (is_main_axis_row && cross_axis_does_not_grow)
                        {
                            SizingMode::MaxContent
                        } else {
                            SizingMode::StretchFit
                        };

                        calculate_layout_internal(
                            child,
                            child_width,
                            child_height,
                            direction,
                            child_width_sizing_mode,
                            child_height_sizing_mode,
                            available_inner_width,
                            available_inner_height,
                            true,
                            LayoutPassReason::Stretch,
                            layout_marker_data,
                            depth,
                            generation_count,
                        );
                    }
                } else {
                    let remaining_cross_dim = container_cross_axis
                        - child.dimension_with_margin(cross_axis, available_inner_width);

                    if child_style.flex_start_margin_is_auto(cross_axis, direction)
                        && child_style.flex_end_margin_is_auto(cross_axis, direction)
                    {
                        leading_cross_dim += max_or_defined(0.0, remaining_cross_dim / 2.0);
                    } else if child_style.flex_end_margin_is_auto(cross_axis, direction) {
                        // No-Op
                    } else if child_style.flex_start_margin_is_auto(cross_axis, direction) {
                        leading_cross_dim += max_or_defined(0.0, remaining_cross_dim);
                    } else if align_item == Align::FlexStart {
                        // No-Op
                    } else if align_item == Align::Center {
                        leading_cross_dim += remaining_cross_dim / 2.0;
                    } else {
                        leading_cross_dim += remaining_cross_dim;
                    }
                }
                // And we apply the position
                child.set_layout_position(
                    child.layout().position(flex_start_edge(cross_axis))
                        + total_line_cross_dim
                        + leading_cross_dim,
                    flex_start_edge(cross_axis),
                );
            }
        }

        let applied_cross_gap = if line_count != 0 { cross_axis_gap } else { 0.0 };
        total_line_cross_dim += flex_line.layout.cross_dim + applied_cross_gap;
        max_line_main_dim = max_or_defined(max_line_main_dim, flex_line.layout.main_dim);

        line_count += 1;
    }

    // STEP 8: MULTI-LINE CONTENT ALIGNMENT
    // currentLead stores the size of the cross dim
    if perform_layout && (is_node_flex_wrap || is_baseline_layout(node)) {
        let mut lead_per_line = 0.0f32;
        let mut current_lead = leading_padding_and_border_cross;
        let mut extra_space_per_line = 0.0f32;

        let unclamped_cross_dim = if sizing_mode_cross_dim == SizingMode::StretchFit {
            available_inner_cross_dim + padding_and_border_axis_cross
        } else if node.has_definite_length(dimension(cross_axis), cross_axis_owner_size) {
            node.resolved_dimension(
                direction,
                dimension(cross_axis),
                cross_axis_owner_size,
                owner_width,
            )
        } else {
            total_line_cross_dim + padding_and_border_axis_cross
        };

        let inner_cross_dim = bound_axis(
            node,
            cross_axis,
            direction,
            unclamped_cross_dim,
            cross_axis_owner_size,
            owner_width,
        ) - padding_and_border_axis_cross;

        let remaining_align_content_dim = inner_cross_dim - total_line_cross_dim;

        let align_content = if remaining_align_content_dim >= 0.0 {
            style.align_content()
        } else {
            fallback_alignment(style.align_content())
        };

        match align_content {
            Align::Start | Align::End => {
                // No-Op
            }
            Align::FlexEnd => {
                current_lead += remaining_align_content_dim;
            }
            Align::Center => {
                current_lead += remaining_align_content_dim / 2.0;
            }
            Align::Stretch => {
                extra_space_per_line = remaining_align_content_dim / line_count as f32;
            }
            Align::SpaceAround => {
                current_lead += remaining_align_content_dim / (2.0 * line_count as f32);
                lead_per_line = remaining_align_content_dim / line_count as f32;
            }
            Align::SpaceEvenly => {
                current_lead += remaining_align_content_dim / (line_count + 1) as f32;
                lead_per_line = remaining_align_content_dim / (line_count + 1) as f32;
            }
            Align::SpaceBetween => {
                if line_count > 1 {
                    lead_per_line = remaining_align_content_dim / (line_count - 1) as f32;
                }
            }
            Align::Auto | Align::FlexStart | Align::Baseline => {}
        }
        let mut end_iterator = node.layout_children();
        for i in 0..line_count {
            let start_iterator = end_iterator.clone();
            let mut iterator = start_iterator.clone();

            // compute the line's height and find the endIndex
            let mut line_height = 0.0f32;
            let mut max_ascent_for_current_line = 0.0f32;
            let mut max_descent_for_current_line = 0.0f32;
            while let Some(child) = iterator.current() {
                let child_style = child.style();
                if child_style.display() == Display::None {
                    iterator.advance();
                    continue;
                }
                if child_style.position_type() != PositionType::Absolute {
                    if child.line_index() != i {
                        break;
                    }
                    if child.is_layout_dimension_defined(cross_axis) {
                        line_height = max_or_defined(
                            line_height,
                            child.layout().measured_dimension(dimension(cross_axis))
                                + child_style
                                    .compute_margin_for_axis(cross_axis, available_inner_width),
                        );
                    }
                    if resolve_child_alignment(node, child) == Align::Baseline {
                        let ascent = calculate_baseline_of_child(
                            child,
                            direction,
                            available_inner_width,
                            available_inner_height,
                            reason,
                            layout_marker_data,
                            depth,
                            generation_count,
                        ) + child_style.compute_flex_start_margin(
                            FlexDirection::Column,
                            direction,
                            available_inner_width,
                        );
                        let descent = child.layout().measured_dimension(Dimension::Height)
                            + child_style.compute_margin_for_axis(
                                FlexDirection::Column,
                                available_inner_width,
                            )
                            - ascent;
                        max_ascent_for_current_line =
                            max_or_defined(max_ascent_for_current_line, ascent);
                        max_descent_for_current_line =
                            max_or_defined(max_descent_for_current_line, descent);
                        line_height = max_or_defined(
                            line_height,
                            max_ascent_for_current_line + max_descent_for_current_line,
                        );
                    }
                }
                iterator.advance();
            }
            end_iterator = iterator;
            current_lead += if i != 0 { cross_axis_gap } else { 0.0 };
            line_height += extra_space_per_line;

            let mut iterator = start_iterator;
            while iterator != end_iterator {
                let Some(child) = iterator.current() else {
                    break;
                };
                iterator.advance();
                let child_style = child.style();
                if child_style.display() == Display::None {
                    continue;
                }
                if child_style.position_type() != PositionType::Absolute {
                    match resolve_child_alignment(node, child) {
                        Align::Start | Align::End => {
                            // Not yet implemented
                        }
                        Align::FlexStart => {
                            child.set_layout_position(
                                current_lead
                                    + child_style.compute_flex_start_position(
                                        cross_axis,
                                        direction,
                                        available_inner_width,
                                    ),
                                flex_start_edge(cross_axis),
                            );
                        }
                        Align::FlexEnd => {
                            child.set_layout_position(
                                current_lead + line_height
                                    - child_style.compute_flex_end_margin(
                                        cross_axis,
                                        direction,
                                        available_inner_width,
                                    )
                                    - child.layout().measured_dimension(dimension(cross_axis)),
                                flex_start_edge(cross_axis),
                            );
                        }
                        Align::Center => {
                            let child_height =
                                child.layout().measured_dimension(dimension(cross_axis));

                            child.set_layout_position(
                                current_lead + (line_height - child_height) / 2.0,
                                flex_start_edge(cross_axis),
                            );
                        }
                        Align::Stretch => {
                            child.set_layout_position(
                                current_lead
                                    + child_style.compute_flex_start_margin(
                                        cross_axis,
                                        direction,
                                        available_inner_width,
                                    ),
                                flex_start_edge(cross_axis),
                            );

                            // Remeasure child with the line height as it as
                            // been only measured with the owners height yet.
                            if !child.has_definite_length(
                                dimension(cross_axis),
                                available_inner_cross_dim,
                            ) {
                                let child_width = if is_main_axis_row {
                                    child.layout().measured_dimension(Dimension::Width)
                                        + child_style.compute_margin_for_axis(
                                            main_axis,
                                            available_inner_width,
                                        )
                                } else {
                                    lead_per_line + line_height
                                };

                                let child_height = if !is_main_axis_row {
                                    child.layout().measured_dimension(Dimension::Height)
                                        + child_style.compute_margin_for_axis(
                                            cross_axis,
                                            available_inner_width,
                                        )
                                } else {
                                    lead_per_line + line_height
                                };

                                if !(inexact_equals(
                                    child_width,
                                    child.layout().measured_dimension(Dimension::Width),
                                ) && inexact_equals(
                                    child_height,
                                    child.layout().measured_dimension(Dimension::Height),
                                )) {
                                    calculate_layout_internal(
                                        child,
                                        child_width,
                                        child_height,
                                        direction,
                                        SizingMode::StretchFit,
                                        SizingMode::StretchFit,
                                        available_inner_width,
                                        available_inner_height,
                                        true,
                                        LayoutPassReason::MultilineStretch,
                                        layout_marker_data,
                                        depth,
                                        generation_count,
                                    );
                                }
                            }
                        }
                        Align::Baseline => {
                            child.set_layout_position(
                                current_lead + max_ascent_for_current_line
                                    - calculate_baseline_of_child(
                                        child,
                                        direction,
                                        available_inner_width,
                                        available_inner_height,
                                        reason,
                                        layout_marker_data,
                                        depth,
                                        generation_count,
                                    )
                                    + child_style.compute_flex_start_position(
                                        FlexDirection::Column,
                                        direction,
                                        available_inner_cross_dim,
                                    ),
                                PhysicalEdge::Top,
                            );
                        }
                        Align::Auto
                        | Align::SpaceBetween
                        | Align::SpaceAround
                        | Align::SpaceEvenly => {}
                    }
                }
            }

            current_lead = current_lead + lead_per_line + line_height;
        }
    }

    // STEP 9: COMPUTING FINAL DIMENSIONS

    node.set_layout_measured_dimension(
        bound_axis(
            node,
            FlexDirection::Row,
            direction,
            available_width - margin_axis_row,
            owner_width,
            owner_width,
        ),
        Dimension::Width,
    );

    node.set_layout_measured_dimension(
        bound_axis(
            node,
            FlexDirection::Column,
            direction,
            available_height - margin_axis_column,
            owner_height,
            owner_width,
        ),
        Dimension::Height,
    );

    // If the user didn't specify a width or height for the node, set the
    // dimensions based on the children.
    if sizing_mode_main_dim == SizingMode::MaxContent
        || (style.overflow() != Overflow::Scroll && sizing_mode_main_dim == SizingMode::FitContent)
    {
        // Clamp the size to the min/max size, if specified, and make sure it
        // doesn't go below the padding and border amount.
        node.set_layout_measured_dimension(
            bound_axis(
                node,
                main_axis,
                direction,
                max_line_main_dim,
                main_axis_owner_size,
                owner_width,
            ),
            dimension(main_axis),
        );
    } else if sizing_mode_main_dim == SizingMode::FitContent && style.overflow() == Overflow::Scroll
    {
        node.set_layout_measured_dimension(
            max_or_defined(
                min_or_defined(
                    available_inner_main_dim + padding_and_border_axis_main,
                    bound_axis_within_min_and_max(
                        node,
                        direction,
                        main_axis,
                        max_line_main_dim,
                        main_axis_owner_size,
                        owner_width,
                    ),
                ),
                padding_and_border_axis_main,
            ),
            dimension(main_axis),
        );
    }

    if sizing_mode_cross_dim == SizingMode::MaxContent
        || (style.overflow() != Overflow::Scroll && sizing_mode_cross_dim == SizingMode::FitContent)
    {
        // Clamp the size to the min/max size, if specified, and make sure it
        // doesn't go below the padding and border amount.
        node.set_layout_measured_dimension(
            bound_axis(
                node,
                cross_axis,
                direction,
                total_line_cross_dim + padding_and_border_axis_cross,
                cross_axis_owner_size,
                owner_width,
            ),
            dimension(cross_axis),
        );
    } else if sizing_mode_cross_dim == SizingMode::FitContent
        && style.overflow() == Overflow::Scroll
    {
        node.set_layout_measured_dimension(
            max_or_defined(
                min_or_defined(
                    available_inner_cross_dim + padding_and_border_axis_cross,
                    bound_axis_within_min_and_max(
                        node,
                        direction,
                        cross_axis,
                        total_line_cross_dim + padding_and_border_axis_cross,
                        cross_axis_owner_size,
                        owner_width,
                    ),
                ),
                padding_and_border_axis_cross,
            ),
            dimension(cross_axis),
        );
    }

    // As we only wrapped in normal direction yet, we need to reverse the
    // positions on wrap-reverse.
    if perform_layout && style.flex_wrap() == Wrap::WrapReverse {
        for child in node.layout_children() {
            if child.style().position_type() != PositionType::Absolute {
                child.set_layout_position(
                    node.layout().measured_dimension(dimension(cross_axis))
                        - child.layout().position(flex_start_edge(cross_axis))
                        - child.layout().measured_dimension(dimension(cross_axis)),
                    flex_start_edge(cross_axis),
                );
            }
        }
    }

    if perform_layout {
        // STEP 10: SETTING TRAILING POSITIONS FOR CHILDREN
        let needs_main_trailing_pos = needs_trailing_position(main_axis);
        let needs_cross_trailing_pos = needs_trailing_position(cross_axis);

        if needs_main_trailing_pos || needs_cross_trailing_pos {
            for child in node.layout_children() {
                // Absolute children will be handled by their containing block
                // since we cannot guarantee that their positions are set when
                // their parents are done with layout.
                if child.style().display() == Display::None
                    || child.style().position_type() == PositionType::Absolute
                {
                    continue;
                }
                if needs_main_trailing_pos {
                    set_child_trailing_position(node, child, main_axis);
                }

                if needs_cross_trailing_pos {
                    set_child_trailing_position(node, child, cross_axis);
                }
            }
        }

        // STEP 11: SIZING AND POSITIONING ABSOLUTE CHILDREN
        // Let the containing block layout its absolute descendants.
        if style.position_type() != PositionType::Static
            || node.always_forms_containing_block()
            || depth == 1
        {
            layout_absolute_descendants(
                node,
                node,
                if is_main_axis_row {
                    sizing_mode_main_dim
                } else {
                    sizing_mode_cross_dim
                },
                direction,
                layout_marker_data,
                depth,
                generation_count,
                0.0,
                0.0,
                available_inner_width,
                available_inner_height,
            );
        }
    }
}

/// barre-only (yoga#2019), no Yoga counterpart: the cache entry a lookup hit.
fn cached_entry(
    layout: &crate::node::layout_results::LayoutResults,
    cached: CachedResult,
) -> CachedMeasurement {
    match cached {
        CachedResult::Layout => layout.cached_layout.get(),
        CachedResult::Measurement(i) => layout.cached_measurements[i].get(),
    }
}

/// barre-only (yoga#2019), no Yoga counterpart: whether `node` has a layout
/// child whose flex basis can be retained by `compute_flex_basis_for_child`'s
/// resolved branch (in flow, displayed, and a non-`auto` processed basis), so
/// that a cached result may depend on bases written under another anchor.
fn has_retainable_flex_basis_child<'a, N: LayoutNode<'a>>(node: N) -> bool {
    node.layout_children().any(|child| {
        child.style().display() != Display::None
            && child.style().position_type() != PositionType::Absolute
            && !child.process_flex_basis().is_auto()
    })
}

/// `calculateLayoutInternal`: a wrapper around [`calculate_layout_impl`]
/// that determines whether the layout request is redundant and can be
/// skipped. Returns whether layout was performed (false if skipped).
///
/// Deliberately differs from Yoga (yoga#1552, yoga#2019) so a relayout
/// matches a fresh layout: it tracks each container's flex-basis anchor
/// ([`FlexBasisAnchor`]), replays an anchor call answered from an earlier
/// layout's cache before the next real run, stamps stored entries with the
/// generation and anchor key, and refuses an earlier layout's entry computed
/// under a different anchor when a child's basis can be retained. Known
/// residuals are listed on `LayoutResults`.
pub fn calculate_layout_internal<'a, N: LayoutNode<'a>>(
    node: N,
    available_width: f32,
    available_height: f32,
    owner_direction: Direction,
    width_sizing_mode: SizingMode,
    height_sizing_mode: SizingMode,
    owner_width: f32,
    owner_height: f32,
    perform_layout: bool,
    reason: LayoutPassReason,
    layout_marker_data: &mut LayoutData,
    mut depth: u32,
    generation_count: u32,
) -> bool {
    let layout = node.layout();

    depth += 1;

    let need_to_visit_node = (node.is_dirty() && layout.generation_count.get() != generation_count)
        || layout.config_version.get() != node.config().version()
        || layout.last_owner_direction.get() != owner_direction;

    if need_to_visit_node {
        // Invalidate the cached results.
        layout.next_cached_measurements_index.set(0);
        layout.cached_layout.set(CachedMeasurement::default());
    }

    // Determine whether the results are already cached. We maintain a
    // separate cache for layouts and measurements. A layout operation
    // modifies the positions and dimensions for nodes in the subtree. The
    // algorithm assumes that each node gets laid out a maximum of one time
    // per tree layout, but multiple measurements may be required to resolve
    // all of the flex dimensions.
    //
    // TODO(ticket: measure cache): `find_cached_result` always misses for
    // now; see `algorithm/cache.rs`.
    let mut cached_results = find_cached_result(
        node,
        available_width,
        available_height,
        width_sizing_mode,
        height_sizing_mode,
        owner_width,
        perform_layout,
    );

    // barre-only (yoga#1552, yoga#2019): track this node's flex-basis
    // anchor. A call starts a new anchor when it is the node's first in this
    // layout (read `generation_count` before the store at the end) or its
    // main axis is `MaxContent` (every non-`auto`-basis child then goes
    // through a rewriting branch).
    let has_children = !node.has_measure_func() && node.layout_child_count() > 0;
    let main_axis_sizing_mode = if is_row(node.style().flex_direction()) {
        width_sizing_mode
    } else {
        height_sizing_mode
    };
    let stored = layout.flex_basis_anchor.get();
    let starts_anchor = layout.generation_count.get() != generation_count
        || main_axis_sizing_mode == SizingMode::MaxContent
        // Defensive: no anchor yet this layout.
        || stored.generation != generation_count;
    let this_call = FlexBasisAnchor {
        generation: generation_count,
        available_width,
        available_height,
        owner_width,
        owner_height,
        width_sizing_mode,
        height_sizing_mode,
        owner_direction,
        reason,
        replay_pending: false,
    };
    let anchor = if starts_anchor { this_call } else { stored };

    // barre-only (yoga#2019): refuse an earlier layout's entry computed under
    // a different anchor, when the node has a child whose basis can be
    // retained. The rejected entry is overwritten in place by the store below.
    let mut rejected: Option<CachedResult> = None;
    if let (false, Some(cached)) = (need_to_visit_node, cached_results) {
        let entry = cached_entry(layout, cached);
        if entry.generation != generation_count
            && has_children
            && has_retainable_flex_basis_child(node)
            && !(inexact_equals(entry.anchor_available_width, anchor.available_width)
                && inexact_equals(entry.anchor_available_height, anchor.available_height)
                && entry.anchor_width_sizing_mode == anchor.width_sizing_mode
                && entry.anchor_height_sizing_mode == anchor.height_sizing_mode)
        {
            rejected = Some(cached);
            cached_results = None;
        }
    }

    if let (false, Some(cached)) = (need_to_visit_node, cached_results) {
        let entry = cached_entry(layout, cached);
        // barre-only (yoga#2019): in a fresh layout this call would have run
        // and rewritten the children's bases; it was answered from an earlier
        // layout, so record it for replay before the next real run.
        if has_children && starts_anchor && entry.generation != generation_count {
            layout.flex_basis_anchor.set(FlexBasisAnchor {
                replay_pending: true,
                ..this_call
            });
        }
        layout.set_measured_dimension(Dimension::Width, entry.computed_width);
        layout.set_measured_dimension(Dimension::Height, entry.computed_height);

        if perform_layout {
            layout_marker_data.cached_layouts += 1;
        } else {
            layout_marker_data.cached_measures += 1;
        }
    } else {
        if has_children {
            // barre-only (yoga#2019): replay, measure-only, the anchor call
            // the cache skipped, so the children get exactly the bases (and
            // their subtrees the calls) a fresh layout gave them before this
            // call. No cache lookup or store, no generation or `hasNewLayout`
            // update; the real run below overwrites what this sets on `node`.
            let pending = layout.flex_basis_anchor.get();
            if pending.generation == generation_count && pending.replay_pending {
                layout.flex_basis_anchor.set(FlexBasisAnchor {
                    replay_pending: false,
                    ..pending
                });
                calculate_layout_impl(
                    node,
                    pending.available_width,
                    pending.available_height,
                    pending.owner_direction,
                    pending.width_sizing_mode,
                    pending.height_sizing_mode,
                    pending.owner_width,
                    pending.owner_height,
                    false, // perform_layout
                    pending.reason,
                    layout_marker_data,
                    depth,
                    generation_count,
                );
            }
            if starts_anchor {
                layout.flex_basis_anchor.set(this_call);
            }
        }

        calculate_layout_impl(
            node,
            available_width,
            available_height,
            owner_direction,
            width_sizing_mode,
            height_sizing_mode,
            owner_width,
            owner_height,
            perform_layout,
            reason,
            layout_marker_data,
            depth,
            generation_count,
        );

        layout.last_owner_direction.set(owner_direction);
        layout.config_version.set(node.config().version());

        let new_entry = CachedMeasurement {
            available_width,
            available_height,
            computed_width: layout.measured_dimension(Dimension::Width),
            computed_height: layout.measured_dimension(Dimension::Height),
            generation: generation_count,
            anchor_available_width: anchor.available_width,
            anchor_available_height: anchor.available_height,
            width_sizing_mode,
            height_sizing_mode,
            anchor_width_sizing_mode: anchor.width_sizing_mode,
            anchor_height_sizing_mode: anchor.height_sizing_mode,
        };

        if let Some(rejected) = rejected {
            // barre-only (yoga#2019): overwrite the entry D5 rejected in
            // place, keeping cache occupancy what Yoga's would be after a hit.
            match rejected {
                CachedResult::Layout => layout.cached_layout.set(new_entry),
                CachedResult::Measurement(i) => layout.cached_measurements[i].set(new_entry),
            }
        } else if cached_results.is_none() {
            layout_marker_data.max_measure_cache = layout_marker_data
                .max_measure_cache
                .max(layout.next_cached_measurements_index.get() + 1);

            if layout.next_cached_measurements_index.get() as usize == MAX_CACHED_MEASUREMENTS {
                layout.next_cached_measurements_index.set(0);
            }

            let new_cache_entry = if perform_layout {
                // Use the single layout cache entry.
                &layout.cached_layout
            } else {
                // Allocate a new measurement cache entry.
                let index = layout.next_cached_measurements_index.get();
                layout.next_cached_measurements_index.set(index + 1);
                &layout.cached_measurements[index as usize]
            };

            new_cache_entry.set(new_entry);
        }
    }

    if perform_layout {
        node.set_layout_dimension(
            node.layout().measured_dimension(Dimension::Width),
            Dimension::Width,
        );
        node.set_layout_dimension(
            node.layout().measured_dimension(Dimension::Height),
            Dimension::Height,
        );

        node.set_has_new_layout(true);
        node.set_dirty(false);
    }

    layout.generation_count.set(generation_count);

    need_to_visit_node || cached_results.is_none()
}

/// `calculateLayout`: lays out the tree rooted at `node`.
pub fn calculate_layout<'a, N: LayoutNode<'a>>(
    node: N,
    owner_width: f32,
    owner_height: f32,
    owner_direction: Direction,
) {
    let mut marker_data = LayoutData::default();

    // Increment the generation count. This will force the recursive routine
    // to visit all dirty nodes at least once. Subsequent visits will be
    // skipped if the input parameters don't change.
    let current_generation_count = CURRENT_GENERATION_COUNT
        .fetch_add(1, Ordering::Relaxed)
        .wrapping_add(1);
    node.process_dimensions();
    let direction = node.resolve_direction(owner_direction);
    let style = node.style();
    let width;
    let width_sizing_mode;
    if node.has_definite_length(Dimension::Width, owner_width) {
        width = node.resolved_dimension(
            direction,
            dimension(FlexDirection::Row),
            owner_width,
            owner_width,
        ) + style.compute_margin_for_axis(FlexDirection::Row, owner_width);
        width_sizing_mode = SizingMode::StretchFit;
    } else if is_defined(style.resolved_max_dimension(
        direction,
        Dimension::Width,
        owner_width,
        owner_width,
    )) {
        width = style.resolved_max_dimension(direction, Dimension::Width, owner_width, owner_width);
        width_sizing_mode = SizingMode::FitContent;
    } else {
        width = owner_width;
        width_sizing_mode = if is_undefined(width) {
            SizingMode::MaxContent
        } else {
            SizingMode::StretchFit
        };
    }

    let height;
    let height_sizing_mode;
    if node.has_definite_length(Dimension::Height, owner_height) {
        height = node.resolved_dimension(
            direction,
            dimension(FlexDirection::Column),
            owner_height,
            owner_width,
        ) + style.compute_margin_for_axis(FlexDirection::Column, owner_width);
        height_sizing_mode = SizingMode::StretchFit;
    } else if is_defined(style.resolved_max_dimension(
        direction,
        Dimension::Height,
        owner_height,
        owner_width,
    )) {
        height =
            style.resolved_max_dimension(direction, Dimension::Height, owner_height, owner_width);
        height_sizing_mode = SizingMode::FitContent;
    } else {
        height = owner_height;
        height_sizing_mode = if is_undefined(height) {
            SizingMode::MaxContent
        } else {
            SizingMode::StretchFit
        };
    }
    let generation_count = if node
        .config()
        .is_experimental_feature_enabled(ExperimentalFeature::FixFlexBasisFitContent)
    {
        current_generation_count
    } else {
        CURRENT_GENERATION_COUNT.load(Ordering::Relaxed)
    };
    if calculate_layout_internal(
        node,
        width,
        height,
        owner_direction,
        width_sizing_mode,
        height_sizing_mode,
        owner_width,
        owner_height,
        true,
        LayoutPassReason::Initial,
        &mut marker_data,
        0, // tree root
        generation_count,
    ) {
        node.set_position(node.layout().direction(), owner_width, owner_height);
        round_layout_results_to_pixel_grid(node, 0.0, 0.0);
    }
}
