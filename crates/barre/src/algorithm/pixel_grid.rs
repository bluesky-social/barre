//! Rounding to the pixel grid (`yoga/algorithm/PixelGrid.cpp`).
//!
//! Yoga does this arithmetic in `double` and stores `float`s; so does this
//! port, operation for operation.

use crate::enums::{Dimension, NodeType, PhysicalEdge};
use crate::node::{LayoutNode, NodeExt};
use crate::numeric::inexact_equals_f64;

/// `roundValueToPixelGridDouble`.
fn round_value_to_pixel_grid_double(
    value: f64,
    point_scale_factor: f64,
    force_ceil: bool,
    force_floor: bool,
) -> f64 {
    let mut scaled_value = value * point_scale_factor;
    // We want to calculate `fractional` such that `floor(scaledValue) =
    // scaledValue - fractional`.
    //
    // `copysign(x - trunc(x), x)` is bit-identical to `fmod(x, 1.0)` for
    // every double (including -0.0, infinities and NaN).
    let mut fractional = (scaled_value - scaled_value.trunc()).copysign(scaled_value);
    if fractional < 0.0 {
        // This branch is for handling negative numbers for `value`: adding 1
        // to the (negative) remainder gives the number that, subtracted from
        // the value, yields its floor.
        fractional += 1.0;
    }
    if inexact_equals_f64(fractional, 0.0) {
        // First we check if the value is already rounded
        scaled_value -= fractional;
    } else if inexact_equals_f64(fractional, 1.0) {
        scaled_value = scaled_value - fractional + 1.0;
    } else if force_ceil {
        // Next we check if we need to use forced rounding
        scaled_value = scaled_value - fractional + 1.0;
    } else if force_floor {
        scaled_value -= fractional;
    } else {
        // Finally we just round the value
        scaled_value = scaled_value - fractional
            + if !fractional.is_nan() && (fractional > 0.5 || inexact_equals_f64(fractional, 0.5)) {
                1.0
            } else {
                0.0
            };
    }
    if scaled_value.is_nan() || point_scale_factor.is_nan() {
        f32::NAN as f64
    } else {
        scaled_value / point_scale_factor
    }
}

/// `roundValueToPixelGrid`: rounds a point value to the nearest whole
/// pixel, given a point scale factor describing pixel density.
pub fn round_value_to_pixel_grid(
    value: f64,
    point_scale_factor: f64,
    force_ceil: bool,
    force_floor: bool,
) -> f32 {
    round_value_to_pixel_grid_double(value, point_scale_factor, force_ceil, force_floor) as f32
}

/// `roundLayoutResultsToPixelGrid`.
pub fn round_layout_results_to_pixel_grid<'a, N: LayoutNode<'a>>(
    node: N,
    absolute_left: f64,
    absolute_top: f64,
) {
    let point_scale_factor = node.config().point_scale_factor() as f64;

    let layout = node.layout();
    let node_left = layout.position(PhysicalEdge::Left) as f64;
    let node_top = layout.position(PhysicalEdge::Top) as f64;

    let node_width = layout.dimension(Dimension::Width) as f64;
    let node_height = layout.dimension(Dimension::Height) as f64;

    let absolute_node_left = absolute_left + node_left;
    let absolute_node_top = absolute_top + node_top;

    let absolute_node_right = absolute_node_left + node_width;
    let absolute_node_bottom = absolute_node_top + node_height;

    if point_scale_factor != 0.0 {
        // If a node has a custom measure function we never want to round down
        // its size as this could lead to unwanted text truncation.
        let text_rounding = node.node_type() == NodeType::Text;

        node.set_layout_position(
            round_value_to_pixel_grid(node_left, point_scale_factor, false, text_rounding),
            PhysicalEdge::Left,
        );

        node.set_layout_position(
            round_value_to_pixel_grid(node_top, point_scale_factor, false, text_rounding),
            PhysicalEdge::Top,
        );

        // We multiply dimension by scale factor and if the result is close to
        // the whole number, we don't have any fraction. To verify if the
        // result is close to whole number we want to check both floor and
        // ceil numbers.
        let scaled_node_width = node_width * point_scale_factor;
        let has_fractional_width =
            !inexact_equals_f64(scaled_node_width.round(), scaled_node_width);
        let scaled_node_height = node_height * point_scale_factor;
        let has_fractional_height =
            !inexact_equals_f64(scaled_node_height.round(), scaled_node_height);

        layout.set_dimension(
            Dimension::Width,
            if text_rounding && !has_fractional_width {
                (scaled_node_width.round() / point_scale_factor) as f32
            } else {
                (round_value_to_pixel_grid_double(
                    absolute_node_right,
                    point_scale_factor,
                    text_rounding,
                    false,
                ) - round_value_to_pixel_grid_double(
                    absolute_node_left,
                    point_scale_factor,
                    false,
                    text_rounding,
                )) as f32
            },
        );

        layout.set_dimension(
            Dimension::Height,
            if text_rounding && !has_fractional_height {
                (scaled_node_height.round() / point_scale_factor) as f32
            } else {
                (round_value_to_pixel_grid_double(
                    absolute_node_bottom,
                    point_scale_factor,
                    text_rounding,
                    false,
                ) - round_value_to_pixel_grid_double(
                    absolute_node_top,
                    point_scale_factor,
                    false,
                    text_rounding,
                )) as f32
            },
        );
    }

    for i in 0..node.child_count() {
        let child = node.child(i);
        if !child.owner().is_some_and(|o| o.is_same(node)) {
            continue;
        }
        round_layout_results_to_pixel_grid(child, absolute_node_left, absolute_node_top);
    }
}
