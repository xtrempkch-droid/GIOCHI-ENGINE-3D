//! Frame profiling and the debug overlay (phase 15).
//!
//! The module is built around three pieces:
//!
//! * [`Profiler`] measures scopes and keeps a rolling window of frame samples;
//! * [`FrameStats`] and [`FrameTimeSummary`] are the data it reports;
//! * [`DebugOverlay`] draws those numbers over the game.
//!
//! # Cost
//!
//! With the `profiling` feature enabled, a frame costs a handful of
//! `Instant::now()` calls and one uncontended mutex lock in [`Profiler::end_frame`].
//! With the feature disabled, every method is an empty inlined body and the renderer
//! counters are not incremented — measured builds and shipped builds run the same
//! source, but only the measured build pays for the measurement (ADR-042).
//!
//! # Example
//!
//! The engine wires this up automatically; a host that owns its own loop can do the
//! same:
//!
//! ```no_run
//! use std::time::Duration;
//! use giochi_engine::perf::{FrameCounters, Profiler};
//!
//! let mut profiler = Profiler::new();
//! profiler.begin_frame();
//! profiler.begin_scope("update");
//! // ... update the world ...
//! profiler.end_scope();
//! let stats = profiler.end_frame(Duration::from_millis(16), FrameCounters::new());
//! assert_eq!(stats.frames, 1);
//! ```

mod frame_stats;
mod overlay;
mod profiler;
mod text;

pub use frame_stats::{FrameCounters, FrameStats, FrameTimeSummary};
pub use overlay::DebugOverlay;
pub use profiler::Profiler;

/// Number of frame samples kept for min/max/avg/p50/p99 aggregation.
///
/// Sixty samples is one second at 60 Hz: long enough to smooth a hitch and short
/// enough that the summary follows a changing workload.
pub const FRAME_WINDOW: usize = 60;
