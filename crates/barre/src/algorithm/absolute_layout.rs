//! Absolutely positioned children (`yoga/algorithm/AbsoluteLayout.cpp`).
//!
//! TICKET (absolute layout): [`layout_absolute_child`] is not implemented
//! yet. [`layout_absolute_descendants`] (the walk from a containing block to
//! its absolute descendants, through static children) is implemented, so the
//! stub is only reached when a tree contains a `position: absolute` node.
//!
//! To implement: port `layoutAbsoluteChild` and its static helpers
//! (`setFlexStartLayoutPosition`, `setFlexEndLayoutPosition`,
//! `setCenterLayoutPosition`, `justifyAbsoluteChild`, `alignAbsoluteChild`,
//! `positionAbsoluteChild`) into this file. Nothing outside this file needs to
//! change. Target tests: `YGAbsolutePositionTest`, `YGStaticPositionTest`,
//! and the absolute cases throughout the generated suite.

use crate::algorithm::calculate_layout::{LayoutData, cleanup_contents_nodes_recursively};
use crate::algorithm::flex_direction::{is_row, resolve_cross_direction, resolve_direction};
use crate::algorithm::sizing_mode::SizingMode;
use crate::algorithm::trailing_position::{needs_trailing_position, set_child_trailing_position};
use crate::enums::{Dimension, Direction, Display, Errata, FlexDirection, PhysicalEdge, PositionType};
use crate::node::{LayoutNode, NodeExt};

/// `layoutAbsoluteChild`: sizes `child` (an absolutely positioned descendant
/// of `node`) and positions it within `containing_node`, whose containing
/// block is `containing_block_width` x `containing_block_height`.
#[allow(unused_variables)]
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
    crate::not_implemented!("layoutAbsoluteChild")
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
                containing_node.layout().measured_dimension(Dimension::Width)
                    - containing_node
                        .style()
                        .compute_border_for_axis(FlexDirection::Row)
            };
            let containing_block_height = if absolute_errata {
                containing_node_available_inner_height
            } else {
                containing_node.layout().measured_dimension(Dimension::Height)
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
