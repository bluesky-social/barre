//! Node lifetime and tree mutation, with the semantics of the C API
//! (`yoga/YGNode.cpp`). The C ABI (`barre-ffi`) is a thin layer over these.

use crate::config::Config;
use crate::debug::assert_fatal::{FatalError, assert_fatal, assert_fatal_with_node};
use crate::enums::Display;
use crate::node::layout_results::LayoutResults;
use crate::node::{Node, NodePtr};

/// `YGNodeNewWithConfig`: allocates a node. Free it with [`node_free`].
pub fn node_new(config: &Config) -> NodePtr {
    let node = Box::new(Node::new(config));
    NodePtr::from_raw(Box::into_raw(node)).expect("Box::into_raw is non-null")
}

/// `YGNodeClone`: a shallow copy with no owner. The children are shared
/// with `node` (their owner is unchanged), as in Yoga's persistent cloning.
pub fn node_clone(node: &Node) -> NodePtr {
    let clone = Box::new(node.clone());
    clone.set_owner(None);
    NodePtr::from_raw(Box::into_raw(clone)).expect("Box::into_raw is non-null")
}

/// `YGNodeFree`: detaches the node from its owner and children, then frees
/// it.
///
/// # Safety
/// `node` must have been allocated by [`node_new`] / [`node_clone`] and not
/// freed, and no references to it may be used afterwards.
pub unsafe fn node_free(node: NodePtr) {
    let n = node.live();
    if let Some(owner) = n.owner() {
        owner.remove_child(node);
        n.set_owner(None);
        owner.mark_dirty_and_propagate();
    }
    for i in 0..n.child_count() {
        n.child(i).set_owner(None);
    }
    n.clear_children();
    // SAFETY: guaranteed by the caller.
    drop(unsafe { Box::from_raw(node.as_ptr()) });
}

/// `YGNodeFreeRecursive`: frees the node and every descendant it owns.
/// Shared children (owned by another node) are left alone.
///
/// # Safety
/// As for [`node_free`], for the node and every node it owns.
pub unsafe fn node_free_recursive(root: NodePtr) {
    let r = root.live();
    let mut skipped = 0;
    while r.child_count() > skipped {
        let child = r.children().get(skipped);
        if !child.live().is_owned_by(r) {
            // Don't free shared nodes that we don't own.
            skipped += 1;
        } else {
            remove_child(root, child);
            // SAFETY: guaranteed by the caller (the root owns `child`).
            unsafe { node_free_recursive(child) };
        }
    }
    // SAFETY: guaranteed by the caller.
    unsafe { node_free(root) };
}

/// `YGNodeFinalize`: frees the node without touching its owner or
/// children.
///
/// # Safety
/// As for [`node_free`]; the caller is responsible for any dangling owner or
/// child pointers.
pub unsafe fn node_finalize(node: NodePtr) {
    // SAFETY: guaranteed by the caller.
    drop(unsafe { Box::from_raw(node.as_ptr()) });
}

/// `YGNodeInsertChild`.
pub fn insert_child(owner: NodePtr, child: NodePtr, index: usize) -> Result<(), FatalError> {
    let o = owner.live();
    let c = child.live();
    assert_fatal_with_node(
        o,
        c.owner_ptr().is_none(),
        "Child already has a owner, it must be removed first.",
    )?;
    assert_fatal_with_node(
        o,
        !o.has_measure_func(),
        "Cannot add child: Nodes with measure functions cannot have children.",
    )?;
    o.insert_child(child, index);
    c.set_owner(Some(owner));
    o.mark_dirty_and_propagate();
    Ok(())
}

/// `YGNodeSwapChild`.
pub fn swap_child(owner: NodePtr, child: NodePtr, index: usize) {
    owner.live().replace_child_at(child, index);
    child.live().set_owner(Some(owner));
}

/// Detaches `child` from its old owner: its layout is no longer valid and it
/// is marked dirty to invalidate caches, without calling its dirtied
/// callback (it is leaving the tree, so the signal must not propagate
/// through external callback mechanisms).
fn reset_detached_child(child: &Node) {
    child.set_layout(&LayoutResults::default());
    child.set_owner(None);
    let dirtied_func = child.dirtied_func();
    child.set_dirtied_func(None);
    child.set_dirty(true);
    child.set_dirtied_func(dirtied_func);
}

/// `YGNodeRemoveChild`.
pub fn remove_child(owner: NodePtr, excluded_child: NodePtr) {
    let o = owner.live();
    if o.child_count() == 0 {
        // This is an empty set. Nothing to remove.
        return;
    }
    // Children may be shared between parents, which is indicated by not
    // having an owner. We only want to reset the child completely if it is
    // owned exclusively by one node.
    let c = excluded_child.live();
    let child_owner = c.owner_ptr();
    if o.remove_child(excluded_child) {
        if child_owner == Some(owner) {
            reset_detached_child(c);
        }
        o.mark_dirty_and_propagate();
    }
}

/// `YGNodeRemoveAllChildren`.
pub fn remove_all_children(owner: NodePtr) {
    let o = owner.live();
    let child_count = o.child_count();
    if child_count == 0 {
        // This is an empty set already. Nothing to do.
        return;
    }
    if o.child(0).is_owned_by(o) {
        // If the first child has this node as its owner, we assume that this
        // child set is unique.
        for i in 0..child_count {
            reset_detached_child(o.child(i));
        }
        o.clear_children();
        o.mark_dirty_and_propagate();
        return;
    }
    // Otherwise, we are not the owner of the child set. We don't have to do
    // anything to clear it.
    o.set_children(Vec::new());
    o.mark_dirty_and_propagate();
}

/// `YGNodeSetChildren`.
pub fn set_children(owner: NodePtr, children: &[NodePtr]) {
    let o = owner.live();
    if children.is_empty() {
        if o.child_count() > 0 {
            for child in o.children().to_vec() {
                let c = child.live();
                c.set_layout(&LayoutResults::default());
                c.set_owner(None);
            }
            o.set_children(Vec::new());
            o.mark_dirty_and_propagate();
        }
    } else {
        if o.child_count() > 0 {
            for old_child in o.children().to_vec() {
                // Our new children may have nodes in common with the old
                // children. We don't reset these common nodes.
                if !children.contains(&old_child) {
                    let c = old_child.live();
                    c.set_layout(&LayoutResults::default());
                    c.set_owner(None);
                }
            }
        }
        o.set_children(children.to_vec());
        for child in children {
            child.live().set_owner(Some(owner));
        }
        o.mark_dirty_and_propagate();
    }
}

/// `YGNodeGetChild`: `None` if out of range.
pub fn get_child(node: &Node, index: usize) -> Option<NodePtr> {
    node.children().try_get(index)
}

/// `YGNodeMarkDirty`: only allowed on nodes with a measure function.
pub fn mark_dirty(node: &Node) -> Result<(), FatalError> {
    assert_fatal_with_node(
        node,
        node.has_measure_func(),
        "Only leaf nodes with custom measure functions should manually mark themselves as dirty",
    )?;
    node.mark_dirty_and_propagate();
    Ok(())
}

/// `YGNodeSetIsReferenceBaseline`.
pub fn set_is_reference_baseline(node: &Node, is_reference_baseline: bool) {
    if node.is_reference_baseline() != is_reference_baseline {
        node.set_is_reference_baseline(is_reference_baseline);
        node.mark_dirty_and_propagate();
    }
}

/// `YGNodeCopyStyle`.
///
/// # Safety
/// No borrow of `dst`'s style may be live (see [`Node::style_mut`]).
pub unsafe fn copy_style(dst: &Node, src: &Node) {
    let same_grid = dst.grid() == src.grid()
        || dst.grid().unwrap_or(&crate::style::DEFAULT_GRID_STYLE)
            == src.grid().unwrap_or(&crate::style::DEFAULT_GRID_STYLE);
    if dst.style() != src.style() || !same_grid {
        let style = *src.style();
        let grid = src.grid().cloned().map(Box::new);
        // SAFETY: guaranteed by the caller.
        unsafe {
            dst.set_style(style);
            *dst.grid_mut() = grid;
        }
        dst.mark_dirty_and_propagate();
    }
}

/// `YGNodeStyleSetDisplay`'s bookkeeping: the owner caches how many of its
/// children use `display: contents`, so it has to be recomputed when an
/// attached child switches to or from it.
pub fn display_changed(node: &Node, was_contents: bool) {
    let is_contents = node.style().display() == Display::Contents;
    if let Some(owner) = node.owner()
        && was_contents != is_contents
    {
        owner.recount_contents_children();
    }
}

/// `YGNodeNewWithConfig`'s null check, for callers holding a possibly-null
/// config.
pub fn assert_config_non_null(non_null: bool) -> Result<(), FatalError> {
    assert_fatal(non_null, "Tried to construct YGNode with null config")
}
