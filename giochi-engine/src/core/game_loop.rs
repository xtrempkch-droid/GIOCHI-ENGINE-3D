//! Fixed-timestep accumulator.
//!
//! The simulation advances at a constant rate ([`FIXED_TIMESTEP`], 60 Hz)
//! independently of the display refresh rate. A frame that takes longer than
//! [`MAX_FRAME_TIME`] is clamped so the simulation cannot enter the "spiral of
//! death" after a hitch (a debugger pause, a slow asset load, a scheduled-out
//! process).
//!
//! The loop itself (event pump, presenting) lives in
//! [`crate::core::engine::Engine`] from phase 5; this type only owns the stepping
//! policy, which makes it unit-testable without a window.

use crate::platform::sdl::QuitSignal;

/// Fixed simulation step: 1/60 s.
pub const FIXED_TIMESTEP: f32 = 1.0 / 60.0;

/// Longest frame time the loop will integrate in one go, in seconds.
pub const MAX_FRAME_TIME: f32 = 0.25;

/// Smallest step [`GameLoop::with_fixed_timestep`] accepts: 1 ms, i.e. 1000 Hz.
///
/// `advance` loops `while accumulator >= fixed_timestep`, so a step of, say, 1e-9 s
/// would make a single 0.25 s frame run 250 million iterations — a frozen process with
/// no error and no frame, the same failure mode `BalanceConfig::sanitised` guards
/// against. One kilohertz is already above anything the engine targets, and
/// `Engine::new` never produces a smaller value (`target_fps < 1000`).
pub const MIN_FIXED_TIMESTEP: f32 = 1.0e-3;

/// Fixed-timestep accumulator.
#[repr(C)]
#[derive(Debug)]
pub struct GameLoop {
    accumulator: f32,
    fixed_timestep: f32,
    running: bool,
    _reserved: [u32; 4],
}

impl GameLoop {
    /// Creates a running loop with an empty accumulator and the default 60 Hz
    /// step.
    pub const fn new() -> Self {
        Self {
            accumulator: 0.0,
            fixed_timestep: FIXED_TIMESTEP,
            running: true,
            _reserved: [0; 4],
        }
    }

    /// Overrides the fixed step, in seconds.
    ///
    /// Returns [`crate::error::GiochiError::InvalidArgument`] for non-finite values and
    /// for values below [`MIN_FIXED_TIMESTEP`]. The lower bound is not cosmetic: a step
    /// that small never makes progress in a reasonable time, and `advance` would spend an
    /// unbounded number of iterations catching up a single frame.
    pub fn with_fixed_timestep(mut self, fixed_timestep: f32) -> Result<Self, crate::GiochiError> {
        if !fixed_timestep.is_finite() || fixed_timestep < MIN_FIXED_TIMESTEP {
            return Err(crate::error::GiochiError::InvalidArgument(
                "fixed timestep must be finite and at least 1 ms (1000 Hz)",
            ));
        }
        self.fixed_timestep = fixed_timestep;
        self.accumulator = 0.0;
        Ok(self)
    }

    /// The fixed simulation step currently in use, in seconds.
    pub fn fixed_timestep(&self) -> f32 {
        self.fixed_timestep
    }

    /// Advances the accumulator by `frame_time` seconds and returns how many
    /// fixed steps should be simulated for this frame.
    ///
    /// `frame_time` is clamped to `0.0..=max(MAX_FRAME_TIME, fixed_timestep)`, so
    /// a single call can request at most 15 steps at 60 Hz and never less than
    /// one step for a coarser timestep. A stopped loop always returns `0`.
    pub fn advance(&mut self, frame_time: f32) -> u32 {
        if !self.running {
            return 0;
        }

        let max_frame_time = MAX_FRAME_TIME.max(self.fixed_timestep);
        let frame_time = if frame_time.is_finite() && frame_time > 0.0 {
            frame_time.min(max_frame_time)
        } else {
            0.0
        };

        self.accumulator += frame_time;

        let mut steps = 0u32;
        while self.accumulator >= self.fixed_timestep {
            self.accumulator -= self.fixed_timestep;
            steps += 1;
        }
        steps
    }

    /// Leftover time that did not fill a whole step, in seconds.
    ///
    /// Useful for interpolating rendering between two simulation states.
    pub fn accumulator(&self) -> f32 {
        self.accumulator
    }

    /// Milliseconds to sleep so that a frame whose *work* took `work_time`
    /// seconds still occupies exactly one fixed timestep.
    ///
    /// `work_time` must be the time spent computing the frame, **not** the
    /// previous frame period: including the previous sleep here makes the limiter
    /// converge to half the budget (sleep *and* work both counted against the same
    /// target).
    ///
    /// Returns `0` when the frame already overran the budget, when the loop is
    /// stopped, or when `work_time` is not a finite, non-negative number.
    pub fn sleep_ms(&self, work_time: f32) -> u32 {
        if !self.running || !work_time.is_finite() || work_time < 0.0 {
            return 0;
        }

        let remaining = self.fixed_timestep - work_time;
        if remaining <= 0.0 {
            return 0;
        }

        (remaining * 1000.0).round() as u32
    }

    /// Interpolation factor in `0.0..1.0` for the current frame.
    pub fn alpha(&self) -> f32 {
        (self.accumulator / self.fixed_timestep).clamp(0.0, 1.0)
    }

    /// Handles a user intent that means "leave the loop".
    ///
    /// `Escape` and a window-close request stop the loop; anything else is
    /// ignored. Taking [`QuitSignal`] instead of an SDL event keeps this module
    /// free of `sdl2` (see the layering rules in the crate docs).
    pub fn handle_quit_signal(&mut self, signal: QuitSignal) {
        if signal != QuitSignal::None {
            self.request_quit();
        }
    }

    /// Asks the loop to stop after the current iteration.
    pub fn request_quit(&mut self) {
        self.running = false;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_frame_advances_exactly_one_step() {
        let mut game_loop = GameLoop::new();

        assert_eq!(game_loop.advance(FIXED_TIMESTEP), 1);
        assert!(game_loop.accumulator() < FIXED_TIMESTEP);
    }

    #[test]
    fn half_a_step_accumulates_without_stepping() {
        let mut game_loop = GameLoop::new();

        assert_eq!(game_loop.advance(FIXED_TIMESTEP / 2.0), 0);
        assert_eq!(game_loop.advance(FIXED_TIMESTEP / 2.0), 1);
    }

    #[test]
    fn a_long_frame_is_clamped_to_max_frame_time() {
        let mut game_loop = GameLoop::new();

        let steps = game_loop.advance(10.0);

        // 0.25 s of catch-up at 1/60 s per step; float rounding may land on
        // either side of the 15th step, but never beyond it.
        assert!((14..=15).contains(&steps), "unexpected step count: {steps}");
        assert!(game_loop.accumulator() < FIXED_TIMESTEP);
    }

    #[test]
    fn negative_and_non_finite_frame_times_produce_no_steps() {
        let mut game_loop = GameLoop::new();

        for bad in [-5.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(game_loop.advance(bad), 0, "no steps expected for {bad}");
        }
        assert_eq!(game_loop.accumulator(), 0.0);
    }

    #[test]
    fn a_stopped_loop_never_advances() {
        let mut game_loop = GameLoop::new();
        game_loop.request_quit();

        assert_eq!(game_loop.advance(MAX_FRAME_TIME), 0);
        assert!(!game_loop.is_running());
    }

    #[test]
    fn quit_signals_stop_the_loop() {
        for signal in [QuitSignal::WindowClose, QuitSignal::Escape] {
            let mut game_loop = GameLoop::new();
            game_loop.handle_quit_signal(signal);
            assert!(!game_loop.is_running(), "{signal:?} must stop the loop");
        }
    }

    #[test]
    fn no_signal_keeps_the_loop_running() {
        let mut game_loop = GameLoop::new();

        game_loop.handle_quit_signal(QuitSignal::None);

        assert!(game_loop.is_running());
    }

    #[test]
    fn alpha_is_always_a_fraction() {
        let mut game_loop = GameLoop::new();
        game_loop.advance(FIXED_TIMESTEP * 3.5);

        let alpha = game_loop.alpha();
        assert!((0.0..=1.0).contains(&alpha));
    }

    #[test]
    fn custom_fixed_timestep_is_used_for_stepping() {
        let mut game_loop = GameLoop::new()
            .with_fixed_timestep(0.5)
            .expect("0.5 s is a valid timestep");

        // A one-second frame is capped at one 0.5 s step: the loop never tries to
        // catch up with more than a single step of a coarse timestep.
        assert_eq!(game_loop.advance(1.0), 1);
        assert_eq!(game_loop.advance(0.5), 1);
    }

    #[test]
    fn invalid_fixed_timesteps_are_rejected() {
        // The tiny values are the ones that used to be accepted and then make `advance`
        // loop hundreds of millions of times for one frame.
        for bad in [
            0.0,
            -1.0,
            f32::NAN,
            f32::INFINITY,
            1.0e-9,
            MIN_FIXED_TIMESTEP / 2.0,
        ] {
            assert!(
                GameLoop::new().with_fixed_timestep(bad).is_err(),
                "{bad} must be rejected"
            );
        }

        // The documented floor is accepted, so the check cannot become "reject 1000 Hz".
        assert!(GameLoop::new()
            .with_fixed_timestep(MIN_FIXED_TIMESTEP)
            .is_ok());
    }

    #[test]
    fn sleep_ms_pads_a_short_frame_up_to_the_budget() {
        let game_loop = GameLoop::new();

        // A frame that did no work still has to wait the whole budget.
        assert_eq!(
            game_loop.sleep_ms(0.0),
            (FIXED_TIMESTEP * 1000.0).round() as u32
        );
        // Half the budget spent → roughly the other half to sleep.
        let half = game_loop.sleep_ms(FIXED_TIMESTEP / 2.0);
        assert!((8..=9).contains(&half), "unexpected sleep: {half}");
    }

    #[test]
    fn sleep_ms_is_zero_when_the_frame_overran_the_budget() {
        let game_loop = GameLoop::new();

        assert_eq!(game_loop.sleep_ms(FIXED_TIMESTEP), 0);
        assert_eq!(game_loop.sleep_ms(1.0), 0);
    }

    #[test]
    fn sleep_ms_ignores_invalid_work_times_and_stopped_loops() {
        let game_loop = GameLoop::new();
        for bad in [-1.0, f32::NAN, f32::INFINITY] {
            assert_eq!(game_loop.sleep_ms(bad), 0, "unexpected sleep for {bad}");
        }

        let mut stopped = GameLoop::new();
        stopped.request_quit();
        assert_eq!(stopped.sleep_ms(0.0), 0);
    }

    #[test]
    fn default_loop_matches_new() {
        let game_loop = GameLoop::default();

        assert!(game_loop.is_running());
        assert_eq!(game_loop.fixed_timestep(), FIXED_TIMESTEP);
        assert_eq!(game_loop.accumulator(), 0.0);
    }
}
