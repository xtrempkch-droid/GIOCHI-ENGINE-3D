//! `profiler_demo` — the phase-15 baseline for the batching work in phase 16.
//!
//! Draws N sprites with one texture, using the ordinary
//! [`RenderContext::draw_sprite`](giochi_engine::RenderContext::draw_sprite) path.
//! That is exactly the workload phase 16 must make faster, so this example is kept
//! deliberately naive: one draw call per sprite, no atlas, no batching.
//!
//! ```text
//! cargo run -p giochi-demo --example profiler_demo --features profiling
//! cargo run -p giochi-demo --example profiler_demo --features profiling -- --sprites 200 --frames 600
//! cargo run --release -p giochi-demo --example profiler_demo --features profiling -- --sprites 1000 --frames 600
//! ```
//!
//! Without `--frames` the example runs interactively at 60 Hz; `F1` toggles the
//! overlay and `Escape` quits. With `--frames` it runs a benchmark loop with no
//! frame limiting and prints one machine-readable line at the end, which is what
//! `docs/PERF_LOG.md` records. `--no-overlay` hides the panel for a benchmark that
//! isolates the scene from the panel's own cost (the panel is a developer tool; on a
//! small ARM64 panel, creating its texture is the expensive half of profiling).

use std::process::ExitCode;
use std::time::Instant;

use giochi_engine::{
    AudioSystem, Engine, EngineConfig, FrameStats, FrameTimeSummary, Input, RenderContext, Scene,
    Sprite, TextureHandle, Time,
};

const DEFAULT_WIDTH: u32 = 1280;
const DEFAULT_HEIGHT: u32 = 720;
const DEFAULT_SPRITES: u32 = 1_000;
const SPRITE_SIZE: f32 = 24.0;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("erro: {error}");
            ExitCode::FAILURE
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Options {
    width: u32,
    height: u32,
    sprites: u32,
    frames: Option<u64>,
    overlay: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
            sprites: DEFAULT_SPRITES,
            frames: None,
            overlay: true,
        }
    }
}

impl Options {
    /// Parses the command line, rejecting unknown flags and malformed values.
    ///
    /// The project already learned this lesson in the second and sixth audits: a
    /// typo such as `--frames=3oo` must be an error, never "run forever".
    fn parse() -> Result<Self, String> {
        Self::parse_from(std::env::args().skip(1))
    }

    fn parse_from(args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut options = Self::default();
        let mut args = args;
        while let Some(arg) = args.next() {
            if let Some(value) = arg.strip_prefix("--sprites=") {
                options.sprites = parse_number(value, "--sprites")?;
            } else if arg == "--sprites" {
                options.sprites = parse_number(next_value(&mut args, "--sprites")?, "--sprites")?;
            } else if let Some(value) = arg.strip_prefix("--frames=") {
                options.frames = Some(parse_number(value, "--frames")?);
            } else if arg == "--frames" {
                options.frames = Some(parse_number(
                    next_value(&mut args, "--frames")?,
                    "--frames",
                )?);
            } else if let Some(value) = arg.strip_prefix("--width=") {
                options.width = parse_number(value, "--width")?;
            } else if arg == "--width" {
                options.width = parse_number(next_value(&mut args, "--width")?, "--width")?;
            } else if let Some(value) = arg.strip_prefix("--height=") {
                options.height = parse_number(value, "--height")?;
            } else if arg == "--height" {
                options.height = parse_number(next_value(&mut args, "--height")?, "--height")?;
            } else if arg == "--no-overlay" {
                options.overlay = false;
            } else if arg == "--overlay" {
                options.overlay = true;
            } else {
                return Err(format!(
                    "unknown argument '{arg}'; accepted: --sprites N, --frames N, \
                     --width N, --height N, --overlay, --no-overlay"
                ));
            }
        }

        if options.width == 0 || options.height == 0 {
            return Err("--width and --height must be non-zero".to_owned());
        }
        if options.sprites == 0 {
            return Err("--sprites must be greater than zero".to_owned());
        }
        if options.frames == Some(0) {
            return Err("--frames must be greater than zero".to_owned());
        }
        Ok(options)
    }
}

fn next_value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    args.next().ok_or_else(|| format!("{flag} needs a value"))
}

fn parse_number<T: std::str::FromStr>(value: impl AsRef<str>, flag: &str) -> Result<T, String> {
    value
        .as_ref()
        .parse()
        .map_err(|_| format!("{flag} expects a number, got '{}'", value.as_ref()))
}

fn run() -> Result<(), String> {
    let options = Options::parse().map_err(|error| format!("argumentos inválidos: {error}"))?;

    let mut config = EngineConfig::new(options.width, options.height, "giochi — profiler_demo");
    // Benchmarking must not be limited by the display; interactive mode is paced by
    // the fixed-timestep loop anyway.
    config.vsync = 0;

    let mut engine = Engine::new(config).map_err(|error| error.to_string())?;
    engine.set_overlay_visible(options.overlay);
    let texture = create_sprite_texture(&mut engine)?;
    engine.push_scene(Box::new(SpriteField::new(
        texture,
        options.sprites,
        options.width as f32,
        options.height as f32,
    )));

    println!(
        "profiler_demo: {} sprites, overlay {} (F1 alterna, ESC sai)",
        options.sprites,
        if engine.is_overlay_visible() {
            "ligado"
        } else {
            "desligado"
        }
    );

    if let Some(frames) = options.frames {
        return benchmark(&mut engine, frames, options.sprites);
    }

    engine.run_current().map_err(|error| error.to_string())
}

/// Runs exactly `frames` iterations without sleeping and prints the measured result.
fn benchmark(engine: &mut Engine, frames: u64, sprites: u32) -> Result<(), String> {
    let started = Instant::now();
    let mut previous = Instant::now();
    let mut rendered = 0u64;
    let mut cpu_ns = 0u128;
    let mut render2d_ns = 0u128;

    for _ in 0..frames {
        let now = Instant::now();
        let frame_time = now.duration_since(previous).as_secs_f32();
        previous = now;
        engine.step(frame_time).map_err(|error| error.to_string())?;
        let stats = engine.frame_stats();
        cpu_ns += u128::from(stats.cpu_time_ns);
        render2d_ns += u128::from(stats.render2d_ns);
        rendered += 1;
        if !engine.is_running() {
            break;
        }
    }

    let wall = started.elapsed().as_secs_f32();
    let stats = engine.frame_stats();
    let summary = engine.frame_time_summary();
    let fps = if wall > 0.0 {
        rendered as f32 / wall
    } else {
        0.0
    };
    let (cpu_avg_ms, render2d_avg_ms) = if rendered > 0 {
        (
            cpu_ns as f64 / rendered as f64 / 1_000_000.0,
            render2d_ns as f64 / rendered as f64 / 1_000_000.0,
        )
    } else {
        (0.0, 0.0)
    };

    let measurement = Measurement {
        sprites,
        frames: rendered,
        wall,
        fps,
        stats,
        summary,
        cpu_avg_ms,
        render2d_avg_ms,
    };
    println!("{}", summary_line(&measurement));
    Ok(())
}

/// Everything the benchmark line reports, in one place so the formatter stays readable.
struct Measurement {
    sprites: u32,
    frames: u64,
    wall: f32,
    fps: f32,
    stats: FrameStats,
    summary: FrameTimeSummary,
    cpu_avg_ms: f64,
    render2d_avg_ms: f64,
}

fn summary_line(measurement: &Measurement) -> String {
    format!(
        "profiler_demo: sprites={} frames={} wall={:.3}s fps={:.1} \
         frame_avg={:.3}ms frame_p99={:.3}ms cpu_avg={:.3}ms \
         render2d_avg={:.3}ms draw_calls={}",
        measurement.sprites,
        measurement.frames,
        measurement.wall,
        measurement.fps,
        measurement.summary.average_ms(),
        measurement.summary.p99_ms(),
        measurement.cpu_avg_ms,
        measurement.render2d_avg_ms,
        measurement.stats.draw_calls
    )
}

/// Builds a 16x16 RGBA sprite with a bright border, so every draw is visible.
fn create_sprite_texture(engine: &mut Engine) -> Result<TextureHandle, String> {
    const SIZE: u32 = 16;
    let mut pixels = vec![0u8; (SIZE * SIZE * 4) as usize];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let border = x < 2 || y < 2 || x >= SIZE - 2 || y >= SIZE - 2;
            let colour = if border {
                [232, 240, 255, 255]
            } else {
                [(40 + x * 10) as u8, (80 + y * 8) as u8, 180u8, 255]
            };
            let offset = ((y * SIZE + x) * 4) as usize;
            pixels[offset..offset + 4].copy_from_slice(&colour);
        }
    }

    engine
        .render_mut()
        .create_texture_from_rgba(SIZE, SIZE, &pixels)
        .map_err(|error| error.to_string())
}

/// A fixed number of sprites bouncing inside the window.
struct SpriteField {
    texture: TextureHandle,
    bodies: Vec<Body>,
    width: f32,
    height: f32,
}

impl SpriteField {
    fn new(texture: TextureHandle, count: u32, width: f32, height: f32) -> Self {
        let mut seed = 0x1234_5678u32;
        let mut bodies = Vec::with_capacity(count as usize);
        for _ in 0..count {
            bodies.push(Body {
                x: random(&mut seed) * (width - SPRITE_SIZE),
                y: random(&mut seed) * (height - SPRITE_SIZE),
                vx: (random(&mut seed) - 0.5) * 120.0,
                vy: (random(&mut seed) - 0.5) * 120.0,
            });
        }
        Self {
            texture,
            bodies,
            width,
            height,
        }
    }
}

impl Scene for SpriteField {
    fn update(&mut self, time: Time, _input: &Input, _audio: &mut AudioSystem) {
        let max_x = self.width - SPRITE_SIZE;
        let max_y = self.height - SPRITE_SIZE;
        for body in &mut self.bodies {
            body.x += body.vx * time.delta;
            body.y += body.vy * time.delta;
            if body.x < 0.0 || body.x > max_x {
                body.vx = -body.vx;
                body.x = body.x.clamp(0.0, max_x);
            }
            if body.y < 0.0 || body.y > max_y {
                body.vy = -body.vy;
                body.y = body.y.clamp(0.0, max_y);
            }
        }
    }

    fn render(&mut self, renderer: &mut RenderContext) {
        // The camera is the identity by default, so world coordinates are screen
        // coordinates and the comparison with phase 16 stays about draw calls.
        for body in &self.bodies {
            let sprite = Sprite::new(self.texture, body.x, body.y, SPRITE_SIZE, SPRITE_SIZE);
            let _ = renderer.draw_sprite(&sprite);
        }
    }
}

struct Body {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
}

fn random(seed: &mut u32) -> f32 {
    *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    (*seed >> 8) as f32 / (1u32 << 24) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Options, String> {
        Options::parse_from(args.iter().map(|value| (*value).to_owned()))
    }

    #[test]
    fn the_parser_rejects_a_mistyped_frame_limit() {
        assert!(parse(&["--frames=3oo"]).is_err());
    }

    #[test]
    fn the_parser_rejects_unknown_flags() {
        let error = parse(&["--sprits", "100"]).expect_err("unknown flag");
        assert!(error.contains("--sprits"));
    }

    #[test]
    fn the_parser_accepts_the_documented_form() {
        let options = parse(&["--sprites", "200", "--frames=600"]).expect("valid");
        assert_eq!(options.sprites, 200);
        assert_eq!(options.frames, Some(600));
        assert!(options.overlay);
    }

    #[test]
    fn the_parser_can_hide_the_overlay() {
        let options = parse(&["--no-overlay"]).expect("valid");
        assert!(!options.overlay);
        let options = parse(&["--no-overlay", "--overlay"]).expect("valid");
        assert!(options.overlay);
    }

    #[test]
    fn the_parser_rejects_zero_work() {
        assert!(parse(&["--sprites", "0"]).is_err());
        assert!(parse(&["--frames", "0"]).is_err());
        assert!(parse(&["--width", "0"]).is_err());
    }

    #[test]
    fn the_summary_line_is_machine_readable() {
        let line = summary_line(&Measurement {
            sprites: 200,
            frames: 600,
            wall: 1.5,
            fps: 400.0,
            stats: FrameStats {
                draw_calls: 200,
                ..FrameStats::new()
            },
            summary: FrameTimeSummary {
                avg_ns: 2_500_000,
                p99_ns: 3_000_000,
                ..FrameTimeSummary::default()
            },
            cpu_avg_ms: 2.5,
            render2d_avg_ms: 2.0,
        });

        assert!(line.contains("sprites=200"));
        assert!(line.contains("frames=600"));
        assert!(line.contains("draw_calls=200"));
        assert!(line.contains("cpu_avg=2.500ms"));
    }
}
