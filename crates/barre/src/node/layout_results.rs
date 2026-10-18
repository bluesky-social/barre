//! Layout results (`yoga/node/LayoutResults.h`, `CachedMeasurement.h`).
//!
//! Every field is a `Cell` because layout writes results through shared
//! references (see `docs/API.md`, "Node storage"). `Cell` of a `Copy` value
//! compiles to plain loads and stores, and is `repr(transparent)`, so
//! [`LayoutResults`] is a fixed-size, pointer-free `repr(C)` blob: a host
//! that keeps its own nodes (the React Native shim) can embed it as raw bytes
//! and copy it when a node is cloned. It holds everything that persists on a
//! node between layouts: results, the measure cache, the computed flex basis
//! and its generation.

use std::cell::Cell;

use crate::algorithm::calculate_layout::LayoutPassReason;
use crate::algorithm::sizing_mode::SizingMode;
use crate::enums::{Dimension, Direction, PhysicalEdge};
use crate::style::{StyleLength, StyleSizeLength};

/// `yoga::CachedMeasurement`.
///
/// Deliberately differs from Yoga (yoga#2019): each entry also records the
/// `generation` of the run that stored it and the flex-basis anchor key
/// (`anchor_*`, see [`FlexBasisAnchor`]) in effect for that run, so that
/// `calculate_layout_internal` can refuse an earlier layout's entry whose
/// run read children's bases retained under a different anchor. The four
/// sizing modes are grouped so `repr(C)` adds no padding; the struct grows
/// from 20 to 32 bytes.
///
/// Deliberately differs from Yoga (yoga#1626, yoga#1003, yoga#1784): each
/// entry also records the `owner_width`/`owner_height` it was computed
/// under, so `calculate_layout_internal` can refuse an entry whose node's
/// own owner-relative inputs (percent padding and margin, resolved against
/// the owner width, yoga#1626 and yoga#1784; percent min/max, resolved
/// against the owner width and height, yoga#1003) resolve differently under
/// the current owner size. These two `f32`s add 8 bytes, for 40 in total
/// (`size_of`, asserted below). The anchor's owner sizes (`FlexBasisAnchor`)
/// are those of the anchor call, not of this entry's call, so they cannot be
/// reused here.
#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub struct CachedMeasurement {
    pub available_width: f32,
    pub available_height: f32,
    pub computed_width: f32,
    pub computed_height: f32,
    /// Owner sizes of the call that stored this entry (yoga#1626, #1003,
    /// #1784).
    pub owner_width: f32,
    pub owner_height: f32,
    /// Generation of the layout whose run stored this entry.
    pub generation: u32,
    /// Available sizes of the flex-basis anchor in effect for that run.
    pub anchor_available_width: f32,
    pub anchor_available_height: f32,
    pub width_sizing_mode: SizingMode,
    pub height_sizing_mode: SizingMode,
    /// Sizing modes of the flex-basis anchor in effect for that run.
    pub anchor_width_sizing_mode: SizingMode,
    pub anchor_height_sizing_mode: SizingMode,
}

impl Default for CachedMeasurement {
    fn default() -> Self {
        CachedMeasurement {
            available_width: -1.0,
            available_height: -1.0,
            computed_width: -1.0,
            computed_height: -1.0,
            owner_width: f32::NAN,
            owner_height: f32::NAN,
            generation: 0,
            anchor_available_width: f32::NAN,
            anchor_available_height: f32::NAN,
            width_sizing_mode: SizingMode::MaxContent,
            height_sizing_mode: SizingMode::MaxContent,
            anchor_width_sizing_mode: SizingMode::MaxContent,
            anchor_height_sizing_mode: SizingMode::MaxContent,
        }
    }
}

/// barre-only, no Yoga counterpart (yoga#1552, yoga#2019): the most recent
/// call to `calculate_layout_internal` on this node in a layout after which,
/// in a fresh layout, its children hold freshly rewritten flex bases. It
/// holds every argument needed to re-issue that call to
/// `calculate_layout_impl` (`replay_pending` is set when the call was
/// answered from an earlier layout's cache entry and must be replayed before
/// the next real run). 28 bytes (`size_of`, asserted below). The default has
/// generation 0, which is never current because the generation counter
/// starts at 1 (the same assumption Yoga makes for `generationCount`).
#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub struct FlexBasisAnchor {
    pub generation: u32,
    pub available_width: f32,
    pub available_height: f32,
    pub owner_width: f32,
    pub owner_height: f32,
    pub width_sizing_mode: SizingMode,
    pub height_sizing_mode: SizingMode,
    pub owner_direction: Direction,
    pub reason: LayoutPassReason,
    pub replay_pending: bool,
    /// Set when, while this anchor waits for its replay, a call with a
    /// `StretchFit` main axis was answered from an earlier layout's cache
    /// entry. In a fresh layout that call ran after the anchor and gave the
    /// node's single flex child (`computeFlexBasisForChildren`'s
    /// `singleFlexChild`) a basis of 0; the replay must be followed by the
    /// same write. Fits in the struct's padding (still 28 bytes).
    pub single_flex_child_pending: bool,
}

impl Default for FlexBasisAnchor {
    fn default() -> Self {
        FlexBasisAnchor {
            generation: 0,
            available_width: f32::NAN,
            available_height: f32::NAN,
            owner_width: f32::NAN,
            owner_height: f32::NAN,
            width_sizing_mode: SizingMode::MaxContent,
            height_sizing_mode: SizingMode::MaxContent,
            owner_direction: Direction::Inherit,
            reason: LayoutPassReason::Initial,
            replay_pending: false,
            single_flex_child_pending: false,
        }
    }
}

const _: () = assert!(std::mem::size_of::<CachedMeasurement>() == 40);
const _: () = assert!(std::mem::size_of::<FlexBasisAnchor>() == 28);

/// `LayoutResults::MaxCachedMeasurements`. This value was chosen based on
/// empirical data: 98% of analyzed layouts require less than 8 entries.
pub const MAX_CACHED_MEASUREMENTS: usize = 8;

/// `yoga::LayoutResults`, plus the per-node layout state Yoga keeps on
/// `yoga::Node` itself (`lineIndex_`, `processedDimensions_`).
///
/// Deliberately differs from Yoga (yoga#1552, yoga#2019): it also holds
/// `flex_basis_anchor`, and each cache entry is 20 bytes larger (12 for the
/// anchor, 8 for owner sizes, yoga#1626, #1003, #1784), so the struct grows
/// by 9 × 20 + 28 = 208 bytes (`size_of` of the two structs).
/// Known residuals this does not fix: measure-function
/// leaves reused through `can_use_cached_measurement`'s compatible sizes;
/// pixel-grid rounding re-applied to cached subtrees; history more than one
/// level deep (only a node's own anchor is compared, and only its latest
/// skipped anchor is replayed, so the calls an earlier skipped anchor or a
/// skipped non-anchor call made into the children are lost:
/// `conformance/tests/YGRelayoutFreshResidualTest.cpp`); and cache-slot
/// occupancy by earlier-layout entries.
#[derive(Clone, Debug)]
#[repr(C)]
pub struct LayoutResults {
    pub computed_flex_basis_generation: Cell<u32>,
    pub computed_flex_basis: Cell<f32>,
    /// Per-flex-item floor along the main axis derived from CSS Flexbox §4.5
    /// automatic minimum sizing (see `computeAutoMinMainSize`). Undefined
    /// means "no auto-min applies".
    pub computed_auto_min_main_size: Cell<f32>,

    // Instead of recomputing the entire layout every single time, we cache
    // some information to break early when nothing changed.
    pub generation_count: Cell<u32>,
    pub config_version: Cell<u32>,
    pub last_owner_direction: Cell<Direction>,
    pub next_cached_measurements_index: Cell<u32>,
    pub cached_measurements: [Cell<CachedMeasurement>; MAX_CACHED_MEASUREMENTS],
    pub cached_layout: Cell<CachedMeasurement>,
    /// barre-only (yoga#1552, yoga#2019): see [`FlexBasisAnchor`].
    pub flex_basis_anchor: Cell<FlexBasisAnchor>,

    /// `Node::lineIndex_`: the flex line this node was placed on by its
    /// parent's last layout.
    pub line_index: Cell<u32>,
    /// `Node::processedDimensions_` (see `processDimensions`).
    pub processed_dimensions: [Cell<StyleSizeLength>; 2],

    /// barre-only memo for incremental pixel-grid rounding
    /// (`algorithm/pixel_grid.rs`): the absolute position of this node's
    /// owner and the point scale factor this subtree was last rounded with,
    /// and whether that rounding left every value in the subtree unchanged
    /// (and the subtree has no absolutely positioned node).
    pub rounding_absolute_left: Cell<f64>,
    pub rounding_absolute_top: Cell<f64>,
    pub rounding_point_scale_factor: Cell<f32>,
    pub rounding_stable: Cell<bool>,

    direction: Cell<Direction>,
    had_overflow: Cell<bool>,
    dimensions: [Cell<f32>; 2],
    measured_dimensions: [Cell<f32>; 2],
    raw_dimensions: [Cell<f32>; 2],
    position: [Cell<f32>; 4],
    margin: [Cell<f32>; 4],
    border: [Cell<f32>; 4],
    padding: [Cell<f32>; 4],
}

impl Default for LayoutResults {
    fn default() -> Self {
        LayoutResults {
            computed_flex_basis_generation: Cell::new(0),
            computed_flex_basis: Cell::new(f32::NAN),
            computed_auto_min_main_size: Cell::new(f32::NAN),
            generation_count: Cell::new(0),
            config_version: Cell::new(0),
            last_owner_direction: Cell::new(Direction::Inherit),
            next_cached_measurements_index: Cell::new(0),
            cached_measurements: Default::default(),
            cached_layout: Cell::new(CachedMeasurement::default()),
            flex_basis_anchor: Cell::new(FlexBasisAnchor::default()),
            line_index: Cell::new(0),
            processed_dimensions: [
                Cell::new(StyleLength::UNDEFINED),
                Cell::new(StyleLength::UNDEFINED),
            ],
            rounding_absolute_left: Cell::new(f64::NAN),
            rounding_absolute_top: Cell::new(f64::NAN),
            rounding_point_scale_factor: Cell::new(f32::NAN),
            rounding_stable: Cell::new(false),
            direction: Cell::new(Direction::Inherit),
            had_overflow: Cell::new(false),
            dimensions: [Cell::new(f32::NAN), Cell::new(f32::NAN)],
            measured_dimensions: [Cell::new(f32::NAN), Cell::new(f32::NAN)],
            raw_dimensions: [Cell::new(f32::NAN), Cell::new(f32::NAN)],
            position: Default::default(),
            margin: Default::default(),
            border: Default::default(),
            padding: Default::default(),
        }
    }
}

/// Byte offsets of the layout results inside [`LayoutResults`], for a host
/// that embeds `LayoutResults` and reads results without calling into Rust
/// (the React Native shim reads them in `YGNodeLayoutGet*`, docs/RN-SHIM.md).
/// `direction` is a `u8` holding the `YGDirection` value, `had_overflow` a
/// `bool`; the rest are `f32` arrays indexed by `Dimension` or
/// `PhysicalEdge`, as their getters are.
#[derive(Copy, Clone, Debug)]
pub struct LayoutResultsOffsets {
    pub direction: usize,
    pub had_overflow: usize,
    pub dimensions: usize,
    pub raw_dimensions: usize,
    pub position: usize,
    pub margin: usize,
    pub border: usize,
    pub padding: usize,
}

impl LayoutResults {
    /// See [`LayoutResultsOffsets`].
    pub const OFFSETS: LayoutResultsOffsets = LayoutResultsOffsets {
        direction: std::mem::offset_of!(LayoutResults, direction),
        had_overflow: std::mem::offset_of!(LayoutResults, had_overflow),
        dimensions: std::mem::offset_of!(LayoutResults, dimensions),
        raw_dimensions: std::mem::offset_of!(LayoutResults, raw_dimensions),
        position: std::mem::offset_of!(LayoutResults, position),
        margin: std::mem::offset_of!(LayoutResults, margin),
        border: std::mem::offset_of!(LayoutResults, border),
        padding: std::mem::offset_of!(LayoutResults, padding),
    };

    /// `layout = {}` / `setLayout({})` in Yoga: resets every field to its
    /// default, except the two that Yoga keeps on the node rather than in
    /// `LayoutResults` (`line_index`, `processed_dimensions`).
    pub fn reset(&self) {
        let line_index = self.line_index.get();
        let processed = [
            self.processed_dimensions[0].get(),
            self.processed_dimensions[1].get(),
        ];
        self.copy_from(&LayoutResults::default());
        self.line_index.set(line_index);
        self.processed_dimensions[0].set(processed[0]);
        self.processed_dimensions[1].set(processed[1]);
    }

    /// Copies every field from `other` (`setLayout` in Yoga).
    pub fn copy_from(&self, other: &LayoutResults) {
        self.computed_flex_basis_generation
            .set(other.computed_flex_basis_generation.get());
        self.computed_flex_basis
            .set(other.computed_flex_basis.get());
        self.computed_auto_min_main_size
            .set(other.computed_auto_min_main_size.get());
        self.generation_count.set(other.generation_count.get());
        self.config_version.set(other.config_version.get());
        self.last_owner_direction
            .set(other.last_owner_direction.get());
        self.next_cached_measurements_index
            .set(other.next_cached_measurements_index.get());
        for (a, b) in self
            .cached_measurements
            .iter()
            .zip(&other.cached_measurements)
        {
            a.set(b.get());
        }
        self.cached_layout.set(other.cached_layout.get());
        self.flex_basis_anchor.set(other.flex_basis_anchor.get());
        self.line_index.set(other.line_index.get());
        self.processed_dimensions[0].set(other.processed_dimensions[0].get());
        self.processed_dimensions[1].set(other.processed_dimensions[1].get());
        self.rounding_absolute_left
            .set(other.rounding_absolute_left.get());
        self.rounding_absolute_top
            .set(other.rounding_absolute_top.get());
        self.rounding_point_scale_factor
            .set(other.rounding_point_scale_factor.get());
        self.rounding_stable.set(other.rounding_stable.get());
        self.direction.set(other.direction.get());
        self.had_overflow.set(other.had_overflow.get());
        copy_cells(&self.dimensions, &other.dimensions);
        copy_cells(&self.measured_dimensions, &other.measured_dimensions);
        copy_cells(&self.raw_dimensions, &other.raw_dimensions);
        copy_cells(&self.position, &other.position);
        copy_cells(&self.margin, &other.margin);
        copy_cells(&self.border, &other.border);
        copy_cells(&self.padding, &other.padding);
    }

    #[inline]
    pub fn direction(&self) -> Direction {
        self.direction.get()
    }
    #[inline]
    pub fn set_direction(&self, direction: Direction) {
        self.direction.set(direction);
    }
    #[inline]
    pub fn had_overflow(&self) -> bool {
        self.had_overflow.get()
    }
    #[inline]
    pub fn set_had_overflow(&self, had_overflow: bool) {
        self.had_overflow.set(had_overflow);
    }
    #[inline]
    pub fn dimension(&self, axis: Dimension) -> f32 {
        self.dimensions[axis as usize].get()
    }
    #[inline]
    pub fn set_dimension(&self, axis: Dimension, value: f32) {
        self.dimensions[axis as usize].set(value);
    }
    #[inline]
    pub fn measured_dimension(&self, axis: Dimension) -> f32 {
        self.measured_dimensions[axis as usize].get()
    }
    #[inline]
    pub fn set_measured_dimension(&self, axis: Dimension, value: f32) {
        self.measured_dimensions[axis as usize].set(value);
    }
    #[inline]
    pub fn raw_dimension(&self, axis: Dimension) -> f32 {
        self.raw_dimensions[axis as usize].get()
    }
    #[inline]
    pub fn set_raw_dimension(&self, axis: Dimension, value: f32) {
        self.raw_dimensions[axis as usize].set(value);
    }
    #[inline]
    pub fn position(&self, edge: PhysicalEdge) -> f32 {
        self.position[edge.index()].get()
    }
    #[inline]
    pub fn set_position(&self, edge: PhysicalEdge, value: f32) {
        self.position[edge.index()].set(value);
    }
    #[inline]
    pub fn margin(&self, edge: PhysicalEdge) -> f32 {
        self.margin[edge.index()].get()
    }
    #[inline]
    pub fn set_margin(&self, edge: PhysicalEdge, value: f32) {
        self.margin[edge.index()].set(value);
    }
    #[inline]
    pub fn border(&self, edge: PhysicalEdge) -> f32 {
        self.border[edge.index()].get()
    }
    #[inline]
    pub fn set_border(&self, edge: PhysicalEdge, value: f32) {
        self.border[edge.index()].set(value);
    }
    #[inline]
    pub fn padding(&self, edge: PhysicalEdge) -> f32 {
        self.padding[edge.index()].get()
    }
    #[inline]
    pub fn set_padding(&self, edge: PhysicalEdge, value: f32) {
        self.padding[edge.index()].set(value);
    }
}

#[inline]
fn copy_cells<const N: usize>(dst: &[Cell<f32>; N], src: &[Cell<f32>; N]) {
    for (a, b) in dst.iter().zip(src) {
        a.set(b.get());
    }
}

// A host may store `LayoutResults` as raw bytes and copy them (docs/API.md):
// it must stay plain data.
const _: () = assert!(!std::mem::needs_drop::<LayoutResults>());

// `LayoutResultsOffsets` promises these representations.
const _: () = assert!(std::mem::size_of::<Direction>() == 1);
const _: () = assert!(std::mem::size_of::<Cell<bool>>() == 1);
const _: () = assert!(std::mem::size_of::<[Cell<f32>; 4]>() == 16);
