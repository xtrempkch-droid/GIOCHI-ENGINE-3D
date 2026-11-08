//! `audio_test` — procedurally generated sound effect plus mixer controls.
//!
//! There is no binary asset: a short WAV is synthesised at start-up, written to a
//! temporary file and loaded through the normal `load_sound` path, so the example
//! exercises exactly the code a game uses without shipping audio files.
//!
//! Controls:
//!
//! * **Space** — play the beep;
//! * **Up / Down** — master volume ±8;
//! * **M** — mute / unmute;
//! * **Escape** — quit.
//!
//! ```text
//! cargo run -p giochi-engine --example audio_test
//! cargo run -p giochi-engine --example audio_test -- --beep --frames 120
//! ```
//!
//! `--beep` plays the sound once at start-up, which makes the example verifiable
//! from a script without a human pressing `Space`.

use std::path::PathBuf;
use std::process::ExitCode;

use giochi_engine::audio::{AudioSystem, MAX_VOLUME};
use giochi_engine::input::keyboard::{KEY_DOWN, KEY_M, KEY_SPACE, KEY_UP};
use giochi_engine::{
    quit_signal, Color, GameLoop, Input, Platform, Rect, RenderContext, WindowConfig,
};

const WIDTH: u32 = 640;
const HEIGHT: u32 = 360;
const BEEP_SECONDS: f32 = 0.35;
const BEEP_FREQUENCY: f32 = 660.0;

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
    let options = parse_args().map_err(|error| format!("argumentos inválidos: {error}"))?;
    let frame_limit = options.frames;
    let platform = Platform::new().map_err(|error| error.to_string())?;

    let window = platform
        .create_window(WindowConfig::new(WIDTH, HEIGHT), "giochi — audio_test")
        .map_err(|error| error.to_string())?;
    let mut render = RenderContext::new(window).map_err(|error| error.to_string())?;
    let mut input = Input::new(&platform);

    let wav_path = write_beep_wav(BEEP_FREQUENCY, BEEP_SECONDS)
        .map_err(|error| format!("falha ao gerar o WAV: {error}"))?;
    let mut audio = AudioSystem::new(&platform).map_err(|error| error.to_string())?;

    let mut muted = false;
    let mut sound = 0u64;
    let mut played = 0u32;

    if audio.is_available() {
        sound = audio
            .load_sound(wav_path.to_str().ok_or("caminho WAV inválido")?)
            .map_err(|error| error.to_string())?;
        println!("audio_test: ESPACO toca o bipe, setas ajustam o volume, M silencia — ESC sai");
    } else {
        println!(
            "audio_test: SEM dispositivo de áudio (modo silencioso). \
             ESPACO reporta o erro em vez de falhar — ESC sai"
        );
    }

    if options.beep {
        match audio.play_sound(sound, 0) {
            Ok(()) => {
                played += 1;
                // Give the mixer a moment so the process does not exit mid-sample.
                platform.delay_ms((BEEP_SECONDS * 1000.0) as u32 + 100);
                println!("bipe #1 (via --beep)");
            }
            Err(error) => println!("sem áudio: {error}"),
        }
    }

    let mut game_loop = GameLoop::new();
    let mut frame = 0u64;

    while game_loop.is_running() {
        let frame_started = std::time::Instant::now();
        let events = platform.poll_events();
        input.update(&events);
        for event in &events {
            game_loop.handle_quit_signal(quit_signal(event));
        }

        if input.key_pressed(KEY_SPACE) {
            match audio.play_sound(sound, 0) {
                Ok(()) => {
                    played += 1;
                    println!("bipe #{played}");
                }
                Err(error) => println!("sem áudio: {error}"),
            }
        }

        if input.key_pressed(KEY_UP) {
            audio.set_volume(audio.volume() + 8);
        }
        if input.key_pressed(KEY_DOWN) {
            audio.set_volume(audio.volume() - 8);
        }
        if input.key_pressed(KEY_M) {
            muted = !muted;
            audio.set_volume(if muted { 0 } else { MAX_VOLUME });
        }

        draw(&mut render, audio.volume(), muted, played);

        frame += 1;
        if frame_limit.is_some_and(|limit| frame >= limit) {
            break;
        }
        platform.delay_ms(game_loop.sleep_ms(frame_started.elapsed().as_secs_f32()));
    }

    println!(
        "encerrado: {frame} iterações, {played} bipes, volume {}, modo silencioso: {}",
        audio.volume(),
        !audio.is_available()
    );

    audio.clear();
    let _ = std::fs::remove_file(&wav_path);
    Ok(())
}

fn draw(render: &mut RenderContext, volume: i32, muted: bool, played: u32) {
    render.clear(Color::rgb(16, 20, 30));

    let (width, height) = render.size();
    let bar_height = 24.0;
    let bar_y = height as f32 * 0.5;
    let fraction = f32::from(volume as i16) / f32::from(MAX_VOLUME as i16);

    // Volume bar.
    render.draw_rect(
        Rect::new(24.0, bar_y, (width as f32 - 48.0) * fraction, bar_height),
        if muted {
            Color::rgb(90, 90, 90)
        } else {
            Color::rgb(70, 200, 120)
        },
    );

    // One tick per beep, wrapping.
    for index in 0..played.min(32) {
        render.draw_rect(
            Rect::new(
                24.0 + index as f32 * 18.0,
                bar_y + bar_height + 24.0,
                12.0,
                12.0,
            ),
            Color::rgb(80, 140, 240),
        );
    }
}

/// Synthesises a 16-bit stereo WAV with a sine tone and short fade in/out.
fn write_beep_wav(frequency: f32, seconds: f32) -> std::io::Result<PathBuf> {
    const SAMPLE_RATE: u32 = 44_100;
    const CHANNELS: u16 = 2;
    const BITS: u16 = 16;

    let frames = (SAMPLE_RATE as f32 * seconds) as u32;
    let mut samples: Vec<u8> = Vec::with_capacity(frames as usize * 4);

    for frame in 0..frames {
        let t = frame as f32 / SAMPLE_RATE as f32;
        // 20 ms fade in/out removes the click at both ends.
        let fade = 0.02;
        let envelope = if t < fade {
            t / fade
        } else if t > seconds - fade {
            (seconds - t) / fade
        } else {
            1.0
        };
        let value = (t * frequency * std::f32::consts::TAU).sin() * envelope * 0.25;
        let sample = (value * f32::from(i16::MAX)) as i16;
        // Same sample on both channels.
        samples.extend_from_slice(&sample.to_le_bytes());
        samples.extend_from_slice(&sample.to_le_bytes());
    }

    let data_len = samples.len() as u32;
    let byte_rate = SAMPLE_RATE * u32::from(CHANNELS) * u32::from(BITS) / 8;
    let block_align = CHANNELS * BITS / 8;

    let mut wav: Vec<u8> = Vec::with_capacity(44 + samples.len());
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&CHANNELS.to_le_bytes());
    wav.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&block_align.to_le_bytes());
    wav.extend_from_slice(&BITS.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    wav.extend_from_slice(&samples);

    let path = std::env::temp_dir().join(format!("giochi-beep-{}.wav", std::process::id()));
    std::fs::write(&path, wav)?;
    Ok(path)
}

/// Command-line options; a tiny parser keeps the example dependency-free.
#[derive(Debug, Default)]
struct Options {
    frames: Option<u64>,
    beep: bool,
}

/// Parses the command line, rejecting anything it does not understand, so a typo such as
/// `--frames=3oo` is a message rather than an unlimited run (the same fix the 3D examples
/// received in an earlier audit).
fn parse_args() -> Result<Options, String> {
    let mut options = Options::default();
    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        if let Some(value) = arg.strip_prefix("--frames=") {
            options.frames = Some(
                value
                    .parse()
                    .map_err(|_| format!("--frames expects a number, got '{value}'"))?,
            );
        } else if arg == "--frames" {
            let value = args.next().ok_or_else(|| {
                "--frames needs a value; accepted: --frames N, --beep".to_string()
            })?;
            options.frames = Some(
                value
                    .parse()
                    .map_err(|_| format!("--frames expects a number, got '{value}'"))?,
            );
        } else if arg == "--beep" {
            options.beep = true;
        } else {
            return Err(format!(
                "unknown argument '{arg}'; accepted: --frames N, --beep"
            ));
        }
    }

    Ok(options)
}
