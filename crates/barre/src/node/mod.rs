//! Layout nodes (`yoga/node/Node.h`, `Node.cpp`).
//!
//! # Storage and handles
//!
//! Each node is its own heap allocation (`Box<Node>`), and the C ABI's
//! `YGNodeRef` is a pointer to it. Nodes are created and freed individually,
//! can move between trees and can be shared between trees (persistent
//! cloning), and a tree can mix nodes from several configs, so there is no
//! tree-level owner an arena could belong to. See `docs/API.md`.
//!
//! Every field of a node is interior-mutable (`Cell`, or `UnsafeCell` behind
//! narrow accessors), and the engine only ever works with shared `&Node`s.
//! Layout writes results through `&Node`, and callbacks (measure, baseline,
//! dirtied, clone) may read any node while the engine holds references to
//! it, without violating Rust's aliasing rules.
//!
//! Liveness: a node stays alive until it is freed with
//! [`tree::node_free`] (or `Box::from_raw` of its pointer). The engine assumes
//! that a node reachable from a node it is working on (a child, an owner)
//! is alive, which is the C API's contract too: freeing a node that is still
//! in use is undefined behaviour in Yoga as well.

pub mod children;
pub mod layout_node;
pub mod layout_results;
pub mod layoutable_children;
pub mod tree;

use std::cell::{Cell, UnsafeCell};
use std::ffi::c_void;
use std::ptr::NonNull;

use crate::config::{Config, config_update_invalidates_layout};
use crate::debug::assert_fatal::{FatalError, assert_fatal_with_config, assert_fatal_with_node};
use crate::debug::log::log;
use crate::enums::*;
use crate::numeric::{is_undefined, max_or_defined};
use crate::style::{GridStyle, Style};

use self::children::Children;
pub use self::layout_node::{LayoutNode, NodeExt};
pub use self::layout_results::{CachedMeasurement, LayoutResults};
pub use self::layoutable_children::LayoutableChildren;

/// `YGSize`.
#[derive(Copy, Clone, Debug, PartialEq, Default)]
#[repr(C)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

/// `YGMeasureFunc` (and `YGMinContentMeasureFunc`, which has the same
/// signature). The modes are `YGMeasureMode` values.
pub type MeasureFunc = unsafe extern "C-unwind" fn(
    node: *const Node,
    width: f32,
    width_mode: u32,
    height: f32,
    height_mode: u32,
) -> Size;

/// `YGBaselineFunc`.
pub type BaselineFunc =
    unsafe extern "C-unwind" fn(node: *const Node, width: f32, height: f32) -> f32;

/// `YGDirtiedFunc`.
pub type DirtiedFunc = unsafe extern "C-unwind" fn(node: *const Node);

/// A pointer to a live node (the C ABI's `YGNodeRef`).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct NodePtr(NonNull<Node>);

impl NodePtr {
    /// `None` for null.
    #[inline]
    pub fn from_raw(ptr: *mut Node) -> Option<NodePtr> {
        NonNull::new(ptr).map(NodePtr)
    }

    #[inline]
    pub fn from_ref(node: &Node) -> NodePtr {
        NodePtr(NonNull::from(node))
    }

    #[inline]
    pub fn as_ptr(self) -> *mut Node {
        self.0.as_ptr()
    }

    /// Borrows the node.
    ///
    /// # Safety
    /// The node must be alive for `'a` (see the module docs).
    #[inline]
    pub unsafe fn get<'a>(self) -> &'a Node {
        // SAFETY: guaranteed by the caller.
        unsafe { self.0.as_ref() }
    }

    /// Borrows a node that the engine knows to be alive because it is
    /// reachable from a node in use (the module-level liveness invariant).
    #[inline]
    pub(crate) fn live<'a>(self) -> &'a Node {
        // SAFETY: the liveness invariant in the module docs.
        unsafe { self.0.as_ref() }
    }
}

/// `yoga::Node`.
pub struct Node {
    has_new_layout: Cell<bool>,
    is_reference_baseline: Cell<bool>,
    is_dirty: Cell<bool>,
    always_forms_containing_block: Cell<bool>,
    node_type: Cell<NodeType>,
    context: Cell<*mut c_void>,
    measure_func: Cell<Option<MeasureFunc>>,
    min_content_measure_func: Cell<Option<MeasureFunc>>,
    min_content_width: Cell<f32>,
    min_content_height: Cell<f32>,
    baseline_func: Cell<Option<BaselineFunc>>,
    dirtied_func: Cell<Option<DirtiedFunc>>,
    /// Only mutated through [`Node::style_mut`].
    style: UnsafeCell<Style>,
    layout: LayoutResults,
    /// Grid properties, allocated only when one of them is set
    /// (`GridStyleStorage`).
    grid: UnsafeCell<Option<Box<GridStyle>>>,
    contents_children_count: Cell<usize>,
    owner: Cell<Option<NodePtr>>,
    children: Children,
    config: Cell<NonNull<Config>>,
}

impl std::fmt::Debug for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Node")
            .field("style", self.style())
            .field("layout", &self.layout)
            .field("children", &self.children)
            .finish_non_exhaustive()
    }
}

impl Clone for Node {
    /// `Node(const Node&)`: a shallow copy. Children are shared with the
    /// original (their owner is unchanged).
    fn clone(&self) -> Node {
        Node {
            has_new_layout: self.has_new_layout.clone(),
            is_reference_baseline: self.is_reference_baseline.clone(),
            is_dirty: self.is_dirty.clone(),
            always_forms_containing_block: self.always_forms_containing_block.clone(),
            node_type: self.node_type.clone(),
            context: self.context.clone(),
            measure_func: self.measure_func.clone(),
            min_content_measure_func: self.min_content_measure_func.clone(),
            min_content_width: self.min_content_width.clone(),
            min_content_height: self.min_content_height.clone(),
            baseline_func: self.baseline_func.clone(),
            dirtied_func: self.dirtied_func.clone(),
            style: UnsafeCell::new(*self.style()),
            layout: self.layout.clone(),
            grid: UnsafeCell::new(self.grid().cloned().map(Box::new)),
            contents_children_count: self.contents_children_count.clone(),
            owner: self.owner.clone(),
            children: self.children.clone(),
            config: self.config.clone(),
        }
    }
}

impl Node {
    /// `Node(const Config*)`.
    pub fn new(config: &Config) -> Node {
        let mut style = Style::new();
        if config.use_web_defaults() {
            // useWebDefaults
            style.set_flex_direction(FlexDirection::Row);
            style.set_align_content(Align::Stretch);
        }
        Node {
            has_new_layout: Cell::new(true),
            is_reference_baseline: Cell::new(false),
            is_dirty: Cell::new(true),
            always_forms_containing_block: Cell::new(false),
            node_type: Cell::new(NodeType::Default),
            context: Cell::new(std::ptr::null_mut()),
            measure_func: Cell::new(None),
            min_content_measure_func: Cell::new(None),
            min_content_width: Cell::new(f32::NAN),
            min_content_height: Cell::new(f32::NAN),
            baseline_func: Cell::new(None),
            dirtied_func: Cell::new(None),
            style: UnsafeCell::new(style),
            layout: LayoutResults::default(),
            grid: UnsafeCell::new(None),
            contents_children_count: Cell::new(0),
            owner: Cell::new(None),
            children: Children::default(),
            config: Cell::new(NonNull::from(config)),
        }
    }

    // Getters

    #[inline]
    pub fn context(&self) -> *mut c_void {
        self.context.get()
    }
    #[inline]
    pub fn always_forms_containing_block(&self) -> bool {
        self.always_forms_containing_block.get()
    }
    #[inline]
    pub fn has_new_layout(&self) -> bool {
        self.has_new_layout.get()
    }
    #[inline]
    pub fn node_type(&self) -> NodeType {
        self.node_type.get()
    }
    #[inline]
    pub fn has_measure_func(&self) -> bool {
        self.measure_func.get().is_some()
    }
    #[inline]
    pub fn measure_func(&self) -> Option<MeasureFunc> {
        self.measure_func.get()
    }

    /// `Node::measure`: calls the measure function. Invalid results (NaN or
    /// negative) are logged and clamped to 0.
    pub fn measure(
        &self,
        available_width: f32,
        width_mode: MeasureMode,
        available_height: f32,
        height_mode: MeasureMode,
    ) -> Size {
        let func = self
            .measure_func
            .get()
            .expect("Node::measure called without a measure function");
        // SAFETY: the embedder's measure function, called with this node as
        // its `YGNodeConstRef`.
        let size = unsafe {
            func(
                self,
                available_width,
                width_mode as u32,
                available_height,
                height_mode as u32,
            )
        };
        if is_undefined(size.height)
            || size.height < 0.0
            || is_undefined(size.width)
            || size.width < 0.0
        {
            log(
                self,
                LogLevel::Warn,
                &format!(
                    "Measure function returned an invalid dimension to Yoga: [width={:.6}, height={:.6}]",
                    size.width, size.height
                ),
            );
            return Size {
                width: max_or_defined(0.0, size.width),
                height: max_or_defined(0.0, size.height),
            };
        }
        size
    }

    #[inline]
    pub fn has_min_content_measure_func(&self) -> bool {
        self.min_content_measure_func.get().is_some()
    }

    /// `Node::measureMinContent`.
    pub fn measure_min_content(
        &self,
        available_width: f32,
        width_mode: MeasureMode,
        available_height: f32,
        height_mode: MeasureMode,
    ) -> Size {
        let func = self
            .min_content_measure_func
            .get()
            .expect("Node::measureMinContent called without a min-content measure function");
        // SAFETY: as in `measure`.
        let size = unsafe {
            func(
                self,
                available_width,
                width_mode as u32,
                available_height,
                height_mode as u32,
            )
        };
        if is_undefined(size.height)
            || size.height < 0.0
            || is_undefined(size.width)
            || size.width < 0.0
        {
            log(
                self,
                LogLevel::Warn,
                &format!(
                    "Min-content measure function returned an invalid dimension to Yoga: [width={:.6}, height={:.6}]",
                    size.width, size.height
                ),
            );
            return Size {
                width: max_or_defined(0.0, size.width),
                height: max_or_defined(0.0, size.height),
            };
        }
        size
    }

    /// Undefined (NaN) when unset.
    #[inline]
    pub fn min_content_width(&self) -> f32 {
        self.min_content_width.get()
    }
    /// Undefined (NaN) when unset.
    #[inline]
    pub fn min_content_height(&self) -> f32 {
        self.min_content_height.get()
    }

    #[inline]
    pub fn has_baseline_func(&self) -> bool {
        self.baseline_func.get().is_some()
    }

    /// `Node::baseline`: calls the baseline function.
    pub fn baseline(&self, width: f32, height: f32) -> f32 {
        let func = self
            .baseline_func
            .get()
            .expect("Node::baseline called without a baseline function");
        // SAFETY: the embedder's baseline function, called with this node.
        unsafe { func(self, width, height) }
    }

    #[inline]
    pub fn has_errata(&self, errata: Errata) -> bool {
        self.config().has_errata(errata)
    }

    #[inline]
    pub fn has_contents_children(&self) -> bool {
        self.contents_children_count.get() != 0
    }

    #[inline]
    pub fn dirtied_func(&self) -> Option<DirtiedFunc> {
        self.dirtied_func.get()
    }

    #[inline]
    pub fn style(&self) -> &Style {
        // SAFETY: the style is only mutated through `style_mut`, whose
        // contract forbids live borrows of it.
        unsafe { &*self.style.get() }
    }

    /// Mutable access to the style.
    ///
    /// # Safety
    /// No other reference to this node's style may be live while the
    /// returned reference is used. In particular, styles must not be changed
    /// during layout (from a callback).
    #[inline]
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn style_mut(&self) -> &mut Style {
        // SAFETY: guaranteed by the caller.
        unsafe { &mut *self.style.get() }
    }

    /// The grid properties, if any was set (`None` means all defaults).
    #[inline]
    pub fn grid(&self) -> Option<&GridStyle> {
        // SAFETY: as for `style`.
        unsafe { (*self.grid.get()).as_deref() }
    }

    /// Mutable access to the grid properties.
    ///
    /// # Safety
    /// As for [`Node::style_mut`].
    #[inline]
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn grid_mut(&self) -> &mut Option<Box<GridStyle>> {
        // SAFETY: guaranteed by the caller.
        unsafe { &mut *self.grid.get() }
    }

    #[inline]
    pub fn layout(&self) -> &LayoutResults {
        &self.layout
    }

    #[inline]
    pub fn is_reference_baseline(&self) -> bool {
        self.is_reference_baseline.get()
    }

    /// `Node::getOwner`: the node that owns this node. An owner is used to
    /// identify the tree a node belongs to: it is the parent when the node
    /// belongs to one tree, or `None` when it is shared between trees.
    #[inline]
    pub fn owner(&self) -> Option<&Node> {
        self.owner.get().map(NodePtr::live)
    }

    #[inline]
    pub fn owner_ptr(&self) -> Option<NodePtr> {
        self.owner.get()
    }

    /// Whether `node` is this node's owner.
    #[inline]
    pub fn is_owned_by(&self, node: &Node) -> bool {
        self.owner.get() == Some(NodePtr::from_ref(node))
    }

    #[inline]
    pub fn children(&self) -> &Children {
        &self.children
    }

    /// `Node::getChild`. Panics if out of range.
    #[inline]
    pub fn child(&self, index: usize) -> &Node {
        self.children.get(index).live()
    }

    #[inline]
    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    #[inline]
    pub fn config(&self) -> &Config {
        // SAFETY: configs outlive the nodes that use them (C API contract).
        unsafe { self.config.get().as_ref() }
    }

    #[inline]
    pub fn is_dirty(&self) -> bool {
        self.is_dirty.get()
    }

    // Setters

    #[inline]
    pub fn set_context(&self, context: *mut c_void) {
        self.context.set(context);
    }
    #[inline]
    pub fn set_always_forms_containing_block(&self, value: bool) {
        self.always_forms_containing_block.set(value);
    }
    #[inline]
    pub fn set_has_new_layout(&self, has_new_layout: bool) {
        self.has_new_layout.set(has_new_layout);
    }
    #[inline]
    pub fn set_node_type(&self, node_type: NodeType) {
        self.node_type.set(node_type);
    }

    /// `Node::setMeasureFunc`. Fails if the node has children.
    pub fn set_measure_func(&self, measure_func: Option<MeasureFunc>) -> Result<(), FatalError> {
        if measure_func.is_none() {
            // TODO: t18095186 Move nodeType to opt-in function and mark
            // appropriate places in Litho
            self.set_node_type(NodeType::Default);
        } else {
            assert_fatal_with_node(
                self,
                self.children.is_empty(),
                "Cannot set measure function: Nodes with measure functions cannot have children.",
            )?;
            self.set_node_type(NodeType::Text);
        }
        self.measure_func.set(measure_func);
        Ok(())
    }

    #[inline]
    pub fn set_min_content_measure_func(&self, func: Option<MeasureFunc>) {
        self.min_content_measure_func.set(func);
    }
    #[inline]
    pub fn set_min_content_width(&self, value: f32) {
        self.min_content_width.set(value);
    }
    #[inline]
    pub fn set_min_content_height(&self, value: f32) {
        self.min_content_height.set(value);
    }
    #[inline]
    pub fn set_baseline_func(&self, func: Option<BaselineFunc>) {
        self.baseline_func.set(func);
    }
    #[inline]
    pub fn set_dirtied_func(&self, func: Option<DirtiedFunc>) {
        self.dirtied_func.set(func);
    }

    /// `Node::setStyle`.
    ///
    /// # Safety
    /// As for [`Node::style_mut`].
    pub unsafe fn set_style(&self, style: Style) {
        // SAFETY: guaranteed by the caller.
        unsafe { *self.style_mut() = style };
    }

    /// `Node::setLayout`.
    pub fn set_layout(&self, layout: &LayoutResults) {
        self.layout.copy_from(layout);
    }

    #[inline]
    pub fn set_is_reference_baseline(&self, value: bool) {
        self.is_reference_baseline.set(value);
    }
    #[inline]
    pub fn set_owner(&self, owner: Option<NodePtr>) {
        self.owner.set(owner);
    }

    /// `Node::setConfig`.
    pub fn set_config(&self, config: &Config) -> Result<(), FatalError> {
        assert_fatal_with_config(
            config,
            config.use_web_defaults() == self.config().use_web_defaults(),
            "UseWebDefaults may not be changed after constructing a Node",
        )?;
        if config_update_invalidates_layout(self.config(), config) {
            self.mark_dirty_and_propagate();
            self.layout.config_version.set(0);
        } else {
            // If the config is functionally the same, then align the
            // configVersion so that we can reuse the layout cache.
            self.layout.config_version.set(config.version());
        }
        self.config.set(NonNull::from(config));
        Ok(())
    }

    /// `Node::setDirty`: calls the dirtied function when the node becomes
    /// dirty.
    #[inline]
    pub fn set_dirty(&self, is_dirty: bool) {
        if is_dirty == self.is_dirty.get() {
            return;
        }
        self.is_dirty.set(is_dirty);
        if is_dirty && let Some(func) = self.dirtied_func.get() {
            // SAFETY: the embedder's dirtied function, called with this node.
            unsafe { func(self) };
        }
    }

    /// `Node::setChildren`.
    pub fn set_children(&self, children: Vec<NodePtr>) {
        let count = children
            .iter()
            .filter(|c| c.live().style().display() == Display::Contents)
            .count();
        self.children.replace_with(children);
        self.contents_children_count.set(count);
    }

    /// Recounts the `display: contents` children (after one of them changed
    /// its display; Yoga does this with `setChildren(getChildren())`).
    pub fn recount_contents_children(&self) {
        let count = self
            .children
            .iter()
            .filter(|c| c.live().style().display() == Display::Contents)
            .count();
        self.contents_children_count.set(count);
    }

    /// `Node::clearChildren`.
    pub fn clear_children(&self) {
        self.children.clear();
        self.contents_children_count.set(0);
    }

    /// `Node::replaceChild(Node* oldChild, Node* newChild)`: replaces the
    /// occurrences of `old_child` with `new_child`.
    pub fn replace_child(&self, old_child: NodePtr, new_child: NodePtr) {
        let old_contents = old_child.live().style().display() == Display::Contents;
        let new_contents = new_child.live().style().display() == Display::Contents;
        if old_contents && !new_contents {
            self.contents_children_count
                .set(self.contents_children_count.get() - 1);
        } else if !old_contents && new_contents {
            self.contents_children_count
                .set(self.contents_children_count.get() + 1);
        }
        self.children.replace_all(old_child, new_child);
    }

    /// `Node::replaceChild(Node* child, size_t index)`.
    pub fn replace_child_at(&self, child: NodePtr, index: usize) {
        let previous_child = self.children.get(index);
        let old_contents = previous_child.live().style().display() == Display::Contents;
        let new_contents = child.live().style().display() == Display::Contents;
        if old_contents && !new_contents {
            self.contents_children_count
                .set(self.contents_children_count.get() - 1);
        } else if !old_contents && new_contents {
            self.contents_children_count
                .set(self.contents_children_count.get() + 1);
        }
        self.children.set(index, child);
    }

    /// `Node::insertChild`.
    pub fn insert_child(&self, child: NodePtr, index: usize) {
        if child.live().style().display() == Display::Contents {
            self.contents_children_count
                .set(self.contents_children_count.get() + 1);
        }
        self.children.insert(index, child);
    }

    /// `Node::removeChild(Node*)`: removes the first occurrence of `child`.
    pub fn remove_child(&self, child: NodePtr) -> bool {
        match self.children.position(child) {
            Some(index) => {
                if child.live().style().display() == Display::Contents {
                    self.contents_children_count
                        .set(self.contents_children_count.get() - 1);
                }
                self.children.remove(index);
                true
            }
            None => false,
        }
    }

    /// `Node::removeChild(size_t)`.
    pub fn remove_child_at(&self, index: usize) {
        if self.child(index).style().display() == Display::Contents {
            self.contents_children_count
                .set(self.contents_children_count.get() - 1);
        }
        self.children.remove(index);
    }

    /// `Node::cloneChildrenIfNeeded`: replaces every child this node does not
    /// own with a clone it owns, so layout can write to it.
    pub fn clone_children_if_needed(&self) {
        let this = NodePtr::from_ref(self);
        for i in 0..self.children.len() {
            let child = self.child(i);
            if !child.is_owned_by(self) {
                let clone = self.config().clone_node(child, self, i);
                self.children.set(i, clone);
                let clone = clone.live();
                clone.set_owner(Some(this));
                if clone.style().display() == Display::Contents {
                    // The contents node's children are treated as children of
                    // the contents node's parent for layout purposes, so they
                    // need to be cloned as well.
                    clone.clone_children_if_needed();
                } else if clone.has_contents_children() {
                    clone.clone_contents_children_if_needed();
                }
            }
        }
    }

    /// `Node::cloneContentsChildrenIfNeeded`.
    pub fn clone_contents_children_if_needed(&self) {
        let this = NodePtr::from_ref(self);
        for i in 0..self.children.len() {
            let child = self.child(i);
            if child.style().display() == Display::Contents && !child.is_owned_by(self) {
                let clone = self.config().clone_node(child, self, i);
                self.children.set(i, clone);
                let clone = clone.live();
                clone.set_owner(Some(this));
                clone.clone_children_if_needed();
            }
        }
    }

    /// `Node::markDirtyAndPropagate`.
    pub fn mark_dirty_and_propagate(&self) {
        if !self.is_dirty.get() {
            self.set_dirty(true);
            self.layout.computed_flex_basis.set(f32::NAN);
            if let Some(owner) = self.owner() {
                owner.mark_dirty_and_propagate();
            }
        }
    }

    /// `Node::reset`: resets everything but the config. Fails if the node
    /// still has children or an owner.
    pub fn reset(&self) -> Result<(), FatalError> {
        assert_fatal_with_node(
            self,
            self.children.is_empty(),
            "Cannot reset a node which still has children attached",
        )?;
        assert_fatal_with_node(
            self,
            self.owner.get().is_none(),
            "Cannot reset a node still attached to a owner",
        )?;
        let fresh = Node::new(self.config());
        self.has_new_layout.set(fresh.has_new_layout.get());
        self.is_reference_baseline
            .set(fresh.is_reference_baseline.get());
        self.is_dirty.set(fresh.is_dirty.get());
        self.always_forms_containing_block
            .set(fresh.always_forms_containing_block.get());
        self.node_type.set(fresh.node_type.get());
        self.context.set(fresh.context.get());
        self.measure_func.set(fresh.measure_func.get());
        self.min_content_measure_func
            .set(fresh.min_content_measure_func.get());
        self.min_content_width.set(fresh.min_content_width.get());
        self.min_content_height.set(fresh.min_content_height.get());
        self.baseline_func.set(fresh.baseline_func.get());
        self.dirtied_func.set(fresh.dirtied_func.get());
        // SAFETY: `reset` is an API entry point; no style borrow is live.
        unsafe {
            self.set_style(*fresh.style());
            *self.grid_mut() = None;
        }
        self.layout.copy_from(&fresh.layout);
        self.contents_children_count.set(0);
        Ok(())
    }
}
