//! Game-level logic that does not need a renderer.
//!
//! Everything here is pure: given the same input and the same time step it produces the
//! same result on any machine, which is what lets the examples and CI replay a session
//! without a display.

pub mod balance;

pub use balance::{tilt_from_input, BalanceConfig, BalanceGame, Ball};
