//! Phase-18 benchmarks: the per-frame cost the engine charges around a game's own work.
//!
//! Phase 16 and 17 measured how fast sprites and meshes go through the backend. This
//! measures what is left over: everything `Engine::step` does that is *not* the game's
//! scene — polling SDL, feeding `Input`, advancing the fixed-timestep loop, clearing,
//! the deferred `present`, the frame statistics ring and the profiler. When the frame
//! budget regresses, this is the series that says whether the regression came from the
//! engine or from the content.
//!
//! [`Engine::step`] is the same entry point the C ABI's `giochi_engine_step` calls, so
//! the numbers describe the frame an embedding host actually gets. It does not sleep,
//! unlike [`Engine::run_current`], so a benchmark of it is a benchmark of work.
//!
//! # Why `present_only` is part of the set
//!
//! Absolute times are only comparable on the machine that produced them. The pair is
//! not: `step_empty / present_only` is a ratio between two measurements taken by the
//! same binary, in the same process, on the same machine, seconds apart. The engine's
//! bookkeeping can only be the difference between them, so a regression in the engine
//! shows up as a rising ratio on *any* machine, while the machine itself cancels out.
//! `tests/perf_regression.rs` gates on that ratio for this reason.
//!
//! ```text
//! cargo bench -p giochi-engine --bench engine_overhead
//! cargo bench -p giochi-engine --bench engine_overhead -- --quick
//! ```

use std::cell::RefCell;
use std::time::Duration;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use giochi_engine::{Color, Engine, EngineConfig};

/// Canvas size, matching the other benchmarks so frames are comparable.
const WIDTH: u32 = 1280;
const HEIGHT: u32 = 720;

thread_local! {
    /// The engine polls SDL and drives one `EventPump` per process, so every benchmark
    /// in this binary has to share the single `Engine`. Criterion runs benchmark
    /// closures one at a time, so a `RefCell` is enough to lend it out.
    static ENGINE: RefCell<Option<Engine>> = const { RefCell::new(None) };
}

fn with_engine<R>(f: impl FnOnce(&mut Engine) -> R) -> R {
    ENGINE.with(|cell| {
        if cell.borrow().is_none() {
            *cell.borrow_mut() = Some(make_engine());
        }
        let mut borrow = cell.borrow_mut();
        f(borrow.as_mut().expect("just built"))
    })
}

fn make_engine() -> Engine {
    // Same backend selection as `sprites_2d`: the software path is deterministic and
    // needs no device. Set the variables before running cargo to measure the
    // accelerated path instead.
    if std::env::var_os("SDL_VIDEODRIVER").is_none() {
        std::env::set_var("SDL_VIDEODRIVER", "dummy");
    }
    if std::env::var_os("SDL_RENDER_DRIVER").is_none() {
        std::env::set_var("SDL_RENDER_DRIVER", "software");
    }

    let mut config = EngineConfig::new(WIDTH, HEIGHT, "giochi engine overhead");
    // Vertical sync would make every measurement the refresh interval, which is the
    // one number that says nothing about the engine.
    config.vsync = 0;
    let mut engine = Engine::new(config).expect("engine");

    // The overlay draws text and uploads a texture once a second; it is opt-in per
    // scene in the demos, but switch it off explicitly so the empty-frame measurement
    // cannot silently start including it.
    engine.set_overlay_visible(false);

    println!(
        "bench backend: {} (accelerated: {})",
        engine.render().renderer_name(),
        engine.render().renderer_is_accelerated(),
    );
    engine
}

/// One frame's worth of simulated time, so the fixed-timestep loop is in its steady
/// state rather than being handed a frame time it has to clamp.
const FRAME_TIME: f32 = 1.0 / 60.0;

fn bench_present_only(c: &mut Criterion) {
    let mut group = c.benchmark_group("engine_overhead");
    group.throughput(Throughput::Elements(1));
    group.bench_function(BenchmarkId::new("present_only", "empty"), |b| {
        with_engine(|engine| {
            let clear = Color::BLACK;
            b.iter(|| {
                let render = engine.render_mut();
                render.clear(clear);
                render.present()
            })
        })
    });
    group.finish();
}

fn bench_step(c: &mut Criterion) {
    let mut group = c.benchmark_group("engine_overhead");
    group.throughput(Throughput::Elements(1));

    // The engine's whole frame with nothing to draw. `step_empty - present_only` is the
    // engine's own bookkeeping: event poll, input, timestep, stats, profiler.
    group.bench_function(BenchmarkId::new("step", "empty"), |b| {
        with_engine(|engine| b.iter(|| engine.step(FRAME_TIME).expect("step")))
    });

    // The same frame with the debug overlay on, which is what a development build
    // runs. Kept separate so the overlay's cost stays attributable to the overlay
    // (ADR-042) instead of being folded into the engine's.
    group.bench_function(BenchmarkId::new("step", "empty_overlay"), |b| {
        with_engine(|engine| {
            engine.set_overlay_visible(true);
            b.iter(|| engine.step(FRAME_TIME).expect("step"));
            engine.set_overlay_visible(false);
        })
    });

    group.finish();
}

/// The statistics the ratios are computed from, for `tests/perf_regression.rs`.
///
/// Criterion's own report is for humans; this is for the gate, which needs two numbers
/// from the same process.
fn bench_stats(c: &mut Criterion) {
    let mut group = c.benchmark_group("engine_overhead");
    group.throughput(Throughput::Elements(1));
    group.bench_function(BenchmarkId::new("frame_stats", "read"), |b| {
        with_engine(|engine| b.iter(|| std::hint::black_box(engine.frame_stats())))
    });
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(50)
        .measurement_time(Duration::from_secs(5))
        .warm_up_time(Duration::from_secs(1));
    targets = bench_present_only, bench_step, bench_stats
}
criterion_main!(benches);
