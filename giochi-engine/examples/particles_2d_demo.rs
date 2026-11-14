//! Demonstra explosões e um rasto de partículas 2D.
//!
//! Clique e mantenha o botão esquerdo para emitir; `--auto` gera movimento e explosões
//! determinísticas para uma verificação sem teclado.
//!
//! ```text
//! cargo run -p giochi-engine --example particles_2d_demo
//! cargo run -p giochi-engine --example particles_2d_demo -- --frames 240 --auto
//! ```

use std::process::ExitCode;

use giochi_engine::input::mouse::MOUSE_LEFT;
use giochi_engine::particles::{presets, ParticleSystem, PointEmitter};
use giochi_engine::{
    AudioSystem, Camera, Engine, EngineConfig, Input, Rect, RenderContext, Scene, Time, Vec2,
};

const WIDTH: u32 = 960;
const HEIGHT: u32 = 540;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("erro: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let options = Options::parse(std::env::args().skip(1))?;
    let mut engine = Engine::new(EngineConfig::new(
        WIDTH,
        HEIGHT,
        "giochi — particles_2d_demo",
    ))?;
    if let Some(frames) = options.frames {
        engine.set_frame_limit(frames);
    }
    engine.render_mut().set_camera(Camera::with_position(
        Rect::new(0.0, 0.0, WIDTH as f32, HEIGHT as f32),
        WIDTH as f32 * 0.5,
        HEIGHT as f32 * 0.5,
    ));

    let texture = engine.render_mut().load_texture(&asset("checker.png"))?;
    let scene = ParticleScene::new(texture, options.auto)?;
    engine.push_scene(Box::new(scene));
    engine.run_current()?;

    let stats = engine.run_stats();
    println!(
        "particles_2d_demo: frames={} fps={:.1}",
        stats.frames,
        stats.fps()
    );
    Ok(())
}

fn asset(name: &str) -> String {
    format!("{}/../assets/{name}", env!("CARGO_MANIFEST_DIR"))
}

struct ParticleScene {
    explosion: ParticleSystem,
    trail: ParticleSystem,
    auto: bool,
    elapsed: f32,
    next_burst: f32,
}

impl ParticleScene {
    fn new(texture: u64, auto: bool) -> Result<Self, giochi_engine::ParticleError> {
        let explosion = ParticleSystem::with_capacity(texture, 5_000, presets::explosion())?;
        let trail = ParticleSystem::with_capacity(texture, 5_000, presets::trail())?;
        Ok(Self {
            explosion,
            trail,
            auto,
            elapsed: 0.0,
            next_burst: 0.0,
        })
    }

    fn emit_at(
        system: &mut ParticleSystem,
        position: Vec2,
        count: usize,
    ) -> Result<usize, giochi_engine::ParticleError> {
        system.emit(&PointEmitter::new(position)?, count)
    }
}

impl Scene for ParticleScene {
    fn update(&mut self, time: Time, input: &Input, _audio: &mut AudioSystem) {
        self.elapsed += time.delta;
        let cursor = if self.auto {
            Vec2::new(
                WIDTH as f32 * 0.5 + (self.elapsed * 1.1).cos() * 150.0,
                HEIGHT as f32 * 0.5 + (self.elapsed * 1.4).sin() * 90.0,
            )
        } else {
            input.mouse().position_vec()
        };

        if self.auto {
            self.next_burst -= time.delta;
            if self.next_burst <= 0.0 {
                if let Err(error) = Self::emit_at(&mut self.explosion, cursor, 400) {
                    log::error!("não foi possível emitir a explosão: {error}");
                }
                self.next_burst = 0.7;
            }
            if let Err(error) = Self::emit_at(&mut self.trail, cursor, 4) {
                log::error!("não foi possível emitir o rasto: {error}");
            }
        } else {
            if input.mouse().is_pressed_this_frame(MOUSE_LEFT) {
                if let Err(error) = Self::emit_at(&mut self.explosion, cursor, 400) {
                    log::error!("não foi possível emitir a explosão: {error}");
                }
            }
            if input.mouse_down(MOUSE_LEFT) {
                if let Err(error) = Self::emit_at(&mut self.trail, cursor, 4) {
                    log::error!("não foi possível emitir o rasto: {error}");
                }
            }
        }

        for system in [&mut self.explosion, &mut self.trail] {
            if let Err(error) = system.update(time.delta) {
                log::error!("não foi possível atualizar partículas: {error}");
            }
        }
    }

    fn render(&mut self, renderer: &mut RenderContext) {
        for system in [&self.explosion, &self.trail] {
            if let Err(error) = system.render(renderer) {
                log::error!("não foi possível desenhar partículas: {error}");
            }
        }
    }
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
