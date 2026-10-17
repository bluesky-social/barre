//! Breaking children into flex lines (`yoga/algorithm/FlexLine.cpp`).

use crate::algorithm::bound_axis::bound_axis_within_min_and_max;
use crate::algorithm::flex_direction::resolve_direction;
use crate::enums::{Direction, Display, PositionType, Wrap};
use crate::node::{LayoutNode, LayoutableChildren, NodeExt};

/// A growable list that keeps its first `CAP` elements inline (on the
/// stack) and moves to the heap only past that. Yoga allocates a
/// `std::vector` per flex line (and barre's yoga#2035 fix two more per
/// `resolve_flexible_length`); malloc + free of those was ~15% of a fresh
/// RN-shaped layout, where nearly every node has a handful of children.
#[derive(Debug)]
pub struct InlineVec<T: Copy, const CAP: usize> {
    len: usize,
    /// Filled with copies of the first element on the first push, so that no
    /// slot is ever uninitialised.
    inline: Option<[T; CAP]>,
    /// Holds every element once there are more than `CAP`.
    heap: Vec<T>,
}

impl<T: Copy, const CAP: usize> InlineVec<T, CAP> {
    #[inline]
    pub fn new() -> Self {
        InlineVec {
            len: 0,
            inline: None,
            heap: Vec::new(),
        }
    }

    #[inline]
    pub fn push(&mut self, value: T) {
        if self.len < CAP {
            match &mut self.inline {
                Some(inline) => inline[self.len] = value,
                None => self.inline = Some([value; CAP]),
            }
        } else {
            if self.len == CAP {
                self.heap.reserve(CAP * 2);
                self.heap
                    .extend_from_slice(self.inline.as_ref().map_or(&[][..], |a| &a[..]));
            }
            self.heap.push(value);
        }
        self.len += 1;
    }
}

impl<T: Copy, const CAP: usize> Default for InlineVec<T, CAP> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Copy, const CAP: usize> std::ops::Deref for InlineVec<T, CAP> {
    type Target = [T];
    #[inline]
    fn deref(&self) -> &[T] {
        if self.len > CAP {
            &self.heap
        } else {
            match &self.inline {
                Some(inline) => &inline[..self.len],
                None => &[],
            }
        }
    }
}

impl<T: Copy, const CAP: usize> std::ops::DerefMut for InlineVec<T, CAP> {
    #[inline]
    fn deref_mut(&mut self) -> &mut [T] {
        if self.len > CAP {
            &mut self.heap
        } else {
            match &mut self.inline {
                Some(inline) => &mut inline[..self.len],
                None => &mut [],
            }
        }
    }
}

impl<'v, T: Copy, const CAP: usize> IntoIterator for &'v InlineVec<T, CAP> {
    type Item = &'v T;
    type IntoIter = std::slice::Iter<'v, T>;
    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Inline capacity of a flex line's item list (and the per-item scratch of
/// `resolve_flexible_length`).
pub const LINE_INLINE_ITEMS: usize = 16;

/// `yoga::FlexLineRunningLayout`.
#[derive(Copy, Clone, Debug, Default)]
pub struct FlexLineRunningLayout {
    /// Total flex grow factors of flex items which are to be laid in the
    /// current line. This is decremented as free space is distributed.
    pub total_flex_grow_factors: f32,
    /// Total flex shrink factors of flex items which are to be laid in the
    /// current line. This is decremented as free space is distributed.
    pub total_flex_shrink_scaled_factors: f32,
    /// The amount of available space within inner dimensions of the line
    /// which may still be distributed.
    pub remaining_free_space: f32,
    /// The size of the mainDim for the row after considering size, padding,
    /// margin and border of flex items. This is used to calculate maxLineDim
    /// after going through all the rows to decide on the main axis size of
    /// owner.
    pub main_dim: f32,
    /// The size of the crossDim for the row after considering size, padding,
    /// margin and border of flex items. Used for calculating containers
    /// crossSize.
    pub cross_dim: f32,
}

/// `yoga::FlexLine`.
#[derive(Debug)]
pub struct FlexLine<N: Copy> {
    /// List of children which are part of the line flow. This means they are
    /// not positioned absolutely, or with `display: "none"`, and do not
    /// overflow the available dimensions.
    pub items_in_flow: InlineVec<N, LINE_INLINE_ITEMS>,
    /// Accumulation of the dimensions and margin of all the children on the
    /// current line. This will be used in order to either set the dimensions
    /// of the node if none already exist or to compute the remaining space
    /// left for the flexible children.
    pub size_consumed: f32,
    /// Number of edges along the line flow with an auto margin.
    pub number_of_auto_margins: usize,
    /// Layout information about the line computed in steps after
    /// line-breaking.
    pub layout: FlexLineRunningLayout,
}

/// `calculateFlexLine`: calculates where a line starting at `iterator`
/// should break, returning information about the collective children on the
/// line, and advances `iterator` past it.
///
/// This function assumes that all the children of node have their
/// computedFlexBasis properly computed (to do this, use the
/// `computeFlexBasisForChildren` function).
pub fn calculate_flex_line<'a, N: LayoutNode<'a>>(
    node: N,
    owner_direction: Direction,
    owner_width: f32,
    main_axis_owner_size: f32,
    available_inner_width: f32,
    available_inner_main_dim: f32,
    iterator: &mut LayoutableChildren<'a, N>,
    line_count: usize,
) -> FlexLine<N> {
    let mut items_in_flow = InlineVec::new();
    let mut size_consumed = 0.0f32;
    let mut total_flex_grow_factors = 0.0f32;
    let mut total_flex_shrink_scaled_factors = 0.0f32;
    let mut number_of_auto_margins = 0usize;
    let mut first_element_in_line: Option<N> = None;

    let mut size_consumed_including_min_constraint = 0.0f32;
    let direction = node.resolve_direction(owner_direction);
    let main_axis = resolve_direction(node.style().flex_direction(), direction);
    let is_node_flex_wrap = node.style().flex_wrap() != Wrap::NoWrap;
    let gap = node
        .style()
        .compute_gap_for_axis(main_axis, available_inner_main_dim);

    // Add items to the current line until it's full or we run out of items.
    while let Some(child) = iterator.current() {
        let child_style = child.style();
        if child_style.display() == Display::None
            || child_style.position_type() == PositionType::Absolute
        {
            iterator.advance();
            continue;
        }

        if first_element_in_line.is_none() {
            first_element_in_line = Some(child);
        }

        if child_style.flex_start_margin_is_auto(main_axis, owner_direction) {
            number_of_auto_margins += 1;
        }
        if child_style.flex_end_margin_is_auto(main_axis, owner_direction) {
            number_of_auto_margins += 1;
        }

        child.set_line_index(line_count);
        let child_margin_main_axis =
            child_style.compute_margin_for_axis(main_axis, available_inner_width);
        let child_leading_gap_main_axis =
            if first_element_in_line.is_some_and(|first| first.is_same(child)) {
                0.0
            } else {
                gap
            };
        let flex_basis_with_min_and_max_constraints = bound_axis_within_min_and_max(
            child,
            direction,
            main_axis,
            child.layout().computed_flex_basis.get(),
            main_axis_owner_size,
            owner_width,
        );

        // If this is a multi-line flow and this item pushes us over the
        // available size, we've hit the end of the current line. Break out of
        // the loop and lay out the current line.
        if size_consumed_including_min_constraint
            + flex_basis_with_min_and_max_constraints
            + child_margin_main_axis
            + child_leading_gap_main_axis
            > available_inner_main_dim
            && is_node_flex_wrap
            && !items_in_flow.is_empty()
        {
            break;
        }

        size_consumed_including_min_constraint += flex_basis_with_min_and_max_constraints
            + child_margin_main_axis
            + child_leading_gap_main_axis;
        size_consumed += flex_basis_with_min_and_max_constraints
            + child_margin_main_axis
            + child_leading_gap_main_axis;

        if child.is_node_flexible() {
            total_flex_grow_factors += child.resolve_flex_grow();

            // Unlike the grow factor, the shrink factor is scaled relative to
            // the child dimension.
            total_flex_shrink_scaled_factors +=
                -child.resolve_flex_shrink() * child.layout().computed_flex_basis.get();
        }

        items_in_flow.push(child);
        iterator.advance();
    }

    // The total flex factor needs to be floored to 1.
    if total_flex_grow_factors > 0.0 && total_flex_grow_factors < 1.0 {
        total_flex_grow_factors = 1.0;
    }

    // The total flex shrink factor needs to be floored to 1.
    if total_flex_shrink_scaled_factors > 0.0 && total_flex_shrink_scaled_factors < 1.0 {
        total_flex_shrink_scaled_factors = 1.0;
    }

    FlexLine {
        items_in_flow,
        size_consumed,
        number_of_auto_margins,
        layout: FlexLineRunningLayout {
            total_flex_grow_factors,
            total_flex_shrink_scaled_factors,
            ..Default::default()
        },
    }
}
