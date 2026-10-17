//! barre as React Native's layout engine (docs/RN-SHIM.md).
//!
//! React Native keeps Yoga's C++ `yoga::Node`, `yoga::Style`, `yoga::Config`
//! and the whole `YG*` C API; a patch to its `ReactCommon/yoga`
//! (`rn/patches/`) replaces only `yoga::calculateLayout`, which calls
//! [`barre_rn_calculate_layout`] when barre is enabled for the process.
//! barre's algorithm walks the C++ nodes through [`node::RnNode`], an
//! implementation of barre's `LayoutNode` over a small host interface
//! ([`host::Host`], `yoga/barre/BarreRn.h` on the C++ side).
//!
//! This staticlib exports only `barre_rn_*` symbols (no `YG*`, so it links
//! next to Yoga's C API). Panics abort: every export is `extern "C"`.

use std::cell::UnsafeCell;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicPtr, AtomicU32, Ordering};

use barre::enums::Direction;
use barre::node::layout_results::LayoutResults;

pub mod host;
pub mod node;
pub mod state;

use host::{Host, LayoutOffsets};
use state::State;

static HOST: AtomicPtr<Host> = AtomicPtr::new(std::ptr::null_mut());

/// The host passed to [`barre_rn_init`] (for its calls).
#[inline(always)]
pub(crate) fn host() -> &'static Host {
    // SAFETY: set once by `barre_rn_init` before any layout (the C++ side
    // only enables barre after init succeeded) to a host that lives for the
    // process (a static in the C++ library).
    unsafe { &*HOST.load(Ordering::Relaxed) }
}

/// The host's offsets the hot path reads, copied into a plain static so each
/// is a single load from a fixed address.
#[derive(Copy, Clone)]
pub(crate) struct Offsets {
    pub node_state: usize,
    pub node_children: u32,
    pub node_owner: u32,
    pub node_config: u32,
    pub node_contents_children_count: u32,
    pub node_measure_func: u32,
    pub node_min_content_measure_func: u32,
    pub node_baseline_func: u32,
    pub node_min_content_width: u32,
    pub node_min_content_height: u32,
    pub flag_dirty: host::Flag,
    pub flag_has_new_layout: host::Flag,
    pub flag_reference_baseline: host::Flag,
    pub flag_always_forms_containing_block: host::Flag,
    pub flag_node_type_text: host::Flag,
    pub config_version: u32,
}

struct OffsetsCell(UnsafeCell<Offsets>);
// SAFETY: written once by `barre_rn_init`, before any layout reads it (the
// C++ side enables barre only after init returned, behind a function-local
// static's guard, which orders the write before every read).
unsafe impl Sync for OffsetsCell {}

const NO_FLAG: host::Flag = host::Flag { offset: 0, mask: 0 };
static OFFSETS: OffsetsCell = OffsetsCell(UnsafeCell::new(Offsets {
    node_state: 0,
    node_children: 0,
    node_owner: 0,
    node_config: 0,
    node_contents_children_count: 0,
    node_measure_func: 0,
    node_min_content_measure_func: 0,
    node_baseline_func: 0,
    node_min_content_width: 0,
    node_min_content_height: 0,
    flag_dirty: NO_FLAG,
    flag_has_new_layout: NO_FLAG,
    flag_reference_baseline: NO_FLAG,
    flag_always_forms_containing_block: NO_FLAG,
    flag_node_type_text: NO_FLAG,
    config_version: 0,
}));

#[inline(always)]
pub(crate) fn off() -> &'static Offsets {
    // SAFETY: see `OffsetsCell`.
    unsafe { &*OFFSETS.0.get() }
}

/// Layout pass ids (`StateHeader::pass`); 0 is never a pass.
static PASS: AtomicU32 = AtomicU32::new(0);

/// Reads a `T` at byte `offset` of `base`.
///
/// # Safety
/// `base + offset` holds a valid, aligned `T`.
#[inline(always)]
pub(crate) unsafe fn read<T: Copy>(base: *const c_void, offset: u32) -> T {
    // SAFETY: guaranteed by the caller.
    unsafe { *(base.byte_add(offset as usize) as *const T) }
}

/// `barre_rn_init`: validates and stores the host.
///
/// # Safety
/// `host` points to a `BarreRnHost` that lives for the rest of the process.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn barre_rn_init(host: *const Host) -> bool {
    if host.is_null() {
        return false;
    }
    // SAFETY: guaranteed by the caller; the first two fields are read before
    // trusting the rest of the layout.
    let h = unsafe { &*host };
    if h.abi_version != host::ABI_VERSION
        || h.struct_size as usize != std::mem::size_of::<Host>()
        || (h.state_capacity as usize) < std::mem::size_of::<State>()
        || h.state_capacity as usize != host::STATE_SIZE
        || !(h.node_state as usize).is_multiple_of(host::STATE_ALIGN)
    {
        return false;
    }
    // SAFETY: init runs before any layout (see `OffsetsCell`).
    unsafe {
        *OFFSETS.0.get() = Offsets {
            node_state: h.node_state as usize,
            node_children: h.node_children,
            node_owner: h.node_owner,
            node_config: h.node_config,
            node_contents_children_count: h.node_contents_children_count,
            node_measure_func: h.node_measure_func,
            node_min_content_measure_func: h.node_min_content_measure_func,
            node_baseline_func: h.node_baseline_func,
            node_min_content_width: h.node_min_content_width,
            node_min_content_height: h.node_min_content_height,
            flag_dirty: h.flag_dirty,
            flag_has_new_layout: h.flag_has_new_layout,
            flag_reference_baseline: h.flag_reference_baseline,
            flag_always_forms_containing_block: h.flag_always_forms_containing_block,
            flag_node_type_text: h.flag_node_type_text,
            config_version: h.config_version,
        };
    }
    HOST.store(host as *mut Host, Ordering::Release);
    true
}

/// `barre_rn_state_size`.
#[unsafe(no_mangle)]
pub extern "C" fn barre_rn_state_size() -> usize {
    std::mem::size_of::<State>()
}

/// `barre_rn_layout_offsets`.
#[unsafe(no_mangle)]
pub extern "C" fn barre_rn_layout_offsets() -> LayoutOffsets {
    let base = std::mem::offset_of!(State, layout);
    let o = LayoutResults::OFFSETS;
    let at = |field: usize| (base + field) as u32;
    LayoutOffsets {
        direction: at(o.direction),
        had_overflow: at(o.had_overflow),
        dimensions: at(o.dimensions),
        raw_dimensions: at(o.raw_dimensions),
        position: at(o.position),
        margin: at(o.margin),
        border: at(o.border),
        padding: at(o.padding),
    }
}

/// `barre_rn_calculate_layout`: `yoga::calculateLayout`, by barre.
///
/// # Safety
/// `node` is a live `yoga::Node` whose tree no other thread mutates during
/// the call, and [`barre_rn_init`] succeeded.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn barre_rn_calculate_layout(
    node: *mut c_void,
    owner_width: f32,
    owner_height: f32,
    owner_direction: u8,
) {
    let direction = Direction::from_raw(owner_direction as u32)
        .unwrap_or_else(|| panic!("barre-rn: invalid owner direction {owner_direction}"));
    let node = NonNull::new(node).expect("barre-rn: null node");
    let mut pass = PASS.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
    if pass == 0 {
        pass = PASS.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
    }
    // SAFETY: guaranteed by the caller.
    let root = unsafe { node::RnNode::new(node, pass) };
    barre::algorithm::calculate_layout(root, owner_width, owner_height, direction);
}
