//! [`LayoutNode`] over React Native's C++ `yoga::Node`s.
//!
//! Every read on the hot path is a plain load from the C++ node at an offset
//! the host computed at init (children, owner, flags, function pointers) or
//! from barre's state blob inside it (style and config snapshots, layout
//! results). Calls into C++ are only what Yoga itself calls out for: measure,
//! baseline, the clone callback (`cloneChildrenIfNeeded`), the dirtied
//! callback, plus re-snapshotting a style or config that changed.

use std::ffi::c_void;
use std::marker::PhantomData;
use std::ptr::NonNull;

use barre::enums::{MeasureMode, NodeType};
use barre::node::layout_node::LayoutNode;
use barre::node::layout_results::LayoutResults;
use barre::style::Style;
use barre::{Config, Size};

use crate::host::Flag;
use crate::state::{State, make_fresh, state_ptr};
use crate::{host, off, read};

/// A `yoga::Node*` during layout pass `pass` (lifetime `'a`).
///
/// `fresh` is true when the node's state was made current when the handle
/// was made (the root and every `child(i)`). Owner handles are not checked
/// up front: the algorithm mostly only compares them, and RN stamps stale
/// owner pointers with a junk sentinel that must not be dereferenced. They
/// are checked on first real use.
///
/// The handle holds the node's state pointer rather than the node pointer:
/// style and layout reads (most of the algorithm's accesses) are then a
/// single offset from it.
#[derive(Copy, Clone)]
pub struct RnNode<'a> {
    state: NonNull<State>,
    pass: u32,
    fresh: bool,
    _pass: PhantomData<&'a ()>,
}

impl<'a> RnNode<'a> {
    /// The handle for a node the algorithm will read.
    ///
    /// # Safety
    /// `node` is a live `yoga::Node` that stays alive and is not mutated by
    /// other threads for `'a`.
    #[inline(always)]
    pub unsafe fn new(node: NonNull<c_void>, pass: u32) -> Self {
        // SAFETY: guaranteed by the caller.
        unsafe { make_fresh(node.as_ptr(), pass) };
        Self::handle(node, pass, true)
    }

    #[inline(always)]
    fn handle(node: NonNull<c_void>, pass: u32, fresh: bool) -> Self {
        RnNode {
            // SAFETY: inside a non-null node.
            state: unsafe { NonNull::new_unchecked(state_ptr(node.as_ptr())) },
            pass,
            fresh,
            _pass: PhantomData,
        }
    }

    /// The `yoga::Node*`.
    #[inline(always)]
    fn ptr(self) -> *mut c_void {
        self.state.as_ptr().wrapping_byte_sub(off().node_state) as *mut c_void
    }

    #[inline(always)]
    fn state(self) -> &'a State {
        if !self.fresh {
            // SAFETY: an owner handle being used for real is a live ancestor
            // of a node being laid out.
            unsafe { make_fresh(self.ptr(), self.pass) };
        }
        // SAFETY: current (above or when the handle was made); lives as
        // long as the node.
        unsafe { self.state.as_ref() }
    }

    #[inline(always)]
    fn flag(self, flag: Flag) -> bool {
        // SAFETY: the flag's byte is inside the node.
        unsafe { read::<u8>(self.ptr(), flag.offset) & flag.mask != 0 }
    }

    #[inline(always)]
    fn set_flag(self, flag: Flag, on: bool) {
        // SAFETY: the flag's byte is inside the node; layout has exclusive
        // access to the nodes it lays out.
        unsafe {
            let byte = self.ptr().byte_add(flag.offset as usize) as *mut u8;
            if on {
                *byte |= flag.mask;
            } else {
                *byte &= !flag.mask;
            }
        }
    }

    /// `children_`'s `{begin, end}`.
    #[inline(always)]
    fn children(self) -> (*const *mut c_void, usize) {
        // SAFETY: `node_children` is the offset of a libc++
        // `std::vector<Node*>`, laid out as {begin, end, capacity}
        // (checked at init).
        unsafe {
            let words =
                self.ptr().byte_add(off().node_children as usize) as *const *const *mut c_void;
            let begin = *words;
            let end = *words.add(1);
            (begin, end.offset_from_unsigned(begin))
        }
    }

    #[inline(always)]
    fn has_fn(self, offset: u32) -> bool {
        // SAFETY: a function pointer field of the node.
        unsafe { !read::<*const c_void>(self.ptr(), offset).is_null() }
    }
}

impl<'a> LayoutNode<'a> for RnNode<'a> {
    #[inline(always)]
    fn style(self) -> &'a Style {
        &self.state().style
    }
    #[inline(always)]
    fn layout(self) -> &'a LayoutResults {
        &self.state().layout
    }
    #[inline(always)]
    fn config(self) -> &'a Config {
        &self.state().config
    }

    #[inline(always)]
    fn child_count(self) -> usize {
        self.children().1
    }
    #[inline(always)]
    fn child(self, index: usize) -> Self {
        let (begin, len) = self.children();
        assert!(index < len, "child index out of range");
        // SAFETY: in range; children of a node being laid out are live.
        unsafe { RnNode::new(NonNull::new_unchecked(*begin.add(index)), self.pass) }
    }
    #[inline(always)]
    fn owner(self) -> Option<Self> {
        // SAFETY: `owner_` field.
        let owner = unsafe { read::<*mut c_void>(self.ptr(), off().node_owner) };
        NonNull::new(owner).map(|node| Self::handle(node, self.pass, false))
    }
    #[inline(always)]
    fn is_same(self, other: Self) -> bool {
        self.state == other.state
    }
    #[inline(always)]
    fn has_contents_children(self) -> bool {
        // SAFETY: `contentsChildrenCount_` (size_t).
        unsafe { read::<usize>(self.ptr(), off().node_contents_children_count) != 0 }
    }
    #[inline]
    fn clone_children_if_needed(self) {
        // Yoga's cloneChildrenIfNeeded only acts on children owned by
        // another node; skip the call when there are none.
        let (begin, len) = self.children();
        let owner_offset = off().node_owner;
        // SAFETY: children of a node being laid out are live.
        let needed = (0..len)
            .any(|i| unsafe { read::<*mut c_void>(*begin.add(i), owner_offset) } != self.ptr());
        if needed {
            // SAFETY: a live node; the host calls Node::cloneChildrenIfNeeded.
            unsafe { (host().clone_children_if_needed)(self.ptr()) };
        }
    }
    #[inline]
    fn clone_contents_children_if_needed(self) {
        // SAFETY: a live node.
        unsafe { (host().clone_contents_children_if_needed)(self.ptr()) };
    }

    #[inline(always)]
    fn is_dirty(self) -> bool {
        self.flag(off().flag_dirty)
    }
    #[inline]
    fn set_dirty(self, dirty: bool) {
        if dirty {
            if !self.is_dirty() {
                // Node::setDirty(true) also runs the dirtied callback.
                // SAFETY: a live node.
                unsafe { (host().mark_dirty)(self.ptr()) };
            }
        } else {
            self.set_flag(off().flag_dirty, false);
        }
    }
    #[inline(always)]
    fn has_new_layout(self) -> bool {
        self.flag(off().flag_has_new_layout)
    }
    #[inline(always)]
    fn set_has_new_layout(self, has_new_layout: bool) {
        self.set_flag(off().flag_has_new_layout, has_new_layout);
    }

    #[inline(always)]
    fn node_type(self) -> NodeType {
        if self.flag(off().flag_node_type_text) {
            NodeType::Text
        } else {
            NodeType::Default
        }
    }
    #[inline(always)]
    fn is_reference_baseline(self) -> bool {
        self.flag(off().flag_reference_baseline)
    }
    #[inline(always)]
    fn always_forms_containing_block(self) -> bool {
        self.flag(off().flag_always_forms_containing_block)
    }

    #[inline(always)]
    fn has_measure_func(self) -> bool {
        self.has_fn(off().node_measure_func)
    }
    #[inline]
    fn measure(
        self,
        available_width: f32,
        width_mode: MeasureMode,
        available_height: f32,
        height_mode: MeasureMode,
    ) -> Size {
        // SAFETY: a live node with a measure function; Node::measure.
        let size = unsafe {
            (host().measure)(
                self.ptr(),
                available_width,
                width_mode as u8,
                available_height,
                height_mode as u8,
            )
        };
        Size {
            width: size.width,
            height: size.height,
        }
    }
    #[inline(always)]
    fn has_min_content_measure_func(self) -> bool {
        self.has_fn(off().node_min_content_measure_func)
    }
    #[inline]
    fn measure_min_content(
        self,
        available_width: f32,
        width_mode: MeasureMode,
        available_height: f32,
        height_mode: MeasureMode,
    ) -> Size {
        // SAFETY: a live node; Node::measureMinContent.
        let size = unsafe {
            (host().measure_min_content)(
                self.ptr(),
                available_width,
                width_mode as u8,
                available_height,
                height_mode as u8,
            )
        };
        Size {
            width: size.width,
            height: size.height,
        }
    }
    #[inline]
    fn min_content_width(self) -> f32 {
        // SAFETY: `minContentWidth_` (FloatOptional: a float, NaN = unset).
        unsafe { read::<f32>(self.ptr(), off().node_min_content_width) }
    }
    #[inline]
    fn min_content_height(self) -> f32 {
        // SAFETY: `minContentHeight_`.
        unsafe { read::<f32>(self.ptr(), off().node_min_content_height) }
    }
    #[inline(always)]
    fn has_baseline_func(self) -> bool {
        self.has_fn(off().node_baseline_func)
    }
    #[inline]
    fn baseline(self, width: f32, height: f32) -> f32 {
        // SAFETY: a live node with a baseline function; Node::baseline.
        unsafe { (host().baseline)(self.ptr(), width, height) }
    }
}
