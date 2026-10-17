//! Min/max clamping (`yoga/algorithm/BoundAxis.h`).

use crate::algorithm::flex_direction::{is_column, is_row};
use crate::enums::{Dimension, Direction, FlexDirection};
use crate::node::LayoutNode;
use crate::numeric::max_or_defined;

/// `paddingAndBorderForAxis`.
#[inline]
pub fn padding_and_border_for_axis<'a, N: LayoutNode<'a>>(
    node: N,
    axis: FlexDirection,
    direction: Direction,
    width_size: f32,
) -> f32 {
    let style = node.style();
    style.compute_inline_start_padding_and_border(axis, direction, width_size)
        + style.compute_inline_end_padding_and_border(axis, direction, width_size)
}

/// `boundAxisWithinMinAndMax`. `value` may be undefined.
pub fn bound_axis_within_min_and_max<'a, N: LayoutNode<'a>>(
    node: N,
    direction: Direction,
    axis: FlexDirection,
    value: f32,
    axis_size: f32,
    width_size: f32,
) -> f32 {
    let style = node.style();
    let mut min = f32::NAN;
    let mut max = f32::NAN;
    if is_column(axis) {
        min = style.resolved_min_dimension(direction, Dimension::Height, axis_size, width_size);
        max = style.resolved_max_dimension(direction, Dimension::Height, axis_size, width_size);
    } else if is_row(axis) {
        min = style.resolved_min_dimension(direction, Dimension::Width, axis_size, width_size);
        max = style.resolved_max_dimension(direction, Dimension::Width, axis_size, width_size);
    }

    // FloatOptional comparisons: false whenever either side is undefined.
    if max >= 0.0 && value > max {
        return max;
    }
    if min >= 0.0 && value < min {
        return min;
    }
    value
}

/// `boundAxis`: like [`bound_axis_within_min_and_max`] but also ensures that
/// the value doesn't go below the padding and border amount.
#[inline]
pub fn bound_axis<'a, N: LayoutNode<'a>>(
    node: N,
    axis: FlexDirection,
    direction: Direction,
    value: f32,
    axis_size: f32,
    width_size: f32,
) -> f32 {
    max_or_defined(
        bound_axis_within_min_and_max(node, direction, axis, value, axis_size, width_size),
        padding_and_border_for_axis(node, axis, direction, width_size),
    )
}
