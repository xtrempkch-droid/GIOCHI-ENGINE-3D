//! Frame timing.

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
    pub delta: f32,
    /// Total time since the clock was created or last reset, in seconds.
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
    /// Implemented in phase 1.
    pub fn advance(&mut self, _frame_time: f32) {
        todo!("fase 1: avancar o relogio de frame")
    }
}
