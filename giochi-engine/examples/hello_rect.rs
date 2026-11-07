//! `hello_rect` — the smallest possible 2D render loop.
//!
//! Draws three coloured rectangles over a dark background and quits on `Escape`
//! or window close.
//!
//! ```text
//! cargo run -p giochi-engine --example hello_rect
//! cargo run -p giochi-engine --example hello_rect -- --frames 60
//! ```

use std::process::ExitCode;

use giochi_engine::{quit_signal, Color, GameLoop, Platform, Rect, RenderContext, WindowConfig};

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

fn run() -> Result<(), String> {
    let frame_limit = parse_frame_limit();
    let platform = Platform::new().map_err(|error| error.to_string())?;
    let window = platform
        .create_window(WindowConfig::new(WIDTH, HEIGHT), "giochi — hello_rect")
        .map_err(|error| error.to_string())?;
    let mut render = RenderContext::new(window).map_err(|error| error.to_string())?;

    println!(
        "hello_rect: {}x{} via '{}' — ESC para sair",
        render.size().0,
        render.size().1,
        platform.video_driver_name()
    );

    let mut game_loop = GameLoop::new();
    let mut frame = 0u64;

    while game_loop.is_running() {
        for event in platform.poll_events() {
            game_loop.handle_quit_signal(quit_signal(&event));
        }

        render.clear(Color::rgb(16, 18, 28));

        let (width, height) = render.size();
        let (fw, fh) = (width as f32, height as f32);

        render.draw_rect(
            Rect::new(fw * 0.1, fh * 0.2, fw * 0.2, fh * 0.2),
            Color::rgb(230, 70, 70),
        );
        render.draw_rect(
            Rect::new(fw * 0.4, fh * 0.4, fw * 0.2, fh * 0.2),
            Color::rgb(70, 200, 120),
        );
        render.draw_rect(
            Rect::new(fw * 0.7, fh * 0.6, fw * 0.2, fh * 0.2),
            Color::rgb(80, 140, 240),
        );

        render.present();

        frame += 1;
        if frame_limit.is_some_and(|limit| frame >= limit) {
            break;
        }
        platform.delay_ms(game_loop.sleep_ms(0.0));
    }

    render.textures().clear();
    Ok(())
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
