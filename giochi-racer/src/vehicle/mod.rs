//! The player's car and everything that hangs off it: input, camera and HUD.

pub mod camera;
pub mod car;
pub mod hud;
pub mod input;

pub use camera::{ChaseCamera, ChaseConfig};
pub use car::{ArcadeModel, Car, CarConfig, CarState};
pub use input::{read_input, CarInput};
