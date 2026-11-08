//! `input_test` — keyboard, mouse and gamepad input in one window.
//!
//! Controls:
//!
//! * **WASD** or **arrow keys** — move the square;
//! * **Space** — cycle the square's colour;
//! * **right mouse button** — the square follows the cursor;
//! * **left mouse button** — draw click markers;
//! * **mouse wheel** — scale the square;
//! * **gamepad** left stick / D-pad — move the square.
//!
//! ```text
//! cargo run -p giochi-engine --example input_test
//! cargo run -p giochi-engine --example input_test -- --frames 120
//! ```

use std::process::ExitCode;

use giochi_engine::input::gamepad::{
    AXIS_LEFT_X, AXIS_LEFT_Y, BUTTON_DPAD_DOWN, BUTTON_DPAD_LEFT, BUTTON_DPAD_RIGHT, BUTTON_DPAD_UP,
};
use giochi_engine::input::keyboard::{
    KEY_A, KEY_D, KEY_DOWN, KEY_LEFT, KEY_RIGHT, KEY_S, KEY_SPACE, KEY_UP, KEY_W,
};
use giochi_engine::input::mouse::MOUSE_RIGHT;
use giochi_engine::{
    quit_signal, Color, FrameClock, GameLoop, Input, Platform, Rect, RenderContext, Time,
    WindowConfig, FIXED_TIMESTEP,
};

const WIDTH: u32 = 960;
const HEIGHT: u32 = 540;
const SPEED: f32 = 260.0;
const MIN_SIZE: f32 = 16.0;
const MAX_SIZE: f32 = 160.0;
const MAX_MARKERS: usize = 32;

const PALETTE: [Color; 5] = [
    Color::rgb(230, 70, 70),
    Color::rgb(70, 200, 120),
    Color::rgb(80, 140, 240),
    Color::rgb(240, 200, 60),
    Color::rgb(200, 90, 230),
];

/// A click marker with the frame it was created in, so it can fade out.
#[derive(Debug, Clone, Copy)]
struct Marker {
    position: (f32, f32),
    age: f32,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("erro: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let frame_limit =
        parse_frame_limit().map_err(|error| format!("argumentos inválidos: {error}"))?;

    let platform = Platform::new().map_err(|error| error.to_string())?;
    let window = platform
        .create_window(WindowConfig::new(WIDTH, HEIGHT), "giochi — input_test")
        .map_err(|error| error.to_string())?;
    let mut render = RenderContext::new(window).map_err(|error| error.to_string())?;
    let mut input = Input::new(&platform);

    println!(
        "input_test: WASD/setas move, ESPACO troca a cor, botao direito segue o mouse, \
         roda escala — ESC para sair"
    );

    let mut game_loop = GameLoop::new();
    let mut clock = FrameClock::new();
    let mut time = Time::new();
    let mut frame = 0u64;

    let mut position = ((WIDTH as f32 - 64.0) * 0.5, (HEIGHT as f32 - 64.0) * 0.5);
    let mut size = 64.0f32;
    let mut color_index = 0usize;
    let mut markers: Vec<Marker> = Vec::new();

    while game_loop.is_running() {
        let frame_started = std::time::Instant::now();
        let events = platform.poll_events();
        // Input must be updated before the simulation sees it.
        input.update(&events);
        for event in &events {
            game_loop.handle_quit_signal(quit_signal(event));
        }

        // One-frame edges are read here, in the frame they belong to.
        if input.key_pressed(KEY_SPACE) {
            color_index = (color_index + 1) % PALETTE.len();
        }
        if input
            .mouse()
            .is_pressed_this_frame(giochi_engine::input::mouse::MOUSE_LEFT)
        {
            let (x, y) = input.mouse().position();
            if markers.len() == MAX_MARKERS {
                markers.remove(0);
            }
            markers.push(Marker {
                position: (x, y),
                age: 0.0,
            });
        }
        let wheel = input.mouse().wheel_delta();
        if wheel != 0.0 {
            size = (size + wheel * 8.0).clamp(MIN_SIZE, MAX_SIZE);
        }

        let steps = game_loop.advance(clock.tick());
        for _ in 0..steps {
            time.advance(FIXED_TIMESTEP);
            update(&input, &mut position, size, time.delta);
            for marker in &mut markers {
                marker.age += time.delta;
            }
            markers.retain(|marker| marker.age < 1.5);
        }

        draw(&mut render, position, size, PALETTE[color_index], &markers);

        frame += 1;
        if frame_limit.is_some_and(|limit| frame >= limit) {
            break;
        }
        platform.delay_ms(game_loop.sleep_ms(frame_started.elapsed().as_secs_f32()));
    }

    println!(
        "encerrado: {frame} iterações, {} passos simulados, gamepad: {}",
        time.frame_count,
        input.gamepad().name().unwrap_or("nenhum")
    );

    Ok(())
}

/// One fixed step: keyboard, gamepad and right-button dragging move the square.
fn update(input: &Input, position: &mut (f32, f32), size: f32, delta: f32) {
    let keyboard = input.keyboard();
    let mut dx = 0.0;
    let mut dy = 0.0;

    if keyboard.is_down(KEY_A) || keyboard.is_down(KEY_LEFT) {
        dx -= 1.0;
    }
    if keyboard.is_down(KEY_D) || keyboard.is_down(KEY_RIGHT) {
        dx += 1.0;
    }
    if keyboard.is_down(KEY_W) || keyboard.is_down(KEY_UP) {
        dy -= 1.0;
    }
    if keyboard.is_down(KEY_S) || keyboard.is_down(KEY_DOWN) {
        dy += 1.0;
    }

    let gamepad = input.gamepad();
    if gamepad.is_connected() {
        // Sticks are analog: use the raw axis value, which already includes the
        // mapping's dead zone.
        dx += gamepad.axis(AXIS_LEFT_X);
        dy += gamepad.axis(AXIS_LEFT_Y);

        if gamepad.is_down(BUTTON_DPAD_LEFT) {
            dx -= 1.0;
        }
        if gamepad.is_down(BUTTON_DPAD_RIGHT) {
            dx += 1.0;
        }
        if gamepad.is_down(BUTTON_DPAD_UP) {
            dy -= 1.0;
        }
        if gamepad.is_down(BUTTON_DPAD_DOWN) {
            dy += 1.0;
        }
    }

    if input.mouse().is_down(MOUSE_RIGHT) {
        // Follow the cursor, centred on it.
        let (mouse_x, mouse_y) = input.mouse().position();
        position.0 = mouse_x - size * 0.5;
        position.1 = mouse_y - size * 0.5;
    } else {
        let length = dx.hypot(dy);
        if length > 1.0 {
            dx /= length;
            dy /= length;
        }
        position.0 += dx * SPEED * delta;
        position.1 += dy * SPEED * delta;
    }

    let max_x = WIDTH as f32 - size;
    let max_y = HEIGHT as f32 - size;
    position.0 = position.0.clamp(0.0, max_x.max(0.0));
    position.1 = position.1.clamp(0.0, max_y.max(0.0));
}

fn draw(
    render: &mut RenderContext,
    position: (f32, f32),
    size: f32,
    color: Color,
    markers: &[Marker],
) {
    render.clear(Color::rgb(14, 16, 24));

    // A faint grid makes movement visible.
    let (width, height) = render.size();
    let grid = Color::rgb(30, 34, 46);
    for x in (0..width).step_by(64) {
        render.draw_rect(Rect::new(x as f32, 0.0, 1.0, height as f32), grid);
    }
    for y in (0..height).step_by(64) {
        render.draw_rect(Rect::new(0.0, y as f32, width as f32, 1.0), grid);
    }

    for marker in markers {
        // Fade out over 1.5 s.
        let alpha = ((1.5 - marker.age) / 1.5).clamp(0.0, 1.0);
        let channel = (alpha * 160.0) as u8;
        render.draw_rect(
            Rect::new(marker.position.0 - 6.0, marker.position.1 - 6.0, 12.0, 12.0),
            Color::new(channel, channel, channel, 255),
        );
    }

    render.draw_rect(Rect::new(position.0, position.1, size, size), color);
    render.present();
}

/// Reads `--frames N` from the command line; used by smoke tests.
///
/// A typo such as `--frames=3oo` is an error rather than "no limit": an example that runs
/// with no frame limit never terminates, and a smoke test driven by a script would hang
/// instead of reporting the typo.
fn parse_frame_limit() -> Result<Option<u64>, String> {
    let mut args = std::env::args().skip(1);
    let Some(arg) = args.next() else {
        return Ok(None);
    };
    if let Some(value) = arg.strip_prefix("--frames=") {
        return value
            .parse()
            .map(Some)
            .map_err(|_| format!("--frames expects a number, got '{value}'"));
    }
    if arg == "--frames" {
        let value = args
            .next()
            .ok_or_else(|| "--frames needs a value".to_string())?;
        return value
            .parse()
            .map(Some)
            .map_err(|_| format!("--frames expects a number, got '{value}'"));
    }
    Err(format!("unknown argument '{arg}'; accepted: --frames N"))
}
