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
//! ```text
//! cargo run -p giochi-engine-3d --example mini_fps
//! cargo run -p giochi-engine-3d --example mini_fps -- --frames 300 --auto
//! cargo run -p giochi-engine-3d --example mini_fps -- --frames 3 --quadro out.bmp
//! ```

use std::process::ExitCode;

use giochi_engine::input::Input;
use giochi_engine::platform::sdl::{quit_signal, QuitSignal};
use giochi_engine::platform::window::{Window, WindowConfig};
use giochi_engine::platform::Platform;
use giochi_engine_3d::context::{self, GlesVersion};
use giochi_engine_3d::math3d::{AxisInput, CameraController, Color3D, Quat, Transform3D, Vec3};
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
/// Edge length of the low crates.
///
/// Deliberately **below** the eye. The phase-13 acceptance criterion is that a cube's top
/// face reads brighter than its side, and with obstacles taller than the camera no top face
/// is ever visible — the player looks up at them. These crates put a top face in view so
/// the lighting can be judged rather than merely trusted.
const CRATE_SIZE: f32 = 0.6;

#[derive(Debug, Default)]
struct Options {
    /// Stop after this many frames.
    frames: Option<u64>,
    /// Walk a scripted path instead of waiting for input.
    auto: bool,
    /// Capture the last frame to this BMP.
    quadro: Option<String>,
}

impl Options {
    fn parse() -> Self {
        let mut options = Self::default();
        let mut args = std::env::args().skip(1);

        while let Some(arg) = args.next() {
            if let Some(value) = arg.strip_prefix("--frames=") {
                options.frames = value.parse().ok();
            } else if arg == "--frames" {
                options.frames = args.next().and_then(|value| value.parse().ok());
            } else if arg == "--auto" {
                options.auto = true;
            } else if arg == "--quadro" {
                options.quadro = args.next();
            } else if let Some(value) = arg.strip_prefix("--quadro=") {
                options.quadro = Some(value.to_string());
            }
        }

        options
    }
}

fn main() -> ExitCode {
    env_logger::init();

    match run(Options::parse()) {
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

    let window = Window::new_opengl(&platform, WindowConfig::new(1280, 720), "giochi mini_fps")?;
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
    let mut frames: u64 = 0;
    let mut fps_timer: f32 = 0.0;
    let mut fps_frames: u32 = 0;

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
                    [floor, cube],
                    shader,
                    frames,
                    wireframe,
                    crates_drawn,
                );
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

        crates_drawn = 0;
        render.begin_frame(&window);
        if wireframe {
            render.set_wireframe(true);
        }

        render.draw_mesh_colored(
            floor,
            shader,
            &Transform3D::IDENTITY,
            &camera,
            Color3D::new(0.32, 0.42, 0.30, 1.0),
        )?;

        for row in 0..OBSTACLES {
            for column in 0..OBSTACLES {
                // Skip the spawn area so the player does not start inside a cube.
                let x = (column - OBSTACLES / 2) as f32 * SPACING;
                let z = (row - OBSTACLES / 2) as f32 * SPACING - 6.0;
                if x.abs() < 1.0 && (z - 8.0).abs() < 3.0 {
                    continue;
                }

                // A fixed tilt per position: identical cubes would hide a broken normal,
                // because every face would be lit the same way.
                let tilt =
                    Quat::from_axis_angle(Vec3::UP, (row * OBSTACLES + column) as f32 * 0.37);
                let model = Transform3D::from_translation(Vec3::new(x, OBSTACLE_SIZE * 0.5, z))
                    .with_rotation(tilt);

                render.draw_mesh_colored(
                    cube,
                    shader,
                    &model,
                    &camera,
                    Color3D::new(0.85, 0.62, 0.35, 1.0),
                )?;

                // A crate between this obstacle and the next one along X. Low enough that
                // the camera looks down on it, which is the only way a top face is ever on
                // screen, and a slightly different colour so the two are distinguishable in
                // a capture.
                let crate_x = x + SPACING * 0.5;
                if crate_x.abs() < FLOOR_SIZE * 0.45 {
                    crates_drawn += 1;
                    let crate_model =
                        Transform3D::from_translation(Vec3::new(crate_x, CRATE_SIZE * 0.5, z))
                            .with_rotation(Quat::from_axis_angle(
                                Vec3::UP,
                                (row * OBSTACLES + column) as f32 * 0.61,
                            ));

                    render.draw_mesh_colored(
                        cube,
                        shader,
                        &crate_model,
                        &camera,
                        Color3D::new(0.62, 0.70, 0.82, 1.0),
                    )?;
                }
            }
        }

        if wireframe {
            render.set_wireframe(false);
        }

        let capture = options
            .quadro
            .as_deref()
            .filter(|_| options.frames.is_some_and(|limit| frames + 1 >= limit));
        if let Some(path) = capture {
            let pixels = render.read_pixels(width, height)?;
            write_bmp(path, width, height, &pixels)?;
            println!("quadro: {path} ({width}x{height})");
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
                    [floor, cube],
                    shader,
                    frames,
                    wireframe,
                    crates_drawn,
                );
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
    meshes: [u64; 2],
    shader: u64,
    frames: u64,
    wireframe: bool,
    crates_drawn: u64,
) {
    println!(
        "{frames} frames rendered, no GL error (wireframe {wireframe}); \
         {crates_drawn} low crates on the last frame"
    );

    for mesh in meshes {
        render.destroy_mesh(mesh);
    }
    render.destroy_shader(shader);
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
