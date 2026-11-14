//! Rigid-body physics for 2D games, backed by [Rapier].
//!
//! ```no_run
//! use giochi_engine::physics::{BodyDesc, ColliderShape, PhysicsWorld2D};
//! use giochi_engine::Vec2;
//!
//! let mut world = PhysicsWorld2D::new(Vec2::new(0.0, -9.81))?;
//! let floor = world.create_body(BodyDesc::static_(Vec2::new(0.0, 0.0)))?;
//! world.attach_collider(floor, ColliderShape::boxed(10.0, 0.5), 0.8, 0.0)?;
//! let ball = world.create_body(BodyDesc::dynamic(Vec2::new(0.0, 5.0)))?;
//! world.attach_collider(ball, ColliderShape::circle(0.5), 0.4, 0.6)?;
//!
//! for _ in 0..120 {
//!     world.step(1.0 / 60.0)?;
//! }
//! world.sync_to_transforms(&mut |handle, transform| {
//!     // `transform.position` is where the body is now.
//!     let _ = (handle, transform);
//! });
//! # Ok::<(), giochi_engine::physics::PhysicsError>(())
//! ```
//!
//! [Rapier]: https://rapier.rs
//!
//! # Design
//!
//! The world is **one fixed timestep**, not the frame time: `step` accumulates the delta
//! and runs `1/60 s` integration steps, so a slow frame does not make a fast body tunnel
//! through a wall. At most [`MAX_SUBSTEPS`] steps run per call, and the backlog beyond
//! that is dropped rather than simulated later — a game that stalls for a second should
//! not teleport its world a second forward.
//!
//! Handles are `u64` and never reused, so a handle kept after `destroy_body` reports
//! [`PhysicsError::InvalidBody`] instead of addressing a different body (the "stale
//! handle" class of bug). Slots grow monotonically; a game that creates and destroys
//! bodies forever should reuse its own entities instead of churning handles.
//!
//! Everything here is `#[repr(C)]`-shaped: a game (and, in phase 24c, the C ABI) passes
//! plain structs around, and no Rapier type appears in a signature.

mod body;
mod collider;
mod world;

use thiserror::Error;

pub use body::{BodyDesc, BodyKind, BodyTransform};
pub use collider::ColliderShape;
pub use world::{Contact, ContactKind, PhysicsWorld2D, RayHit};

/// Handle to a body in a [`PhysicsWorld2D`].
///
/// Zero is never a valid handle: it is what a failed `create_body` leaves behind in a
/// language without `Result`.
pub type BodyHandle = u64;

/// The integration step, in seconds (60 Hz).
pub const FIXED_TIMESTEP: f32 = 1.0 / 60.0;
/// How many integration steps one [`PhysicsWorld2D::step`] call may run.
///
/// Four covers a frame that took 66 ms without letting a hitch simulate an unbounded
/// amount of world.
pub const MAX_SUBSTEPS: u32 = 4;
/// Largest delta accepted by [`PhysicsWorld2D::step`], in seconds.
///
/// A `dt` above this is treated as this: a debugger pause or a window drag is not
/// simulated as real time.
pub const MAX_FRAME_DELTA: f32 = 0.25;
/// Largest number of bodies one world may hold.
pub const MAX_BODIES: usize = 65_536;

/// Errors returned by the physics world.
///
/// Every constructor and every handle-taking method returns one of these rather than
/// panicking: a game that respawns an entity twice must not take the process down.
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
}
