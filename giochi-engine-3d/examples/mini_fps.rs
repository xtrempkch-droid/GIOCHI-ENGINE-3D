//! First-person walker — the phase 12 acceptance check.
//!
//! A textured floor, some cubes to navigate around, low crates to look down on, WASD
//! movement, mouse look, TAB to toggle wireframe and ESC to release the mouse (a second
//! ESC quits).
//!
//! The crates exist for the phase-13 acceptance check: a top face is only on screen if the
//! camera is above it, and a walker at eye height sees nothing but the sides of anything
//! taller than itself.
//!
//! "Playable" is not something a test can assert, so this example is built to be
//! *measurable* instead: it prints the frame rate once a second and can run
//! non-interactively with a scripted path, which is how CI and the console exercise the
//! same code the manual check uses.
//!
//! # Phase 17 toggles
//!
//! - **F2** switches the 100-cube field between instanced (one draw call) and a loop
//!   (one draw call per cube). The scene, the camera and the colours are identical, so
//!   the frame rate difference is the batching and nothing else.
//! - **F3** cycles the render scale 100 % → 75 % → 50 %, upscaling a smaller framebuffer
//!   over the window.
//! - The static scenery is always drawn from one [`StaticBatch`], so the walls, pillars
//!   and floor are a single draw call instead of ~30.
//!
//! `--instanced`/`--loop` and `--scale N` set the same things from the command line,
//! which is how the benchmark runs without a keyboard.
//!
//! ```text
//! cargo run -p giochi-engine-3d --example mini_fps
//! cargo run -p giochi-engine-3d --example mini_fps -- --frames 300 --auto
//! cargo run -p giochi-engine-3d --example mini_fps -- --frames 3 --quadro out.bmp
//! cargo run --release -p giochi-engine-3d --example mini_fps -- \
//!     --frames 600 --auto --bench --instanced
//! ```

use std::process::ExitCode;

use giochi_engine::input::Input;
use giochi_engine::platform::sdl::{quit_signal, QuitSignal};
use giochi_engine::platform::window::{Window, WindowConfig};
use giochi_engine::platform::Platform;
use giochi_engine_3d::context::{self, GlesVersion};
use giochi_engine_3d::math3d::{
    AxisInput, CameraController, Color3D, Mat4, Quat, Transform3D, Vec3,
};
use giochi_engine_3d::mesh::{StaticBatch, StaticBatchBuilder};
use giochi_engine_3d::shader::BuiltinShader;
use giochi_engine_3d::{DirectionalLight, RenderContext3D};
use sdl2::keyboard::Keycode;

/// Floor edge length, in world units.
const FLOOR_SIZE: f32 = 60.0;
/// Obstacle cubes per side of the grid.
const OBSTACLES: i32 = 5;
/// Spacing between obstacle centres.
const SPACING: f32 = 6.0;
/// Obstacle edge length.
const OBSTACLE_SIZE: f32 = 2.0;
/// Eye height above the floor.
const EYE_HEIGHT: f32 = 1.7;
/// Spacing of the instanced cube field.
const CUBE_FIELD_SPACING: f32 = 1.4;
/// Edge length of one cube in the instanced field.
const CUBE_FIELD_SIZE: f32 = 0.8;
/// The render scales F3 cycles through.
const SCALE_STEPS: [f32; 3] = [1.0, 0.75, 0.5];

/// Index in [`SCALE_STEPS`] closest to `scale`, so `--scale 0.75` and two presses of F3
/// agree about where the cycle is.
fn scale_index_for(scale: f32) -> usize {
    let mut best = 0;
    let mut best_distance = f32::INFINITY;
    for (index, step) in SCALE_STEPS.iter().enumerate() {
        let distance = (step - scale).abs();
        if distance < best_distance {
            best_distance = distance;
            best = index;
        }
    }
    best
}
/// Edge length of the low crates.
///
/// Deliberately **below** the eye. The phase-13 acceptance criterion is that a cube's top
/// face reads brighter than its side, and with obstacles taller than the camera no top face
/// is ever visible — the player looks up at them. These crates put a top face in view so
/// the lighting can be judged rather than merely trusted.
const CRATE_SIZE: f32 = 0.6;

#[derive(Debug)]
struct Options {
    /// Stop after this many frames.
    frames: Option<u64>,
    /// Walk a scripted path instead of waiting for input.
    auto: bool,
    /// Capture the last frame to this BMP.
    quadro: Option<String>,
    /// Draw the cube field with instancing instead of a loop.
    instanced: bool,
    /// Render scale, `1.0` for full resolution.
    scale: f32,
    /// Print one machine-readable line at the end, for `docs/PERF_LOG.md`.
    bench: bool,
    /// Window size. KMSDRM ignores it (the panel is the panel); a desktop or the
    /// `offscreen` driver honours it, which is how the fill-bound case is measured.
    width: u32,
    height: u32,
    /// Cubes in the instanced field.
    ///
    /// The default 100 is the phase-17 acceptance scene. A console holds 60 FPS by
    /// vsync down to workloads far heavier than that, so the device measurement raises
    /// it until the loop arm actually misses frames — otherwise both arms report 60.0
    /// and the comparison measures the panel.
    cubes: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            frames: None,
            auto: false,
            quadro: None,
            // Instancing is the fast path and the reason the field exists, so it is the
            // default; `--loop` and F2 switch to the comparison arm.
            instanced: true,
            scale: 1.0,
            bench: false,
            width: 1280,
            height: 720,
            cubes: 100,
        }
    }
}

impl Options {
    /// Parses the command line, rejecting anything it does not understand.
    ///
    /// It used to ignore unknown flags and turn an unparseable number into `None`, which
    /// for `--frames` means "no limit": a typo such as `--frames=3oo` produced an
    /// unattended run that never terminates, and in CI that is a job timeout rather than
    /// a message. Every uncertainty is an error now.
    fn parse() -> Result<Self, String> {
        let mut options = Self::default();
        let mut args = std::env::args().skip(1);

        while let Some(arg) = args.next() {
            if let Some(value) = arg.strip_prefix("--frames=") {
                options.frames = Some(parse_number(value, "--frames")?);
            } else if arg == "--frames" {
                options.frames = Some(parse_number(
                    next_value(&mut args, "--frames")?,
                    "--frames",
                )?);
            } else if arg == "--auto" {
                options.auto = true;
            } else if arg == "--quadro" {
                options.quadro = Some(next_value(&mut args, "--quadro")?.to_string());
            } else if let Some(value) = arg.strip_prefix("--quadro=") {
                options.quadro = Some(value.to_string());
            } else if arg == "--bench" {
                options.bench = true;
            } else if let Some(value) = arg.strip_prefix("--cubes=") {
                options.cubes = parse_number(value, "--cubes")?;
            } else if arg == "--cubes" {
                options.cubes = parse_number(next_value(&mut args, "--cubes")?, "--cubes")?;
            } else if let Some(value) = arg.strip_prefix("--width=") {
                options.width = parse_number(value, "--width")?;
            } else if arg == "--width" {
                options.width = parse_number(next_value(&mut args, "--width")?, "--width")?;
            } else if let Some(value) = arg.strip_prefix("--height=") {
                options.height = parse_number(value, "--height")?;
            } else if arg == "--height" {
                options.height = parse_number(next_value(&mut args, "--height")?, "--height")?;
            } else if arg == "--instanced" {
                options.instanced = true;
            } else if arg == "--loop" {
                options.instanced = false;
            } else if let Some(value) = arg.strip_prefix("--scale=") {
                options.scale = parse_number(value, "--scale")?;
            } else if arg == "--scale" {
                options.scale = parse_number(next_value(&mut args, "--scale")?, "--scale")?;
            } else {
                return Err(format!(
                    "unknown argument '{arg}'; accepted: --frames N, --auto, --quadro PATH, \
                     --instanced, --loop, --scale N, --bench, --width N, --height N, \
                     --cubes N"
                ));
            }
        }

        if !(options.scale.is_finite() && options.scale > 0.0) {
            return Err("--scale must be a positive number".to_owned());
        }
        if options.width == 0 || options.height == 0 {
            return Err("--width and --height must be non-zero".to_owned());
        }
        if options.cubes == 0 {
            return Err("--cubes must be greater than zero".to_owned());
        }

        Ok(options)
    }
}

/// Returns the value that follows a flag, or an error naming it.
fn next_value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("{flag} needs a value; accepted: --frames N, --auto, --quadro PATH"))
}

/// Parses a flag's value, naming the flag when it is not a number.
fn parse_number<T: std::str::FromStr>(value: impl AsRef<str>, flag: &str) -> Result<T, String> {
    let value = value.as_ref();
    value
        .parse()
        .map_err(|_| format!("{flag} expects a number, got '{value}'"))
}

fn main() -> ExitCode {
    env_logger::init();

    // Both steps produce the same kind of message, and a bad argument is reported the same
    // way a failed run is: one line on stderr and a non-zero exit.
    match Options::parse()
        .map_err(Box::<dyn std::error::Error>::from)
        .and_then(run)
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("erro: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(options: Options) -> Result<(), Box<dyn std::error::Error>> {
    let platform = Platform::new()?;
    println!("video driver: {}", platform.video_driver_name());

    let requested = GlesVersion::from_env();
    context::configure_attributes(&platform, requested)?;

    let window = Window::new_opengl(
        &platform,
        WindowConfig::new(options.width, options.height),
        "giochi mini_fps",
    )?;
    let mut render = RenderContext3D::new(context::create(&window)?);
    println!(
        "GLES {} on {}",
        render.gles_version().major(),
        render.adapter()
    );

    // Two meshes, one shader: a big flat floor and a cube reused for every obstacle.
    let floor = render.create_plane_mesh(FLOOR_SIZE, 8)?;
    let cube = render.create_cube_mesh(OBSTACLE_SIZE)?;
    // Phong rather than flat: with one hard-coded Lambert term a cube shows four sides
    // and a top all at once with nothing to tell them apart except their silhouette.
    let shader = render.create_builtin_shader(BuiltinShader::Phong)?;
    println!(
        "floor {floor}: {} tris; cube {cube}: {} tris; VAOs: {} / {}",
        render.mesh_index_count(floor) / 3,
        render.mesh_index_count(cube) / 3,
        render.mesh_uses_vao(floor),
        render.mesh_uses_vao(cube),
    );

    render.set_clear_color(Color3D::new(0.55, 0.72, 0.92, 1.0));

    // A fixed sun, tilted so no two cube faces share a diffuse term. The renderer already
    // defaults to this light; setting it explicitly is what the example is for.
    let light = DirectionalLight::sun()
        .with_direction(Vec3::new(0.4, -1.0, 0.3))
        .with_specular(0.3, 24.0);
    render.set_light(light);
    println!(
        "light direction {:?}, ambient {:.2}, specular {:.2} @ {:.0}",
        light.direction.to_array(),
        light.ambient,
        light.specular,
        light.shininess
    );

    // ---- phase 17: static batching, instancing and upscaling -------------

    // The obstacles and the low crates never move, so they are assembled into one mesh
    // each at load time. Two batches rather than one because a batch has a single
    // colour: the obstacles are tan and the crates are blue, and losing that distinction
    // would make the phase-13 "top face is visible" check harder to read.
    let (cube_vertices, cube_indices) = giochi_engine_3d::mesh::cube(OBSTACLE_SIZE);
    let (crate_vertices, crate_indices) = giochi_engine_3d::mesh::cube(CRATE_SIZE);
    let mut obstacles =
        StaticBatchBuilder::with_capacity(cube_vertices.len() * 32, cube_indices.len() * 32);
    let mut crates =
        StaticBatchBuilder::with_capacity(cube_vertices.len() * 32, cube_indices.len() * 32);
    let mut obstacle_count = 0usize;
    let mut crate_count = 0usize;

    for row in 0..OBSTACLES {
        for column in 0..OBSTACLES {
            let x = (column - OBSTACLES / 2) as f32 * SPACING;
            let z = (row - OBSTACLES / 2) as f32 * SPACING - 6.0;
            if x.abs() < 1.0 && (z - 8.0).abs() < 3.0 {
                continue;
            }

            let tilt = Quat::from_axis_angle(Vec3::UP, (row * OBSTACLES + column) as f32 * 0.37);
            let model = Transform3D::from_translation(Vec3::new(x, OBSTACLE_SIZE * 0.5, z))
                .with_rotation(tilt)
                .matrix();
            obstacles = obstacles.add_mesh(&cube_vertices, &cube_indices, &model)?;
            obstacle_count += 1;

            let crate_x = x + SPACING * 0.5;
            if crate_x.abs() < FLOOR_SIZE * 0.45 {
                let crate_model =
                    Transform3D::from_translation(Vec3::new(crate_x, CRATE_SIZE * 0.5, z))
                        .with_rotation(Quat::from_axis_angle(
                            Vec3::UP,
                            (row * OBSTACLES + column) as f32 * 0.61,
                        ))
                        .matrix();
                crates = crates.add_mesh(&crate_vertices, &crate_indices, &crate_model)?;
                crate_count += 1;
            }
        }
    }

    let obstacle_batch = StaticBatch::build(&mut render, obstacles)?;
    let crate_batch = StaticBatch::build(&mut render, crates)?;
    println!(
        "static batches: {obstacle_count} obstacles -> 1 draw ({} verts), \
         {crate_count} crates -> 1 draw ({} verts)",
        obstacle_batch.vertex_count(),
        crate_batch.vertex_count(),
    );

    // The instanced cube field: one mesh, 100 matrices. Drawn with one call when the
    // driver has the extension, 100 when it does not.
    let field_cube = render.create_cube_mesh(CUBE_FIELD_SIZE)?;
    let instanced_shader = render.create_builtin_shader(BuiltinShader::Instanced)?;
    // A square-ish grid, laid out from the count so `--cubes` alone decides the scene.
    let side = (options.cubes as f64).sqrt().ceil() as i32;
    let field: Vec<Mat4> = (0..options.cubes)
        .map(|index| {
            let row = (index as i32) / side;
            let column = (index as i32) % side;
            let position = Vec3::new(
                (column - side / 2) as f32 * CUBE_FIELD_SPACING,
                CUBE_FIELD_SIZE * 0.5 + 0.2,
                (row - side / 2) as f32 * CUBE_FIELD_SPACING - 16.0,
            );
            Transform3D::from_translation(position)
                .with_rotation(Quat::from_axis_angle(Vec3::UP, index as f32 * 0.4))
                .matrix()
        })
        .collect();
    println!(
        "instanced field: {} cubes, instancing {} ({:?}); toggled with F2",
        field.len(),
        if render.supports_instancing() {
            "available"
        } else {
            "UNAVAILABLE — the fallback draws one at a time"
        },
        render.instancing_support().extension(),
    );

    let mut instanced = options.instanced;
    render.set_render_scale(Some(options.scale));
    let mut scale_index = scale_index_for(options.scale);
    // `render.drawable_size()` is still the placeholder until the first `begin_frame`
    // learns the real surface (KMSDRM ignores the requested size), so the expected
    // render size is derived from the window rather than from the renderer.
    let (window_width, window_height) = window.drawable_size();
    let (render_width, render_height) = giochi_engine_3d::scaled_size(
        window_width,
        window_height,
        render.render_scale().unwrap_or(1.0),
    );
    println!(
        "window {window_width}x{window_height}, render scale {:.0}% -> {render_width}x{render_height} \
         (toggled with F3)",
        render.render_scale().unwrap_or(1.0) * 100.0,
    );

    let mut controller = CameraController::new(Vec3::new(0.0, EYE_HEIGHT, 8.0), 0.0, -0.15);
    let mut input = Input::new(&platform);
    // Mouse capture is on by default: a walker without it cannot look around. ESC
    // releases the pointer, and a second ESC quits.
    let mouse = platform.sdl_mouse();
    controller.set_mouse_captured(mouse.as_ref(), !options.auto);

    let mut wireframe = false;
    // Reported at the end: a crate whose top face is visible is what makes the phase-13
    // acceptance criterion observable at all, and "zero crates drawn" would mean the
    // criterion is being asserted on geometry the player never sees.
    let mut crates_drawn: u64 = 0;
    // Draw calls the instanced field cost on the last frame: 1 with the extension, one
    // per cube without it. Printed by `--bench` so the claim is measured, not assumed.
    let mut field_calls: u32 = 0;
    let mut frames: u64 = 0;
    let mut fps_timer: f32 = 0.0;
    let mut fps_frames: u32 = 0;
    let started_at = std::time::Instant::now();

    let frame_ms = if options.frames.is_some() {
        0
    } else {
        1000 / 60
    };

    loop {
        let started = std::time::Instant::now();

        let events = platform.poll_events();
        for event in &events {
            if quit_signal(event) != QuitSignal::None {
                finish(
                    &mut render,
                    &[
                        floor,
                        cube,
                        field_cube,
                        obstacle_batch.mesh(),
                        crate_batch.mesh(),
                    ],
                    &[shader, instanced_shader],
                    frames,
                    wireframe,
                    crates_drawn,
                );
                if options.bench {
                    let wall = started_at.elapsed().as_secs_f32().max(f32::EPSILON);
                    let (render_width, render_height) = render.render_size();
                    // Counted here rather than read from the profiler: the counter only
                    // exists with the `profiling` feature, and the acceptance criterion
                    // has to hold in a plain release build.
                    let draw_calls = 3 + u64::from(field_calls);
                    println!(
                        "mini_fps: frames={frames} wall={wall:.3}s fps={:.1} \
                         draw_calls={draw_calls} field_calls={field_calls} \
                         instanced={} cubes={} scale={:.2} render={render_width}x{render_height} \
                         support={}",
                        frames as f32 / wall,
                        u8::from(instanced),
                        field.len(),
                        render.render_scale().unwrap_or(1.0),
                        render.instancing_support().name(),
                    );
                }
                return Ok(());
            }
            // TAB toggles wireframe: the fastest way to see whether a face is missing
            // because of culling or because it was never drawn.
            if let sdl2::event::Event::KeyDown {
                keycode: Some(Keycode::Tab),
                repeat: false,
                ..
            } = event
            {
                wireframe = !wireframe;
                println!("wireframe: {wireframe}");
            }
            // F2 toggles instancing so the two arms can be compared on the same scene
            // by eye as well as by number.
            if let sdl2::event::Event::KeyDown {
                keycode: Some(Keycode::F2),
                repeat: false,
                ..
            } = event
            {
                instanced = !instanced;
                println!(
                    "instanced cube field: {instanced}{}",
                    if instanced && !render.supports_instancing() {
                        " (no extension here: this is the one-per-instance fallback)"
                    } else {
                        ""
                    }
                );
            }
            // F3 cycles the render scale. `SCALE_STEPS` is shared with `--scale` so the
            // command-line and the key reach the same values.
            if let sdl2::event::Event::KeyDown {
                keycode: Some(Keycode::F3),
                repeat: false,
                ..
            } = event
            {
                scale_index = (scale_index + 1) % SCALE_STEPS.len();
                render.set_render_scale(Some(SCALE_STEPS[scale_index]));
                println!(
                    "render scale {:.0}% -> {}x{}",
                    SCALE_STEPS[scale_index] * 100.0,
                    render.render_size().0,
                    render.render_size().1
                );
            }
        }

        input.update(&events);

        // Auto mode walks a fixed path so the run is deterministic: forward and right,
        // turning slowly. Without it, `--frames` would produce a still frame and the
        // movement code would go untested on CI.
        if options.auto {
            // A scripted path instead of a keyboard. `Input`'s keyboard state is only
            // populated by real SDL events, so an unattended run has to drive the axes
            // directly -- otherwise "the demo moved" would be untested on CI and on a
            // console with no controller attached.
            let axes = AxisInput {
                forward: 1.0,
                strafe: 0.35,
                rise: 0.0,
                sprint: false,
            };
            controller.update_with_axes(axes, (6.0, 0.0), 1.0 / 60.0);
        } else {
            controller.update(&input, mouse_delta(&input), 1.0 / 60.0);
        }

        let (width, height) = render.drawable_size();
        let camera = controller.camera().with_drawable_size(width, height);

        render.begin_frame(&window);
        if wireframe {
            render.set_wireframe(true);
        }

        // One draw for the floor, one for every obstacle, one for every crate.
        render.draw_mesh_colored(
            floor,
            shader,
            &Transform3D::IDENTITY,
            &camera,
            Color3D::new(0.32, 0.42, 0.30, 1.0),
        )?;
        render.draw_mesh_colored(
            obstacle_batch.mesh(),
            shader,
            &Transform3D::IDENTITY,
            &camera,
            Color3D::new(0.85, 0.62, 0.35, 1.0),
        )?;
        render.draw_mesh_colored(
            crate_batch.mesh(),
            shader,
            &Transform3D::IDENTITY,
            &camera,
            Color3D::new(0.62, 0.70, 0.82, 1.0),
        )?;
        crates_drawn = crate_count as u64;

        // The instanced field: one call, or one per cube on a driver without the
        // extension — which is exactly the comparison F2 switches between.
        field_calls = if instanced {
            render.draw_instanced(
                field_cube,
                instanced_shader,
                &field,
                &camera,
                Color3D::new(0.55, 0.80, 0.55, 1.0),
            )?
        } else {
            let mut calls = 0;
            for model in &field {
                render.draw_mesh_colored(
                    field_cube,
                    shader,
                    &Transform3D::from_translation(Vec3::new(
                        model.data[12],
                        model.data[13],
                        model.data[14],
                    )),
                    &camera,
                    Color3D::new(0.55, 0.80, 0.55, 1.0),
                )?;
                calls += 1;
            }
            calls
        };

        if wireframe {
            render.set_wireframe(false);
        }

        let capture = options
            .quadro
            .as_deref()
            .filter(|_| options.frames.is_some_and(|limit| frames + 1 >= limit));
        if let Some(path) = capture {
            // `read_pixels` runs the upscale pass before reading, so the picture is the
            // window-sized one even when `--scale` rendered into a smaller target. Ask
            // for the window's size: passing `width`/`height` (the low-resolution size)
            // returned the bottom-left corner of the upscaled frame.
            let (capture_width, capture_height) = window.drawable_size();
            let pixels = render.read_pixels(capture_width, capture_height)?;
            write_bmp(path, capture_width, capture_height, &pixels)?;
            println!("quadro: {path} ({capture_width}x{capture_height})");
        }

        render.end_frame(&window);

        let error = render.take_gl_error();
        if error != 0 {
            return Err(format!("GL error 0x{error:04x} on frame {frames}").into());
        }

        frames += 1;

        // Frame rate once a second, which is what "playable" is judged on.
        fps_frames += 1;
        fps_timer += 1.0 / 60.0;
        if fps_timer >= 1.0 {
            println!(
                "fps: {:.1} (frame {frames}, position {:.1},{:.1},{:.1})",
                fps_frames as f32 / fps_timer,
                controller.position().x,
                controller.position().y,
                controller.position().z,
            );
            fps_timer = 0.0;
            fps_frames = 0;
        }

        if let Some(limit) = options.frames {
            if frames >= limit {
                finish(
                    &mut render,
                    &[
                        floor,
                        cube,
                        field_cube,
                        obstacle_batch.mesh(),
                        crate_batch.mesh(),
                    ],
                    &[shader, instanced_shader],
                    frames,
                    wireframe,
                    crates_drawn,
                );
                if options.bench {
                    let wall = started_at.elapsed().as_secs_f32().max(f32::EPSILON);
                    let (render_width, render_height) = render.render_size();
                    // Counted here rather than read from the profiler: the counter only
                    // exists with the `profiling` feature, and the acceptance criterion
                    // has to hold in a plain release build.
                    let draw_calls = 3 + u64::from(field_calls);
                    println!(
                        "mini_fps: frames={frames} wall={wall:.3}s fps={:.1} \
                         draw_calls={draw_calls} field_calls={field_calls} \
                         instanced={} cubes={} scale={:.2} render={render_width}x{render_height} \
                         support={}",
                        frames as f32 / wall,
                        u8::from(instanced),
                        field.len(),
                        render.render_scale().unwrap_or(1.0),
                        render.instancing_support().name(),
                    );
                }
                return Ok(());
            }
            continue;
        }

        let spent = started.elapsed().as_millis() as u32;
        if spent < frame_ms {
            platform.delay_ms(frame_ms - spent);
        }
    }
}

/// Returns the mouse movement since the last frame, when the pointer is captured.
fn mouse_delta(input: &Input) -> (f32, f32) {
    let delta = input.mouse().delta();
    (delta.x, delta.y)
}

fn finish(
    render: &mut RenderContext3D,
    meshes: &[u64],
    shaders: &[u64],
    frames: u64,
    wireframe: bool,
    crates_drawn: u64,
) {
    println!(
        "{frames} frames rendered, no GL error (wireframe {wireframe}); \
         {crates_drawn} low crates on the last frame"
    );

    for mesh in meshes {
        render.destroy_mesh(*mesh);
    }
    for shader in shaders {
        render.destroy_shader(*shader);
    }
    println!(
        "released: {} meshes and {} shaders left",
        render.mesh_count(),
        render.shader_count()
    );
}

/// Writes RGBA8 pixels as a 32-bit BMP, bottom-up.
fn write_bmp(path: &str, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    let expected = (width as usize) * (height as usize) * 4;
    if rgba.len() != expected {
        return Err(format!(
            "capture has {} bytes; expected {expected}",
            rgba.len()
        ));
    }

    let mut pixels = Vec::with_capacity(expected);
    for pixel in rgba.chunks_exact(4) {
        pixels.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
    }

    let header = 54usize;
    let mut bmp = Vec::with_capacity(header + pixels.len());
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&((header + pixels.len()) as u32).to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&(header as u32).to_le_bytes());
    bmp.extend_from_slice(&40u32.to_le_bytes());
    bmp.extend_from_slice(&(width as i32).to_le_bytes());
    bmp.extend_from_slice(&(height as i32).to_le_bytes());
    bmp.extend_from_slice(&1u16.to_le_bytes());
    bmp.extend_from_slice(&32u16.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&(pixels.len() as u32).to_le_bytes());
    bmp.extend_from_slice(&2835i32.to_le_bytes());
    bmp.extend_from_slice(&2835i32.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&pixels);

    std::fs::write(path, &bmp).map_err(|err| format!("failed to write '{path}': {err}"))
}
