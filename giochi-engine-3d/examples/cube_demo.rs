//! Phase 9 smoke test: a window that is one solid colour and closes on ESC.
//!
//! This is the whole point of the phase — prove the EGL/GLES path opens, clears and
//! presents on a real driver before anything is drawn into it. If the screen is blue,
//! the context works; if it is black or the process dies, the context does not, and
//! every later phase would be built on sand.
//!
//! ```text
//! cargo run -p giochi-engine-3d --example cube_demo
//! cargo run -p giochi-engine-3d --example cube_demo -- --frames 120   # non-interactive
//! GIOCHI_GL_VERSION=3 cargo run -p giochi-engine-3d --example cube_demo
//! ```
//!
//! `--frames` exists because `SDL_VIDEODRIVER=offscreen` provides a real EGL/GLES
//! context with no display attached, which is the only way CI can exercise this path.
//! Measured: the `dummy` driver has no OpenGL at all ("OpenGL support is either not
//! configured in SDL"), while `offscreen` gets the full driver stack.

use std::process::ExitCode;

use giochi_engine::platform::sdl::{quit_signal, QuitSignal};
use giochi_engine::platform::window::{Window, WindowConfig};
use giochi_engine::platform::Platform;
use giochi_engine_3d::context::{self, GlesVersion};
use giochi_engine_3d::math3d::{Camera3D, Color3D};
use giochi_engine_3d::RenderContext3D;

/// Command line options.
#[derive(Debug, Default)]
struct Options {
    /// Stop after this many frames instead of waiting for ESC.
    frames: Option<u64>,
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

    // Phase 9's acceptance criterion: the screen is this colour. Everything else in the
    // frame skeleton is already in place, so "it is blue" means the whole EGL path --
    // attributes, context, clear, swap -- works end to end.
    render.set_clear_color(Color3D::SKY);

    println!(
        "GLES {} on {}",
        render.gles_version().major(),
        render.adapter()
    );

    // The camera is created here and resolved against the real drawable size on every
    // frame. On KMSDRM that is the panel, which is not the size the config asked for
    // (ADR-035) -- a hardcoded 16:9 would crop a 4:3 panel.
    let base_camera = Camera3D::new();
    if requested == GlesVersion::Gles3 && !render.supports_gles3() {
        println!("GLES 3.0 was requested but the driver gave 2.0; continuing on 2.0");
    }

    // The frame cap keeps the demo from pinning a core, and matches the 2D engine's
    // 60 Hz timestep. It is skipped when a frame limit is given, so a smoke run
    // finishes as fast as the driver allows.
    let frame_ms = if options.frames.is_some() {
        0
    } else {
        1000 / 60
    };
    let mut frames: u64 = 0;

    loop {
        let started = std::time::Instant::now();

        for event in platform.poll_events() {
            if quit_signal(&event) != QuitSignal::None {
                println!("{frames} frames rendered");
                return Ok(());
            }
        }

        render.begin_frame(&window);

        let camera =
            base_camera.with_drawable_size(render.drawable_size().0, render.drawable_size().1);
        if camera.view_projection().is_err() {
            return Err("the camera produced no view-projection matrix".into());
        }

        render.end_frame(&window);

        // The context is double-buffered, so a frame is only "good" once the driver has
        // not raised an error while building it. `glGetError` is the cheapest possible
        // assertion that the whole path executed.
        let error = render.take_gl_error();
        if error != 0 {
            return Err(format!("GL error 0x{error:04x} on frame {frames}").into());
        }

        frames += 1;

        if let Some(limit) = options.frames {
            if frames >= limit {
                println!("{frames} frames rendered, no GL error");
                return Ok(());
            }
            continue;
        }

        let elapsed = started.elapsed().as_millis() as u32;
        if elapsed < frame_ms {
            platform.delay_ms(frame_ms - elapsed);
        }
    }
}
