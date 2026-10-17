//! Iteration over the children that take part in layout
//! (`yoga/node/LayoutableChildren.h`).
//!
//! A `display: contents` child does not take part in layout itself; its
//! children are laid out as if they were children of its parent, recursively.
//! The iterator yields the children of a node with every `display: contents`
//! child replaced, in place, by its own layoutable children.
//!
//! TICKET (display: contents): only the common case is implemented — nodes
//! without `display: contents` children. Reaching a `display: contents` child
//! hits `not_implemented!`. Implementing it means porting
//! `LayoutableChildren::Iterator::next` / `skipContentsNodes` (the
//! `backtrack_` stack) into `advance` / `skip_contents_nodes` below; nothing
//! outside this file needs to change. Target tests:
//! `YGDisplayContentsTest`, `YGLayoutableChildrenTest`,
//! `YGContentsNodeHasNewLayoutTest`.

use crate::enums::Display;
use crate::node::layout_node::LayoutNode;

/// `LayoutableChildren<Node>::Iterator`: a position in the flattened list of
/// a node's layoutable children. The end position has no node.
#[derive(Clone)]
pub struct LayoutableChildren<'a, N: LayoutNode<'a>> {
    node: Option<N>,
    child_index: usize,
    /// Positions to return to after finishing the children of a
    /// `display: contents` node (Yoga's `backtrack_`; empty unless such nodes
    /// are present, so it never allocates otherwise).
    backtrack: Vec<(N, usize)>,
    _tree: std::marker::PhantomData<&'a ()>,
}

impl<'a, N: LayoutNode<'a>> LayoutableChildren<'a, N> {
    /// `LayoutableChildren::begin`.
    pub fn begin(node: N) -> LayoutableChildren<'a, N> {
        if node.child_count() > 0 {
            let mut result = LayoutableChildren {
                node: Some(node),
                child_index: 0,
                backtrack: Vec::new(),
                _tree: std::marker::PhantomData,
            };
            if node.child(0).style().display() == Display::Contents {
                result.skip_contents_nodes();
            }
            result
        } else {
            LayoutableChildren::end()
        }
    }

    /// `LayoutableChildren::end`.
    pub fn end() -> LayoutableChildren<'a, N> {
        LayoutableChildren {
            node: None,
            child_index: 0,
            backtrack: Vec::new(),
            _tree: std::marker::PhantomData,
        }
    }

    #[inline]
    pub fn is_end(&self) -> bool {
        self.node.is_none()
    }

    /// `Iterator::operator*`: the child at this position (`None` at the end).
    #[inline]
    pub fn current(&self) -> Option<N> {
        self.node.map(|n| n.child(self.child_index))
    }

    /// `Iterator::operator++` (`next`).
    pub fn advance(&mut self) {
        let Some(node) = self.node else {
            return;
        };
        if self.child_index + 1 >= node.child_count() {
            // If the current node has no more children, try to backtrack and
            // visit its successor.
            if self.backtrack.is_empty() {
                // If there are no nodes to backtrack to, the last node has
                // been visited.
                *self = LayoutableChildren::end();
            } else {
                crate::not_implemented!("LayoutableChildren backtracking (display: contents)");
            }
        } else {
            // Current node has more children to visit, go to next.
            self.child_index += 1;
            // Skip all display: contents nodes, possibly going deeper into
            // the tree.
            if node.child(self.child_index).style().display() == Display::Contents {
                self.skip_contents_nodes();
            }
        }
    }

    /// `Iterator::skipContentsNodes`.
    fn skip_contents_nodes(&mut self) {
        crate::not_implemented!("LayoutableChildren::skipContentsNodes (display: contents)");
    }
}

impl<'a, N: LayoutNode<'a>> PartialEq for LayoutableChildren<'a, N> {
    /// `Iterator::operator==`: same node and child index.
    fn eq(&self, other: &Self) -> bool {
        let same_node = match (self.node, other.node) {
            (None, None) => true,
            (Some(a), Some(b)) => a.is_same(b),
            _ => false,
        };
        same_node && self.child_index == other.child_index
    }
}

impl<'a, N: LayoutNode<'a>> Iterator for LayoutableChildren<'a, N> {
    type Item = N;

    #[inline]
    fn next(&mut self) -> Option<N> {
        let current = self.current()?;
        self.advance();
        Some(current)
    }
}
