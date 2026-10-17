//! Alignment helpers (`yoga/algorithm/Align.h`).

use crate::algorithm::flex_direction::is_column;
use crate::enums::{Align, Display, Justify};
use crate::node::LayoutNode;

/// `resolveChildAlignment`.
#[inline]
pub fn resolve_child_alignment<'a, N: LayoutNode<'a>>(node: N, child: N) -> Align {
    let node_style = node.style();
    let child_align_self = child.style().align_self();
    let align = if child_align_self == Align::Auto {
        node_style.align_items()
    } else {
        child_align_self
    };
    if node_style.display() == Display::Flex
        && align == Align::Baseline
        && is_column(node_style.flex_direction())
    {
        return Align::FlexStart;
    }
    align
}

/// `resolveChildJustification`.
#[inline]
pub fn resolve_child_justification<'a, N: LayoutNode<'a>>(node: N, child: N) -> Justify {
    let justify_self = child.style().justify_self();
    if justify_self == Justify::Auto {
        node.style().justify_items()
    } else {
        justify_self
    }
}

/// `fallbackAlignment(Align)`.
#[inline]
pub fn fallback_alignment(align: Align) -> Align {
    match align {
        // Fallback to flex-start
        Align::SpaceBetween | Align::Stretch => Align::FlexStart,
        // Fallback to safe center. TODO (T208209388): This should be aligned
        // to Start instead of FlexStart (for row-reverse containers)
        Align::SpaceAround | Align::SpaceEvenly => Align::FlexStart,
        _ => align,
    }
}

/// `fallbackAlignment(Justify)`.
#[inline]
pub fn fallback_justification(justify: Justify) -> Justify {
    match justify {
        // Fallback to flex-start
        // TODO: Support `justify-content: stretch`
        Justify::SpaceBetween => Justify::FlexStart,
        // Fallback to safe center. TODO (T208209388): This should be aligned
        // to Start instead of FlexStart (for row-reverse containers)
        Justify::SpaceAround | Justify::SpaceEvenly => Justify::FlexStart,
        _ => justify,
    }
}
