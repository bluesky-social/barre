//! The measure/layout cache (`yoga/algorithm/Cache.cpp`, and the cache
//! lookup in `calculateLayoutInternal`).
//!
//! TICKET (measure cache): not implemented yet. The spine never reuses a
//! cached result: [`find_cached_result`] always returns `None`, so every
//! `calculate_layout_internal` call recomputes. Results are still *recorded*
//! in `LayoutResults::{cached_layout, cached_measurements}` exactly as Yoga
//! records them, so implementing the lookup is all that is needed.
//!
//! To implement: port `canUseCachedMeasurement` (and its three static
//! helpers) into [`can_use_cached_measurement`], and port the lookup block of
//! `calculateLayoutInternal` (from `if (node->hasMeasureFunc())` down to the
//! measurement-cache loop) into [`find_cached_result`]. Nothing outside this
//! file needs to change. Target tests: `YGMeasureCacheTest`,
//! `YGMeasureTest` (measure call counts), `YGRelayoutTest`,
//! `YGDirtyMarkingTest`, and the `YGNodeCanUseCachedMeasurement` C API.

use crate::algorithm::sizing_mode::SizingMode;
use crate::config::Config;
use crate::node::LayoutNode;

/// Which cache entry a lookup hit.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CachedResult {
    /// `layout->cachedLayout`.
    Layout,
    /// `layout->cachedMeasurements[i]`.
    Measurement(usize),
}

/// `canUseCachedMeasurement`.
#[allow(unused_variables)]
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
    crate::not_implemented!("canUseCachedMeasurement")
}

/// The cache lookup of `calculateLayoutInternal`: the entry of `node`'s
/// layout cache (for `perform_layout`) or measurement cache whose results can
/// be reused for these constraints, if any.
///
/// Not implemented yet (see the module docs): always `None`, which is
/// correct, only slower — every layout is recomputed.
#[allow(unused_variables)]
pub fn find_cached_result<'a, N: LayoutNode<'a>>(
    node: N,
    available_width: f32,
    available_height: f32,
    width_sizing_mode: SizingMode,
    height_sizing_mode: SizingMode,
    owner_width: f32,
    perform_layout: bool,
) -> Option<CachedResult> {
    // TODO(ticket: measure cache): port the lookup from
    // `calculateLayoutInternal`.
    None
}
