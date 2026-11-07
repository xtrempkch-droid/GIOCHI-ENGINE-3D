//! Fixed-timestep game loop.
//!
//! The loop advances the simulation at a constant rate
//! ([`FIXED_TIMESTEP`], 60 Hz) independently of the display refresh rate. A
//! frame that takes longer than [`MAX_FRAME_TIME`] is clamped so the simulation
//! cannot enter the "spiral of death" after a hitch (e.g. a debugger pause).

/// Fixed simulation step: 1/60 s.
pub const FIXED_TIMESTEP: f32 = 1.0 / 60.0;

/// Longest frame time the loop will integrate in one go, in seconds.
pub const MAX_FRAME_TIME: f32 = 0.25;

/// Fixed-timestep accumulator.
///
/// The loop itself (event pump, window handling) lives in
/// [`crate::core::engine::Engine`]; this type only owns the accumulator so the
/// stepping rules are unit-testable without a window.
#[repr(C)]
#[derive(Debug)]
pub struct GameLoop {
    accumulator: f32,
    running: bool,
    _reserved: [u32; 4],
}

impl GameLoop {
    /// Creates a running loop with an empty accumulator.
    pub const fn new() -> Self {
        Self {
            accumulator: 0.0,
            running: true,
            _reserved: [0; 4],
        }
    }

    /// Advances the accumulator by `frame_time` seconds and returns how many
    /// fixed steps should be simulated for this frame.
    ///
    /// `frame_time` is clamped to [`MAX_FRAME_TIME`]. A negative value is
    /// treated as `0.0`.
    ///
    /// Implemented in phase 1.
    pub fn advance(&mut self, _frame_time: f32) -> u32 {
        todo!("fase 1: acumulador de fixed timestep")
    }

    /// Asks the loop to stop after the current iteration.
    ///
    /// Implemented in phase 1.
    pub fn request_quit(&mut self) {
        todo!("fase 1: sinalizar encerramento do loop")
    }

    /// Returns whether the loop should keep running.
    pub fn is_running(&self) -> bool {
        self.running
    }
}

impl Default for GameLoop {
    fn default() -> Self {
        Self::new()
    }
}
