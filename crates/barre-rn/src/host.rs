//! Mirrors of the C definitions in React Native's patched
//! `ReactCommon/yoga/yoga/barre/BarreRn.h` (`rn/patches/`). Keep the two in
//! sync: `barre_rn_init` rejects a host whose `abi_version` or `struct_size`
//! differs.

use std::ffi::c_void;

pub const ABI_VERSION: u32 = 2;
/// `BARRE_RN_STATE_SIZE`: bytes reserved in every `yoga::Node`.
pub const STATE_SIZE: usize = 1280;
/// `BARRE_RN_STATE_ALIGN`.
pub const STATE_ALIGN: usize = 16;

pub const PENDING_LAYOUT_RESET: u32 = 1;
pub const PENDING_FLEX_BASIS_RESET: u32 = 2;
pub const PENDING_STYLE: u32 = 4;
pub const PENDING_CONFIG: u32 = 8;

/// `BarreRnStateHeader`.
#[repr(C)]
pub struct StateHeader {
    pub initialized: u32,
    pub pending: u32,
    pub pass: u32,
    pub config_version: u32,
    pub config: *const c_void,
}

/// `BarreRnLength`.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Length {
    pub value: f32,
    pub unit: u8,
}

/// `BarreRnStyle`.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct FlatStyle {
    pub direction: u8,
    pub flex_direction: u8,
    pub justify_content: u8,
    pub justify_items: u8,
    pub justify_self: u8,
    pub align_content: u8,
    pub align_items: u8,
    pub align_self: u8,
    pub position_type: u8,
    pub flex_wrap: u8,
    pub overflow: u8,
    pub display: u8,
    pub box_sizing: u8,
    pub flex: f32,
    pub flex_grow: f32,
    pub flex_shrink: f32,
    pub aspect_ratio: f32,
    pub flex_basis: Length,
    pub defined_margin: u16,
    pub defined_position: u16,
    pub defined_padding: u16,
    pub defined_border: u16,
    pub margin: [Length; 9],
    pub position: [Length; 9],
    pub padding: [Length; 9],
    pub border: [Length; 9],
    pub gap: [Length; 3],
    pub dimensions: [Length; 2],
    pub min_dimensions: [Length; 2],
    pub max_dimensions: [Length; 2],
}

/// `BarreRnConfig`.
#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct FlatConfig {
    pub errata: u32,
    pub experimental_features: u32,
    pub point_scale_factor: f32,
    pub use_web_defaults: bool,
}

/// `BarreRnSize`.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct HostSize {
    pub width: f32,
    pub height: f32,
}

/// `BarreRnFlag`.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Flag {
    pub offset: u32,
    pub mask: u8,
}

pub type MeasureFn = unsafe extern "C" fn(*mut c_void, f32, u8, f32, u8) -> HostSize;

/// `BarreRnHost`.
#[repr(C)]
pub struct Host {
    pub abi_version: u32,
    pub struct_size: u32,
    pub state_capacity: u32,

    pub node_state: u32,
    pub node_children: u32,
    pub node_owner: u32,
    pub node_config: u32,
    pub node_contents_children_count: u32,
    pub node_measure_func: u32,
    pub node_min_content_measure_func: u32,
    pub node_baseline_func: u32,
    pub node_min_content_width: u32,
    pub node_min_content_height: u32,
    pub flag_dirty: Flag,
    pub flag_has_new_layout: Flag,
    pub flag_reference_baseline: Flag,
    pub flag_always_forms_containing_block: Flag,
    pub flag_node_type_text: Flag,
    pub config_version: u32,

    pub flatten_style: unsafe extern "C" fn(*const c_void, *mut FlatStyle),
    pub read_config: unsafe extern "C" fn(*const c_void, *mut FlatConfig),
    pub mark_dirty: unsafe extern "C" fn(*mut c_void),
    pub clone_children_if_needed: unsafe extern "C" fn(*mut c_void),
    pub clone_contents_children_if_needed: unsafe extern "C" fn(*mut c_void),
    pub measure: MeasureFn,
    pub measure_min_content: MeasureFn,
    pub baseline: unsafe extern "C" fn(*mut c_void, f32, f32) -> f32,
}

/// `BarreRnLayoutOffsets`.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct LayoutOffsets {
    pub direction: u32,
    pub had_overflow: u32,
    pub dimensions: u32,
    pub raw_dimensions: u32,
    pub position: u32,
    pub margin: u32,
    pub border: u32,
    pub padding: u32,
}
