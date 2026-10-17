//! The measure/layout cache (`yoga/algorithm/Cache.cpp`, and the cache
//! lookup in `calculateLayoutInternal`).
//!
//! [`find_cached_result`] looks up the entry of a node's layout cache or
//! measurement cache whose results can be reused for the given constraints.
//! Results are recorded by `calculate_layout_internal` in
//! `calculate_layout.rs`, exactly as Yoga records them.

use crate::algorithm::pixel_grid::round_value_to_pixel_grid;
use crate::algorithm::sizing_mode::SizingMode;
use crate::config::Config;
use crate::enums::FlexDirection;
use crate::node::LayoutNode;
use crate::numeric::{inexact_equals, is_defined};

/// Which cache entry a lookup hit.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CachedResult {
    /// `layout->cachedLayout`.
    Layout,
    /// `layout->cachedMeasurements[i]`.
    Measurement(usize),
}

/// `canUseCachedMeasurement`.
pub fn can_use_cached_measurement(
    width_mode: SizingMode,
    available_width: f32,
    height_mode: SizingMode,
    available_height: f32,
    last_width_mode: SizingMode,
    last_available_width: f32,
    last_height_mode: SizingMode,
    last_available_height: f32,
    last_computed_width: f32,
    last_computed_height: f32,
    margin_row: f32,
    margin_column: f32,
    config: &Config,
) -> bool {
    if (is_defined(last_computed_height) && last_computed_height < 0.0)
        || (is_defined(last_computed_width) && last_computed_width < 0.0)
    {
        return false;
    }

    let point_scale_factor = config.point_scale_factor();

    // Yoga's `config != nullptr` check is dropped: `config` is a reference.
    let use_rounded_comparison = point_scale_factor != 0.0;
    let effective_width = if use_rounded_comparison {
        round_value_to_pixel_grid(
            available_width as f64,
            point_scale_factor as f64,
            false,
            false,
        )
    } else {
        available_width
    };
    let effective_height = if use_rounded_comparison {
        round_value_to_pixel_grid(
            available_height as f64,
            point_scale_factor as f64,
            false,
            false,
        )
    } else {
        available_height
    };
    let effective_last_width = if use_rounded_comparison {
        round_value_to_pixel_grid(
            last_available_width as f64,
            point_scale_factor as f64,
            false,
            false,
        )
    } else {
        last_available_width
    };
    let effective_last_height = if use_rounded_comparison {
        round_value_to_pixel_grid(
            last_available_height as f64,
            point_scale_factor as f64,
            false,
            false,
        )
    } else {
        last_available_height
    };

    let has_same_width_spec =
        last_width_mode == width_mode && inexact_equals(effective_last_width, effective_width);
    let has_same_height_spec =
        last_height_mode == height_mode && inexact_equals(effective_last_height, effective_height);

    let width_is_compatible = has_same_width_spec
        || size_is_exact_and_matches_old_measured_size(
            width_mode,
            available_width - margin_row,
            last_computed_width,
        )
        || old_size_is_max_content_and_still_fits(
            width_mode,
            available_width - margin_row,
            last_width_mode,
            last_computed_width,
        )
        || new_size_is_stricter_and_still_valid(
            width_mode,
            available_width - margin_row,
            last_width_mode,
            last_available_width,
            last_computed_width,
        );

    let height_is_compatible = has_same_height_spec
        || size_is_exact_and_matches_old_measured_size(
            height_mode,
            available_height - margin_column,
            last_computed_height,
        )
        || old_size_is_max_content_and_still_fits(
            height_mode,
            available_height - margin_column,
            last_height_mode,
            last_computed_height,
        )
        || new_size_is_stricter_and_still_valid(
            height_mode,
            available_height - margin_column,
            last_height_mode,
            last_available_height,
            last_computed_height,
        );

    width_is_compatible && height_is_compatible
}

/// `sizeIsExactAndMatchesOldMeasuredSize`.
fn size_is_exact_and_matches_old_measured_size(
    size_mode: SizingMode,
    size: f32,
    last_computed_size: f32,
) -> bool {
    size_mode == SizingMode::StretchFit && inexact_equals(size, last_computed_size)
}

/// `oldSizeIsMaxContentAndStillFits`.
fn old_size_is_max_content_and_still_fits(
    size_mode: SizingMode,
    size: f32,
    last_size_mode: SizingMode,
    last_computed_size: f32,
) -> bool {
    size_mode == SizingMode::FitContent
        && last_size_mode == SizingMode::MaxContent
        && (size >= last_computed_size || inexact_equals(size, last_computed_size))
}

/// `newSizeIsStricterAndStillValid`.
fn new_size_is_stricter_and_still_valid(
    size_mode: SizingMode,
    size: f32,
    last_size_mode: SizingMode,
    last_size: f32,
    last_computed_size: f32,
) -> bool {
    last_size_mode == SizingMode::FitContent
        && size_mode == SizingMode::FitContent
        && is_defined(last_size)
        && is_defined(size)
        && is_defined(last_computed_size)
        && last_size > size
        && (last_computed_size <= size || inexact_equals(size, last_computed_size))
}

/// `calculateLayoutInternal` (the cache lookup): the entry of `node`'s layout
/// cache (for `perform_layout`) or measurement cache whose results can be
/// reused for these constraints, if any.
pub fn find_cached_result<'a, N: LayoutNode<'a>>(
    node: N,
    available_width: f32,
    available_height: f32,
    width_sizing_mode: SizingMode,
    height_sizing_mode: SizingMode,
    owner_width: f32,
    perform_layout: bool,
) -> Option<CachedResult> {
    let layout = node.layout();

    // Determine whether the results are already cached. We maintain a
    // separate cache for layouts and measurements. A layout operation
    // modifies the positions and dimensions for nodes in the subtree. The
    // algorithm assumes that each node gets laid out a maximum of one time
    // per tree layout, but multiple measurements may be required to resolve
    // all of the flex dimensions. We handle nodes with measure functions
    // specially here because they are the most expensive to measure, so it's
    // worth avoiding redundant measurements if at all possible.
    if node.has_measure_func() {
        let margin_axis_row = node
            .style()
            .compute_margin_for_axis(FlexDirection::Row, owner_width);
        let margin_axis_column = node
            .style()
            .compute_margin_for_axis(FlexDirection::Column, owner_width);

        // First, try to use the layout cache.
        let cached_layout = layout.cached_layout.get();
        if can_use_cached_measurement(
            width_sizing_mode,
            available_width,
            height_sizing_mode,
            available_height,
            cached_layout.width_sizing_mode,
            cached_layout.available_width,
            cached_layout.height_sizing_mode,
            cached_layout.available_height,
            cached_layout.computed_width,
            cached_layout.computed_height,
            margin_axis_row,
            margin_axis_column,
            node.config(),
        ) {
            return Some(CachedResult::Layout);
        }

        // Try to use the measurement cache.
        for i in 0..layout.next_cached_measurements_index.get() as usize {
            let entry = layout.cached_measurements[i].get();
            if can_use_cached_measurement(
                width_sizing_mode,
                available_width,
                height_sizing_mode,
                available_height,
                entry.width_sizing_mode,
                entry.available_width,
                entry.height_sizing_mode,
                entry.available_height,
                entry.computed_width,
                entry.computed_height,
                margin_axis_row,
                margin_axis_column,
                node.config(),
            ) {
                return Some(CachedResult::Measurement(i));
            }
        }
        None
    } else if perform_layout {
        let cached_layout = layout.cached_layout.get();
        if inexact_equals(cached_layout.available_width, available_width)
            && inexact_equals(cached_layout.available_height, available_height)
            && cached_layout.width_sizing_mode == width_sizing_mode
            && cached_layout.height_sizing_mode == height_sizing_mode
        {
            Some(CachedResult::Layout)
        } else {
            None
        }
    } else {
        for i in 0..layout.next_cached_measurements_index.get() as usize {
            let entry = layout.cached_measurements[i].get();
            if inexact_equals(entry.available_width, available_width)
                && inexact_equals(entry.available_height, available_height)
                && entry.width_sizing_mode == width_sizing_mode
                && entry.height_sizing_mode == height_sizing_mode
            {
                return Some(CachedResult::Measurement(i));
            }
        }
        None
    }
}
