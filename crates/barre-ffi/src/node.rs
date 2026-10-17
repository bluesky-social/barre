//! `YGNode.h` (`yoga/YGNode.cpp`).

use std::ffi::c_void;

use barre::algorithm::cache::can_use_cached_measurement;
use barre::algorithm::sizing_mode::sizing_mode;
use barre::node::tree;
use barre::node::{BaselineFunc, DirtiedFunc, MeasureFunc};
use barre::{Config, Direction, MeasureMode, NodePtr, NodeType};

use crate::{
    YGConfigConstRef, YGConfigRef, YGNodeConstRef, YGNodeRef, config, from_raw, guard, node,
    node_ptr,
};

#[unsafe(no_mangle)]
pub extern "C-unwind" fn YGNodeNew() -> YGNodeRef {
    YGNodeNewWithConfig(Config::get_default())
}

#[unsafe(no_mangle)]
pub extern "C-unwind" fn YGNodeNewWithConfig(c: YGConfigConstRef) -> YGNodeRef {
    guard(|| {
        tree::assert_config_non_null(!c.is_null())?;
        Ok(tree::node_new(config(c)).as_ptr())
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeClone(n: YGNodeConstRef) -> YGNodeRef {
    tree::node_clone(node(n)).as_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeFree(n: YGNodeRef) {
    // SAFETY: the C API's contract: `n` was allocated by Yoga and is not
    // used afterwards.
    unsafe { tree::node_free(node_ptr(n)) }
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeFreeRecursive(n: YGNodeRef) {
    // SAFETY: as for YGNodeFree.
    unsafe { tree::node_free_recursive(node_ptr(n)) }
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeFinalize(n: YGNodeRef) {
    // SAFETY: as for YGNodeFree.
    unsafe { tree::node_finalize(node_ptr(n)) }
}

#[unsafe(no_mangle)]
pub extern "C-unwind" fn YGNodeReset(n: YGNodeRef) {
    guard(|| node(n).reset())
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeCalculateLayout(
    n: YGNodeRef,
    owner_width: f32,
    owner_height: f32,
    owner_direction: u32,
) {
    barre::algorithm::calculate_layout(
        node(n),
        owner_width,
        owner_height,
        from_raw(owner_direction, Direction::from_raw),
    );
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeGetHasNewLayout(n: YGNodeConstRef) -> bool {
    node(n).has_new_layout()
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeSetHasNewLayout(n: YGNodeRef, has_new_layout: bool) {
    node(n).set_has_new_layout(has_new_layout);
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeIsDirty(n: YGNodeConstRef) -> bool {
    node(n).is_dirty()
}

#[unsafe(no_mangle)]
pub extern "C-unwind" fn YGNodeMarkDirty(n: YGNodeRef) {
    guard(|| tree::mark_dirty(node(n)))
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeSetDirtiedFunc(n: YGNodeRef, dirtied_func: Option<DirtiedFunc>) {
    node(n).set_dirtied_func(dirtied_func);
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeGetDirtiedFunc(n: YGNodeConstRef) -> Option<DirtiedFunc> {
    node(n).dirtied_func()
}

#[unsafe(no_mangle)]
pub extern "C-unwind" fn YGNodeInsertChild(owner: YGNodeRef, child: YGNodeRef, index: usize) {
    guard(|| tree::insert_child(node_ptr(owner), node_ptr(child), index))
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeSwapChild(owner: YGNodeRef, child: YGNodeRef, index: usize) {
    tree::swap_child(node_ptr(owner), node_ptr(child), index);
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeRemoveChild(owner: YGNodeRef, excluded_child: YGNodeRef) {
    tree::remove_child(node_ptr(owner), node_ptr(excluded_child));
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeRemoveAllChildren(owner: YGNodeRef) {
    tree::remove_all_children(node_ptr(owner));
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeSetChildren(owner: YGNodeRef, children: *const YGNodeRef, count: usize) {
    let Some(owner) = NodePtr::from_raw(owner) else {
        return;
    };
    let children: Vec<NodePtr> = if count == 0 {
        Vec::new()
    } else {
        // SAFETY: the C API's contract: `children` points at `count` nodes.
        unsafe { std::slice::from_raw_parts(children, count) }
            .iter()
            .map(|&c| node_ptr(c))
            .collect()
    };
    tree::set_children(owner, &children);
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeGetChild(n: YGNodeRef, index: usize) -> YGNodeRef {
    tree::get_child(node(n), index).map_or(std::ptr::null_mut(), NodePtr::as_ptr)
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeGetChildCount(n: YGNodeConstRef) -> usize {
    node(n).child_count()
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeGetOwner(n: YGNodeRef) -> YGNodeRef {
    node(n).owner_ptr().map_or(std::ptr::null_mut(), NodePtr::as_ptr)
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeGetParent(n: YGNodeRef) -> YGNodeRef {
    node(n).owner_ptr().map_or(std::ptr::null_mut(), NodePtr::as_ptr)
}

#[unsafe(no_mangle)]
pub extern "C-unwind" fn YGNodeSetConfig(n: YGNodeRef, c: YGConfigRef) {
    guard(|| {
        barre::debug::assert_fatal::assert_fatal(
            !c.is_null(),
            "Attempting to set a null config on a Node",
        )?;
        node(n).set_config(config(c))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeGetConfig(n: YGNodeRef) -> YGConfigConstRef {
    node(n).config()
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeSetContext(n: YGNodeRef, context: *mut c_void) {
    node(n).set_context(context);
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeGetContext(n: YGNodeConstRef) -> *mut c_void {
    node(n).context()
}

#[unsafe(no_mangle)]
pub extern "C-unwind" fn YGNodeSetMeasureFunc(n: YGNodeRef, measure_func: Option<MeasureFunc>) {
    guard(|| node(n).set_measure_func(measure_func))
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeHasMeasureFunc(n: YGNodeConstRef) -> bool {
    node(n).has_measure_func()
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeSetMinContentMeasureFunc(n: YGNodeRef, func: Option<MeasureFunc>) {
    node(n).set_min_content_measure_func(func);
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeHasMinContentMeasureFunc(n: YGNodeConstRef) -> bool {
    node(n).has_min_content_measure_func()
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeSetMinContentWidth(n: YGNodeRef, min_content_width: f32) {
    node(n).set_min_content_width(min_content_width);
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeSetMinContentHeight(n: YGNodeRef, min_content_height: f32) {
    node(n).set_min_content_height(min_content_height);
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeGetMinContentWidth(n: YGNodeConstRef) -> f32 {
    node(n).min_content_width()
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeGetMinContentHeight(n: YGNodeConstRef) -> f32 {
    node(n).min_content_height()
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeSetBaselineFunc(n: YGNodeRef, baseline_func: Option<BaselineFunc>) {
    node(n).set_baseline_func(baseline_func);
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeHasBaselineFunc(n: YGNodeConstRef) -> bool {
    node(n).has_baseline_func()
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeSetIsReferenceBaseline(n: YGNodeRef, is_reference_baseline: bool) {
    tree::set_is_reference_baseline(node(n), is_reference_baseline);
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeIsReferenceBaseline(n: YGNodeConstRef) -> bool {
    node(n).is_reference_baseline()
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeSetNodeType(n: YGNodeRef, node_type: u32) {
    node(n).set_node_type(from_raw(node_type, NodeType::from_raw));
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeGetNodeType(n: YGNodeConstRef) -> u32 {
    node(n).node_type() as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeSetAlwaysFormsContainingBlock(n: YGNodeRef, value: bool) {
    node(n).set_always_forms_containing_block(value);
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeGetAlwaysFormsContainingBlock(n: YGNodeConstRef) -> bool {
    node(n).always_forms_containing_block()
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeCanUseCachedMeasurement(
    width_mode: u32,
    available_width: f32,
    height_mode: u32,
    available_height: f32,
    last_width_mode: u32,
    last_available_width: f32,
    last_height_mode: u32,
    last_available_height: f32,
    last_computed_width: f32,
    last_computed_height: f32,
    margin_row: f32,
    margin_column: f32,
    c: YGConfigRef,
) -> bool {
    let mode = |raw| sizing_mode(from_raw(raw, MeasureMode::from_raw));
    can_use_cached_measurement(
        mode(width_mode),
        available_width,
        mode(height_mode),
        available_height,
        mode(last_width_mode),
        last_available_width,
        mode(last_height_mode),
        last_available_height,
        last_computed_width,
        last_computed_height,
        margin_row,
        margin_column,
        config(c),
    )
}
