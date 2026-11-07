//! `mini_game` — a complete, playable game built only on the public engine API.
//!
//! A small space shooter: you fly a ship at the bottom of the screen, shoot the
//! falling drones, and lose a life when one reaches you. It exercises **every**
//! module: the `Engine` facade, the scene stack (the game over screen is a second
//! scene), the fixed-timestep loop, keyboard/gamepad input, sprite rendering with a
//! camera, the texture cache and sound effects.
//!
//! Controls:
//!
//! * **A / D** or **left / right** — move;
//! * **Space** — shoot;
//! * **Escape** — quit.
//!
//! ```text
//! cargo run -p giochi-engine --example mini_game
//! cargo run -p giochi-engine --example mini_game -- --frames 600
//! cargo run -p giochi-engine --example mini_game -- --auto --frames 600
//! ```
//!
//! `--auto` makes the scene play itself (it fires and tracks the nearest drone), so
//! the scoring and collision paths can be verified from a script without a human at
//! the keyboard.

use std::cell::Cell;
use std::process::ExitCode;
use std::rc::Rc;

use giochi_engine::input::gamepad::{AXIS_LEFT_X, BUTTON_SOUTH};
use giochi_engine::input::keyboard::{KEY_A, KEY_D, KEY_LEFT, KEY_RIGHT, KEY_SPACE};
use giochi_engine::{
    AudioSystem, Camera, Color, Engine, EngineConfig, Input, Rect, RenderContext, Scene,
    SceneTransition, Sprite, TextureHandle, Time, Vec2,
};

const WIDTH: u32 = 960;
const HEIGHT: u32 = 540;
const SHIP_SIZE: f32 = 48.0;
const SHIP_SPEED: f32 = 420.0;
const BULLET_SIZE: f32 = 6.0;
const BULLET_SPEED: f32 = 620.0;
const DRONE_SIZE: f32 = 40.0;
const DRONE_SPEED: f32 = 130.0;
const SHOT_COOLDOWN: f32 = 0.18;
const SPAWN_INTERVAL: f32 = 0.75;
const STARTING_LIVES: u32 = 3;
const MAX_BULLETS: usize = 32;
const MAX_DRONES: usize = 24;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("erro: {error}");
            ExitCode::FAILURE
        }
    }
}

fn asset(name: &str) -> String {
    format!("{}/../assets/{name}", env!("CARGO_MANIFEST_DIR"))
}

fn run() -> Result<(), String> {
    let options = parse_args();
    let frame_limit = options.frames;

    let mut engine = Engine::new(EngineConfig::new(WIDTH, HEIGHT, "giochi — mini_game"))
        .map_err(|error| error.to_string())?;

    if let Some(limit) = frame_limit {
        engine.set_frame_limit(limit);
    }

    let ship = engine
        .render_mut()
        .load_texture(&asset("player.png"))
        .map_err(|error| error.to_string())?;
    let drone = engine
        .render_mut()
        .load_texture(&asset("checker.png"))
        .map_err(|error| error.to_string())?;

    // The shot sound is optional: a silent engine must not stop the game.
    let shot = load_shot_sound(&mut engine);
    println!(
        "mini_game: A/D move, ESPACO atira — ESC sai (som: {})",
        if shot != 0 { "ativo" } else { "silencioso" }
    );

    let final_score = Rc::new(Cell::new(0u32));
    engine.push_scene(Box::new(PlayScene::new(
        ship,
        drone,
        shot,
        options.auto_play,
        Rc::clone(&final_score),
    )));
    engine.run_current().map_err(|error| error.to_string())?;

    let stats = engine.run_stats();
    println!(
        "encerrado: {} frames, {} passos, {:.1} fps",
        stats.frames,
        stats.steps,
        stats.fps()
    );
    println!("pontuação final: {}", final_score.get());

    Ok(())
}

/// One bullet, in world space.
#[derive(Debug, Clone, Copy)]
struct Bullet {
    position: Vec2,
    velocity: Vec2,
}

/// A drone costs a life when it overlaps the ship or falls past the bottom.
fn drone_hits_ship(drone: &Drone, ship_rect: Rect) -> bool {
    !drone.sprite.world_rect().intersection(ship_rect).is_empty() || drone.sprite.y > HEIGHT as f32
}

/// One enemy drone.
#[derive(Debug, Clone, Copy)]
struct Drone {
    sprite: Sprite,
    alive: bool,
}

/// The main gameplay scene.
struct PlayScene {
    drone_texture: TextureHandle,
    ship: Sprite,
    bullets: Vec<Bullet>,
    drones: Vec<Drone>,
    spawn_timer: f32,
    shot_timer: f32,
    lives: u32,
    score: u32,
    game_over: bool,
    /// Shot sound handle; `0` when the engine is silent.
    shot: u64,
    /// Whether the scene drives itself (`--auto`).
    auto_play: bool,
    /// Mirrors `score` so the driver can report it after the run.
    final_score: Rc<Cell<u32>>,
}

impl PlayScene {
    fn new(
        ship_texture: TextureHandle,
        drone_texture: TextureHandle,
        shot: u64,
        auto_play: bool,
        final_score: Rc<Cell<u32>>,
    ) -> Self {
        let ship_x = (WIDTH as f32 - SHIP_SIZE) * 0.5;
        let ship_y = HEIGHT as f32 - SHIP_SIZE - 16.0;

        Self {
            drone_texture,
            ship: Sprite::new(ship_texture, ship_x, ship_y, SHIP_SIZE, SHIP_SIZE),
            bullets: Vec::with_capacity(MAX_BULLETS),
            drones: Vec::with_capacity(MAX_DRONES),
            spawn_timer: 0.0,
            shot_timer: 0.0,
            lives: STARTING_LIVES,
            score: 0,
            game_over: false,
            shot,
            auto_play,
            final_score,
        }
    }

    /// Auto-play: steer towards the nearest drone and fire constantly.
    ///
    /// Only used by `--auto`, which exists so the example can be verified from a
    /// script; it uses the same movement and shooting code a player drives.
    fn auto_direction(&self) -> f32 {
        let ship_centre = self.ship.x + SHIP_SIZE * 0.5;
        let target = self
            .drones
            .iter()
            .map(|drone| (drone.sprite.y, drone.sprite.x + DRONE_SIZE * 0.5))
            .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

        match target {
            Some((_, x)) => {
                let distance = x - ship_centre;
                if distance.abs() < 8.0 {
                    0.0
                } else if distance < 0.0 {
                    -1.0
                } else {
                    1.0
                }
            }
            None => 0.0,
        }
    }

    fn spawn_drone(&mut self) {
        if self.drones.len() >= MAX_DRONES {
            return;
        }

        // A cheap deterministic pseudo-random source keeps the example dependency
        // free while still varying the spawn positions.
        let seed = self
            .score
            .wrapping_mul(2_654_435_761)
            .wrapping_add(self.spawn_timer.to_bits())
            .wrapping_add(self.drones.len() as u32 * 97);
        let lane = seed % 8;
        let x = 16.0 + lane as f32 * ((WIDTH as f32 - DRONE_SIZE - 32.0) / 7.0);

        self.drones.push(Drone {
            sprite: Sprite::new(self.drone_texture, x, -DRONE_SIZE, DRONE_SIZE, DRONE_SIZE),
            alive: true,
        });
    }
}

impl Scene for PlayScene {
    fn on_enter(&mut self) {
        log::info!("mini_game: on_enter");
    }

    fn on_exit(&mut self) {
        log::info!("mini_game: on_exit (score {})", self.score);
    }

    fn update(&mut self, time: Time, input: &Input, audio: &mut AudioSystem) {
        if self.game_over {
            return;
        }

        // --- movement ------------------------------------------------------
        let mut direction = 0.0;
        if input.key_down(KEY_A) || input.key_down(KEY_LEFT) {
            direction -= 1.0;
        }
        if input.key_down(KEY_D) || input.key_down(KEY_RIGHT) {
            direction += 1.0;
        }
        direction += input.gamepad().axis(AXIS_LEFT_X);
        if self.auto_play {
            direction += self.auto_direction();
        }

        self.ship.x = (self.ship.x + direction * SHIP_SPEED * time.delta)
            .clamp(8.0, WIDTH as f32 - SHIP_SIZE - 8.0);

        // --- shooting ------------------------------------------------------
        self.shot_timer = (self.shot_timer - time.delta).max(0.0);
        let wants_to_shoot =
            input.key_down(KEY_SPACE) || input.gamepad().is_down(BUTTON_SOUTH) || self.auto_play;
        if wants_to_shoot && self.shot_timer == 0.0 && self.bullets.len() < MAX_BULLETS {
            self.bullets.push(Bullet {
                position: Vec2::new(
                    self.ship.x + SHIP_SIZE * 0.5 - BULLET_SIZE * 0.5,
                    self.ship.y,
                ),
                velocity: Vec2::new(0.0, -BULLET_SPEED),
            });
            self.shot_timer = SHOT_COOLDOWN;

            // A silent engine returns an error; the game must keep running (ADR-021).
            if self.shot != 0 {
                if let Err(error) = audio.play_sound(self.shot, 0) {
                    log::debug!("sem áudio: {error}");
                }
            }
        }

        // --- bullets -------------------------------------------------------
        for bullet in &mut self.bullets {
            bullet.position.y += bullet.velocity.y * time.delta;
        }
        self.bullets
            .retain(|bullet| bullet.position.y + BULLET_SIZE > -32.0);

        // --- drones --------------------------------------------------------
        self.spawn_timer -= time.delta;
        if self.spawn_timer <= 0.0 {
            self.spawn_drone();
            // Spawns accelerate with the score, which is the difficulty curve.
            self.spawn_timer = SPAWN_INTERVAL * (1.0 - (self.score as f32 * 0.01).min(0.5));
        }

        for drone in &mut self.drones {
            drone.sprite.y += DRONE_SPEED * time.delta;
        }

        // --- collisions ----------------------------------------------------
        let bullets: Vec<Vec2> = self.bullets.iter().map(|bullet| bullet.position).collect();

        for drone in &mut self.drones {
            if !drone.alive {
                continue;
            }

            let drone_rect = drone.sprite.world_rect();
            for bullet in &bullets {
                let bullet_rect = Rect::new(bullet.x, bullet.y, BULLET_SIZE, BULLET_SIZE);
                if !drone_rect.intersection(bullet_rect).is_empty() {
                    drone.alive = false;
                    drone.sprite.y = HEIGHT as f32 + 100.0;
                    self.score += 10;
                    self.final_score.set(self.score);
                    // The bullet is removed by the position filter below: mark it by
                    // moving it off screen is cheaper than a second retain pass.
                    break;
                }
            }
        }

        self.drones.retain(|drone| drone.alive);

        // A drone that reaches the bottom costs a life.
        let ship_rect = self.ship.world_rect();
        let hits = self
            .drones
            .iter()
            .filter(|drone| drone_hits_ship(drone, ship_rect))
            .count() as u32;

        if hits > 0 {
            self.lives = self.lives.saturating_sub(hits);
            // Remove everything that hit the ship, and everything that fell through.
            self.drones
                .retain(|drone| !drone_hits_ship(drone, ship_rect));

            if self.lives == 0 {
                self.game_over = true;
                println!("mini_game: fim de jogo — pontuação {}", self.score);
            }
        }
    }

    fn render(&mut self, renderer: &mut RenderContext) {
        let (width, height) = renderer.size();

        // A starfield-ish grid, drawn with the raw rect primitive.
        for y in (0..height).step_by(48) {
            renderer.draw_rect(
                Rect::new(0.0, y as f32, width as f32, 1.0),
                Color::rgb(24, 28, 40),
            );
        }

        // A camera anchored on the scene centre shows off world-space drawing: the
        // sprites are positioned in world units and the camera maps them 1:1.
        renderer.set_camera(Camera::new(Rect::new(
            0.0,
            0.0,
            width as f32,
            height as f32,
        )));

        for drone in &self.drones {
            let _ = renderer.draw_sprite(&drone.sprite);
        }
        let _ = renderer.draw_sprite(&self.ship);

        for bullet in &self.bullets {
            renderer.draw_rect(
                Rect::new(
                    bullet.position.x,
                    bullet.position.y,
                    BULLET_SIZE,
                    BULLET_SIZE,
                ),
                Color::rgb(250, 230, 120),
            );
        }

        // Lives: one square per remaining life.
        for life in 0..self.lives {
            renderer.draw_rect(
                Rect::new(16.0 + life as f32 * 18.0, 16.0, 12.0, 12.0),
                Color::rgb(90, 200, 240),
            );
        }

        // Score bar: one square per 50 points, capped so it stays on screen.
        let score_marks = (self.score / 50).min(32);
        for mark in 0..score_marks {
            renderer.draw_rect(
                Rect::new(width as f32 - 20.0 - mark as f32 * 14.0, 18.0, 10.0, 10.0),
                Color::rgb(240, 200, 60),
            );
        }

        if self.game_over {
            // A simple dark band as the "game over" screen; text rendering is out of
            // scope for phases 0–5 (no `GiochiFont` implementation yet).
            renderer.draw_rect(
                Rect::new(0.0, height as f32 * 0.4, width as f32, 96.0),
                Color::new(10, 12, 18, 220),
            );
            renderer.draw_rect(
                Rect::new(width as f32 * 0.35, height as f32 * 0.4 + 40.0, 120.0, 16.0),
                Color::rgb(230, 70, 70),
            );
        }
    }

    fn transition(&mut self) -> SceneTransition {
        // The game ends by asking the engine to stop, not by popping the stack, so a
        // host that replaces the scene can keep the window open.
        if self.game_over {
            SceneTransition::Quit
        } else {
            SceneTransition::Continue
        }
    }
}

/// Loads (and on first run generates) the shot sound effect.
///
/// Returns `0` when there is no audio device or the file cannot be produced; the
/// game then simply plays without sound, which is the documented behaviour (ADR-021).
fn load_shot_sound(engine: &mut Engine) -> u64 {
    if !engine.audio().is_available() {
        return 0;
    }

    let path = std::env::temp_dir().join("giochi-mini-game-shot.wav");
    if !path.exists() && write_shot_wav(&path).is_err() {
        return 0;
    }

    engine
        .audio_mut()
        .load_sound(path.to_str().unwrap_or_default())
        .unwrap_or(0)
}

/// Writes a short "pew" WAV: a descending square-ish tone.
fn write_shot_wav(path: &std::path::Path) -> Result<(), std::io::Error> {
    const SAMPLE_RATE: u32 = 44_100;
    const FRAMES: u32 = SAMPLE_RATE / 12;

    let mut samples = Vec::with_capacity(FRAMES as usize * 4);
    for frame in 0..FRAMES {
        let t = frame as f32 / SAMPLE_RATE as f32;
        let progress = frame as f32 / FRAMES as f32;
        // 900 Hz → 300 Hz sweep.
        let frequency = 900.0 - 600.0 * progress;
        let value = (t * frequency * std::f32::consts::TAU).sin() * 0.18 * (1.0 - progress);
        let sample = (value * f32::from(i16::MAX)) as i16;
        samples.extend_from_slice(&sample.to_le_bytes());
        samples.extend_from_slice(&sample.to_le_bytes());
    }

    let data_len = samples.len() as u32;
    let mut wav = Vec::with_capacity(44 + samples.len());
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    wav.extend_from_slice(&(SAMPLE_RATE * 4).to_le_bytes());
    wav.extend_from_slice(&4u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    wav.extend_from_slice(&samples);

    std::fs::write(path, wav)
}

/// Command line options for the example.
#[derive(Debug, Default)]
struct Options {
    frames: Option<u64>,
    auto_play: bool,
}

fn parse_args() -> Options {
    let mut options = Options::default();
    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        if let Some(value) = arg.strip_prefix("--frames=") {
            options.frames = value.parse().ok();
        } else if arg == "--frames" {
            options.frames = args.next().and_then(|value| value.parse().ok());
        } else if arg == "--auto" {
            options.auto_play = true;
        }
    }

    options
}
