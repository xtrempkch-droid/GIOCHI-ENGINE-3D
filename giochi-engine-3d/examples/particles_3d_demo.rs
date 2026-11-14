//! Demonstra billboards 3D com uma explosão ao clicar.
//!
//! ```text
//! cargo run -p giochi-engine-3d --example particles_3d_demo
//! cargo run -p giochi-engine-3d --example particles_3d_demo -- --frames 240 --auto
//! ```

use std::process::ExitCode;
use std::time::Instant;

use giochi_engine::platform::sdl::{quit_signal, QuitSignal};
use giochi_engine::platform::window::{Window, WindowConfig};
use giochi_engine::platform::Platform;
use giochi_engine_3d::context::{self, GlState, GlesVersion};
use giochi_engine_3d::math3d::{Camera3D, Color3D, Vec3};
use giochi_engine_3d::mesh::Vertex;
use giochi_engine_3d::particles::BillboardParticleSystem;
use giochi_engine_3d::shader::BuiltinShader;
use giochi_engine_3d::RenderContext3D;
use sdl2::event::Event;
use sdl2::mouse::MouseButton;

const WIDTH: u32 = 960;
const HEIGHT: u32 = 540;

fn main() -> ExitCode {
    match run(Options::parse(std::env::args().skip(1))) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("erro: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(options: Result<Options, String>) -> Result<(), Box<dyn std::error::Error>> {
    let options = options?;
    let platform = Platform::new()?;
    context::configure_attributes(&platform, GlesVersion::from_env())?;
    let window = Window::new_opengl(
        &platform,
        WindowConfig::new(WIDTH, HEIGHT),
        "giochi — particles_3d_demo",
    )?;
    let mut renderer = RenderContext3D::new(context::create(&window)?);

    let state = GlState {
        blend: true,
        cull_face: false,
        ..GlState::default()
    };
    renderer.set_state(state);
    renderer.set_clear_color(Color3D::new(0.025, 0.035, 0.07, 1.0));

    let quad = renderer.create_mesh(
        &[
            Vertex::flat([-0.5, -0.5, 0.0], [0.0, 0.0, 1.0]),
            Vertex::flat([0.5, -0.5, 0.0], [0.0, 0.0, 1.0]),
            Vertex::flat([0.5, 0.5, 0.0], [0.0, 0.0, 1.0]),
            Vertex::flat([-0.5, 0.5, 0.0], [0.0, 0.0, 1.0]),
        ],
        &[0, 1, 2, 0, 2, 3],
    )?;
    let shader = renderer.create_builtin_shader(BuiltinShader::Instanced)?;
    let mut particles = BillboardParticleSystem::new(5_000, Vec3::new(0.0, -1.6, 0.0), 23)?;
    let camera = Camera3D::from_parts(
        Vec3::new(0.0, 0.0, 2.0),
        Vec3::new(0.0, 0.0, -6.0),
        Vec3::UP,
    );
    let mut last_frame = Instant::now();
    let mut elapsed = 0.0f32;
    let mut next_burst = 0.0f32;
    let mut frames = 0u64;

    loop {
        let now = Instant::now();
        let delta = now.duration_since(last_frame).as_secs_f32().min(0.1);
        last_frame = now;
        elapsed += delta;

        let mut should_quit = false;
        for event in platform.poll_events() {
            if quit_signal(&event) != QuitSignal::None {
                should_quit = true;
            }
            if matches!(
                event,
                Event::MouseButtonDown {
                    mouse_btn: MouseButton::Left,
                    ..
                }
            ) {
                particles.emit_burst(Vec3::new(0.0, 0.0, -6.0), 500, 4.0, 1.5, 0.24)?;
            }
        }
        if should_quit {
            break;
        }

        if options.auto {
            next_burst -= delta;
            if next_burst <= 0.0 {
                particles.emit_burst(
                    Vec3::new((elapsed * 0.8).sin() * 0.8, 0.0, -6.0),
                    500,
                    4.0,
                    1.5,
                    0.24,
                )?;
                next_burst = 0.65;
            }
        }
        particles.update(delta)?;

        let (width, height) = renderer.drawable_size();
        let camera = camera.with_drawable_size(width, height);
        renderer.begin_frame(&window);
        let calls = particles.render(
            &mut renderer,
            quad,
            shader,
            &camera,
            Color3D::new(1.0, 0.56, 0.16, 0.72),
        )?;
        renderer.end_frame(&window);

        frames += 1;
        if options.frames.is_some_and(|limit| frames >= limit) {
            break;
        }
        if options.frames.is_none() {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        if frames % 120 == 0 {
            println!(
                "frames={frames} partículas={} chamadas={calls} instancing={}",
                particles.len(),
                renderer.supports_instancing()
            );
        }
    }

    renderer.destroy_mesh(quad);
    renderer.destroy_shader(shader);
    println!(
        "particles_3d_demo: frames={frames} partículas restantes={}",
        particles.len()
    );
    Ok(())
}

struct Options {
    frames: Option<u64>,
    auto: bool,
}

impl Options {
    fn parse(arguments: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut options = Self {
            frames: None,
            auto: false,
        };
        let mut arguments = arguments.peekable();
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--auto" => options.auto = true,
                "--frames" => {
                    let value = arguments
                        .next()
                        .ok_or_else(|| "--frames precisa de um valor".to_owned())?;
                    options.frames = Some(
                        value
                            .parse()
                            .map_err(|_| format!("--frames inválido: {value}"))?,
                    );
                }
                other => return Err(format!("argumento desconhecido: {other}")),
            }
        }
        if options.frames == Some(0) {
            return Err("--frames 0 não desenha nada".to_owned());
        }
        Ok(options)
    }
}
