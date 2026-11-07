//! `moving_sprite` — textured sprite bouncing inside the window, plus a camera.
//!
//! Demonstrates the handle lifecycle (`create_texture` / `destroy_texture`), the
//! texture registry's live count, world-space sprite drawing through a camera and
//! sub-rectangle `draw_texture`.
//!
//! ```text
//! cargo run -p giochi-engine --example moving_sprite
//! cargo run -p giochi-engine --example moving_sprite -- --frames 120
//! ```

use std::process::ExitCode;

use giochi_engine::{
    quit_signal, Camera, Color, FrameClock, GameLoop, Platform, Rect, RenderContext, Sprite, Time,
    WindowConfig, FIXED_TIMESTEP,
};

const WIDTH: u32 = 960;
const HEIGHT: u32 = 540;
const SPRITE_SIZE: f32 = 64.0;

fn asset(name: &str) -> String {
    format!("{}/../assets/{name}", env!("CARGO_MANIFEST_DIR"))
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
    let frame_limit = parse_frame_limit();
    let platform = Platform::new().map_err(|error| error.to_string())?;
    let window = platform
        .create_window(WindowConfig::new(WIDTH, HEIGHT), "giochi — moving_sprite")
        .map_err(|error| error.to_string())?;
    let mut render = RenderContext::new(window).map_err(|error| error.to_string())?;

    let player = render
        .create_texture(&asset("player.png"))
        .map_err(|error| error.to_string())?;
    let checker = render
        .create_texture(&asset("checker.png"))
        .map_err(|error| error.to_string())?;

    let (player_w, player_h) = render
        .texture_size(player)
        .ok_or("player texture disappeared")?;
    println!(
        "moving_sprite: player {player_w}x{player_h}, {} texturas vivas, ESC para sair",
        render.texture_count()
    );

    let (width, height) = render.size();
    let mut sprite = Sprite::new(player, 0.0, 0.0, SPRITE_SIZE, SPRITE_SIZE);
    let mut velocity = (220.0f32, 170.0f32);
    let mut time = Time::new();
    let mut clock = FrameClock::new();
    let mut game_loop = GameLoop::new();
    let mut frame = 0u64;

    while game_loop.is_running() {
        for event in platform.poll_events() {
            game_loop.handle_quit_signal(quit_signal(&event));
        }

        let work_started = std::time::Instant::now();
        let steps = game_loop.advance(clock.tick());

        for _ in 0..steps {
            time.advance(FIXED_TIMESTEP);
            step(&mut sprite, &mut velocity, time.delta, width, height);
        }

        // The camera follows the sprite and zooms in slightly, which also scales
        // the sprite's world rectangle.
        let mut camera = Camera::new(Rect::new(0.0, 0.0, width as f32, height as f32));
        camera.x = sprite.x + sprite.w * 0.5;
        camera.y = sprite.y + sprite.h * 0.5;
        camera.zoom = 1.25;
        render.set_camera(camera);

        render.clear(Color::rgb(12, 14, 22));

        // A checkerboard drawn with the raw screen-space primitive.
        for row in 0..3 {
            for column in 0..6 {
                render
                    .draw_texture(
                        checker,
                        Rect::new(
                            24.0 + column as f32 * 72.0,
                            24.0 + row as f32 * 72.0,
                            64.0,
                            64.0,
                        ),
                        Rect::ZERO,
                    )
                    .map_err(|error| error.to_string())?;
            }
        }

        // The player, drawn through the camera in world space.
        render
            .draw_sprite(&sprite)
            .map_err(|error| error.to_string())?;

        render.present();

        frame += 1;
        if frame_limit.is_some_and(|limit| frame >= limit) {
            break;
        }
        platform.delay_ms(game_loop.sleep_ms(work_started.elapsed().as_secs_f32()));
    }

    // Explicit teardown: the count must return to zero.
    let destroyed = render.destroy_texture(player) && render.destroy_texture(checker);
    println!(
        "encerrado: {} iterações, texturas destruídas: {}, restantes: {}",
        frame,
        destroyed,
        render.texture_count()
    );

    Ok(())
}

/// One fixed simulation step: move the sprite and bounce it off the window edges.
fn step(sprite: &mut Sprite, velocity: &mut (f32, f32), delta: f32, width: u32, height: u32) {
    sprite.translate(velocity.0 * delta, velocity.1 * delta);

    let max_x = width as f32 - sprite.w;
    let max_y = height as f32 - sprite.h;

    if sprite.x < 0.0 {
        sprite.x = 0.0;
        velocity.0 = velocity.0.abs();
    } else if sprite.x > max_x {
        sprite.x = max_x;
        velocity.0 = -velocity.0.abs();
    }

    if sprite.y < 0.0 {
        sprite.y = 0.0;
        velocity.1 = velocity.1.abs();
    } else if sprite.y > max_y {
        sprite.y = max_y;
        velocity.1 = -velocity.1.abs();
    }
}

/// Reads `--frames N` from the command line; used by smoke tests.
fn parse_frame_limit() -> Option<u64> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if let Some(value) = arg.strip_prefix("--frames=") {
            return value.parse().ok();
        }
        if arg == "--frames" {
            return args.next().and_then(|value| value.parse().ok());
        }
    }
    None
}
