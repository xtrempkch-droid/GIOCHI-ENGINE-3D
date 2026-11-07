//! 3D maths.
//!
//! Everything that crosses the C ABI is `#[repr(C)]` and built from `f32` arrays; the
//! `glam` types are re-exported for Rust callers only. That split is deliberate: `glam`
//! is `#[repr(simd)]`-adjacent and must never appear in a public struct field.
//!
//! Phase 9 only needs [`Color3D`]; phase 10 adds `Mat4`, `Transform3D` and
//! `Camera3D`.

mod color;

pub use color::Color3D;

/// Re-exported for Rust callers. Never part of a `#[repr(C)]` type.
pub use glam::{Quat, Vec3};
