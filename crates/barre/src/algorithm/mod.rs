//! The layout algorithm (`yoga/algorithm/`). Each file mirrors the Yoga file
//! of the same name; each function names the Yoga function it ports.
//!
//! Everything here is generic over [`crate::node::LayoutNode`]: the
//! algorithm reaches nodes only through that interface.

pub mod absolute_layout;
pub mod align;
pub mod baseline;
pub mod bound_axis;
pub mod cache;
pub mod calculate_layout;
pub mod flex_direction;
pub mod flex_line;
pub mod pixel_grid;
pub mod sizing_mode;
pub mod trailing_position;

pub use calculate_layout::calculate_layout;
