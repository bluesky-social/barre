//! barre: a Rust reimplementation of [Yoga](https://github.com/react/yoga).
//!
//! The Yoga C++ source in `vendor/yoga/yoga/` is the spec. Modules mirror its
//! layout: `algorithm/` mirrors `yoga/algorithm/`, `node/` mirrors
//! `yoga/node/`, `style/` mirrors `yoga/style/`, and so on. See `docs/API.md`.

/// Marks code that is not implemented yet. Panics with a message naming the
/// missing piece; the C ABI turns any panic into an abort, which the
/// conformance runner counts as a failure. Never replace this with a guess.
#[macro_export]
macro_rules! not_implemented {
    ($name:expr) => {
        panic!("not implemented: {}", $name)
    };
}

pub mod algorithm;
pub mod config;
pub mod debug;
pub mod enums;
pub mod node;
pub mod numeric;
pub mod stats;
pub mod style;

pub use config::Config;
pub use enums::*;
pub use node::{Node, NodePtr, Size};
