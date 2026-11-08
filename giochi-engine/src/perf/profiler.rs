//! Structured frame instrumentation.
//!
//! [`Profiler`] measures the frame as it happens: [`Profiler::begin_frame`] starts the
//! clock, [`Profiler::begin_scope`] / [`Profiler::end_scope`] bracket a system, and
//! [`Profiler::end_frame`] closes the sample, folds in the renderer counters and
//! appends it to the rolling window used for percentiles.
//!
//! # Feature gating
//!
//! Instrumentation is compiled behind the `profiling` feature. Without it every method
//! is an empty inlined body, [`Profiler::snapshot`] returns a zeroed [`FrameStats`]
//! and [`Profiler::summary`] returns a zeroed [`FrameTimeSummary`] — the engine calls
//! the same code in both builds, and the optimizer removes the calls in the default
//! build. This is what "no overhead without `profiling`" means in practice; it is also
//! why the profiler is explicitly opt-in rather than "on in debug, off in release",
//! which Cargo cannot express as a default feature (ADR-042).
//!
//! # Scope names
//!
//! The four names the overlay understands are `"update"`, `"render2d"`, `"render3d"`
//! and `"audio"`. Any other name is accumulated in
//! [`FrameStats::other_ns`](crate::perf::FrameStats::other_ns) instead of being
//! dropped, so a host can instrument extra systems without inventing new fields.

#[cfg(feature = "profiling")]
use std::sync::Mutex;
use std::time::Duration;
#[cfg(feature = "profiling")]
use std::time::Instant;

#[cfg(feature = "profiling")]
use crate::perf::frame_stats::{fps, percentile};
#[cfg(feature = "profiling")]
use crate::perf::FRAME_WINDOW;
use crate::perf::{FrameCounters, FrameStats, FrameTimeSummary};

/// Rolling profiler owned by the frame loop.
///
/// The window is a `Mutex<Vec<FrameStats>>` even though the engine is single-threaded:
/// the prompt requires the collection to be thread-safe, and it costs one uncontended
/// lock per frame. The mutex is only locked when a sample is appended or the summary
/// is read, never while drawing.
///
/// # C ABI
///
/// Not exposed. C consumers get `giochi_engine_last_frame_stats` in phase 18c, which
/// copies [`FrameStats`] into `GiochiFrameStats`.
#[repr(C)]
pub struct Profiler {
    #[cfg(feature = "profiling")]
    frame_start: Instant,
    #[cfg(feature = "profiling")]
    frame_active: bool,
    #[cfg(feature = "profiling")]
    scopes: Vec<Scope>,
    #[cfg(feature = "profiling")]
    current: FrameStats,
    #[cfg(feature = "profiling")]
    last: FrameStats,
    #[cfg(feature = "profiling")]
    audio_ns: u64,
    #[cfg(feature = "profiling")]
    history: Mutex<Vec<FrameStats>>,
    _reserved: [u32; 4],
}

impl std::fmt::Debug for Profiler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut builder = f.debug_struct("Profiler");
        builder.field("enabled", &cfg!(feature = "profiling"));
        #[cfg(feature = "profiling")]
        builder.field("samples", &self.history_len());
        builder.finish_non_exhaustive()
    }
}

impl Profiler {
    /// Creates a profiler with an empty window.
    pub fn new() -> Self {
        Self {
            #[cfg(feature = "profiling")]
            frame_start: Instant::now(),
            #[cfg(feature = "profiling")]
            frame_active: false,
            #[cfg(feature = "profiling")]
            scopes: Vec::new(),
            #[cfg(feature = "profiling")]
            current: FrameStats::new(),
            #[cfg(feature = "profiling")]
            last: FrameStats::new(),
            #[cfg(feature = "profiling")]
            audio_ns: 0,
            #[cfg(feature = "profiling")]
            history: Mutex::new(Vec::with_capacity(FRAME_WINDOW)),
            _reserved: [0; 4],
        }
    }

    /// Whether detailed collection is compiled in.
    pub fn is_enabled(&self) -> bool {
        cfg!(feature = "profiling")
    }

    /// Starts a frame sample and clears the previous frame's scopes and counters.
    ///
    /// Any scope still open from the previous frame is discarded: a frame boundary is
    /// not allowed to inherit a half-open timer.
    pub fn begin_frame(&mut self) {
        #[cfg(feature = "profiling")]
        {
            self.frame_start = Instant::now();
            self.frame_active = true;
            self.scopes.clear();
            self.current = FrameStats::new();
            self.audio_ns = 0;
        }
    }

    /// Opens a named timing scope.
    ///
    /// Names are matched against the four known systems; anything else accumulates in
    /// [`FrameStats::other_ns`]. Unbalanced calls are tolerated — [`Profiler::end_frame`]
    /// closes whatever is still open — because a profiler must never be the reason a
    /// frame fails.
    pub fn begin_scope(&mut self, name: &str) {
        let _ = name;
        #[cfg(feature = "profiling")]
        {
            self.scopes.push(Scope {
                kind: ScopeKind::of(name),
                started: Instant::now(),
            });
        }
    }

    /// Closes the most recently opened scope.
    ///
    /// Does nothing when no scope is open.
    pub fn end_scope(&mut self) {
        #[cfg(feature = "profiling")]
        {
            if let Some(scope) = self.scopes.pop() {
                self.add_elapsed(scope.kind, scope.started.elapsed());
            } else {
                log::debug!("profiler: end_scope called with no open scope");
            }
        }
    }

    /// Adds the time spent inside audio API calls during this frame.
    ///
    /// The mixer measures its own calls because the engine lends the scene a
    /// `&mut AudioSystem` directly; there is no wrapper to time on the scene's behalf.
    /// The value is a subset of the `"update"` scope, not a sibling of it.
    pub fn add_audio_time(&mut self, time_ns: u64) {
        let _ = time_ns;
        #[cfg(feature = "profiling")]
        {
            self.audio_ns = self.audio_ns.saturating_add(time_ns);
        }
    }

    /// Closes the frame, records `counters` and appends the sample to the window.
    ///
    /// `frame_time` is the wall-clock period the engine measured (not the work time);
    /// it becomes [`FrameStats::frame_time_ns`] and determines [`FrameStats::fps`].
    /// The return value is the same sample the next [`Profiler::snapshot`] will report.
    pub fn end_frame(&mut self, frame_time: Duration, counters: FrameCounters) -> FrameStats {
        let _ = (frame_time, counters);
        #[cfg(feature = "profiling")]
        {
            while let Some(scope) = self.scopes.pop() {
                self.add_elapsed(scope.kind, scope.started.elapsed());
            }

            if self.frame_active {
                self.current.cpu_time_ns = nanos(self.frame_start.elapsed());
                self.frame_active = false;
            }

            self.current.frame_time_ns = nanos(frame_time);
            self.current.audio_ns = self.current.audio_ns.saturating_add(self.audio_ns);
            self.current.draw_calls = counters.draw_calls;
            self.current.sprites_drawn = counters.sprites_drawn;
            self.current.meshes_drawn = counters.meshes_drawn;
            self.current.vertices_uploaded = counters.vertices_uploaded;
            self.current.bytes_uploaded = counters.bytes_uploaded;
            self.current.frames = 1;
            self.current.fps = fps(1, self.current.frame_time_ns);

            self.last = self.current;
            let mut history = self
                .history
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            history.push(self.last);
            if history.len() > FRAME_WINDOW {
                history.remove(0);
            }
            self.last
        }
        #[cfg(not(feature = "profiling"))]
        {
            FrameStats::new()
        }
    }

    /// The most recently completed frame, or a zeroed sample before the first one.
    pub fn snapshot(&self) -> FrameStats {
        #[cfg(feature = "profiling")]
        {
            self.last
        }
        #[cfg(not(feature = "profiling"))]
        {
            FrameStats::new()
        }
    }

    /// Aggregation over the last [`FRAME_WINDOW`](crate::perf::FRAME_WINDOW) samples.
    ///
    /// Percentiles are computed from the frame-time samples in the window; a zeroed
    /// summary means "no frame has been measured yet".
    pub fn summary(&self) -> FrameTimeSummary {
        #[cfg(feature = "profiling")]
        {
            let history = self
                .history
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let mut times: Vec<u64> = history
                .iter()
                .map(|sample| sample.frame_time_ns)
                .filter(|time| *time > 0)
                .collect();
            if times.is_empty() {
                FrameTimeSummary::default()
            } else {
                times.sort_unstable();

                let frames: u32 = history.iter().map(|sample| sample.frames).sum();
                let total: u64 = times.iter().sum();
                let average = total / times.len() as u64;

                FrameTimeSummary {
                    samples: times.len() as u32,
                    min_ns: times[0],
                    max_ns: times[times.len() - 1],
                    avg_ns: average,
                    p50_ns: percentile(&times, 0.50),
                    p99_ns: percentile(&times, 0.99),
                    fps: fps(frames, total),
                    _reserved: [0; 4],
                }
            }
        }
        #[cfg(not(feature = "profiling"))]
        {
            FrameTimeSummary::default()
        }
    }

    /// Number of samples currently in the window (`0` without `profiling`).
    pub fn history_len(&self) -> usize {
        #[cfg(feature = "profiling")]
        {
            let history = self
                .history
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            history.len()
        }
        #[cfg(not(feature = "profiling"))]
        {
            0
        }
    }

    #[cfg(feature = "profiling")]
    fn add_elapsed(&mut self, kind: ScopeKind, elapsed: Duration) {
        let ns = nanos(elapsed);
        match kind {
            ScopeKind::Update => self.current.update_ns = self.current.update_ns.saturating_add(ns),
            ScopeKind::Render2d => {
                self.current.render2d_ns = self.current.render2d_ns.saturating_add(ns)
            }
            ScopeKind::Render3d => {
                self.current.render3d_ns = self.current.render3d_ns.saturating_add(ns)
            }
            ScopeKind::Audio => self.current.audio_ns = self.current.audio_ns.saturating_add(ns),
            ScopeKind::Other => self.current.other_ns = self.current.other_ns.saturating_add(ns),
        }
    }
}

impl Default for Profiler {
    fn default() -> Self {
        Self::new()
    }
}

/// Converts a [`Duration`] into saturating whole nanoseconds.
#[cfg(feature = "profiling")]
fn nanos(duration: Duration) -> u64 {
    duration.as_nanos().min(u64::MAX as u128) as u64
}

#[cfg(feature = "profiling")]
struct Scope {
    kind: ScopeKind,
    started: Instant,
}

#[cfg(feature = "profiling")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScopeKind {
    Update,
    Render2d,
    Render3d,
    Audio,
    Other,
}

#[cfg(feature = "profiling")]
impl ScopeKind {
    fn of(name: &str) -> Self {
        match name {
            "update" => Self::Update,
            "render2d" => Self::Render2d,
            "render3d" => Self::Render3d,
            "audio" => Self::Audio,
            _ => Self::Other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_profiler_has_no_samples() {
        let profiler = Profiler::new();

        assert!(profiler.snapshot().is_empty());
        assert_eq!(profiler.summary().samples, 0);
        assert_eq!(profiler.history_len(), 0);
        assert_eq!(profiler.snapshot()._reserved, [0; 8]);
    }

    #[cfg(feature = "profiling")]
    #[test]
    fn scopes_land_in_their_named_system() {
        let mut profiler = Profiler::new();
        profiler.begin_frame();
        profiler.add_elapsed(ScopeKind::Update, Duration::from_millis(3));
        profiler.add_elapsed(ScopeKind::Render2d, Duration::from_millis(4));
        profiler.add_elapsed(ScopeKind::Render3d, Duration::from_millis(5));
        profiler.add_elapsed(ScopeKind::Audio, Duration::from_millis(6));
        profiler.add_elapsed(ScopeKind::Other, Duration::from_millis(7));

        let stats = profiler.end_frame(Duration::from_millis(20), FrameCounters::new());

        assert_eq!(stats.update_ns, 3_000_000);
        assert_eq!(stats.render2d_ns, 4_000_000);
        assert_eq!(stats.render3d_ns, 5_000_000);
        assert_eq!(stats.audio_ns, 6_000_000);
        assert_eq!(stats.other_ns, 7_000_000);
        assert!((stats.fps - 50.0).abs() < 0.001);
    }

    #[cfg(feature = "profiling")]
    #[test]
    fn end_frame_folds_in_the_renderer_counters() {
        let mut profiler = Profiler::new();
        let counters = FrameCounters {
            draw_calls: 12,
            sprites_drawn: 1000,
            meshes_drawn: 2,
            vertices_uploaded: 72,
            bytes_uploaded: 2_304,
            ..FrameCounters::new()
        };

        let stats = profiler.end_frame(Duration::from_millis(16), counters);

        assert_eq!(stats.draw_calls, 12);
        assert_eq!(stats.sprites_drawn, 1_000);
        assert_eq!(stats.meshes_drawn, 2);
        assert_eq!(stats.vertices_uploaded, 72);
        assert_eq!(stats.bytes_uploaded, 2_304);
        assert_eq!(profiler.snapshot(), stats);
    }

    #[cfg(feature = "profiling")]
    #[test]
    fn an_unbalanced_scope_is_closed_at_the_frame_boundary() {
        let mut profiler = Profiler::new();
        profiler.begin_frame();
        profiler.begin_scope("render2d");

        // No `end_scope`: the frame boundary must still produce a sample.
        profiler.end_frame(Duration::from_millis(10), FrameCounters::new());

        assert_eq!(profiler.history_len(), 1);
        assert!(profiler.snapshot().frames == 1);
    }

    #[cfg(feature = "profiling")]
    #[test]
    fn the_window_keeps_the_last_sixty_frames() {
        let mut profiler = Profiler::new();
        for frame in 1..=70u64 {
            profiler.begin_frame();
            profiler.end_frame(Duration::from_millis(frame), FrameCounters::new());
        }

        assert_eq!(profiler.history_len(), FRAME_WINDOW);
    }

    #[cfg(feature = "profiling")]
    #[test]
    fn the_summary_reports_percentiles_over_the_window() {
        let mut profiler = Profiler::new();
        for frame in 1..=60u64 {
            profiler.begin_frame();
            profiler.end_frame(Duration::from_millis(frame), FrameCounters::new());
        }

        let summary = profiler.summary();

        assert_eq!(summary.samples, 60);
        assert_eq!(summary.min_ns, 1_000_000);
        assert_eq!(summary.max_ns, 60_000_000);
        assert_eq!(summary.avg_ns, 30_500_000);
        assert_eq!(summary.p50_ns, 30_000_000);
        assert_eq!(summary.p99_ns, 60_000_000);
        assert!(
            (summary.fps - 32.786_884).abs() < 0.01,
            "fps: {}",
            summary.fps
        );
    }

    #[cfg(feature = "profiling")]
    #[test]
    fn audio_time_accumulates_and_survives_until_the_frame_ends() {
        let mut profiler = Profiler::new();
        profiler.begin_frame();
        profiler.add_audio_time(2_000_000);
        profiler.add_audio_time(3_000_000);

        let stats = profiler.end_frame(Duration::from_millis(16), FrameCounters::new());

        assert_eq!(stats.audio_ns, 5_000_000);
    }

    #[cfg(not(feature = "profiling"))]
    #[test]
    fn without_the_feature_every_method_is_a_no_op() {
        let mut profiler = Profiler::new();
        profiler.begin_frame();
        profiler.begin_scope("update");
        profiler.add_audio_time(1_000_000);
        let stats = profiler.end_frame(Duration::from_millis(16), FrameCounters::new());
        profiler.end_scope();

        assert!(!profiler.is_enabled());
        assert!(stats.is_empty());
        assert_eq!(profiler.summary(), FrameTimeSummary::default());
        assert_eq!(profiler.history_len(), 0);
    }
}
