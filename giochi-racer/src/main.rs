//! GIOCHI RACER — the game loop.
//!
//! Usage:
//!
//! ```text
//! cargo run -p giochi-racer --release
//! cargo run -p giochi-racer --release -- --track giochi-racer/assets/tracks/track_easy.scene
//! cargo run -p giochi-racer -- --frames 120 --auto --quadro /tmp/race.bmp
//! ```
//!
//! `--frames` and `--auto` are what make the game checkable without a human: the gates run
//! a scripted driver for a few seconds, read the framebuffer and assert the picture is not
//! the clear colour. Everything the flags touch is the code the player runs.

use std::process::ExitCode;
use std::time::{Duration, Instant};

use giochi_engine::input::keyboard::{KEY_Q, KEY_R};
use giochi_engine::input::Input;
use giochi_engine::platform::sdl::{quit_signal, QuitSignal};
use giochi_engine::platform::window::{Window, WindowConfig};
use giochi_engine::platform::Platform;
use giochi_engine_3d::context::{self, GlesVersion};
use giochi_engine_3d::RenderContext3D;

use giochi_racer::config::GameConfig;
use giochi_racer::game::RaceGame;
use giochi_racer::screenshot;
use giochi_racer::vehicle::input::{read_input, scripted_input};

/// One simulation step, in seconds. The physics world, the car model and the camera all
/// integrate with this, so a frame is exactly 1/60 s of game time. Phase 45 replaces this
/// with a frame limiter and a measured delta.
const STEP: f32 = 1.0 / 60.0;

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(Error::Usage(message)) => {
            eprintln!("giochi-racer: {message}\n\nTry `giochi-racer --help`.");
            ExitCode::from(2)
        }
        Err(Error::Runtime(error)) => {
            eprintln!("giochi-racer: {error:#}");
            ExitCode::FAILURE
        }
    }
}

/// Why the game stopped before it started, or while it was running.
#[derive(Debug)]
enum Error {
    /// The command line could not be understood.
    Usage(String),
    /// The window or the game failed.
    Runtime(anyhow::Error),
}

impl From<anyhow::Error> for Error {
    fn from(error: anyhow::Error) -> Self {
        Self::Runtime(error)
    }
}

impl From<giochi_engine::GiochiError> for Error {
    fn from(error: giochi_engine::GiochiError) -> Self {
        Self::Runtime(error.into())
    }
}

impl From<giochi_engine_3d::Error3D> for Error {
    fn from(error: giochi_engine_3d::Error3D) -> Self {
        Self::Runtime(error.into())
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Runtime(error.into())
    }
}

/// What the command line asked for.
#[derive(Debug, Clone, PartialEq)]
struct Options {
    config: GameConfig,
    /// Stop after this many frames.
    frames: Option<u64>,
    /// Drive with the scripted input instead of the keyboard or a gamepad.
    auto: bool,
    /// Write the last frame here.
    quadro: Option<String>,
    /// Window size, in physical pixels.
    size: (u32, u32),
}

impl Default for Options {
    fn default() -> Self {
        Self {
            config: GameConfig::default(),
            frames: None,
            auto: false,
            quadro: None,
            size: (1280, 720),
        }
    }
}

const USAGE: &str = "\
giochi-racer — an arcade racer built with the GIOCHI editor

USAGE:
    giochi-racer [OPTIONS]

OPTIONS:
    --track PATH     Race on this .scene file (default: assets/tracks/track_easy.scene)
    --laps N         Laps in the race (default: 3)
    --size WxH       Window size in pixels (default: 1280x720)
    --frames N       Stop after N frames, for a headless check
    --auto           Drive with the scripted input instead of the keyboard/gamepad
    --quadro PATH    Write the last frame to PATH as a BMP
    --version        Print the version and exit
    -h, --help       Print this help and exit

CONTROLS:
    Gamepad  RT accelerate, LT brake/reverse, left stick steer, B handbrake, Start pause
    Keyboard W/S accelerate/brake, A/D steer, Space handbrake, R respawn, Esc quit
";

impl Options {
    /// Parses the process arguments.
    fn parse() -> Result<Option<Self>, Error> {
        Self::parse_args(std::env::args().skip(1))
    }

    /// Parses `arguments`, or returns `None` when the caller asked for help or a version.
    fn parse_args(arguments: impl Iterator<Item = String>) -> Result<Option<Self>, Error> {
        let mut options = Self::default();
        let mut args = arguments;
        while let Some(argument) = args.next() {
            match argument.as_str() {
                "-h" | "--help" => {
                    print!("{USAGE}");
                    return Ok(None);
                }
                "--version" => {
                    println!("giochi-racer {}", giochi_racer::VERSION);
                    return Ok(None);
                }
                "--auto" => options.auto = true,
                "--track" => {
                    options.config = options
                        .config
                        .clone()
                        .with_track(next(&mut args, "--track")?);
                }
                "--laps" => {
                    let value: u32 = parse(next(&mut args, "--laps")?, "--laps")?;
                    if value == 0 {
                        return Err(Error::Usage("--laps must be at least 1".to_owned()));
                    }
                    options.config.laps = value;
                }
                "--frames" => {
                    let value: u64 = parse(next(&mut args, "--frames")?, "--frames")?;
                    if value == 0 {
                        return Err(Error::Usage("--frames must be at least 1".to_owned()));
                    }
                    options.frames = Some(value);
                }
                "--quadro" => options.quadro = Some(next(&mut args, "--quadro")?),
                "--size" => options.size = parse_size(&next(&mut args, "--size")?)?,
                other => {
                    return Err(Error::Usage(format!(
                        "unknown argument '{other}'; --help lists the ones it accepts"
                    )))
                }
            }
        }
        Ok(Some(options))
    }
}

fn next(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, Error> {
    args.next()
        .ok_or_else(|| Error::Usage(format!("{flag} needs a value")))
}

fn parse<T: std::str::FromStr>(value: String, flag: &str) -> Result<T, Error> {
    value
        .parse()
        .map_err(|_| Error::Usage(format!("{flag} expects a number, got '{value}'")))
}

fn parse_size(value: &str) -> Result<(u32, u32), Error> {
    let (width, height) = value
        .split_once(['x', 'X'])
        .ok_or_else(|| Error::Usage(format!("--size expects WxH, got '{value}'")))?;
    let width = parse(width.to_owned(), "--size")?;
    let height = parse(height.to_owned(), "--size")?;
    if width == 0 || height == 0 {
        return Err(Error::Usage("--size must be positive".to_owned()));
    }
    Ok((width, height))
}

fn run() -> Result<(), Error> {
    let Some(options) = Options::parse()? else {
        return Ok(());
    };

    let platform = Platform::new()?;
    log::info!("video driver: {}", platform.video_driver_name());
    let requested = GlesVersion::from_env();
    context::configure_attributes(&platform, requested)?;
    let window = Window::new_opengl(
        &platform,
        WindowConfig::new(options.size.0, options.size.1),
        &format!("GIOCHI RACER {}", giochi_racer::VERSION),
    )?;
    let mut render = RenderContext3D::new(context::create(&window)?);
    log::info!(
        "GLES {} on {}",
        render.gles_version().major(),
        render.adapter()
    );

    let mut input = Input::new(&platform);
    let mut game = RaceGame::new(&mut render, options.config)?;

    let mut frames: u64 = 0;
    let mut elapsed = 0.0f32;
    let started = Instant::now();
    let mut fps_frames = 0u32;
    let mut fps_timer = 0.0f32;

    loop {
        let frame_started = Instant::now();
        let events = platform.poll_events();
        let mut window_closed = false;
        for event in &events {
            if quit_signal(event) == QuitSignal::WindowClose {
                window_closed = true;
            }
        }
        input.update(&events);
        if window_closed || input.key_pressed(KEY_Q) {
            break;
        }

        let car_input = if options.auto {
            scripted_input(elapsed)
        } else {
            read_input(&input)
        };
        // Phase 41 has no pause menu, so pause *is* quit; phase 44 turns this into the
        // overlay and keeps the flag.
        if car_input.pause {
            break;
        }
        if input.key_pressed(KEY_R) {
            game.respawn_car();
        }

        game.step(car_input, STEP);
        elapsed += STEP;

        let (width, height) = render.drawable_size();
        render.begin_frame(&window);
        game.render(&mut render, (width, height), STEP)?;

        let last_frame = options.frames.is_some_and(|limit| frames + 1 >= limit);
        if last_frame {
            if let Some(path) = options.quadro.as_deref() {
                capture(&mut render, &window, path)?;
            }
        }
        render.end_frame(&window);

        let gl_error = render.take_gl_error();
        if gl_error != 0 {
            return Err(Error::Runtime(anyhow::anyhow!(
                "GL error 0x{gl_error:04x} on frame {frames}"
            )));
        }

        frames += 1;
        fps_frames += 1;
        fps_timer += STEP;
        if fps_timer >= 1.0 {
            log::debug!(
                "{fps_frames} fps, car at {:.1}, {:.1}, {:.1} at {:.0} km/h",
                game.car_state().position.to_glam().x,
                game.car_state().position.to_glam().y,
                game.car_state().position.to_glam().z,
                game.car_state().speed_kmh
            );
            fps_frames = 0;
            fps_timer = 0.0;
        }

        if let Some(limit) = options.frames {
            if frames >= limit {
                break;
            }
        } else {
            // Without a frame limit, pace the loop to 60 Hz instead of spinning a core at
            // two thousand frames a second. Phase 45 replaces this with the real limiter.
            let budget = Duration::from_secs_f32(STEP);
            if let Some(rest) = budget.checked_sub(frame_started.elapsed()) {
                std::thread::sleep(rest);
            }
        }
    }

    let wall = started.elapsed().as_secs_f32().max(f32::EPSILON);
    log::info!(
        "{} frame(s) in {:.2}s ({:.1} fps), {:.3}s of race time",
        frames,
        wall,
        frames as f32 / wall,
        elapsed
    );
    game.release(&mut render);
    Ok(())
}

/// Reads the last frame and writes it as a BMP.
fn capture(render: &mut RenderContext3D, window: &Window, path: &str) -> Result<(), Error> {
    let (width, height) = window.drawable_size();
    let pixels = render.read_pixels(width, height)?;
    screenshot::write_bmp(path, width, height, &pixels)?;
    log::info!("quadro: {path} ({width}x{height})");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_from(arguments: &[&str]) -> Result<Option<Options>, Error> {
        Options::parse_args(arguments.iter().map(|value| value.to_string()))
    }

    #[test]
    fn the_defaults_race_three_laps_on_the_easy_track() {
        let options = parse_from(&[]).expect("no arguments").expect("options");
        assert_eq!(options.config.laps, 3);
        assert!(!options.auto);
        assert_eq!(options.frames, None);
        assert_eq!(options.quadro, None);
        assert_eq!(options.size, (1280, 720));
    }

    #[test]
    fn every_flag_is_understood() {
        let options = parse_from(&[
            "--track",
            "elsewhere/hard.scene",
            "--laps",
            "5",
            "--frames",
            "90",
            "--quadro",
            "/tmp/a.bmp",
            "--size",
            "800x480",
            "--auto",
        ])
        .expect("valid arguments")
        .expect("options");
        assert_eq!(options.config.track.to_str(), Some("elsewhere/hard.scene"));
        assert_eq!(options.config.laps, 5);
        assert_eq!(options.frames, Some(90));
        assert_eq!(options.quadro.as_deref(), Some("/tmp/a.bmp"));
        assert_eq!(options.size, (800, 480));
        assert!(options.auto);
    }

    #[test]
    fn help_and_version_stop_before_opening_a_window() {
        assert!(parse_from(&["--help"]).expect("help").is_none());
        assert!(parse_from(&["-h"]).expect("help").is_none());
        assert!(parse_from(&["--version"]).expect("version").is_none());
    }

    #[test]
    fn a_missing_value_names_the_flag() {
        let error = parse_from(&["--track"]).expect_err("no value");
        assert!(matches!(error, Error::Usage(message) if message.contains("--track")));
    }

    #[test]
    fn a_non_numeric_value_names_the_flag() {
        let error = parse_from(&["--frames", "lots"]).expect_err("not a number");
        assert!(matches!(error, Error::Usage(message) if message.contains("--frames")));
    }

    #[test]
    fn zero_frames_or_zero_laps_are_refused_instead_of_producing_a_dead_run() {
        assert!(parse_from(&["--frames", "0"]).is_err());
        assert!(parse_from(&["--laps", "0"]).is_err());
    }

    #[test]
    fn an_unknown_flag_is_an_error_not_a_silent_default() {
        let error = parse_from(&["--turbo"]).expect_err("unknown");
        assert!(matches!(error, Error::Usage(message) if message.contains("--turbo")));
    }

    #[test]
    fn a_size_must_be_width_by_height() {
        assert_eq!(parse_size("640x360").unwrap(), (640, 360));
        assert_eq!(parse_size("640X360").unwrap(), (640, 360));
        assert!(parse_size("640").is_err());
        assert!(parse_size("0x360").is_err());
        assert!(parse_size("640x").is_err());
    }
}
