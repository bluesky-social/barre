//! barre's per-node state, embedded in every `yoga::Node` as
//! `Node::barreState_` (`BARRE_RN_STATE_SIZE` bytes, zeroed in a new node):
//! the header C++ also writes to, snapshots of the node's style and config
//! in barre's representation, and barre's layout results and caches.
//!
//! The snapshots are refreshed lazily, when a node is visited and C++ has
//! flagged a change since (`pending`: any mutable access to the style,
//! `setConfig`) or its config's `version_` moved. Each node is checked once
//! per layout pass (`pass`); every later visit in the pass is one compare.
//! Only a changed node calls back into C++ (`flatten_style`, `read_config`).

use std::ffi::c_void;
use std::mem::{align_of, size_of};
use std::ptr::{addr_of, addr_of_mut};

use barre::Config;
use barre::enums::*;
use barre::node::layout_results::LayoutResults;
use barre::style::{Style, StyleLength};

use crate::host::{
    FlatConfig, FlatStyle, Length, PENDING_CONFIG, PENDING_FLEX_BASIS_RESET, PENDING_LAYOUT_RESET,
    PENDING_STYLE, STATE_ALIGN, STATE_SIZE, StateHeader,
};
use crate::{host, off, read};

/// The state blob. `repr(C)` so the header comes first (C++ reads and writes
/// it) and the layout results' offset is fixed (C++ reads them).
#[repr(C, align(16))]
pub struct State {
    pub header: StateHeader,
    pub config: Config,
    pub style: Style,
    pub layout: LayoutResults,
}

const _: () = assert!(size_of::<State>() <= STATE_SIZE);
const _: () = assert!(align_of::<State>() <= STATE_ALIGN);
// The blob is copied with memcpy and never dropped.
const _: () = assert!(!std::mem::needs_drop::<State>());

/// The errata barre lays out with for a config whose `errata_` is `errata`.
///
/// RN 0.88's Yoga knows errata bits 1|2|4|8 only, and always behaves as
/// barre does with `FlexFirstPassUsesRunningTotals` (16) set: that bit
/// preserves the pre-yoga#2006 behaviour, which is all RN 0.88 has. So keep
/// RN's known bits and force 16 on, whatever RN passed (it passes
/// `YGErrataAll` or `YGErrataNone`, whose meaning in RN 0.88 never included
/// the fix).
pub fn rn_errata(errata: u32) -> Errata {
    Errata((errata & 0xF) | Errata::FLEX_FIRST_PASS_USES_RUNNING_TOTALS.0)
}

/// The state of `node` (computed, not dereferenced).
#[inline(always)]
pub fn state_ptr(node: *mut c_void) -> *mut State {
    node.wrapping_byte_add(off().node_state) as *mut State
}

/// Makes `node`'s state current for layout pass `pass`: initialised,
/// pending resets applied, snapshots up to date.
///
/// # Safety
/// `node` is a live `yoga::Node` that no other thread is mutating.
#[inline(always)]
pub unsafe fn make_fresh(node: *mut c_void, pass: u32) {
    let s = state_ptr(node);
    // SAFETY: the state is inside the live node.
    unsafe {
        let header = addr_of!((*s).header);
        if (*header).pass != pass || (*header).pending != 0 {
            check(node, s, pass);
        }
    }
}

/// The once-per-pass check.
#[inline(never)]
unsafe fn check(node: *mut c_void, s: *mut State, pass: u32) {
    // SAFETY: the host's offsets describe yoga::Node and yoga::Config
    // (checked at init); `node` is live, and so is its config.
    unsafe {
        let header = addr_of_mut!((*s).header);
        let config = read::<*const c_void>(node, off().node_config);
        let config_version = read::<u32>(config, off().config_version);
        if (*header).initialized == 0
            || (*header).pending != 0
            || (*header).config != config
            || (*header).config_version != config_version
        {
            refresh(node, s, config, config_version);
        }
        (*header).pass = pass;
    }
}

#[cold]
#[inline(never)]
unsafe fn refresh(node: *mut c_void, s: *mut State, config: *const c_void, config_version: u32) {
    let h = host();
    // SAFETY: as for `check`. Nothing borrows this node's snapshots while it
    // is refreshed: a node is refreshed when first visited in a pass, before
    // the algorithm reads it.
    unsafe {
        let header = addr_of_mut!((*s).header);
        let initialized = (*header).initialized != 0;
        let pending = if initialized { (*header).pending } else { 0 };
        if !initialized {
            addr_of_mut!((*s).config).write(Config::new());
            addr_of_mut!((*s).layout).write(LayoutResults::default());
        } else {
            let layout = &*addr_of!((*s).layout);
            if pending & PENDING_LAYOUT_RESET != 0 {
                layout.reset();
            }
            if pending & PENDING_FLEX_BASIS_RESET != 0 {
                layout.computed_flex_basis.set(f32::NAN);
            }
        }
        if !initialized || pending & PENDING_STYLE != 0 {
            let mut flat = std::mem::MaybeUninit::<FlatStyle>::uninit();
            (h.flatten_style)(node, flat.as_mut_ptr());
            let style = addr_of_mut!((*s).style);
            if !initialized {
                style.write(Style::new());
            }
            fill_style(&mut *style, &flat.assume_init());
        }
        if !initialized
            || pending & PENDING_CONFIG != 0
            || (*header).config != config
            || (*header).config_version != config_version
        {
            let mut flat = FlatConfig::default();
            (h.read_config)(config, &mut flat);
            sync_config(&mut *addr_of_mut!((*s).config), &flat);
        }
        header.write(StateHeader {
            initialized: 1,
            pending: 0,
            pass: 0,
            config_version,
            config,
        });
    }
}

/// Applies `flat` to the config snapshot through its setters, so the
/// snapshot's version (which barre's caches are keyed on) changes exactly
/// when a setting that affects layout does. That is what Yoga's
/// `Node::setConfig` arranges with `configUpdateInvalidatesLayout`.
fn sync_config(config: &mut Config, flat: &FlatConfig) {
    config.set_errata(rn_errata(flat.errata));
    for feature in ExperimentalFeature::ALL {
        config.set_experimental_feature_enabled(
            *feature,
            flat.experimental_features & (1 << (*feature as u32)) != 0,
        );
    }
    config.set_point_scale_factor(flat.point_scale_factor);
    config.set_use_web_defaults(flat.use_web_defaults);
}

#[inline]
fn enum_value<T>(raw: u8, from_raw: fn(u32) -> Option<T>, what: &str) -> T {
    match from_raw(raw as u32) {
        Some(value) => value,
        None => panic!("barre-rn: invalid {what} {raw} in a yoga::Style"),
    }
}

fn length(l: Length) -> StyleLength {
    match enum_value(l.unit, Unit::from_raw, "unit") {
        Unit::Undefined => StyleLength::UNDEFINED,
        Unit::Point => StyleLength::points(l.value),
        Unit::Percent => StyleLength::percent(l.value),
        Unit::Auto => StyleLength::AUTO,
        Unit::MaxContent => StyleLength::MAX_CONTENT,
        Unit::FitContent => StyleLength::FIT_CONTENT,
        Unit::Stretch => {
            if l.value.is_nan() {
                StyleLength::STRETCH
            } else {
                StyleLength::stretch(l.value)
            }
        }
    }
}

/// Sets `s` to a flattened `yoga::Style`, in place.
fn fill_style(s: &mut Style, f: &FlatStyle) {
    *s = Style::new();
    s.set_direction(enum_value(f.direction, Direction::from_raw, "direction"));
    s.set_flex_direction(enum_value(
        f.flex_direction,
        FlexDirection::from_raw,
        "flexDirection",
    ));
    s.set_justify_content(enum_value(
        f.justify_content,
        Justify::from_raw,
        "justifyContent",
    ));
    s.set_justify_items(enum_value(
        f.justify_items,
        Justify::from_raw,
        "justifyItems",
    ));
    s.set_justify_self(enum_value(f.justify_self, Justify::from_raw, "justifySelf"));
    s.set_align_content(enum_value(f.align_content, Align::from_raw, "alignContent"));
    s.set_align_items(enum_value(f.align_items, Align::from_raw, "alignItems"));
    s.set_align_self(enum_value(f.align_self, Align::from_raw, "alignSelf"));
    s.set_position_type(enum_value(
        f.position_type,
        PositionType::from_raw,
        "positionType",
    ));
    s.set_flex_wrap(enum_value(f.flex_wrap, Wrap::from_raw, "flexWrap"));
    s.set_overflow(enum_value(f.overflow, Overflow::from_raw, "overflow"));
    s.set_display(enum_value(f.display, Display::from_raw, "display"));
    s.set_box_sizing(enum_value(f.box_sizing, BoxSizing::from_raw, "boxSizing"));
    s.set_flex(f.flex);
    s.set_flex_grow(f.flex_grow);
    s.set_flex_shrink(f.flex_shrink);
    s.set_aspect_ratio(f.aspect_ratio);
    s.set_flex_basis(length(f.flex_basis));
    for edge in Edge::ALL {
        let i = *edge as usize;
        let bit = 1u16 << i;
        if f.defined_margin & bit != 0 {
            s.set_margin(*edge, length(f.margin[i]));
        }
        if f.defined_position & bit != 0 {
            s.set_position(*edge, length(f.position[i]));
        }
        if f.defined_padding & bit != 0 {
            s.set_padding(*edge, length(f.padding[i]));
        }
        if f.defined_border & bit != 0 {
            s.set_border(*edge, length(f.border[i]));
        }
    }
    for gutter in Gutter::ALL {
        let value = length(f.gap[*gutter as usize]);
        if value.is_defined() {
            s.set_gap(*gutter, value);
        }
    }
    for dim in [Dimension::Width, Dimension::Height] {
        let i = dim as usize;
        s.set_dimension(dim, length(f.dimensions[i]));
        s.set_min_dimension(dim, length(f.min_dimensions[i]));
        s.set_max_dimension(dim, length(f.max_dimensions[i]));
    }
}
