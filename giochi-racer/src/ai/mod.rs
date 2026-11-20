//! The racing line, and the controller that follows it.
//!
//! Two pieces, deliberately separate:
//!
//! * [`Route`] — pure geometry. Where the next aim point is, given a position and a list of
//!   waypoints. No physics, no GL, no car: this is the part a test can check with a ruler.
//! * [`AiDriver`](crate::ai::AiDriver) — a car plus a [`Route`] plus a skill factor,
//!   producing [`CarInput`](crate::vehicle::input::CarInput) exactly like a human does.
//!
//! # Why pure pursuit and not a physics-perfect racing line
//!
//! The plan allows "IA no trilho": move the opponent along the waypoints instead of
//! simulating it. That was rejected — an opponent that ignores the walls looks wrong the
//! first time it overlaps one, and the whole point of phase 41's car was that the physics
//! is trustworthy. The opponents drive the **same `Car`** as the player, with the same
//! colliders and the same arcade model; only the input source differs.
//!
//! The controller is the same pure pursuit that `tests/lap_logic.rs` used to prove the
//! circuits are drivable, promoted here so the phase-42 test and the phase-43 AI cannot
//! drift apart.

pub mod driver;
pub mod pilot;
pub mod waypoint;

pub use driver::AiDriver;
pub use pilot::{clamp_skill, Pilot};
pub use waypoint::Route;
