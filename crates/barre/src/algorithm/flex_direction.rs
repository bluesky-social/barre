//! Axis helpers (`yoga/algorithm/FlexDirection.h`).

use crate::enums::{Dimension, Direction, FlexDirection, PhysicalEdge};

/// `isRow`.
#[inline]
pub fn is_row(flex_direction: FlexDirection) -> bool {
    flex_direction == FlexDirection::Row || flex_direction == FlexDirection::RowReverse
}

/// `isColumn`.
#[inline]
pub fn is_column(flex_direction: FlexDirection) -> bool {
    flex_direction == FlexDirection::Column || flex_direction == FlexDirection::ColumnReverse
}

/// `resolveDirection`: row directions flip under RTL.
#[inline]
pub fn resolve_direction(flex_direction: FlexDirection, direction: Direction) -> FlexDirection {
    if direction == Direction::RTL {
        if flex_direction == FlexDirection::Row {
            return FlexDirection::RowReverse;
        } else if flex_direction == FlexDirection::RowReverse {
            return FlexDirection::Row;
        }
    }
    flex_direction
}

/// `resolveCrossDirection`.
#[inline]
pub fn resolve_cross_direction(
    flex_direction: FlexDirection,
    direction: Direction,
) -> FlexDirection {
    if is_column(flex_direction) {
        resolve_direction(FlexDirection::Row, direction)
    } else {
        FlexDirection::Column
    }
}

/// `flexStartEdge`.
#[inline]
pub fn flex_start_edge(flex_direction: FlexDirection) -> PhysicalEdge {
    match flex_direction {
        FlexDirection::Column => PhysicalEdge::Top,
        FlexDirection::ColumnReverse => PhysicalEdge::Bottom,
        FlexDirection::Row => PhysicalEdge::Left,
        FlexDirection::RowReverse => PhysicalEdge::Right,
    }
}

/// `flexEndEdge`.
#[inline]
pub fn flex_end_edge(flex_direction: FlexDirection) -> PhysicalEdge {
    match flex_direction {
        FlexDirection::Column => PhysicalEdge::Bottom,
        FlexDirection::ColumnReverse => PhysicalEdge::Top,
        FlexDirection::Row => PhysicalEdge::Right,
        FlexDirection::RowReverse => PhysicalEdge::Left,
    }
}

/// `inlineStartEdge`.
#[inline]
pub fn inline_start_edge(flex_direction: FlexDirection, direction: Direction) -> PhysicalEdge {
    if is_row(flex_direction) {
        if direction == Direction::RTL {
            PhysicalEdge::Right
        } else {
            PhysicalEdge::Left
        }
    } else {
        PhysicalEdge::Top
    }
}

/// `inlineEndEdge`.
#[inline]
pub fn inline_end_edge(flex_direction: FlexDirection, direction: Direction) -> PhysicalEdge {
    if is_row(flex_direction) {
        if direction == Direction::RTL {
            PhysicalEdge::Left
        } else {
            PhysicalEdge::Right
        }
    } else {
        PhysicalEdge::Bottom
    }
}

/// `dimension`: the dimension an axis measures.
#[inline]
pub fn dimension(flex_direction: FlexDirection) -> Dimension {
    match flex_direction {
        FlexDirection::Column | FlexDirection::ColumnReverse => Dimension::Height,
        FlexDirection::Row | FlexDirection::RowReverse => Dimension::Width,
    }
}
