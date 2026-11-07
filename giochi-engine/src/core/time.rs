//! Frame timing.

use std::time::Instant;

/// Timing snapshot for a single frame.
///
/// # C ABI
///
/// ```c
/// typedef struct GiochiTime {
///     float    delta, elapsed;
///     uint64_t frame_count;
///     uint32_t _reserved[4];
/// } GiochiTime;
/// ```
///
/// Size: 32 bytes, alignment: 8 bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Time {
    /// Duration of the last frame, in seconds.
    ///
    /// Negative and non-finite inputs are treated as `0.0`, so `delta` is always
    /// a sane, non-decreasing measurement. It is *not* clamped to
    /// [`crate::core::game_loop::MAX_FRAME_TIME`]: the loop clamps what it feeds
    /// to the simulation, while `delta` reports what actually happened.
    pub delta: f32,
    /// Total simulated time since the clock was created or last reset, in
    /// seconds.
    pub elapsed: f32,
    /// Number of frames completed since the clock was created or last reset.
    pub frame_count: u64,
    /// Reserved for future ABI-compatible fields. Must be zero.
    pub _reserved: [u32; 4],
}

impl Time {
    /// Creates a zeroed clock.
    pub const fn new() -> Self {
        Self {
            delta: 0.0,
            elapsed: 0.0,
            frame_count: 0,
            _reserved: [0; 4],
        }
    }

    /// Resets the accumulated time and the frame counter.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Advances the clock by one frame of `frame_time` seconds.
    ///
    /// Negative, `NaN` and infinite values are ignored (treated as `0.0`) so a
    /// bad measurement can never corrupt `elapsed`. `frame_count` saturates
    /// instead of wrapping.
    pub fn advance(&mut self, frame_time: f32) {
        let delta = sanitize(frame_time);
        self.delta = delta;
        self.elapsed += delta;
        self.frame_count = self.frame_count.saturating_add(1);
    }
}

/// Returns `value` when it is a finite, non-negative duration; `0.0` otherwise.
fn sanitize(value: f32) -> f32 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        0.0
    }
}

/// Wall-clock source used to measure the real duration of a frame.
///
/// Uses `std::time::Instant` (CLOCK_MONOTONIC) so it works identically on
/// development machines and on the target consoles, and needs no SDL timer.
///
/// # C ABI
///
/// Not exposed: frame pacing belongs to the loop, and a C host that owns its own
/// pacing calls `giochi_engine_step(frame_time)` with the time *it* measured.
/// `#[repr(C)]` and `_reserved` are kept anyway, because R1/R3 apply to every public
/// struct in this crate regardless of whether it ever reaches the boundary.
#[repr(C)]
#[derive(Debug, Clone)]
pub struct FrameClock {
    last: Instant,
    /// Reserved for future ABI-compatible fields. Must be zero.
    _reserved: [u32; 4],
}

impl FrameClock {
    /// Starts the clock at the current instant.
    pub fn new() -> Self {
        Self {
            last: Instant::now(),
            _reserved: [0; 4],
        }
    }

    /// Returns the seconds elapsed since the previous call and restarts the
    /// measurement.
    ///
    /// The value is always finite and non-negative.
    pub fn tick(&mut self) -> f32 {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last);
        self.last = now;
        let seconds = elapsed.as_secs_f32();
        if seconds.is_finite() && seconds >= 0.0 {
            seconds
        } else {
            0.0
        }
    }

    /// Restarts the measurement without reporting the elapsed time.
    pub fn reset(&mut self) {
        self.last = Instant::now();
    }
}

impl Default for FrameClock {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_clock_is_zeroed() {
        let time = Time::new();

        assert_eq!(time.delta, 0.0);
        assert_eq!(time.elapsed, 0.0);
        assert_eq!(time.frame_count, 0);
        assert_eq!(time._reserved, [0; 4]);
    }

    #[test]
    fn advance_accumulates_elapsed_and_counts_frames() {
        let mut time = Time::new();

        time.advance(0.5);
        time.advance(0.25);

        assert_eq!(time.delta, 0.25);
        assert!((time.elapsed - 0.75).abs() < 1e-6);
        assert_eq!(time.frame_count, 2);
    }

    #[test]
    fn advance_ignores_negative_and_non_finite_values() {
        let mut time = Time::new();
        time.advance(1.0);

        for bad in [-1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            time.advance(bad);
            assert_eq!(time.delta, 0.0, "delta must be sanitised for {bad}");
        }

        assert!((time.elapsed - 1.0).abs() < 1e-6);
        assert_eq!(time.frame_count, 5);
    }

    #[test]
    fn reset_clears_every_field_and_keeps_reserved_zero() {
        let mut time = Time::new();
        time.advance(1.0);

        time.reset();

        assert_eq!(time, Time::new());
    }

    #[test]
    fn frame_clock_reports_a_non_negative_delta() {
        let mut clock = FrameClock::new();
        let first = clock.tick();
        let second = clock.tick();

        assert!(first >= 0.0);
        assert!(second >= 0.0);
        assert!(first.is_finite() && second.is_finite());
    }

    #[test]
    fn frame_clock_reset_does_not_produce_huge_delta() {
        let mut clock = FrameClock::new();
        std::thread::sleep(std::time::Duration::from_millis(2));
        clock.reset();

        assert!(clock.tick() < 0.1);
    }
}
