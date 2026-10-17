//! Baseline alignment (`yoga/algorithm/Baseline.cpp`).

use crate::algorithm::align::resolve_child_alignment;
use crate::algorithm::flex_direction::is_column;
use crate::enums::{Align, Dimension, PhysicalEdge, PositionType};
use crate::node::{LayoutNode, NodeExt};

/// `calculateBaseline`: the baseline of `node`, as an offset from its top
/// edge.
pub fn calculate_baseline<'a, N: LayoutNode<'a>>(node: N) -> f32 {
    if node.has_baseline_func() {
        // Event::publish<Event::NodeBaselineStart>(node) is not ported.

        let baseline = node.baseline(
            node.layout().measured_dimension(Dimension::Width),
            node.layout().measured_dimension(Dimension::Height),
        );

        // Event::publish<Event::NodeBaselineEnd>(node) is not ported.

        // assertFatalWithNode: no FatalError path from algorithm code, so
        // this panics with Yoga's message.
        assert!(
            !baseline.is_nan(),
            "Expect custom baseline function to not return NaN"
        );
        return baseline;
    }

    let mut baseline_child = None;
    for child in node.layout_children() {
        if child.line_index() > 0 {
            break;
        }
        if child.style().position_type() == PositionType::Absolute {
            continue;
        }
        if resolve_child_alignment(node, child) == Align::Baseline || child.is_reference_baseline()
        {
            baseline_child = Some(child);
            break;
        }

        if baseline_child.is_none() {
            baseline_child = Some(child);
        }
    }

    let Some(baseline_child) = baseline_child else {
        return node.layout().measured_dimension(Dimension::Height);
    };

    let baseline = calculate_baseline(baseline_child);
    baseline + baseline_child.layout().position(PhysicalEdge::Top)
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
