//! GIOCHI-ENGINE demo binary.
//!
//! Phase 1: opens a 1280x720 window, runs the fixed-timestep loop and closes on
//! `Escape` or on a window-close request. Nothing is drawn yet, so the window
//! stays black; phase 2 adds the 2D renderer and replaces this loop with
//! [`giochi_engine::Engine`] in phase 5.
//!
//! Usage:
//!
//! ```text
//! cargo run -p giochi-demo                # runs until Escape / window close
//! cargo run -p giochi-demo -- --frames 60 # runs 60 frames, then exits
//! ```
//!
//! On a desktop session the engine auto-selects KMSDRM and falls back to the
//! desktop driver. Set `SDL_VIDEODRIVER=x11` (or `wayland`) to skip the fallback.

use std::process::ExitCode;

use giochi_engine::{
    quit_signal, FrameClock, GameLoop, Platform, Time, WindowConfig, FIXED_TIMESTEP,
};

const WINDOW_WIDTH: u32 = 1280;
const WINDOW_HEIGHT: u32 = 720;
const WINDOW_TITLE: &str = "GIOCHI-ENGINE";

/// Parsed command line.
#[derive(Debug, Default)]
struct Args {
    /// Stop after this many frames; `None` runs until the user quits.
    frames: Option<u64>,
}

fn main() -> ExitCode {
    env_logger::init();

    match run(parse_args(std::env::args().skip(1))) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("erro: {error}");
            ExitCode::FAILURE
        }
    }
}

fn parse_args(args: impl Iterator<Item = String>) -> Args {
    let mut parsed = Args::default();
    let mut args = args.peekable();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--frames" => {
                if let Some(value) = args.next().and_then(|v| v.parse().ok()) {
                    parsed.frames = Some(value);
                }
            }
            other => {
                if let Some(value) = other.strip_prefix("--frames=") {
                    parsed.frames = value.parse().ok();
                }
            }
        }
    }

    parsed
}

fn run(args: Args) -> Result<(), String> {
    println!("giochi-engine bootstrap OK");

    let platform = Platform::new().map_err(|error| error.to_string())?;
    log::info!(
        "video driver '{}', selection {:?}",
        platform.video_driver_name(),
        platform.video_driver_choice()
    );

    let mut window = platform
        .create_window(WindowConfig::new(WINDOW_WIDTH, WINDOW_HEIGHT), WINDOW_TITLE)
        .map_err(|error| error.to_string())?;

    let (width, height) = window.size();
    println!("janela {width}x{height} — ESC ou fechar a janela para sair");

    window
        .set_title(WINDOW_TITLE)
        .map_err(|error| error.to_string())?;

    let mut game_loop = GameLoop::new();
    let mut clock = FrameClock::new();
    let mut time = Time::new();
    let mut frame = 0u64;
    let started = std::time::Instant::now();

    while game_loop.is_running() {
        let frame_time = clock.tick();
        let work_started = std::time::Instant::now();

        for event in platform.poll_events() {
            game_loop.handle_quit_signal(quit_signal(&event));
        }

        let steps = game_loop.advance(frame_time);

        for _ in 0..steps {
            time.advance(FIXED_TIMESTEP);
            update(&time);
        }

        // Phase 2 presents the frame here; with no drawing, SDL keeps the window
        // black.
        frame += 1;
        if args.frames.is_some_and(|limit| frame >= limit) {
            break;
        }

        // `sleep_ms` takes the work done in *this* iteration, not the whole frame
        // period, so the sleep is not counted against its own budget.
        platform.delay_ms(game_loop.sleep_ms(work_started.elapsed().as_secs_f32()));
    }

    let wall = started.elapsed().as_secs_f64();
    println!(
        "encerrado: {frame} iterações, {} passos simulados, {:.1} fps reais",
        time.frame_count,
        if wall > 0.0 { frame as f64 / wall } else { 0.0 }
    );

    Ok(())
}

/// One fixed simulation step.
///
/// Phase 1 has no scene; phase 5 replaces this with `Scene::update`.
fn update(_time: &Time) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Args {
        parse_args(items.iter().map(|item| (*item).to_string()))
    }

    #[test]
    fn no_arguments_means_run_forever() {
        assert_eq!(args(&[]).frames, None);
    }

    #[test]
    fn frames_flag_with_separate_value_is_parsed() {
        assert_eq!(args(&["--frames", "30"]).frames, Some(30));
    }

    #[test]
    fn frames_flag_with_equals_is_parsed() {
        assert_eq!(args(&["--frames=45"]).frames, Some(45));
    }

    #[test]
    fn invalid_or_missing_frame_values_are_ignored() {
        assert_eq!(args(&["--frames"]).frames, None);
        assert_eq!(args(&["--frames", "abc"]).frames, None);
        assert_eq!(args(&["--frames", "0"]).frames, Some(0));
    }

    #[test]
    fn unknown_arguments_do_not_panic() {
        assert_eq!(args(&["--verbose", "x"]).frames, None);
    }
}
