//! Math primitives with a stable C layout.
//!
//! This module depends on nothing else inside the crate. Every type here is
//! `#[repr(C)]` and carries a `_reserved` tail so that fields can be added in a
//! future ABI without breaking layout for existing consumers.

pub mod color;
pub mod rect;
pub mod vec2;

pub use color::Color;
pub use rect::Rect;
pub use vec2::Vec2;
