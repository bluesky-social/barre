//! Baseline alignment (`yoga/algorithm/Baseline.cpp`).
//!
//! TICKET (baseline): [`calculate_baseline`] is not implemented yet; it is
//! only reached when some node actually uses baseline alignment
//! ([`is_baseline_layout`], which decides that, is implemented). To
//! implement: port `calculateBaseline` into [`calculate_baseline`]; nothing
//! outside this file needs to change. Target tests: `YGAlignBaselineTest`,
//! `YGBaselineFuncTest`, the `align_baseline*` cases in
//! `YGAlignItemsTest`/`YGAlignSelfTest`.

use crate::algorithm::flex_direction::is_column;
use crate::enums::{Align, PositionType};
use crate::node::{LayoutNode, NodeExt};

/// `calculateBaseline`: the baseline of `node`, as an offset from its top
/// edge.
#[allow(unused_variables)]
pub fn calculate_baseline<'a, N: LayoutNode<'a>>(node: N) -> f32 {
    crate::not_implemented!("calculateBaseline")
}

/// `isBaselineLayout`: whether any of the children of this node participate
/// in baseline alignment.
pub fn is_baseline_layout<'a, N: LayoutNode<'a>>(node: N) -> bool {
    if is_column(node.style().flex_direction()) {
        return false;
    }
    if node.style().align_items() == Align::Baseline {
        return true;
    }
    for child in node.layout_children() {
        if child.style().position_type() != PositionType::Absolute
            && child.style().align_self() == Align::Baseline
        {
            return true;
        }
    }
    false
}
