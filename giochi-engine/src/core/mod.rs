//! Engine core: timing, the fixed-timestep loop and the engine facade.
//!
//! Layering rule: this module never imports `sdl2` directly. Anything that needs
//! the platform goes through [`crate::platform`].

pub mod engine;
pub mod game_loop;
pub mod time;

pub use engine::{
    clamp_frame_time, Engine, EngineConfig, RunStats, DEFAULT_CLEAR_COLOR, ENGINE_TITLE_CAPACITY,
};
pub use game_loop::{GameLoop, FIXED_TIMESTEP, MAX_FRAME_TIME};
pub use time::{FrameClock, Time};
