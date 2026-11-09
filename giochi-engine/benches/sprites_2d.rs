//! Phase-16 benchmarks: sprite throughput with and without batching.
//!
//! Each iteration is one complete frame — queue the sprites, then flush — so the
//! numbers are frames per second of 2D submission plus the backend calls, not the cost
//! of building a vector.
//!
//! The `unbatched` scenarios call [`RenderContext::draw_texture_immediate`], the path
//! `draw_texture` used before phase 16, so the comparison is made by the same binary on
//! the same machine. `docs/PERF_LOG.md` records the results and the method.
//!
//! ```text
//! cargo bench -p giochi-engine
//! cargo bench -p giochi-engine -- --quick
//! ```

use std::cell::RefCell;
use std::time::Duration;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use giochi_engine::{
    Color, Platform, Rect, RenderContext, TextureAtlas, TextureHandle, WindowConfig,
};

/// Canvas size. Large enough that none of the benchmark sprites are culled.
const WIDTH: u32 = 1280;
const HEIGHT: u32 = 720;
/// Side of every sprite, in pixels.
const SPRITE: f32 = 8.0;

const COUNTS: [u32; 4] = [100, 500, 1_000, 5_000];
const MIXED_COUNTS: [u32; 3] = [100, 500, 1_000];

struct Bench {
    render: RenderContext,
    one: TextureHandle,
    ten: Vec<TextureHandle>,
    atlas: TextureAtlas,
}

thread_local! {
    /// SDL allows one `EventPump` per process, so the canvas is built once and reused.
    static BENCH: RefCell<Option<Bench>> = const { RefCell::new(None) };
}

/// Runs `f` with the shared render context, building it on first use.
fn with_bench<R>(f: impl FnOnce(&mut Bench) -> R) -> R {
    BENCH.with(|cell| {
        if cell.borrow().is_none() {
            *cell.borrow_mut() = Some(make_bench());
        }
        let mut borrow = cell.borrow_mut();
        f(borrow.as_mut().expect("just built"))
    })
}

fn make_bench() -> Bench {
    // The default is the `dummy` video driver on the software renderer, which is the
    // backend `docs/PERF_LOG.md` records for the CPU-side baseline. Set either
    // variable before invoking cargo to measure the accelerated path instead:
    //
    // ```text
    // SDL_VIDEODRIVER=offscreen SDL_RENDER_DRIVER=opengl cargo bench -p giochi-engine
    // ```
    if std::env::var_os("SDL_VIDEODRIVER").is_none() {
        std::env::set_var("SDL_VIDEODRIVER", "dummy");
    }
    if std::env::var_os("SDL_RENDER_DRIVER").is_none() {
        std::env::set_var("SDL_RENDER_DRIVER", "software");
    }

    let platform = Platform::new().expect("video driver");
    let window = platform
        .create_window(WindowConfig::new(WIDTH, HEIGHT), "giochi bench")
        .expect("window");
    let render = RenderContext::new(window).expect("renderer");
    // The engine default is what a game gets; the benchmark measures that, not a
    // configuration tuned for the measurement.
    println!(
        "bench backend: {} (accelerated: {}, batch_max: {})",
        render.renderer_name(),
        render.renderer_is_accelerated(),
        render.sprite_batch().config().resolved_max_sprites(),
    );
    let mut render = render;

    let one = solid(&render, [220, 40, 50, 255]);
    let ten: Vec<TextureHandle> = (0..10)
        .map(|index| {
            solid(
                &render,
                [(20 + index * 20) as u8, (40 + index * 10) as u8, 200, 255],
            )
        })
        .collect();

    let assets = format!("{}/../assets", env!("CARGO_MANIFEST_DIR"));
    let player = format!("{assets}/player.png");
    let checker = format!("{assets}/checker.png");
    let atlas = render
        .load_atlas(&[player.as_str(), checker.as_str()])
        .expect("atlas");

    Bench {
        render,
        one,
        ten,
        atlas,
    }
}

fn solid(render: &RenderContext, color: [u8; 4]) -> TextureHandle {
    let pixels: Vec<u8> = color.iter().copied().cycle().take(8 * 8 * 4).collect();
    render
        .create_texture_from_rgba(8, 8, &pixels)
        .expect("texture")
}

/// Position of sprite `index` inside the canvas.
fn position(index: u32) -> Rect {
    let columns = (WIDTH as f32 / SPRITE) as u32;
    Rect::new(
        (index % columns) as f32 * SPRITE,
        (index / columns) as f32 * SPRITE,
        SPRITE,
        SPRITE,
    )
}

fn draw_batched(render: &mut RenderContext, count: u32, textures: &[TextureHandle]) {
    render.clear(Color::BLACK);
    for index in 0..count {
        let texture = textures[index as usize % textures.len()];
        render
            .draw_texture(texture, position(index), Rect::ZERO)
            .expect("draw");
    }
    render.flush_batch();
}

fn draw_unbatched(render: &mut RenderContext, count: u32, textures: &[TextureHandle]) {
    render.clear(Color::BLACK);
    for index in 0..count {
        let texture = textures[index as usize % textures.len()];
        render
            .draw_texture_immediate(texture, position(index), Rect::ZERO)
            .expect("draw");
    }
}

fn batch_group(c: &mut Criterion) {
    let mut group = c.benchmark_group("sprites_2d");
    group.warm_up_time(Duration::from_millis(300));
    group.measurement_time(Duration::from_secs(1));
    group.sample_size(10);

    for count in COUNTS {
        group.throughput(Throughput::Elements(u64::from(count)));
        group.bench_with_input(
            BenchmarkId::new("same_texture", count),
            &count,
            |b, count| {
                with_bench(|bench| {
                    let one = bench.one;
                    b.iter(|| draw_batched(&mut bench.render, *count, std::slice::from_ref(&one)));
                });
            },
        );
        group.bench_with_input(
            BenchmarkId::new("same_texture_unbatched", count),
            &count,
            |b, count| {
                with_bench(|bench| {
                    let one = bench.one;
                    b.iter(|| {
                        draw_unbatched(&mut bench.render, *count, std::slice::from_ref(&one))
                    });
                });
            },
        );
    }

    // Ten textures, drawn in ten groups: the order a tilemap or a set of animated
    // entities naturally produces. Ten draw calls, not one.
    for count in MIXED_COUNTS {
        group.throughput(Throughput::Elements(u64::from(count)));
        group.bench_with_input(
            BenchmarkId::new("ten_textures_grouped", count),
            &count,
            |b, count| {
                with_bench(|bench| {
                    b.iter(|| {
                        bench.render.clear(Color::BLACK);
                        let per_texture = count.div_ceil(bench.ten.len() as u32);
                        let mut remaining = *count;
                        let mut offset = 0;
                        for texture in &bench.ten {
                            let batch = remaining.min(per_texture);
                            for index in offset..offset + batch {
                                bench
                                    .render
                                    .draw_texture(*texture, position(index), Rect::ZERO)
                                    .expect("draw");
                            }
                            offset += batch;
                            remaining -= batch;
                        }
                        bench.render.flush_batch();
                    });
                });
            },
        );
        // Ten textures interleaved: the batch cannot merge neighbours, so this is the
        // worst case — one call per sprite, documented rather than hidden.
        group.bench_with_input(
            BenchmarkId::new("ten_textures_interleaved", count),
            &count,
            |b, count| {
                with_bench(|bench| {
                    let ten = bench.ten.clone();
                    b.iter(|| {
                        let render = &mut bench.render;
                        render.clear(Color::BLACK);
                        for index in 0..*count {
                            render
                                .draw_texture(
                                    ten[index as usize % ten.len()],
                                    position(index),
                                    Rect::ZERO,
                                )
                                .expect("draw");
                        }
                        render.flush_batch();
                    });
                });
            },
        );
    }

    for count in COUNTS {
        group.throughput(Throughput::Elements(u64::from(count)));
        group.bench_with_input(BenchmarkId::new("atlas", count), &count, |b, count| {
            with_bench(|bench| {
                let texture = bench.atlas.texture();
                let player = bench.atlas.region("player").pixel_rect();
                let checker = bench.atlas.region("checker").pixel_rect();
                b.iter(|| {
                    let render = &mut bench.render;
                    render.clear(Color::BLACK);
                    for index in 0..*count {
                        let src = if index % 2 == 0 { player } else { checker };
                        render
                            .draw_texture(texture, position(index), src)
                            .expect("draw");
                    }
                    render.flush_batch();
                });
            });
        });
    }

    group.finish();
}

criterion_group!(benches, batch_group);
criterion_main!(benches);
