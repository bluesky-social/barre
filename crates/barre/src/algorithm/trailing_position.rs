//! Trailing-edge positions (`yoga/algorithm/TrailingPosition.h`).

use crate::algorithm::flex_direction::{dimension, flex_end_edge, flex_start_edge};
use crate::enums::FlexDirection;
use crate::node::{LayoutNode, NodeExt};

/// `getPositionOfOppositeEdge`: given an offset to an edge, returns the
/// offset to the opposite edge on the same axis. This assumes that the
/// width/height of both nodes is determined at this point.
#[inline]
pub fn get_position_of_opposite_edge<'a, N: LayoutNode<'a>>(
    position: f32,
    axis: FlexDirection,
    containing_node: N,
    node: N,
) -> f32 {
    containing_node.layout().measured_dimension(dimension(axis))
        - node.layout().measured_dimension(dimension(axis))
        - position
}

/// `setChildTrailingPosition`.
#[inline]
pub fn set_child_trailing_position<'a, N: LayoutNode<'a>>(node: N, child: N, axis: FlexDirection) {
    child.set_layout_position(
        get_position_of_opposite_edge(
            child.layout().position(flex_start_edge(axis)),
            axis,
            node,
            child,
        ),
        flex_end_edge(axis),
    );
}

/// `needsTrailingPosition`.
#[inline]
pub fn needs_trailing_position(axis: FlexDirection) -> bool {
    axis == FlexDirection::RowReverse || axis == FlexDirection::ColumnReverse
}
