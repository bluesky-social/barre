//! Absolutely positioned children (`yoga/algorithm/AbsoluteLayout.cpp`).
//!
//! [`layout_absolute_child`] sizes and positions one absolute child, and
//! [`layout_absolute_descendants`] walks from a containing block to its
//! absolute descendants, through static children.

use crate::algorithm::align::{resolve_child_alignment, resolve_child_justification};
use crate::algorithm::bound_axis::bound_axis;
use crate::algorithm::calculate_layout::{
    LayoutData, LayoutPassReason, calculate_layout_internal, cleanup_contents_nodes_recursively,
};
use crate::algorithm::flex_direction::{
    dimension, flex_end_edge, flex_start_edge, inline_start_edge, is_row, resolve_cross_direction,
    resolve_direction,
};
use crate::algorithm::sizing_mode::SizingMode;
use crate::algorithm::trailing_position::{
    get_position_of_opposite_edge, needs_trailing_position, set_child_trailing_position,
};
use crate::enums::{
    Align, Dimension, Direction, Display, Errata, FlexDirection, Justify, PhysicalEdge,
    PositionType, Wrap,
};
use crate::node::{LayoutNode, NodeExt};
use crate::numeric::{UNDEFINED, is_defined, is_undefined};

/// `layoutAbsoluteChild`: sizes `child` (an absolutely positioned descendant
/// of `node`) and positions it within `containing_node`, whose containing
/// block is `containing_block_width` x `containing_block_height`.
pub fn layout_absolute_child<'a, N: LayoutNode<'a>>(
    containing_node: N,
    node: N,
    child: N,
    containing_block_width: f32,
    containing_block_height: f32,
    width_mode: SizingMode,
    direction: Direction,
    layout_marker_data: &mut LayoutData,
    depth: u32,
    generation_count: u32,
) {
    // For grid containers, use inline (Row) and block (Column) axes for
    // positioning, since grid alignment properties (justify-self, align-self)
    // operate on inline/block axes, not main/cross axes based on flex-direction.
    let main_axis = if node.style().display() == Display::Grid {
        resolve_direction(FlexDirection::Row, direction)
    } else {
        resolve_direction(node.style().flex_direction(), direction)
    };
    let cross_axis = if node.style().display() == Display::Grid {
        FlexDirection::Column
    } else {
        resolve_cross_direction(main_axis, direction)
    };
    let is_main_axis_row = is_row(main_axis);

    let mut child_width = UNDEFINED;
    let mut child_height = UNDEFINED;

    let margin_row = child
        .style()
        .compute_margin_for_axis(FlexDirection::Row, containing_block_width);
    let margin_column = child
        .style()
        .compute_margin_for_axis(FlexDirection::Column, containing_block_width);

    if child.has_definite_length(Dimension::Width, containing_block_width) {
        child_width = child.resolved_dimension(
            direction,
            Dimension::Width,
            containing_block_width,
            containing_block_width,
        ) + margin_row;
    } else if child
        .style()
        .is_flex_start_position_defined(FlexDirection::Row, direction)
        && child
            .style()
            .is_flex_end_position_defined(FlexDirection::Row, direction)
        && !child
            .style()
            .is_flex_start_position_auto(FlexDirection::Row, direction)
        && !child
            .style()
            .is_flex_end_position_auto(FlexDirection::Row, direction)
    {
        // If the child doesn't have a specified width, compute the width based on
        // the left/right offsets if they're defined.
        child_width = containing_node
            .layout()
            .measured_dimension(Dimension::Width)
            - (containing_node
                .style()
                .compute_flex_start_border(FlexDirection::Row, direction)
                + containing_node
                    .style()
                    .compute_flex_end_border(FlexDirection::Row, direction))
            - (child.style().compute_flex_start_position(
                FlexDirection::Row,
                direction,
                containing_block_width,
            ) + child.style().compute_flex_end_position(
                FlexDirection::Row,
                direction,
                containing_block_width,
            ));
        child_width = bound_axis(
            child,
            FlexDirection::Row,
            direction,
            child_width,
            containing_block_width,
            containing_block_width,
        );
    }

    if child.has_definite_length(Dimension::Height, containing_block_height) {
        child_height = child.resolved_dimension(
            direction,
            Dimension::Height,
            containing_block_height,
            containing_block_width,
        ) + margin_column;
    } else if child
        .style()
        .is_flex_start_position_defined(FlexDirection::Column, direction)
        && child
            .style()
            .is_flex_end_position_defined(FlexDirection::Column, direction)
        && !child
            .style()
            .is_flex_start_position_auto(FlexDirection::Column, direction)
        && !child
            .style()
            .is_flex_end_position_auto(FlexDirection::Column, direction)
    {
        // If the child doesn't have a specified height, compute the height based
        // on the top/bottom offsets if they're defined.
        child_height = containing_node
            .layout()
            .measured_dimension(Dimension::Height)
            - (containing_node
                .style()
                .compute_flex_start_border(FlexDirection::Column, direction)
                + containing_node
                    .style()
                    .compute_flex_end_border(FlexDirection::Column, direction))
            - (child.style().compute_flex_start_position(
                FlexDirection::Column,
                direction,
                containing_block_height,
            ) + child.style().compute_flex_end_position(
                FlexDirection::Column,
                direction,
                containing_block_height,
            ));
        child_height = bound_axis(
            child,
            FlexDirection::Column,
            direction,
            child_height,
            containing_block_height,
            containing_block_width,
        );
    }

    // Exactly one dimension needs to be defined for us to be able to do aspect
    // ratio calculation. One dimension being the anchor and the other being
    // flexible.
    let child_style = child.style();
    if (is_undefined(child_width) ^ is_undefined(child_height))
        && is_defined(child_style.aspect_ratio())
    {
        if is_undefined(child_width) {
            child_width = margin_row + (child_height - margin_column) * child_style.aspect_ratio();
        } else if is_undefined(child_height) {
            child_height = margin_column + (child_width - margin_row) / child_style.aspect_ratio();
        }
    }

    // If we're still missing one or the other dimension, measure the content.
    if is_undefined(child_width) || is_undefined(child_height) {
        let mut child_width_sizing_mode = if is_undefined(child_width) {
            SizingMode::MaxContent
        } else {
            SizingMode::StretchFit
        };
        let child_height_sizing_mode = if is_undefined(child_height) {
            SizingMode::MaxContent
        } else {
            SizingMode::StretchFit
        };

        // If the size of the owner is defined then try to constrain the absolute
        // child to that size as well. This allows text within the absolute child
        // to wrap to the size of its owner. This is the same behavior as many
        // browsers implement.
        if !is_main_axis_row
            && is_undefined(child_width)
            && width_mode != SizingMode::MaxContent
            && is_defined(containing_block_width)
            && containing_block_width > 0.0
        {
            child_width = containing_block_width;
            child_width_sizing_mode = SizingMode::FitContent;
        }

        calculate_layout_internal(
            child,
            child_width,
            child_height,
            direction,
            child_width_sizing_mode,
            child_height_sizing_mode,
            containing_block_width,
            containing_block_height,
            false,
            LayoutPassReason::AbsMeasureChild,
            layout_marker_data,
            depth,
            generation_count,
        );
        child_width = child.layout().measured_dimension(Dimension::Width)
            + child
                .style()
                .compute_margin_for_axis(FlexDirection::Row, containing_block_width);
        child_height = child.layout().measured_dimension(Dimension::Height)
            + child
                .style()
                .compute_margin_for_axis(FlexDirection::Column, containing_block_width);
    }

    calculate_layout_internal(
        child,
        child_width,
        child_height,
        direction,
        SizingMode::StretchFit,
        SizingMode::StretchFit,
        containing_block_width,
        containing_block_height,
        true,
        LayoutPassReason::AbsLayout,
        layout_marker_data,
        depth,
        generation_count,
    );

    position_absolute_child(
        containing_node,
        node,
        child,
        direction,
        main_axis,
        true,
        containing_block_width,
        containing_block_height,
    );
    position_absolute_child(
        containing_node,
        node,
        child,
        direction,
        cross_axis,
        false,
        containing_block_width,
        containing_block_height,
    );
}

/// `setFlexStartLayoutPosition`.
fn set_flex_start_layout_position<'a, N: LayoutNode<'a>>(
    parent: N,
    child: N,
    direction: Direction,
    axis: FlexDirection,
    containing_block_width: f32,
) {
    let mut position =
        child
            .style()
            .compute_flex_start_margin(axis, direction, containing_block_width)
            + parent.layout().border(flex_start_edge(axis));

    // https://www.w3.org/TR/css-grid-1/#abspos
    // absolute positioned grid items are positioned relative to the padding edge
    // of the grid container
    if !child.has_errata(Errata::ABSOLUTE_POSITION_WITHOUT_INSETS_EXCLUDES_PADDING)
        && parent.style().display() != Display::Grid
    {
        position += parent.layout().padding(flex_start_edge(axis));
    }

    child.set_layout_position(position, flex_start_edge(axis));
}

/// `setFlexEndLayoutPosition`.
fn set_flex_end_layout_position<'a, N: LayoutNode<'a>>(
    parent: N,
    child: N,
    direction: Direction,
    axis: FlexDirection,
    containing_block_width: f32,
) {
    let mut flex_end_position = parent.layout().border(flex_end_edge(axis))
        + child
            .style()
            .compute_flex_end_margin(axis, direction, containing_block_width);

    // https://www.w3.org/TR/css-grid-1/#abspos
    // absolute positioned grid items are positioned relative to the padding edge
    // of the grid container
    if !child.has_errata(Errata::ABSOLUTE_POSITION_WITHOUT_INSETS_EXCLUDES_PADDING)
        && parent.style().display() != Display::Grid
    {
        flex_end_position += parent.layout().padding(flex_end_edge(axis));
    }

    child.set_layout_position(
        get_position_of_opposite_edge(flex_end_position, axis, parent, child),
        flex_start_edge(axis),
    );
}

/// `setCenterLayoutPosition`.
fn set_center_layout_position<'a, N: LayoutNode<'a>>(
    parent: N,
    child: N,
    direction: Direction,
    axis: FlexDirection,
    containing_block_width: f32,
) {
    let mut parent_content_box_size = parent.layout().measured_dimension(dimension(axis))
        - parent.layout().border(flex_start_edge(axis))
        - parent.layout().border(flex_end_edge(axis));

    // https://www.w3.org/TR/css-grid-1/#abspos
    // absolute positioned grid items are positioned relative to the padding edge
    // of the grid container
    if !child.has_errata(Errata::ABSOLUTE_POSITION_WITHOUT_INSETS_EXCLUDES_PADDING)
        && parent.style().display() != Display::Grid
    {
        parent_content_box_size -= parent.layout().padding(flex_start_edge(axis));
        parent_content_box_size -= parent.layout().padding(flex_end_edge(axis));
    }

    let child_outer_size = child.layout().measured_dimension(dimension(axis))
        + child
            .style()
            .compute_margin_for_axis(axis, containing_block_width);

    let mut position = (parent_content_box_size - child_outer_size) / 2.0
        + parent.layout().border(flex_start_edge(axis))
        + child
            .style()
            .compute_flex_start_margin(axis, direction, containing_block_width);

    // https://www.w3.org/TR/css-grid-1/#abspos
    // absolute positioned grid items are positioned relative to the padding edge
    // of the grid container
    if !child.has_errata(Errata::ABSOLUTE_POSITION_WITHOUT_INSETS_EXCLUDES_PADDING)
        && parent.style().display() != Display::Grid
    {
        position += parent.layout().padding(flex_start_edge(axis));
    }

    child.set_layout_position(position, flex_start_edge(axis));
}

/// `justifyAbsoluteChild`.
fn justify_absolute_child<'a, N: LayoutNode<'a>>(
    parent: N,
    child: N,
    direction: Direction,
    main_axis: FlexDirection,
    containing_block_width: f32,
) {
    let justify = if parent.style().display() == Display::Grid {
        resolve_child_justification(parent, child)
    } else {
        parent.style().justify_content()
    };
    match justify {
        Justify::Start
        | Justify::Auto
        | Justify::Stretch
        | Justify::FlexStart
        | Justify::SpaceBetween => {
            set_flex_start_layout_position(
                parent,
                child,
                direction,
                main_axis,
                containing_block_width,
            );
        }
        Justify::End | Justify::FlexEnd => {
            set_flex_end_layout_position(
                parent,
                child,
                direction,
                main_axis,
                containing_block_width,
            );
        }
        Justify::Center | Justify::SpaceAround | Justify::SpaceEvenly => {
            set_center_layout_position(parent, child, direction, main_axis, containing_block_width);
        }
    }
}

/// `alignAbsoluteChild`.
fn align_absolute_child<'a, N: LayoutNode<'a>>(
    parent: N,
    child: N,
    direction: Direction,
    cross_axis: FlexDirection,
    containing_block_width: f32,
) {
    let mut item_align = resolve_child_alignment(parent, child);
    let parent_wrap = parent.style().flex_wrap();
    if parent_wrap == Wrap::WrapReverse {
        if item_align == Align::FlexEnd {
            item_align = Align::FlexStart;
        } else if item_align != Align::Center {
            item_align = Align::FlexEnd;
        }
    }

    match item_align {
        Align::Start
        | Align::Auto
        | Align::FlexStart
        | Align::Baseline
        | Align::SpaceAround
        | Align::SpaceBetween
        | Align::Stretch
        | Align::SpaceEvenly => {
            set_flex_start_layout_position(
                parent,
                child,
                direction,
                cross_axis,
                containing_block_width,
            );
        }
        Align::End | Align::FlexEnd => {
            set_flex_end_layout_position(
                parent,
                child,
                direction,
                cross_axis,
                containing_block_width,
            );
        }
        Align::Center => {
            set_center_layout_position(
                parent,
                child,
                direction,
                cross_axis,
                containing_block_width,
            );
        }
    }
}

/// `positionAbsoluteChild`.
///
/// Absolutely positioned nodes do not participate in flex layout and thus their
/// positions can be determined independently from the rest of their siblings.
/// For each axis there are essentially two cases:
///
/// 1) The node has insets defined. In this case we can just use these to
///    determine the position of the node.
/// 2) The node does not have insets defined. In this case we look at the style
///    of the parent to position the node. Things like justify content and
///    align content will move absolute children around. If none of these
///    special properties are defined, the child is positioned at the start
///    (defined by flex direction) of the leading flex line.
///
/// This function does that positioning for the given axis. The spec has more
/// information on this topic: <https://www.w3.org/TR/css-flexbox-1/#abspos-items>
fn position_absolute_child<'a, N: LayoutNode<'a>>(
    containing_node: N,
    parent: N,
    child: N,
    direction: Direction,
    axis: FlexDirection,
    is_main_axis: bool,
    containing_block_width: f32,
    containing_block_height: f32,
) {
    let is_axis_row = is_row(axis);
    let containing_block_size = if is_axis_row {
        containing_block_width
    } else {
        containing_block_height
    };

    // The inline-start position takes priority over the end position in the case
    // that they are both set and the node has a fixed width. Thus we only have 2
    // cases here: if inline-start is defined and if inline-end is defined.
    //
    // Despite checking inline-start to honor prioritization of insets, we write
    // to the flex-start edge because this algorithm works by positioning on the
    // flex-start edge and then filling in the flex-end direction at the end if
    // necessary.
    if child
        .style()
        .is_inline_start_position_defined(axis, direction)
        && !child.style().is_inline_start_position_auto(axis, direction)
    {
        let position_relative_to_inline_start =
            child
                .style()
                .compute_inline_start_position(axis, direction, containing_block_size)
                + containing_node
                    .style()
                    .compute_inline_start_border(axis, direction)
                + child
                    .style()
                    .compute_inline_start_margin(axis, direction, containing_block_size);
        let position_relative_to_flex_start =
            if inline_start_edge(axis, direction) != flex_start_edge(axis) {
                get_position_of_opposite_edge(
                    position_relative_to_inline_start,
                    axis,
                    containing_node,
                    child,
                )
            } else {
                position_relative_to_inline_start
            };

        child.set_layout_position(position_relative_to_flex_start, flex_start_edge(axis));
    } else if child
        .style()
        .is_inline_end_position_defined(axis, direction)
        && !child.style().is_inline_end_position_auto(axis, direction)
    {
        let position_relative_to_inline_start =
            containing_node.layout().measured_dimension(dimension(axis))
                - child.layout().measured_dimension(dimension(axis))
                - containing_node
                    .style()
                    .compute_inline_end_border(axis, direction)
                - child
                    .style()
                    .compute_inline_end_margin(axis, direction, containing_block_size)
                - child
                    .style()
                    .compute_inline_end_position(axis, direction, containing_block_size);
        let position_relative_to_flex_start =
            if inline_start_edge(axis, direction) != flex_start_edge(axis) {
                get_position_of_opposite_edge(
                    position_relative_to_inline_start,
                    axis,
                    containing_node,
                    child,
                )
            } else {
                position_relative_to_inline_start
            };

        child.set_layout_position(position_relative_to_flex_start, flex_start_edge(axis));
    } else if is_main_axis {
        justify_absolute_child(parent, child, direction, axis, containing_block_width);
    } else {
        align_absolute_child(parent, child, direction, axis, containing_block_width);
    }
}

/// `layoutAbsoluteDescendants`: lays out the absolute descendants of
/// `current_node` whose containing block is `containing_node`, descending
/// through static children. Returns whether some absolute descendant has new
/// layout.
pub fn layout_absolute_descendants<'a, N: LayoutNode<'a>>(
    containing_node: N,
    current_node: N,
    width_sizing_mode: SizingMode,
    current_node_direction: Direction,
    layout_marker_data: &mut LayoutData,
    current_depth: u32,
    generation_count: u32,
    current_node_left_offset_from_containing_block: f32,
    current_node_top_offset_from_containing_block: f32,
    containing_node_available_inner_width: f32,
    containing_node_available_inner_height: f32,
) -> bool {
    let mut has_new_layout = false;
    for child in current_node.layout_children() {
        let child_style = child.style();
        if child_style.display() == Display::None {
            continue;
        } else if child_style.position_type() == PositionType::Absolute {
            let absolute_errata =
                current_node.has_errata(Errata::ABSOLUTE_PERCENT_AGAINST_INNER_SIZE);
            let containing_block_width = if absolute_errata {
                containing_node_available_inner_width
            } else {
                containing_node
                    .layout()
                    .measured_dimension(Dimension::Width)
                    - containing_node
                        .style()
                        .compute_border_for_axis(FlexDirection::Row)
            };
            let containing_block_height = if absolute_errata {
                containing_node_available_inner_height
            } else {
                containing_node
                    .layout()
                    .measured_dimension(Dimension::Height)
                    - containing_node
                        .style()
                        .compute_border_for_axis(FlexDirection::Column)
            };

            layout_absolute_child(
                containing_node,
                current_node,
                child,
                containing_block_width,
                containing_block_height,
                width_sizing_mode,
                current_node_direction,
                layout_marker_data,
                current_depth,
                generation_count,
            );

            has_new_layout = has_new_layout || child.has_new_layout();

            // At this point the child has its position set but only on its
            // the parent's flexStart edge. Additionally, this position should
            // be interpreted relative to the containing block of the child if
            // it had insets defined. So we need to adjust the position by
            // subtracting the the parents offset from the containing block.
            // However, getting that offset is complicated since the two nodes
            // can have different main/cross axes.
            let parent_main_axis = resolve_direction(
                current_node.style().flex_direction(),
                current_node_direction,
            );
            let parent_cross_axis =
                resolve_cross_direction(parent_main_axis, current_node_direction);

            if needs_trailing_position(parent_main_axis) {
                let main_insets_defined = if is_row(parent_main_axis) {
                    child_style.horizontal_insets_defined()
                } else {
                    child_style.vertical_insets_defined()
                };
                set_child_trailing_position(
                    if main_insets_defined {
                        containing_node
                    } else {
                        current_node
                    },
                    child,
                    parent_main_axis,
                );
            }
            if needs_trailing_position(parent_cross_axis) {
                let cross_insets_defined = if is_row(parent_cross_axis) {
                    child_style.horizontal_insets_defined()
                } else {
                    child_style.vertical_insets_defined()
                };
                set_child_trailing_position(
                    if cross_insets_defined {
                        containing_node
                    } else {
                        current_node
                    },
                    child,
                    parent_cross_axis,
                );
            }

            // At this point we know the left and top physical edges of the
            // child are set with positions that are relative to the
            // containing block if insets are defined.
            let child_left_position = child.layout().position(PhysicalEdge::Left);
            let child_top_position = child.layout().position(PhysicalEdge::Top);

            let child_left_offset_from_parent = if child_style.horizontal_insets_defined() {
                child_left_position - current_node_left_offset_from_containing_block
            } else {
                child_left_position
            };
            let child_top_offset_from_parent = if child_style.vertical_insets_defined() {
                child_top_position - current_node_top_offset_from_containing_block
            } else {
                child_top_position
            };

            child.set_layout_position(child_left_offset_from_parent, PhysicalEdge::Left);
            child.set_layout_position(child_top_offset_from_parent, PhysicalEdge::Top);
        } else if child_style.position_type() == PositionType::Static
            && !child.always_forms_containing_block()
        {
            // We may write new layout results for absolute descendants of
            // "child" which are positioned relative to the current containing
            // block instead of their parent. "child" may not be dirty, or have
            // new constraints, so absolute positioning may be the first time
            // during this layout pass that we need to mutate these
            // descendants. Make sure the path of nodes to them is mutable
            // before positioning.
            child.clone_children_if_needed();
            let child_direction = child.resolve_direction(current_node_direction);
            // By now all descendants of the containing block that are not
            // absolute will have their positions set for left and top.
            let child_left_offset_from_containing_block =
                current_node_left_offset_from_containing_block
                    + child.layout().position(PhysicalEdge::Left);
            let child_top_offset_from_containing_block =
                current_node_top_offset_from_containing_block
                    + child.layout().position(PhysicalEdge::Top);

            has_new_layout = layout_absolute_descendants(
                containing_node,
                child,
                width_sizing_mode,
                child_direction,
                layout_marker_data,
                current_depth + 1,
                generation_count,
                child_left_offset_from_containing_block,
                child_top_offset_from_containing_block,
                containing_node_available_inner_width,
                containing_node_available_inner_height,
            ) || has_new_layout;

            cleanup_contents_nodes_recursively(child, has_new_layout);
            if has_new_layout {
                child.set_has_new_layout(has_new_layout);
            }
        }
    }
    has_new_layout
}
