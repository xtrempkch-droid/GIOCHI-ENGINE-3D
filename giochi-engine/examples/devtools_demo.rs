//! `devtools_demo` — console, debug draw and the expanded overlay in one running game.
//!
//! Cubes fall, bounce, and can be spawned from the console while the game runs:
//!
//! ```text
//! F1  expanded overlay (draw split, VRAM, entities, commands, time scale)
//! F2  console          — `help`, `spawn cube 5`, `time_scale 0.25`, `god`, `fps`
//! F3  debug draw       — grid, AABBs, velocity vectors
//! ```
//!
//! ```text
//! cargo run -p giochi-engine --example devtools_demo --features devtools
//! cargo run -p giochi-engine --example devtools_demo --features devtools -- --frames 600
//! ```
//!
//! The `spawn` command is the built-in one: it queues a [`SpawnRequest`] and this game
//! drains it. That is deliberate — the engine has no idea what a "cube" is, and a
//! built-in that invented one would be an engine that forces a game design.

use std::cell::RefCell;
use std::process::ExitCode;
use std::rc::Rc;

use giochi_engine::devtools::DevTools;
use giochi_engine::input::keyboard::KEY_ESCAPE;
use giochi_engine::{
    AudioSystem, Color, Engine, EngineConfig, Input, Rect, RenderContext, Scene, SceneTransition,
    Time, Vec2,
};

const WIDTH: u32 = 960;
const HEIGHT: u32 = 540;
/// Half-width (and depth) of a cube, in pixels. The "3D" box has a z extent so the AABB
/// path is the one being exercised rather than a 2D rectangle in disguise.
const HALF: f32 = 14.0;
const GRAVITY: f32 = 900.0;
const RESTITUTION: f32 = 0.55;
/// Cubes are capped so `spawn cube 100000` cannot make the demo allocate until it dies.
const MAX_CUBES: usize = 512;

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("erro: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let frames = parse_frames(std::env::args().skip(1))?;

    let mut engine = Engine::new(EngineConfig::new(WIDTH, HEIGHT, "giochi — devtools_demo"))
        .map_err(|error| error.to_string())?;
    engine.set_clear_color(Color::rgb(16, 18, 28));

    let state = Rc::new(RefCell::new(World::new()));
    engine.push_scene(Box::new(DemoScene {
        state: Rc::clone(&state),
    }));

    println!("F1 overlay  F2 console  F3 debug draw  ESC quit");
    println!("console: 'help', 'spawn cube 5', 'time_scale 0.25', 'god'");

    let dt = 1.0 / 60.0;
    let mut frame = 0u64;
    while engine.is_running() {
        // Drain what the console queued for the game, before the frame that draws it.
        drain_requests(&mut engine, &state);
        // The debug geometry belongs to the game: the engine does not know where the
        // cubes are. Queued before the frame, drawn by the engine inside it, cleared
        // after.
        if engine.devtools().debug_draw().is_visible() {
            draw_debug(&mut engine, &state);
        }

        engine.step(dt).map_err(|error| error.to_string())?;
        frame += 1;

        if engine.input().key_pressed(KEY_ESCAPE) && !engine.devtools().console().is_open() {
            break;
        }
        if frames.is_some_and(|limit| frame >= limit) {
            break;
        }
    }

    // `--frames` exists so the demo can be run in CI and on a console with no keyboard.
    let world = state.borrow();
    println!(
        "cubes: {}  spawns: {}  commands: {}  console lines: {}",
        world.cubes.len(),
        world.spawned,
        engine.devtools().commands().len(),
        engine.devtools().console().output_len()
    );
    let extra = engine.devtools().overlay_extra();
    println!(
        "overlay: entities {}  textures {}  vram {} B  time_scale {:.2}",
        extra.entities, extra.textures, extra.texture_vram_bytes, extra.time_scale
    );
    if engine.devtools().debug_draw().is_visible() {
        println!("debug draw still visible");
    }
    Ok(())
}

fn parse_frames(arguments: impl Iterator<Item = String>) -> Result<Option<u64>, String> {
    let mut arguments = arguments.peekable();
    let mut frames = None;
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--frames" => {
                let value = arguments
                    .next()
                    .ok_or_else(|| "--frames precisa de um valor".to_owned())?;
                let parsed: u64 = value
                    .parse()
                    .map_err(|_| format!("--frames inválido: {value}"))?;
                if parsed == 0 {
                    return Err("--frames 0 não desenha nada".to_owned());
                }
                frames = Some(parsed);
            }
            other => return Err(format!("argumento desconhecido: {other}")),
        }
    }
    Ok(frames)
}

/// Hands the console's requests to the game, and tells the console how big the world is.
fn drain_requests(engine: &mut Engine, state: &Rc<RefCell<World>>) {
    let devtools: &mut DevTools = engine.devtools_mut();
    while let Some(request) = devtools.take_spawn() {
        state.borrow_mut().spawn(request.kind(), request.count());
    }
    devtools.set_entity_count(state.borrow().cubes.len() as u32);
}

/// Queues this frame's debug geometry: a grid, one AABB per cube, and its velocity.
fn draw_debug(engine: &mut Engine, state: &Rc<RefCell<World>>) {
    let matrix = ortho();
    let world = state.borrow();
    let debug = engine.devtools_mut().debug_draw_mut();

    debug.draw_grid_2d(
        Rect::new(0.0, 0.0, WIDTH as f32, HEIGHT as f32),
        48.0,
        Color::rgb(38, 42, 58),
    );
    for cube in &world.cubes {
        debug.draw_aabb_3d(
            [cube.position.x - HALF, cube.position.y - HALF, -HALF],
            [cube.position.x + HALF, cube.position.y + HALF, HALF],
            &matrix,
            Color::rgb(0, 220, 160),
        );
        debug.draw_ray_3d(
            [cube.position.x, cube.position.y, 0.0],
            [cube.velocity.x / 300.0, cube.velocity.y / 300.0, 0.0],
            40.0,
            &matrix,
            Color::rgb(255, 200, 60),
        );
    }
}

struct Cube {
    position: Vec2,
    velocity: Vec2,
    color: Color,
}

/// The game's state, shared with the scene and the demo loop.
struct World {
    cubes: Vec<Cube>,
    spawned: u64,
    seed: u32,
}

impl World {
    fn new() -> Self {
        let mut world = Self {
            cubes: Vec::new(),
            spawned: 0,
            seed: 0x2545_F491,
        };
        world.spawn("cube", 6);
        world
    }

    /// Adds `count` cubes, dropping the oldest beyond [`MAX_CUBES`].
    fn spawn(&mut self, _kind: &str, count: u32) {
        for _ in 0..count {
            // Draw the random numbers before touching `self.cubes`: a field borrow of
            // `self` and a call to `self.next_unit()` cannot overlap.
            let x = 80.0 + self.next_unit() * (WIDTH as f32 - 160.0);
            let y = 40.0 + self.next_unit() * 60.0;
            let vx = (self.next_unit() - 0.5) * 260.0;
            let vy = self.next_unit() * 120.0;
            let red = 90 + (self.next_unit() * 150.0) as u8;
            let green = 90 + (self.next_unit() * 150.0) as u8;
            let blue = 120 + (self.next_unit() * 130.0) as u8;
            self.cubes.push(Cube {
                position: Vec2::new(x, y),
                velocity: Vec2::new(vx, vy),
                color: Color::rgb(red, green, blue),
            });
            self.spawned += 1;
        }
        if self.cubes.len() > MAX_CUBES {
            let excess = self.cubes.len() - MAX_CUBES;
            self.cubes.drain(..excess);
        }
    }

    /// xorshift: a deterministic generator, so the demo behaves the same every run.
    fn next_unit(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        (self.seed % 10_000) as f32 / 10_000.0
    }

    fn step(&mut self, dt: f32) {
        let floor = HEIGHT as f32 - HALF;
        for cube in &mut self.cubes {
            cube.velocity.y += GRAVITY * dt;
            cube.position = cube.position + cube.velocity * dt;

            if cube.position.y > floor {
                cube.position.y = floor;
                cube.velocity.y = -cube.velocity.y * RESTITUTION;
                // Kill the bounce once it is too small to see, so cubes come to rest.
                if cube.velocity.y.abs() < 30.0 {
                    cube.velocity.y = 0.0;
                }
            }
            if cube.position.x < HALF {
                cube.position.x = HALF;
                cube.velocity.x = -cube.velocity.x * RESTITUTION;
            } else if cube.position.x > WIDTH as f32 - HALF {
                cube.position.x = WIDTH as f32 - HALF;
                cube.velocity.x = -cube.velocity.x * RESTITUTION;
            }
        }
    }
}

struct DemoScene {
    state: Rc<RefCell<World>>,
}

impl Scene for DemoScene {
    fn update(&mut self, time: Time, _input: &Input, _audio: &mut AudioSystem) {
        self.state.borrow_mut().step(time.delta);
    }

    fn render(&mut self, renderer: &mut RenderContext) {
        let world = self.state.borrow();
        for cube in &world.cubes {
            let rect = Rect::new(
                cube.position.x - HALF,
                cube.position.y - HALF,
                HALF * 2.0,
                HALF * 2.0,
            );
            renderer.draw_rect(rect, cube.color);
            renderer.stroke_rect(rect, Color::rgb(240, 240, 255));
        }
    }

    fn transition(&mut self) -> SceneTransition {
        // The demo loop drives the engine with `step`, so this is only consulted by
        // `run_current`; returning Continue keeps both drivers valid.
        SceneTransition::Continue
    }
}

/// An orthographic view-projection that maps world pixels to screen pixels.
///
/// Column-major, the layout `draw_aabb_3d` expects: `x` and `y` map 1:1, `z` is ignored.
/// A 3D call with a 2D camera is exactly what a physics debug view of a side-scroller is,
/// and it keeps the demo on the code path the phase added.
fn ortho() -> [f32; 16] {
    let mut m = [0.0f32; 16];
    m[0] = 2.0 / WIDTH as f32;
    m[5] = -2.0 / HEIGHT as f32;
    m[10] = 1.0;
    m[12] = -1.0;
    m[13] = 1.0;
    m[15] = 1.0;
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ortho_projection_maps_world_pixels_to_screen_pixels() {
        let m = ortho();
        // `draw_aabb_3d` projects through `project`; recompute it here to keep the demo's
        // matrix honest without exporting the helper.
        let project = |x: f32, y: f32| {
            let clip_x = m[0] * x + m[4] * y + m[8] * 0.0 + m[12];
            let clip_y = m[1] * x + m[5] * y + m[9] * 0.0 + m[13];
            let clip_w = m[3] * x + m[7] * y + m[11] * 0.0 + m[15];
            let ndc = (clip_x / clip_w, clip_y / clip_w);
            (
                (ndc.0 * 0.5 + 0.5) * WIDTH as f32,
                (0.5 - ndc.1 * 0.5) * HEIGHT as f32,
            )
        };
        let (x, y) = project(0.0, 0.0);
        assert!(x.abs() < 0.001 && y.abs() < 0.001, "origin -> {x},{y}");
        let (x, y) = project(WIDTH as f32, HEIGHT as f32);
        assert!(
            (x - WIDTH as f32).abs() < 0.001 && (y - HEIGHT as f32).abs() < 0.001,
            "far corner -> {x},{y}"
        );
    }

    #[test]
    fn spawn_is_capped_so_a_console_command_cannot_exhaust_memory() {
        let mut world = World::new();
        world.spawn("cube", 100_000);
        assert_eq!(world.cubes.len(), MAX_CUBES);
    }

    #[test]
    fn the_generator_is_deterministic_and_in_range() {
        let mut a = World::new();
        let mut b = World::new();
        for _ in 0..100 {
            let value = a.next_unit();
            assert!((0.0..1.0).contains(&value));
            assert_eq!(value, b.next_unit());
        }
    }

    #[test]
    fn a_cube_comes_to_rest_on_the_floor() {
        let mut world = World::new();
        world.cubes.clear();
        world.cubes.push(Cube {
            position: Vec2::new(WIDTH as f32 * 0.5, 10.0),
            velocity: Vec2::new(0.0, 0.0),
            color: Color::rgb(255, 255, 255),
        });
        for _ in 0..600 {
            world.step(1.0 / 60.0);
        }
        let cube = &world.cubes[0];
        assert!(
            (cube.position.y - (HEIGHT as f32 - HALF)).abs() < 0.5,
            "cube should rest on the floor, y = {}",
            cube.position.y
        );
    }

    #[test]
    fn frames_argument_is_validated() {
        assert_eq!(
            parse_frames(["--frames".to_owned(), "5".to_owned()].into_iter()),
            Ok(Some(5))
        );
        assert!(parse_frames(["--frames".to_owned()].into_iter()).is_err());
        assert!(parse_frames(["--frames".to_owned(), "0".to_owned()].into_iter()).is_err());
        assert!(parse_frames(["--nope".to_owned()].into_iter()).is_err());
    }
}
