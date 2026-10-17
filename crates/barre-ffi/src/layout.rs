//! `YGNodeLayout.h` (`yoga/YGNodeLayout.cpp`).

use barre::debug::assert_fatal::{FatalError, assert_fatal_with_node};
use barre::node::LayoutResults;
use barre::{Dimension, Direction, Edge, Node, PhysicalEdge};

use crate::{YGNodeConstRef, guard, node};

/// `getResolvedLayoutProperty`: resolves start/end against the layout
/// direction; shorthand edges are an error.
fn resolved_layout_property(
    n: &Node,
    raw_edge: u32,
    get: fn(&LayoutResults, PhysicalEdge) -> f32,
) -> Result<f32, FatalError> {
    let edge = Edge::from_raw(raw_edge);
    assert_fatal_with_node(
        n,
        edge.is_some_and(|e| e as u32 <= Edge::End as u32),
        "Cannot get layout properties of multi-edge shorthands",
    )?;
    let layout = n.layout();
    let rtl = layout.direction() == Direction::RTL;
    let physical = match edge.expect("checked above") {
        Edge::Left => PhysicalEdge::Left,
        Edge::Top => PhysicalEdge::Top,
        Edge::Right => PhysicalEdge::Right,
        Edge::Bottom => PhysicalEdge::Bottom,
        Edge::Start if rtl => PhysicalEdge::Right,
        Edge::Start => PhysicalEdge::Left,
        Edge::End if rtl => PhysicalEdge::Left,
        Edge::End => PhysicalEdge::Right,
        Edge::Horizontal | Edge::Vertical | Edge::All => unreachable!("checked above"),
    };
    Ok(get(layout, physical))
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeLayoutGetLeft(n: YGNodeConstRef) -> f32 {
    node(n).layout().position(PhysicalEdge::Left)
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeLayoutGetTop(n: YGNodeConstRef) -> f32 {
    node(n).layout().position(PhysicalEdge::Top)
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeLayoutGetRight(n: YGNodeConstRef) -> f32 {
    node(n).layout().position(PhysicalEdge::Right)
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeLayoutGetBottom(n: YGNodeConstRef) -> f32 {
    node(n).layout().position(PhysicalEdge::Bottom)
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeLayoutGetWidth(n: YGNodeConstRef) -> f32 {
    node(n).layout().dimension(Dimension::Width)
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeLayoutGetHeight(n: YGNodeConstRef) -> f32 {
    node(n).layout().dimension(Dimension::Height)
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeLayoutGetDirection(n: YGNodeConstRef) -> u32 {
    node(n).layout().direction() as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeLayoutGetHadOverflow(n: YGNodeConstRef) -> bool {
    node(n).layout().had_overflow()
}

#[unsafe(no_mangle)]
pub extern "C-unwind" fn YGNodeLayoutGetMargin(n: YGNodeConstRef, edge: u32) -> f32 {
    guard(|| resolved_layout_property(node(n), edge, LayoutResults::margin))
}

#[unsafe(no_mangle)]
pub extern "C-unwind" fn YGNodeLayoutGetBorder(n: YGNodeConstRef, edge: u32) -> f32 {
    guard(|| resolved_layout_property(node(n), edge, LayoutResults::border))
}

#[unsafe(no_mangle)]
pub extern "C-unwind" fn YGNodeLayoutGetPadding(n: YGNodeConstRef, edge: u32) -> f32 {
    guard(|| resolved_layout_property(node(n), edge, LayoutResults::padding))
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeLayoutGetRawHeight(n: YGNodeConstRef) -> f32 {
    node(n).layout().raw_dimension(Dimension::Height)
}

#[unsafe(no_mangle)]
pub extern "C" fn YGNodeLayoutGetRawWidth(n: YGNodeConstRef) -> f32 {
    node(n).layout().raw_dimension(Dimension::Width)
}
