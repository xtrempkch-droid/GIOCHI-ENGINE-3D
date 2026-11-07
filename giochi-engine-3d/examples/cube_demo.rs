//! Rotating cube — the phase 11 acceptance check.
//!
//! A window, a mesh, a shader, a camera and a depth buffer: if the cube spins and its
//! back faces stay hidden, the whole 3D pipeline works. If the screen is a flat hexagon,
//! depth testing is off; if it is empty, the winding is backwards or the projection is
//! wrong; if it is black, the shader failed.
//!
//! ```text
//! cargo run -p giochi-engine-3d --example cube_demo
//! cargo run -p giochi-engine-3d --example cube_demo -- --frames 120
//! cargo run -p giochi-engine-3d --example cube_demo -- --frames 3 --quadro cube.bmp
//! SDL_VIDEODRIVER=offscreen GIOCHI_FORCE_NO_VAO=1 \
//!     cargo run -p giochi-engine-3d --example cube_demo -- --frames 60
//! ```
//!
//! `--frames` and the two environment variables exist for CI and for the console, where
//! there is no keyboard to press ESC on.

use std::process::ExitCode;

use giochi_engine::platform::sdl::{quit_signal, QuitSignal};
use giochi_engine::platform::window::{Window, WindowConfig};
use giochi_engine::platform::Platform;
use giochi_engine_3d::context::{self, GlesVersion};
use giochi_engine_3d::math3d::{Camera3D, Color3D, Quat, Transform3D, Vec3};
use giochi_engine_3d::shader::BuiltinShader;
use giochi_engine_3d::RenderContext3D;

/// How fast the cube spins, in radians per second.
const SPIN_RATE: f32 = 0.9;
/// Cube edge length, in world units.
const CUBE_SIZE: f32 = 2.0;
/// Distance from the camera to the cube.
const CUBE_DISTANCE: f32 = 5.0;

#[derive(Debug, Default)]
struct Options {
    /// Stop after this many frames instead of waiting for ESC.
    frames: Option<u64>,
    /// `--quadro <arquivo.bmp>`: capture the last frame.
    ///
    /// Exists for the same reason the 2D demo has it: "drew a frame" and "drew the right
    /// frame" are different claims, and over SSH to a console there is no way to look at
    /// the screen. Works with `offscreen`, so it also produces a picture in CI.
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

    let window = Window::new_opengl(&platform, WindowConfig::new(1280, 720), "giochi 3D")?;
    let mut render = RenderContext3D::new(context::create(&window)?);

    println!(
        "GLES {} on {}",
        render.gles_version().major(),
        render.adapter()
    );
    if requested == GlesVersion::Gles3 && !render.supports_gles3() {
        println!("GLES 3.0 was requested but the driver gave 2.0; continuing on 2.0");
    }

    let mesh = render.create_cube_mesh(CUBE_SIZE)?;
    let shader = render.create_builtin_shader(BuiltinShader::Flat)?;
    println!(
        "mesh {mesh}: {} vertices, {} indices, vertex array object: {}",
        render.mesh_vertex_count(mesh),
        render.mesh_index_count(mesh),
        if render.mesh_uses_vao(mesh) {
            "yes"
        } else {
            "no (fallback path)"
        }
    );

    render.set_clear_color(Color3D::new(0.06, 0.08, 0.13, 1.0));

    // The camera is resolved against the real drawable size every frame. On KMSDRM that
    // is the panel, which is not the size the config asked for (ADR-035).
    let base_camera = Camera3D::from_parts(
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(0.0, 0.0, -1.0),
        Vec3::UP,
    );

    let frame_ms = if options.frames.is_some() {
        0
    } else {
        1000 / 60
    };
    let mut frames: u64 = 0;
    let mut elapsed: f32 = 0.0;

    loop {
        let started = std::time::Instant::now();

        for event in platform.poll_events() {
            if quit_signal(&event) != QuitSignal::None {
                report(&mut render, mesh, shader, frames);
                return Ok(());
            }
        }

        let (width, height) = window.drawable_size();
        let camera = base_camera.with_drawable_size(width, height);

        // Two rotations about different axes: the cube tumbles, so every frame shows a
        // different combination of faces. A single-axis spin can hide a broken normal.
        let model = Transform3D::from_rotation(
            Quat::from_axis_angle(Vec3::new(1.0, 0.0, 0.0), elapsed * SPIN_RATE * 0.6)
                * Quat::from_axis_angle(Vec3::UP, elapsed * SPIN_RATE),
        )
        .with_position(Vec3::new(0.0, 0.0, -CUBE_DISTANCE));

        render.begin_frame(&window);
        render.draw_mesh_colored(
            mesh,
            shader,
            &model,
            &camera,
            Color3D::new(0.95, 0.55, 0.25, 1.0),
        )?;

        // The capture has to happen before the swap: after `end_frame` the back buffer
        // holds the *next* frame's undefined contents.
        let capture = options
            .quadro
            .as_deref()
            .filter(|_| options.frames.is_some_and(|limit| frames + 1 >= limit));

        if let Some(path) = capture {
            let (width, height) = render.drawable_size();
            let pixels = render.read_pixels(width, height)?;
            write_bmp(path, width, height, &pixels)?;
            println!("quadro: {path} ({width}x{height})");
        }

        render.end_frame(&window);

        // Draining the error queue after every frame is what turns a "black screen" into
        // a named failure.
        let error = render.take_gl_error();
        if error != 0 {
            return Err(format!("GL error 0x{error:04x} on frame {frames}").into());
        }

        frames += 1;
        elapsed += 1.0 / 60.0;

        if let Some(limit) = options.frames {
            if frames >= limit {
                report(&mut render, mesh, shader, frames);
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

fn report(render: &mut RenderContext3D, mesh: u64, shader: u64, frames: u64) {
    println!("{frames} frames rendered, no GL error");

    // The leak check the 2D engine also prints: after destroying its own resources a game
    // should have nothing left in the registry.
    render.destroy_mesh(mesh);
    render.destroy_shader(shader);
    println!(
        "released: {} meshes and {} shaders left",
        render.mesh_count(),
        render.shader_count()
    );
}

/// Writes RGBA8 pixels as a 32-bit BMP, bottom-up.
///
/// BMP and not PNG for the same reason the 2D demo uses it: `SDL2_image` on the console
/// is built without `libpng`, and carrying a deflate implementation for a capture the PC
/// converts anyway would be silly. This writer is self-contained, so it works on a target
/// with no image libraries at all.
fn write_bmp(path: &str, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    let expected = (width as usize) * (height as usize) * 4;
    if rgba.len() != expected {
        return Err(format!(
            "capture has {} bytes; expected {expected} ({width}x{height} RGBA)",
            rgba.len()
        ));
    }

    // GL returns rows bottom-up, which is exactly BMP's order, so no flip is needed.
    // The channel order does change: RGBA to BGRA.
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
    // BITMAPINFOHEADER: 40 bytes, 32 bpp, BI_RGB, bottom-up (positive height).
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
