//! Demonstra animação por spritesheet, tween de posição e transições por eventos.
//!
//! Prima Espaço para alternar entre idle e run. `--auto` alterna os estados sem
//! interação, útil para executar a demonstração em CI.
//!
//! ```text
//! cargo run -p giochi-engine --example animation_demo
//! cargo run -p giochi-engine --example animation_demo -- --frames 240 --auto
//! ```

use std::process::ExitCode;

use giochi_engine::animation::{
    AnimationError, AnimationFrame, AnimationStateMachine, Easing, PlaybackMode, SpriteAnimation,
    Tween,
};
use giochi_engine::input::keyboard::KEY_SPACE;
use giochi_engine::{
    AudioSystem, Camera, Engine, EngineConfig, Input, Rect, RenderContext, Scene, Time, Vec2,
};

const WIDTH: u32 = 640;
const HEIGHT: u32 = 360;
const FRAME_SIZE: f32 = 32.0;
const STATE_IDLE: u32 = 0;
const STATE_RUN: u32 = 1;
const EVENT_RUN: u32 = 1;
const EVENT_IDLE: u32 = 2;

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
    let mut engine = Engine::new(EngineConfig::new(WIDTH, HEIGHT, "giochi — animation_demo"))?;
    if let Some(frames) = options.frames {
        engine.set_frame_limit(frames);
    }
    engine.render_mut().set_camera(Camera::with_position(
        Rect::new(0.0, 0.0, WIDTH as f32, HEIGHT as f32),
        WIDTH as f32 * 0.5,
        HEIGHT as f32 * 0.5,
    ));

    let texture = engine.render_mut().create_texture_from_rgba(
        FRAME_SIZE as u32 * 4,
        FRAME_SIZE as u32,
        &sprite_sheet(),
    )?;
    engine.push_scene(Box::new(AnimationScene::new(texture, options.auto)?));
    engine.run_current()?;

    let stats = engine.run_stats();
    println!(
        "animation_demo: frames={} fps={:.1}",
        stats.frames,
        stats.fps()
    );
    Ok(())
}

fn sprite_sheet() -> Vec<u8> {
    let colors = [
        [80, 190, 245, 255],
        [70, 165, 235, 255],
        [255, 170, 55, 255],
        [245, 115, 35, 255],
    ];
    let width = FRAME_SIZE as usize * colors.len();
    let height = FRAME_SIZE as usize;
    let mut pixels = vec![0; width * height * 4];

    for (frame, color) in colors.iter().enumerate() {
        for y in 0..height {
            for x in 0..FRAME_SIZE as usize {
                let checker = (x / 8 + y / 8) % 2 == 0;
                let shade = if checker { 1.0 } else { 0.75 };
                let offset = (y * width + frame * FRAME_SIZE as usize + x) * 4;
                for channel in 0..3 {
                    pixels[offset + channel] = (f32::from(color[channel]) * shade) as u8;
                }
                pixels[offset + 3] = color[3];
            }
        }
    }
    pixels
}

struct AnimationScene {
    idle: SpriteAnimation,
    running: SpriteAnimation,
    states: AnimationStateMachine,
    position_tween: Tween<Vec2>,
    position: Vec2,
    auto: bool,
    auto_timer: f32,
}

impl AnimationScene {
    fn new(texture: u64, auto: bool) -> Result<Self, AnimationError> {
        let frame = |index: u32| {
            AnimationFrame::new(
                Rect::new(index as f32 * FRAME_SIZE, 0.0, FRAME_SIZE, FRAME_SIZE),
                0.18,
            )
        };
        let idle =
            SpriteAnimation::new(texture, vec![frame(0)?, frame(1)?], PlaybackMode::PingPong)?;
        let running =
            SpriteAnimation::new(texture, vec![frame(2)?, frame(3)?], PlaybackMode::Loop)?;
        let mut states = AnimationStateMachine::new(STATE_IDLE);
        states.add_transition(STATE_IDLE, EVENT_RUN, STATE_RUN)?;
        states.add_transition(STATE_RUN, EVENT_IDLE, STATE_IDLE)?;
        let position_tween = Tween::new(
            Vec2::new(80.0, (HEIGHT as f32 - FRAME_SIZE) * 0.5),
            Vec2::new(
                WIDTH as f32 - FRAME_SIZE - 80.0,
                (HEIGHT as f32 - FRAME_SIZE) * 0.5,
            ),
            4.0,
            Easing::EaseInOut,
        )?;
        let position = position_tween.value();

        Ok(Self {
            idle,
            running,
            states,
            position_tween,
            position,
            auto,
            auto_timer: 0.0,
        })
    }

    fn toggle_state(&mut self) {
        let event = if self.states.current_state() == STATE_IDLE {
            EVENT_RUN
        } else {
            EVENT_IDLE
        };
        self.states.trigger(event);
    }
}

impl Scene for AnimationScene {
    fn update(&mut self, time: Time, input: &Input, _audio: &mut AudioSystem) {
        if input.key_pressed(KEY_SPACE) {
            self.toggle_state();
            self.auto_timer = 1.0;
        }

        if self.auto {
            self.auto_timer -= time.delta;
            if self.auto_timer <= 0.0 {
                self.toggle_state();
                self.auto_timer = 1.0;
            }
        }

        let update = if self.states.current_state() == STATE_RUN {
            self.running.update(time.delta)
        } else {
            self.idle.update(time.delta)
        };
        if let Err(error) = update {
            eprintln!("não foi possível atualizar a animação: {error}");
        }

        match self.position_tween.update(time.delta) {
            Ok(position) => self.position = position,
            Err(error) => eprintln!("não foi possível atualizar o tween: {error}"),
        }
    }

    fn render(&mut self, renderer: &mut RenderContext) {
        let animation = if self.states.current_state() == STATE_RUN {
            &self.running
        } else {
            &self.idle
        };
        if let Err(error) = animation.draw(
            renderer,
            Rect::new(self.position.x, self.position.y, FRAME_SIZE, FRAME_SIZE),
        ) {
            eprintln!("não foi possível desenhar o sprite: {error}");
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
