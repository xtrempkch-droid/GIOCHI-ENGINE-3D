//! Phase-17 benchmarks: instancing and static batching, against the loop they replace.
//!
//! Every iteration is one complete frame — begin, clear, submit, present — so the
//! numbers include the state setup and the buffer swap, not just the draw calls. A
//! benchmark that stopped before `end_frame` would measure a driver accepting commands
//! it has not had to execute yet.
//!
//! The two arms of each pair draw the **same picture** with the same shader and the same
//! light; only the submission differs. `tests/instancing.rs` and `tests/static_batch.rs`
//! assert that on pixels, which is why the ratio here is a real speed-up rather than a
//! scene that stopped drawing.
//!
//! ```text
//! SDL_VIDEODRIVER=offscreen cargo bench -p giochi-engine-3d
//! SDL_VIDEODRIVER=offscreen cargo bench -p giochi-engine-3d -- --quick
//! ```
//!
//! `docs/PERF_LOG.md` records the results, the hardware and the method.

use std::cell::RefCell;
use std::time::Duration;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use giochi_engine::platform::window::{Window, WindowConfig};
use giochi_engine::platform::Platform;
use giochi_engine_3d::context::{self, GlesVersion};
use giochi_engine_3d::math3d::{Camera3D, Color3D, Mat4, Quat, Transform3D, Vec3};
use giochi_engine_3d::mesh::{StaticBatch, StaticBatchBuilder};
use giochi_engine_3d::shader::BuiltinShader;
use giochi_engine_3d::{DirectionalLight, RenderContext3D};

const WIDTH: u32 = 1280;
const HEIGHT: u32 = 720;
const CUBE_SIZE: f32 = 0.8;
const COUNTS: [usize; 4] = [1, 10, 100, 1000];

struct Bench {
    render: RenderContext3D,
    window: Window,
    cube: u64,
    loop_shader: u64,
    instanced_shader: u64,
    camera: Camera3D,
    /// `scenery` static batch: 50 tiles in one mesh.
    batch_mesh: u64,
    /// The same 50 tiles as private meshes, for the comparison arm.
    separate_meshes: Vec<u64>,
    transforms: Vec<Transform3D>,
    _platform: Platform,
}

thread_local! {
    /// SDL allows one `Platform` per process, so the context is built once and reused.
    static BENCH: RefCell<Option<Bench>> = const { RefCell::new(None) };
}

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
    // `offscreen` is the only SDL video driver with a real EGL/GLES context and no
    // display. The 2D benchmark uses `dummy` + the software renderer for the opposite
    // reason: that one has no OpenGL at all.
    if std::env::var_os("SDL_VIDEODRIVER").is_none() {
        std::env::set_var("SDL_VIDEODRIVER", "offscreen");
    }

    let platform = Platform::new().expect("video driver");
    let requested = GlesVersion::from_env();
    context::configure_attributes(&platform, requested).expect("GL attributes");
    let window = Window::new_opengl(&platform, WindowConfig::new(WIDTH, HEIGHT), "giochi bench")
        .expect("window");
    let mut render = RenderContext3D::new(context::create(&window).expect("GL context"));

    let cube = render.create_cube_mesh(CUBE_SIZE).expect("cube");
    let loop_shader = render
        .create_builtin_shader(BuiltinShader::Phong)
        .expect("phong");
    let instanced_shader = render
        .create_builtin_shader(BuiltinShader::Instanced)
        .expect("instanced");

    // A field that fills the view: 1000 cubes in a 32x32 grid is 29 units across, which
    // is inside the frustum from the camera below.
    let transforms: Vec<Transform3D> = (0..COUNTS[3])
        .map(|index| {
            let column = (index % 32) as f32;
            let row = (index / 32) as f32;
            Transform3D::from_translation(Vec3::new(
                (column - 16.0) * 0.9,
                CUBE_SIZE * 0.5,
                (row - 16.0) * 0.9,
            ))
            .with_rotation(Quat::from_axis_angle(Vec3::UP, index as f32 * 0.1))
        })
        .collect();

    let camera = Camera3D::from_parts(
        Vec3::new(0.0, 18.0, 26.0),
        Vec3::new(0.0, 0.0, -4.0),
        Vec3::UP,
    )
    .with_clip_planes(0.1, 200.0);

    render.set_clear_color(Color3D::BLACK);
    render.set_light(DirectionalLight::sun().with_direction(Vec3::new(0.4, -1.0, 0.3)));

    // The static scenery: a 10x5 grid of tiles, the "room" case.
    let (tile_vertices, tile_indices) = giochi_engine_3d::mesh::plane(2.0, 1);
    let mut builder =
        StaticBatchBuilder::with_capacity(tile_vertices.len() * 50, tile_indices.len() * 50);
    let mut separate_meshes = Vec::with_capacity(50);
    for row in 0..5 {
        for column in 0..10 {
            let model = Transform3D::from_translation(Vec3::new(
                column as f32 * 2.0 - 9.0,
                0.0,
                row as f32 * 2.0 - 4.0,
            ))
            .matrix();
            builder = builder
                .add_mesh(&tile_vertices, &tile_indices, &model)
                .expect("add");
            separate_meshes.push(
                render
                    .create_mesh(&tile_vertices, &tile_indices)
                    .expect("separate tile"),
            );
        }
    }
    let batch = StaticBatch::build(&mut render, builder).expect("batch");
    let batch_mesh = batch.mesh();

    println!(
        "bench backend: {} (GLES {}), instancing {}, batch pieces {}",
        render.adapter(),
        render.gles_version().major(),
        render.instancing_support().name(),
        batch.pieces()
    );

    Bench {
        render,
        window,
        cube,
        loop_shader,
        instanced_shader,
        camera,
        batch_mesh,
        separate_meshes,
        transforms,
        _platform: platform,
    }
}

fn begin(bench: &mut Bench) {
    bench.render.begin_frame(&bench.window);
    bench.render.clear(&bench.window, Color3D::BLACK);
}

/// One frame drawing `count` cubes with a single instanced call.
fn frame_instanced(bench: &mut Bench, count: usize) {
    begin(bench);
    let matrices: Vec<Mat4> = bench.transforms[..count]
        .iter()
        .map(Transform3D::matrix)
        .collect();
    bench
        .render
        .draw_instanced(
            bench.cube,
            bench.instanced_shader,
            &matrices,
            &bench.camera,
            Color3D::new(0.8, 0.7, 0.3, 1.0),
        )
        .expect("instanced draw");
    bench.render.end_frame(&bench.window);
}

/// One frame drawing `count` cubes, one call each.
fn frame_loop(bench: &mut Bench, count: usize) {
    begin(bench);
    for transform in &bench.transforms[..count] {
        bench
            .render
            .draw_mesh_colored(
                bench.cube,
                bench.loop_shader,
                transform,
                &bench.camera,
                Color3D::new(0.8, 0.7, 0.3, 1.0),
            )
            .expect("draw");
    }
    bench.render.end_frame(&bench.window);
}

fn instancing_group(c: &mut Criterion) {
    let mut group = c.benchmark_group("meshes_3d");
    group.warm_up_time(Duration::from_millis(300));
    group.measurement_time(Duration::from_secs(1));
    group.sample_size(10);

    for count in COUNTS {
        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(BenchmarkId::new("instanced", count), &count, |b, count| {
            with_bench(|bench| b.iter(|| frame_instanced(bench, *count)))
        });
        group.bench_with_input(BenchmarkId::new("loop", count), &count, |b, count| {
            with_bench(|bench| b.iter(|| frame_loop(bench, *count)))
        });
    }

    group.finish();
}

fn static_batch_group(c: &mut Criterion) {
    let mut group = c.benchmark_group("static_batch");
    group.warm_up_time(Duration::from_millis(300));
    group.measurement_time(Duration::from_secs(1));
    group.sample_size(10);
    group.throughput(Throughput::Elements(50));

    // One mesh, one call.
    group.bench_function("batched_50", |b| {
        with_bench(|bench| {
            b.iter(|| {
                begin(bench);
                bench
                    .render
                    .draw_mesh_colored(
                        bench.batch_mesh,
                        bench.loop_shader,
                        &Transform3D::IDENTITY,
                        &bench.camera,
                        Color3D::new(0.4, 0.6, 0.4, 1.0),
                    )
                    .expect("batch draw");
                bench.render.end_frame(&bench.window);
            });
        });
    });

    // The same 50 tiles as 50 meshes, one call each: what a scene looks like without the
    // batch.
    group.bench_function("separate_50", |b| {
        with_bench(|bench| {
            b.iter(|| {
                begin(bench);
                for mesh in &bench.separate_meshes {
                    bench
                        .render
                        .draw_mesh_colored(
                            *mesh,
                            bench.loop_shader,
                            &Transform3D::IDENTITY,
                            &bench.camera,
                            Color3D::new(0.4, 0.6, 0.4, 1.0),
                        )
                        .expect("draw");
                }
                bench.render.end_frame(&bench.window);
            });
        });
    });

    group.finish();
}

criterion_group!(benches, instancing_group, static_batch_group);
criterion_main!(benches);
