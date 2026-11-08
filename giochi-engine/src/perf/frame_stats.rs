//! Per-frame statistics and counters.
//!
//! The types here are the data half of the phase-15 profiler: [`FrameStats`] is what
//! one frame cost, [`FrameTimeSummary`] is the aggregation over the last
//! [`FRAME_WINDOW`](crate::perf::FRAME_WINDOW) frames, and [`FrameCounters`] is what
//! a renderer records while it draws.
//!
//! # Why nanoseconds instead of `Duration`
//!
//! The prompt describes `FrameStats` with `std::time::Duration` fields. A `Duration`
//! has no guaranteed C layout, and these types are meant to be exposed through the C
//! ABI in phase 18c (`GiochiFrameStats` stores `frame_time_ns` as a `uint64_t`). Storing
//! `u64` nanoseconds here and exposing `Duration` through accessors keeps the struct
//! genuinely C-shaped without losing the idiomatic Rust view. See ADR-042.

use std::time::Duration;

/// Cost and workload of one frame (or of several merged frames).
///
/// Time is stored as whole nanoseconds so the struct remains `#[repr(C)]`-ready; use
/// the `*_time()` accessors for a [`Duration`] and the `*_ms()` accessors for
/// milliseconds.
///
/// All fields are zero for a frame that was not measured. With the `profiling`
/// feature disabled the engine still returns this type, but every value is zero.
///
/// # C ABI
///
/// Not exposed in ABI v2. Phase 18c projects a subset of these fields as
/// `GiochiFrameStats` (ABI v3) without changing the Rust type.
///
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FrameStats {
    /// Wall-clock frame period, in nanoseconds.
    pub frame_time_ns: u64,
    /// Time the engine spent computing the frame, in nanoseconds.
    pub cpu_time_ns: u64,
    /// GPU time hint, in nanoseconds.
    ///
    /// There is no GPU timer query in GLES 2.0 or through `sdl2::render`, so the
    /// engine reports `0` unless a backend learns how to measure it.
    pub gpu_time_hint_ns: u64,
    /// Time spent in the `"update"` scope, in nanoseconds.
    pub update_ns: u64,
    /// Time spent in the `"render2d"` scope, in nanoseconds.
    pub render2d_ns: u64,
    /// Time spent in the `"render3d"` scope, in nanoseconds.
    pub render3d_ns: u64,
    /// Time spent in audio calls during the frame, in nanoseconds.
    pub audio_ns: u64,
    /// Time spent in scopes other than the four named systems, in nanoseconds.
    pub other_ns: u64,
    /// Instantaneous frames per second (`1 / frame_time`).
    pub fps: f32,
    /// Renderer draw calls issued during the frame.
    pub draw_calls: u32,
    /// Sprites drawn during the frame.
    pub sprites_drawn: u32,
    /// Meshes drawn during the frame.
    pub meshes_drawn: u32,
    /// Vertices submitted to the renderer during the frame.
    pub vertices_uploaded: u64,
    /// Bytes submitted to the renderer during the frame.
    pub bytes_uploaded: u64,
    /// Number of frames this sample covers. `1` for a single frame; larger after
    /// [`FrameStats::merge`].
    pub frames: u32,
    /// Reserved for future ABI-compatible fields. Must be zero.
    pub _reserved: [u32; 8],
}

impl FrameStats {
    /// Creates an empty sample with the reserved tail zeroed.
    pub const fn new() -> Self {
        Self {
            frame_time_ns: 0,
            cpu_time_ns: 0,
            gpu_time_hint_ns: 0,
            update_ns: 0,
            render2d_ns: 0,
            render3d_ns: 0,
            audio_ns: 0,
            other_ns: 0,
            fps: 0.0,
            draw_calls: 0,
            sprites_drawn: 0,
            meshes_drawn: 0,
            vertices_uploaded: 0,
            bytes_uploaded: 0,
            frames: 0,
            _reserved: [0; 8],
        }
    }

    /// Wall-clock frame period.
    pub fn frame_time(&self) -> Duration {
        nanos(self.frame_time_ns)
    }

    /// Time the engine spent computing the frame.
    pub fn cpu_time(&self) -> Duration {
        nanos(self.cpu_time_ns)
    }

    /// GPU time hint; `0` when the backend cannot measure it.
    pub fn gpu_time_hint(&self) -> Duration {
        nanos(self.gpu_time_hint_ns)
    }

    /// Time spent in the `"update"` scope.
    pub fn update_time(&self) -> Duration {
        nanos(self.update_ns)
    }

    /// Time spent in the `"render2d"` scope.
    pub fn render2d_time(&self) -> Duration {
        nanos(self.render2d_ns)
    }

    /// Time spent in the `"render3d"` scope.
    pub fn render3d_time(&self) -> Duration {
        nanos(self.render3d_ns)
    }

    /// Time spent in audio calls.
    pub fn audio_time(&self) -> Duration {
        nanos(self.audio_ns)
    }

    /// Time spent in scopes that are not one of the four named systems.
    pub fn other_time(&self) -> Duration {
        nanos(self.other_ns)
    }

    /// [`FrameStats::frame_time`] in milliseconds.
    pub fn frame_time_ms(&self) -> f32 {
        ms(self.frame_time_ns)
    }

    /// [`FrameStats::cpu_time`] in milliseconds.
    pub fn cpu_time_ms(&self) -> f32 {
        ms(self.cpu_time_ns)
    }

    /// [`FrameStats::gpu_time_hint`] in milliseconds.
    pub fn gpu_time_hint_ms(&self) -> f32 {
        ms(self.gpu_time_hint_ns)
    }

    /// [`FrameStats::update_time`] in milliseconds.
    pub fn update_time_ms(&self) -> f32 {
        ms(self.update_ns)
    }

    /// [`FrameStats::render2d_time`] in milliseconds.
    pub fn render2d_time_ms(&self) -> f32 {
        ms(self.render2d_ns)
    }

    /// [`FrameStats::render3d_time`] in milliseconds.
    pub fn render3d_time_ms(&self) -> f32 {
        ms(self.render3d_ns)
    }

    /// [`FrameStats::audio_time`] in milliseconds.
    pub fn audio_time_ms(&self) -> f32 {
        ms(self.audio_ns)
    }

    /// [`FrameStats::other_time`] in milliseconds.
    pub fn other_time_ms(&self) -> f32 {
        ms(self.other_ns)
    }

    /// Whether this sample covers no frames.
    pub fn is_empty(&self) -> bool {
        self.frames == 0
    }

    /// Combines `other` into this sample.
    ///
    /// Counters and durations add (merging two consecutive frames yields the total
    /// cost of both), and `fps` is recomputed from the summed frame count and frame
    /// time. That makes `merge` the right operation for accumulating a window of
    /// samples; it is not a weighted average.
    pub fn merge(&mut self, other: &FrameStats) {
        self.frame_time_ns = self.frame_time_ns.saturating_add(other.frame_time_ns);
        self.cpu_time_ns = self.cpu_time_ns.saturating_add(other.cpu_time_ns);
        self.gpu_time_hint_ns = self.gpu_time_hint_ns.saturating_add(other.gpu_time_hint_ns);
        self.update_ns = self.update_ns.saturating_add(other.update_ns);
        self.render2d_ns = self.render2d_ns.saturating_add(other.render2d_ns);
        self.render3d_ns = self.render3d_ns.saturating_add(other.render3d_ns);
        self.audio_ns = self.audio_ns.saturating_add(other.audio_ns);
        self.other_ns = self.other_ns.saturating_add(other.other_ns);
        self.draw_calls = self.draw_calls.saturating_add(other.draw_calls);
        self.sprites_drawn = self.sprites_drawn.saturating_add(other.sprites_drawn);
        self.meshes_drawn = self.meshes_drawn.saturating_add(other.meshes_drawn);
        self.vertices_uploaded = self
            .vertices_uploaded
            .saturating_add(other.vertices_uploaded);
        self.bytes_uploaded = self.bytes_uploaded.saturating_add(other.bytes_uploaded);
        self.frames = self.frames.saturating_add(other.frames);
        self.fps = fps(self.frames, self.frame_time_ns);
    }
}

/// Aggregation of the last [`FRAME_WINDOW`](crate::perf::FRAME_WINDOW) frames.
///
/// Percentiles are computed over the frame-time samples in the window: `p50` is the
/// median and `p99` is the slow-tail sample that a stutter shows up in.
///
/// # C ABI
///
/// Not exposed in ABI v2; phase 18c can project it alongside `GiochiFrameStats`.
///
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FrameTimeSummary {
    /// Number of frames in the window.
    pub samples: u32,
    /// Shortest frame time, in nanoseconds.
    pub min_ns: u64,
    /// Longest frame time, in nanoseconds.
    pub max_ns: u64,
    /// Mean frame time, in nanoseconds.
    pub avg_ns: u64,
    /// Median frame time, in nanoseconds.
    pub p50_ns: u64,
    /// 99th-percentile frame time, in nanoseconds.
    pub p99_ns: u64,
    /// Throughput over the whole window, in frames per second.
    pub fps: f32,
    /// Reserved for future ABI-compatible fields. Must be zero.
    pub _reserved: [u32; 4],
}

impl FrameTimeSummary {
    /// Shortest frame time in the window.
    pub fn min(&self) -> Duration {
        nanos(self.min_ns)
    }

    /// Longest frame time in the window.
    pub fn max(&self) -> Duration {
        nanos(self.max_ns)
    }

    /// Mean frame time in the window.
    pub fn average(&self) -> Duration {
        nanos(self.avg_ns)
    }

    /// Median frame time in the window.
    pub fn p50(&self) -> Duration {
        nanos(self.p50_ns)
    }

    /// 99th-percentile frame time in the window.
    pub fn p99(&self) -> Duration {
        nanos(self.p99_ns)
    }

    /// [`FrameTimeSummary::min`] in milliseconds.
    pub fn min_ms(&self) -> f32 {
        ms(self.min_ns)
    }

    /// [`FrameTimeSummary::max`] in milliseconds.
    pub fn max_ms(&self) -> f32 {
        ms(self.max_ns)
    }

    /// [`FrameTimeSummary::average`] in milliseconds.
    pub fn average_ms(&self) -> f32 {
        ms(self.avg_ns)
    }

    /// [`FrameTimeSummary::p50`] in milliseconds.
    pub fn p50_ms(&self) -> f32 {
        ms(self.p50_ns)
    }

    /// [`FrameTimeSummary::p99`] in milliseconds.
    pub fn p99_ms(&self) -> f32 {
        ms(self.p99_ns)
    }
}

/// Workload counters recorded by a renderer.
///
/// The 2D renderer records draw calls and sprites; the 3D renderer records draw
/// calls, meshes, vertices and bytes. A host that drives both merges the two with
/// [`FrameCounters::merge`] before handing the result to
/// [`Profiler::end_frame`](crate::perf::Profiler::end_frame).
///
/// # C ABI
///
/// Not exposed in ABI v2.
///
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameCounters {
    /// Backend draw calls issued.
    pub draw_calls: u32,
    /// Sprites drawn.
    pub sprites_drawn: u32,
    /// Meshes drawn.
    pub meshes_drawn: u32,
    /// Vertices submitted to the renderer.
    pub vertices_uploaded: u64,
    /// Bytes submitted to the renderer.
    pub bytes_uploaded: u64,
    /// Reserved for future ABI-compatible fields. Must be zero.
    pub _reserved: [u32; 4],
}

impl FrameCounters {
    /// Creates a zeroed set of counters.
    pub const fn new() -> Self {
        Self {
            draw_calls: 0,
            sprites_drawn: 0,
            meshes_drawn: 0,
            vertices_uploaded: 0,
            bytes_uploaded: 0,
            _reserved: [0; 4],
        }
    }

    /// Clears every counter. Called at the start of a frame.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Records one draw call that is neither a sprite nor a mesh (a rectangle, a
    /// line, a fill batch).
    pub fn record_draw_call(&mut self) {
        self.draw_calls = self.draw_calls.saturating_add(1);
    }

    /// Records one sprite draw call.
    ///
    /// The 2D backend does not expose how many vertices it submits, so the vertex
    /// and byte counters stay untouched.
    pub fn record_sprite(&mut self) {
        self.draw_calls = self.draw_calls.saturating_add(1);
        self.sprites_drawn = self.sprites_drawn.saturating_add(1);
    }

    /// Records one mesh draw call and the vertices and bytes it submitted.
    pub fn record_mesh(&mut self, vertices: u64, bytes: u64) {
        self.draw_calls = self.draw_calls.saturating_add(1);
        self.meshes_drawn = self.meshes_drawn.saturating_add(1);
        self.vertices_uploaded = self.vertices_uploaded.saturating_add(vertices);
        self.bytes_uploaded = self.bytes_uploaded.saturating_add(bytes);
    }

    /// Adds `other` into this set.
    pub fn merge(&mut self, other: &FrameCounters) {
        self.draw_calls = self.draw_calls.saturating_add(other.draw_calls);
        self.sprites_drawn = self.sprites_drawn.saturating_add(other.sprites_drawn);
        self.meshes_drawn = self.meshes_drawn.saturating_add(other.meshes_drawn);
        self.vertices_uploaded = self
            .vertices_uploaded
            .saturating_add(other.vertices_uploaded);
        self.bytes_uploaded = self.bytes_uploaded.saturating_add(other.bytes_uploaded);
    }
}

fn nanos(value: u64) -> Duration {
    Duration::from_nanos(value)
}

fn ms(value: u64) -> f32 {
    value as f32 / 1_000_000.0
}

pub(crate) fn fps(frames: u32, frame_time_ns: u64) -> f32 {
    if frames == 0 || frame_time_ns == 0 {
        0.0
    } else {
        frames as f32 * 1_000_000_000.0 / frame_time_ns as f32
    }
}

/// Selects the `percentile`-th sample (0.0–1.0) from an ascending slice.
#[cfg(feature = "profiling")]
pub(crate) fn percentile(sorted: &[u64], percentile: f32) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = (percentile.clamp(0.0, 1.0) * sorted.len() as f32).ceil() as usize;
    let index = rank.saturating_sub(1).min(sorted.len() - 1);
    sorted[index]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_sample_is_empty_and_zeroed() {
        let stats = FrameStats::new();

        assert!(stats.is_empty());
        assert_eq!(stats.frame_time(), Duration::ZERO);
        assert_eq!(stats._reserved, [0; 8]);
    }

    #[test]
    fn merge_adds_counters_and_recomputes_fps() {
        let mut total = FrameStats {
            frame_time_ns: 16_666_667,
            fps: 60.0,
            frames: 1,
            draw_calls: 10,
            sprites_drawn: 200,
            vertices_uploaded: 800,
            bytes_uploaded: 12_800,
            ..FrameStats::new()
        };
        let second = FrameStats {
            frame_time_ns: 16_666_667,
            fps: 60.0,
            frames: 1,
            draw_calls: 5,
            sprites_drawn: 100,
            vertices_uploaded: 400,
            bytes_uploaded: 6_400,
            ..FrameStats::new()
        };

        total.merge(&second);

        assert_eq!(total.frames, 2);
        assert_eq!(total.frame_time_ns, 33_333_334);
        assert_eq!(total.draw_calls, 15);
        assert_eq!(total.sprites_drawn, 300);
        assert_eq!(total.vertices_uploaded, 1_200);
        assert_eq!(total.bytes_uploaded, 19_200);
        assert!((total.fps - 60.0).abs() < 0.01, "fps: {}", total.fps);
    }

    #[test]
    fn merging_an_empty_sample_is_a_no_op() {
        let mut stats = FrameStats {
            frames: 1,
            frame_time_ns: 16_000_000,
            fps: 62.5,
            draw_calls: 3,
            ..FrameStats::new()
        };
        let before = stats;

        stats.merge(&FrameStats::new());

        assert_eq!(stats, before);
    }

    #[test]
    fn counters_record_their_workload() {
        let mut counters = FrameCounters::new();
        counters.record_draw_call();
        counters.record_sprite();
        counters.record_mesh(36, 1_152);

        assert_eq!(counters.draw_calls, 3);
        assert_eq!(counters.sprites_drawn, 1);
        assert_eq!(counters.meshes_drawn, 1);
        assert_eq!(counters.vertices_uploaded, 36);
        assert_eq!(counters.bytes_uploaded, 1_152);
    }

    #[test]
    fn counters_merge_and_reset() {
        let mut left = FrameCounters::new();
        left.record_sprite();
        let mut right = FrameCounters::new();
        right.record_mesh(3, 96);

        left.merge(&right);
        assert_eq!(left.draw_calls, 2);
        assert_eq!(left.sprites_drawn, 1);
        assert_eq!(left.meshes_drawn, 1);

        left.reset();
        assert_eq!(left, FrameCounters::new());
    }

    #[cfg(feature = "profiling")]
    #[test]
    fn percentile_handles_the_edges() {
        let sorted = [10, 20, 30, 40, 50];

        assert_eq!(percentile(&sorted, 0.0), 10);
        assert_eq!(percentile(&sorted, 0.5), 30);
        assert_eq!(percentile(&sorted, 1.0), 50);
        assert_eq!(percentile(&[], 0.5), 0);
    }

    #[test]
    fn milliseconds_are_nanoseconds_div_1e6() {
        let stats = FrameStats {
            frame_time_ns: 16_666_667,
            cpu_time_ns: 2_500_000,
            ..FrameStats::new()
        };

        assert!((stats.frame_time_ms() - 16.666_667).abs() < 0.001);
        assert_eq!(stats.cpu_time_ms(), 2.5);
    }

    #[test]
    fn frame_window_is_the_documented_size() {
        use crate::perf::FRAME_WINDOW;

        assert_eq!(FRAME_WINDOW, 60);
    }
}
