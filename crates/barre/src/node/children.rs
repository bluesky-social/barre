//! A node's child list.
//!
//! Children are mutated through shared `&Node` references (layout replaces
//! shared children with private clones; the C API mutates trees while
//! callbacks may be reading them), so the list lives in an `UnsafeCell`. No
//! reference into the `Vec` ever escapes a method, and no method calls out to
//! other code while it holds one, which is what makes the `unsafe` blocks
//! sound. Like `Cell`, this makes the type `!Sync`.

use std::cell::UnsafeCell;

use super::NodePtr;

#[derive(Default)]
pub struct Children(UnsafeCell<Vec<NodePtr>>);

impl Clone for Children {
    fn clone(&self) -> Self {
        Children(UnsafeCell::new(self.to_vec()))
    }
}

impl std::fmt::Debug for Children {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.to_vec()).finish()
    }
}

impl Children {
    #[inline]
    fn with<R>(&self, f: impl FnOnce(&mut Vec<NodePtr>) -> R) -> R {
        // SAFETY: see the module docs; `f` is always one of the short,
        // non-reentrant closures below.
        f(unsafe { &mut *self.0.get() })
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.with(|v| v.len())
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The child at `index`. Panics if out of range (`std::vector::at`).
    #[inline]
    pub fn get(&self, index: usize) -> NodePtr {
        self.with(|v| v[index])
    }

    /// The child at `index`, or `None` if out of range.
    #[inline]
    pub fn try_get(&self, index: usize) -> Option<NodePtr> {
        self.with(|v| v.get(index).copied())
    }

    #[inline]
    pub fn set(&self, index: usize, child: NodePtr) {
        self.with(|v| v[index] = child);
    }

    #[inline]
    pub fn insert(&self, index: usize, child: NodePtr) {
        self.with(|v| v.insert(index, child));
    }

    #[inline]
    pub fn remove(&self, index: usize) -> NodePtr {
        self.with(|v| v.remove(index))
    }

    /// The index of the first occurrence of `child`.
    #[inline]
    pub fn position(&self, child: NodePtr) -> Option<usize> {
        self.with(|v| v.iter().position(|&c| c == child))
    }

    /// Replaces every occurrence of `old` with `new`.
    #[inline]
    pub fn replace_all(&self, old: NodePtr, new: NodePtr) {
        self.with(|v| {
            for c in v.iter_mut() {
                if *c == old {
                    *c = new;
                }
            }
        });
    }

    /// `children_.clear(); children_.shrink_to_fit();`
    #[inline]
    pub fn clear(&self) {
        self.with(|v| *v = Vec::new());
    }

    #[inline]
    pub fn replace_with(&self, children: Vec<NodePtr>) -> Vec<NodePtr> {
        self.with(|v| std::mem::replace(v, children))
    }

    /// A copy of the list.
    #[inline]
    pub fn to_vec(&self) -> Vec<NodePtr> {
        self.with(|v| v.clone())
    }

    /// Iterates by index, re-reading the list on every step (so the list may
    /// change during iteration, as with Yoga's index loops).
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = NodePtr> + '_ {
        (0..).map_while(move |i| self.try_get(i))
    }
}
