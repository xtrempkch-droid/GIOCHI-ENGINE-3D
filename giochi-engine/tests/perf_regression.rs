//! Phase 18b: the regression gate for the per-frame engine budget.
//!
//! `cargo bench` answers "how fast is it"; this answers "did it get slower", and it has
//! to answer that the same way on a developer's laptop, on the console and on a shared
//! CI runner. Wall-clock assertions cannot do that — the same code is 20x apart on a
//! PX30 and a desktop — so the gate is built from quantities that are *invariant*, and
//! time is only used where an order of magnitude of headroom makes the machine
//! irrelevant.
//!
//! What is actually asserted:
//!
//! 1. **Allocations per steady-state frame.** Counted with a wrapping global allocator,
//!    so the number is exact and machine-independent. A frame that has nothing to do
//!    must not allocate; an engine that starts allocating per frame is the regression
//!    that a frame-time average hides, because one 64-byte `malloc` never shows up in
//!    a p99 until it lands in a hot path.
//! 2. **Draw calls per queued sprite.** Batching (ADR-043) means N sprites cost
//!    `ceil(N / max_sprites)` draw calls. Deterministic, and it is the counter-check of
//!    the phase-16 win — a regression there is a batching regression, not a slow
//!    driver. Only recorded when the `profiling` feature is on (ADR-042), so these
//!    assertions compile out without it and CI runs the test with the feature too.
//! 3. **Frame-budget sanity.** An empty frame on the software backend costs ~0.25 ms on
//!    the reference machine; the gate is the 16.67 ms of a 60 Hz frame, 66x of headroom,
//!    which no plausible machine gets wrong but a catastrophic regression does.
//!
//! Everything lives in one `#[test]`: SDL allows a single `EventPump` per process, and
//! an allocator counter cannot be attributed to a thread. One test also means the
//! numbers are produced in a fixed order on one warm engine, which is what makes them
//! comparable to the recorded baseline at all.
//!
//! ```text
//! cargo test -p giochi-engine --test perf_regression
//! cargo test -p giochi-engine --test perf_regression --features profiling
//! ```

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use giochi_engine::{Engine, EngineConfig, Rect, TextureHandle};

/// Wraps the system allocator to count what Rust code asks for.
///
/// SDL's own `malloc`s happen inside C and are invisible here, which is the point: the
/// gate is about the engine's Rust-side frame cost.
struct CountingAllocator;

static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        System.alloc(layout)
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        System.alloc_zeroed(layout)
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(new_size as u64, Ordering::Relaxed);
        System.realloc(ptr, layout, new_size)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

/// Canvas size, matching the other measurements.
const WIDTH: u32 = 1280;
const HEIGHT: u32 = 720;
/// One frame of simulated time at 60 Hz.
const FRAME_TIME: f32 = 1.0 / 60.0;

/// The engine is only allowed this many allocations per steady-state frame.
///
/// Recorded on 2026-11-08 with an empty scene on the software backend: **0**. The
/// counter is here so the number is visible in a failure instead of being implied.
const MAX_FRAME_ALLOCATIONS: u64 = 0;

/// Ceiling for one empty frame.
///
/// Reference measurement: 0.254 ms (`present` 0.249 ms + 4.7 us of engine bookkeeping)
/// on an E5-2620 v3 with the software renderer at 1280x720. A 60 Hz frame is 16.67 ms.
/// The margin is deliberate: this is a trap for a regression of the "engine now does
/// 20 ms of work" kind, not a performance target — the target lives in `PERF_LOG.md`.
const MAX_EMPTY_FRAME: Duration = Duration::from_micros(16_667);

fn make_engine() -> Engine {
    if std::env::var_os("SDL_VIDEODRIVER").is_none() {
        std::env::set_var("SDL_VIDEODRIVER", "dummy");
    }
    if std::env::var_os("SDL_RENDER_DRIVER").is_none() {
        std::env::set_var("SDL_RENDER_DRIVER", "software");
    }
    let mut config = EngineConfig::new(WIDTH, HEIGHT, "giochi perf regression");
    config.vsync = 0;
    let mut engine = Engine::new(config).expect("engine");
    engine.set_overlay_visible(false);
    engine
}

/// Runs `frames` steps and returns the live allocations they made.
fn allocations_over_steps(engine: &mut Engine, frames: u32) -> u64 {
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    for _ in 0..frames {
        engine.step(FRAME_TIME).expect("step");
    }
    ALLOCATIONS.load(Ordering::Relaxed) - before
}

fn solid(engine: &mut Engine, color: [u8; 4]) -> TextureHandle {
    let pixels: Vec<u8> = color.iter().copied().cycle().take(8 * 8 * 4).collect();
    engine
        .render_mut()
        .create_texture_from_rgba(8, 8, &pixels)
        .expect("texture")
}

#[test]
fn the_per_frame_engine_budget_has_not_regressed() {
    let mut engine = make_engine();
    println!(
        "perf_regression: backend={} accelerated={} profiling={}",
        engine.render().renderer_name(),
        engine.render().renderer_is_accelerated(),
        cfg!(feature = "profiling"),
    );

    // --- 1. Allocation gate ------------------------------------------------------
    //
    // Warm up first: the first frames legitimately allocate — the profiler's history
    // ring fills, the batch's vertex scratch grows to the high-water mark of the scene,
    // SDL lazily initialises the event subsystem. Those are one-time costs, and they
    // were already paid before the first real frame of any game.
    for _ in 0..120 {
        engine.step(FRAME_TIME).expect("warm-up step");
    }

    const MEASURED_FRAMES: u32 = 600;
    let allocations = allocations_over_steps(&mut engine, MEASURED_FRAMES);
    let per_frame = allocations / u64::from(MEASURED_FRAMES);
    println!(
        "perf_regression: empty frame allocations = {allocations} over {MEASURED_FRAMES} frames \
         ({per_frame}/frame, ceiling {MAX_FRAME_ALLOCATIONS}/frame)"
    );
    assert_eq!(
        per_frame, MAX_FRAME_ALLOCATIONS,
        "an empty frame allocated {allocations} times over {MEASURED_FRAMES} frames \
         ({per_frame} per frame), ceiling is {MAX_FRAME_ALLOCATIONS} per frame: \
         something in the per-frame path started allocating"
    );

    // Same question for the draw path with content on screen: batching must reuse its
    // scratch buffers, not rebuild them every frame.
    let texture = solid(&mut engine, [220, 40, 50, 255]);
    let draw_sprites = |engine: &mut Engine, frames: u32| {
        let before = ALLOCATIONS.load(Ordering::Relaxed);
        for _ in 0..frames {
            let render = engine.render_mut();
            for index in 0..512u32 {
                let x = ((index % 40) as f32) * 7.0;
                let y = ((index / 40) as f32) * 9.0;
                render
                    .draw_texture(
                        texture,
                        Rect::new(x, y, 8.0, 8.0),
                        Rect::new(0.0, 0.0, 0.0, 0.0),
                    )
                    .expect("draw");
            }
            render.present();
        }
        ALLOCATIONS.load(Ordering::Relaxed) - before
    };
    // One unmeasured frame so the batch scratch reaches its high-water mark.
    draw_sprites(&mut engine, 1);
    let sprite_allocations = draw_sprites(&mut engine, 120);
    println!(
        "perf_regression: 512-sprite frame allocations = {sprite_allocations} over 120 frames"
    );
    assert_eq!(
        sprite_allocations, 0,
        "drawing 512 batched sprites allocated {sprite_allocations} times over 120 frames: \
         the batch is rebuilding its scratch buffers per frame"
    );

    // --- 2. Draw-call gate -------------------------------------------------------
    //
    // Counters are recorded only with the `profiling` feature (ADR-042), so without it
    // this section is not merely skipped — there is nothing to read.
    #[cfg(feature = "profiling")]
    {
        let assets = format!("{}/../assets", env!("CARGO_MANIFEST_DIR"));
        let atlas = engine
            .render_mut()
            .load_atlas(&[
                format!("{assets}/player.png").as_str(),
                format!("{assets}/checker.png").as_str(),
            ])
            .expect("atlas");
        // Both regions come from one texture, which is exactly the case batching is
        // supposed to collapse into a single draw call.
        let texture = atlas.texture();
        let player = atlas.region("player").pixel_rect();
        let checker = atlas.region("checker").pixel_rect();

        const SPRITES: u32 = 5_000;
        let render = engine.render_mut();
        render.begin_frame_stats();
        for index in 0..SPRITES {
            let src = if index % 2 == 0 { player } else { checker };
            // A grid that fits inside the canvas: culled sprites are never queued, so an
            // off-screen layout would silently measure fewer draw calls than it claims.
            let x = ((index % 200) as f32) * 6.0;
            let y = ((index / 200) as f32) * 6.0;
            render
                .draw_texture(texture, Rect::new(x, y, 6.0, 6.0), src)
                .expect("draw");
        }
        render.present();
        let counters = render.frame_counters();
        let max = render.sprite_batch().config().resolved_max_sprites();
        let expected_calls = SPRITES.div_ceil(max);
        println!(
            "perf_regression: {SPRITES} atlas sprites -> {} draw calls \
             (batch max {max}, expected {expected_calls})",
            counters.draw_calls
        );
        assert_eq!(
            counters.draw_calls, expected_calls,
            "{SPRITES} sprites at {max} per batch should be {expected_calls} draw calls, got {}: \
             batching regressed",
            counters.draw_calls
        );
        assert_eq!(
            counters.sprites_drawn, SPRITES,
            "the profiler counted {} sprites drawn, expected {SPRITES}",
            counters.sprites_drawn
        );
    }
    #[cfg(not(feature = "profiling"))]
    println!("perf_regression: draw-call gate skipped (build without `profiling`)");

    // --- 3. Frame-budget sanity --------------------------------------------------
    //
    // Deliberately generous (see `MAX_EMPTY_FRAME`): it only has to catch an
    // order-of-magnitude regression on whatever machine runs the test.
    //
    // The same engine is reused: SDL allows one `EventPump` per process, so a second
    // `Engine` cannot be built. Nothing is queued between frames — the sprite work
    // above was one-shot, not a scene — so these really are empty frames.
    for _ in 0..30 {
        engine.step(FRAME_TIME).expect("warm-up step");
    }
    let mut step_total = Duration::ZERO;
    const TIMED_FRAMES: u32 = 200;
    for _ in 0..TIMED_FRAMES {
        let started = Instant::now();
        engine.step(FRAME_TIME).expect("timed step");
        step_total += started.elapsed();
    }
    let per_step = step_total / TIMED_FRAMES;
    println!("perf_regression: empty frame {per_step:?} (ceiling {MAX_EMPTY_FRAME:?})");
    assert!(
        per_step < MAX_EMPTY_FRAME,
        "an empty frame took {per_step:?}, over the {MAX_EMPTY_FRAME:?} ceiling"
    );
}
