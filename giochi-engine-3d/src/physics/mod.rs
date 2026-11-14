//! Rigid-body physics for 3D games, backed by [Rapier].
//!
//! ```no_run
//! use giochi_engine_3d::math3d::Vec3;
//! use giochi_engine_3d::physics::{BodyDesc, ColliderShape, PhysicsWorld3D};
//!
//! let mut world = PhysicsWorld3D::new(Vec3::new(0.0, -9.81, 0.0))?;
//! let ground = world.create_body(BodyDesc::static_(Vec3::ZERO))?;
//! world.attach_collider(ground, ColliderShape::cuboid(20.0, 0.5, 20.0), 0.9, 0.0)?;
//! let crate_ = world.create_body(BodyDesc::dynamic(Vec3::new(0.0, 6.0, 0.0)))?;
//! world.attach_collider(crate_, ColliderShape::cuboid(0.5, 0.5, 0.5), 0.6, 0.1)?;
//!
//! for _ in 0..120 {
//!     world.step(1.0 / 60.0)?;
//! }
//! world.sync_to_transforms(&mut |handle, transform| {
//!     let _ = (handle, transform.position);
//! });
//! # Ok::<(), giochi_engine_3d::physics::PhysicsError>(())
//! ```
//!
//! [Rapier]: https://rapier.rs
//!
//! # Why this lives in the 3D crate
//!
//! The plan put `PhysicsWorld2D` and `PhysicsWorld3D` in one module. They cannot share a
//! crate: `giochi-engine` is the 2D engine and does not depend on the 3D renderer, and
//! `giochi-engine-3d` depends on the 2D engine, not the other way round (ADR-037). Two
//! crates, two features (`physics-2d`, `physics-3d`), so a 2D game never links Rapier 3D
//! and its ~2 MB (ADR-057).
//!
//! # Same rules as the 2D world
//!
//! Fixed 60 Hz step with an accumulator, `u64` handles that are never reused, plain
//! `#[repr(C)]` structs at the boundary, and no Rapier type in any signature. The one
//! addition is [`PhysicsWorld3D::move_character`], which wraps Rapier's kinematic
//! character controller: a first-person player needs sliding, ground snapping and slope
//! limits, and writing that by hand is a phase of its own.

mod body;
mod collider;
mod world;

use thiserror::Error;

pub use body::{BodyDesc, BodyKind, BodyTransform};
pub use collider::ColliderShape;
pub use world::{CharacterMove, Contact, ContactKind, PhysicsWorld3D, RayHit};

/// Handle to a body in a [`PhysicsWorld3D`].
///
/// Zero is never a valid handle.
pub type BodyHandle = u64;

/// The integration step, in seconds (60 Hz).
pub const FIXED_TIMESTEP: f32 = 1.0 / 60.0;
/// How many integration steps one [`PhysicsWorld3D::step`] call may run.
pub const MAX_SUBSTEPS: u32 = 4;
/// Largest delta accepted by [`PhysicsWorld3D::step`], in seconds.
pub const MAX_FRAME_DELTA: f32 = 0.25;
/// Largest number of bodies one world may hold.
pub const MAX_BODIES: usize = 65_536;

/// Errors returned by the 3D physics world.
#[repr(C)]
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum PhysicsError {
    /// A handle does not name a live body.
    #[error("the body handle does not name a live body")]
    InvalidBody,
    /// A body description held a `NaN`, an infinity or a negative extent.
    #[error("the body description is out of range")]
    InvalidBodyDesc,
    /// A collider shape had a zero or negative extent.
    #[error("a collider needs a positive extent")]
    InvalidShape,
    /// The world already holds [`MAX_BODIES`] bodies.
    #[error("the physics world is full")]
    BodyLimitReached,
    /// The body already has a collider.
    #[error("the body already has a collider")]
    ColliderAlreadyAttached,
    /// The delta passed to `step` was negative or not finite.
    #[error("the frame delta is out of range")]
    InvalidDelta,
    /// A ray direction was zero or not finite.
    #[error("a ray needs a finite, non-zero direction")]
    InvalidRay,
    /// The body is not a character created with [`PhysicsWorld3D::create_character`].
    #[error("the body is not a character")]
    NotACharacter,
}
