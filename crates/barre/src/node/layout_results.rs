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

use crate::algorithm::sizing_mode::SizingMode;
use crate::enums::{Dimension, Direction, PhysicalEdge};
use crate::style::{StyleLength, StyleSizeLength};

/// `yoga::CachedMeasurement`.
#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub struct CachedMeasurement {
    pub available_width: f32,
    pub available_height: f32,
    pub width_sizing_mode: SizingMode,
    pub height_sizing_mode: SizingMode,
    pub computed_width: f32,
    pub computed_height: f32,
}

impl Default for CachedMeasurement {
    fn default() -> Self {
        CachedMeasurement {
            available_width: -1.0,
            available_height: -1.0,
            width_sizing_mode: SizingMode::MaxContent,
            height_sizing_mode: SizingMode::MaxContent,
            computed_width: -1.0,
            computed_height: -1.0,
        }
    }
}

/// `LayoutResults::MaxCachedMeasurements`. This value was chosen based on
/// empirical data: 98% of analyzed layouts require less than 8 entries.
pub const MAX_CACHED_MEASUREMENTS: usize = 8;

/// `yoga::LayoutResults`, plus the per-node layout state Yoga keeps on
/// `yoga::Node` itself (`lineIndex_`, `processedDimensions_`).
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

    /// `Node::lineIndex_`: the flex line this node was placed on by its
    /// parent's last layout.
    pub line_index: Cell<u32>,
    /// `Node::processedDimensions_` (see `processDimensions`).
    pub processed_dimensions: [Cell<StyleSizeLength>; 2],

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
            line_index: Cell::new(0),
            processed_dimensions: [
                Cell::new(StyleLength::UNDEFINED),
                Cell::new(StyleLength::UNDEFINED),
            ],
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

impl LayoutResults {
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
        self.computed_flex_basis.set(other.computed_flex_basis.get());
        self.computed_auto_min_main_size
            .set(other.computed_auto_min_main_size.get());
        self.generation_count.set(other.generation_count.get());
        self.config_version.set(other.config_version.get());
        self.last_owner_direction.set(other.last_owner_direction.get());
        self.next_cached_measurements_index
            .set(other.next_cached_measurements_index.get());
        for (a, b) in self.cached_measurements.iter().zip(&other.cached_measurements) {
            a.set(b.get());
        }
        self.cached_layout.set(other.cached_layout.get());
        self.line_index.set(other.line_index.get());
        self.processed_dimensions[0].set(other.processed_dimensions[0].get());
        self.processed_dimensions[1].set(other.processed_dimensions[1].get());
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
