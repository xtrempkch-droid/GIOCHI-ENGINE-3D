//! `profiler_demo` — the phase-15 baseline for the batching work in phase 16.
//!
//! Draws N sprites with one texture through
//! [`RenderContext::draw_texture`](giochi_engine::RenderContext::draw_texture), the
//! path `draw_sprite` uses. That is exactly the workload phase 16 must make faster,
//! so the scene itself is deliberately naive; the speed-up has to come from the engine.
//!
//! `--immediate` draws through
//! [`RenderContext::draw_texture_immediate`](giochi_engine::RenderContext::draw_texture_immediate)
//! instead, which is the phase-15 `SDL_RenderCopy` path. Same scene, same binary, so the
//! A/B does not depend on rebuilding an old commit.
//!
//! `--textures N` gives each sprite one of `N` textures (`i % N`), which is the
//! worst case for batching: a texture change starts a new run. `--use-atlas` packs
//! those same `N` images into one [`TextureAtlas`] and draws regions of it, so the
//! scene and the pixel coverage stay identical while the draw calls collapse to one.
//!
//! ```text
//! cargo run -p giochi-demo --example profiler_demo --features profiling
//! cargo run -p giochi-demo --example profiler_demo --features profiling -- --sprites 200 --frames 600
//! cargo run --release -p giochi-demo --example profiler_demo --features profiling -- --sprites 1000 --frames 600
//! cargo run --release -p giochi-demo --example profiler_demo --features profiling -- --sprites 1000 --textures 10
//! cargo run --release -p giochi-demo --example profiler_demo --features profiling -- --sprites 1000 --textures 10 --use-atlas
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
    AudioSystem, Camera, Engine, EngineConfig, FrameStats, FrameTimeSummary, Input, Rect,
    RenderContext, Scene, SpriteBatchConfig, TextureAtlas, TextureHandle, Time, Vec2,
};

const DEFAULT_WIDTH: u32 = 1280;
const DEFAULT_HEIGHT: u32 = 720;
const DEFAULT_SPRITES: u32 = 1_000;
/// Default drawn size of a sprite, in pixels. At 24x24 a thousand sprites already
/// cover the screen several times over, so a run at that size measures the
/// rasteriser; `--sprite-size 8` shifts the balance towards per-call overhead.
const DEFAULT_SPRITE_SIZE: f32 = 24.0;
/// Side of every generated source image, in pixels.
const SOURCE_SIZE: u32 = 16;

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
    /// Distinct source images. Sprite `i` uses image `i % textures`.
    textures: u32,
    /// Pack the source images into one atlas instead of separate textures.
    use_atlas: bool,
    /// Draw through `draw_texture_immediate`, the pre-phase-16 path, for an A/B.
    immediate: bool,
    /// Keep the camera at its default (centred on the world origin), which is what
    /// the phase-15 baseline measured, so the two tables stay comparable.
    centred_camera: bool,
    /// `SpriteBatchConfig::max_sprites`; `0` keeps the engine default.
    batch_max: u32,
    /// Drawn side of a sprite, in pixels.
    sprite_size: f32,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
            sprites: DEFAULT_SPRITES,
            frames: None,
            overlay: true,
            textures: 1,
            use_atlas: false,
            immediate: false,
            centred_camera: false,
            batch_max: 0,
            sprite_size: DEFAULT_SPRITE_SIZE,
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
            } else if let Some(value) = arg.strip_prefix("--textures=") {
                options.textures = parse_number(value, "--textures")?;
            } else if arg == "--textures" {
                options.textures =
                    parse_number(next_value(&mut args, "--textures")?, "--textures")?;
            } else if arg == "--use-atlas" {
                options.use_atlas = true;
            } else if arg == "--immediate" {
                options.immediate = true;
            } else if arg == "--centred-camera" {
                options.centred_camera = true;
            } else if let Some(value) = arg.strip_prefix("--sprite-size=") {
                options.sprite_size = parse_number(value, "--sprite-size")?;
            } else if arg == "--sprite-size" {
                options.sprite_size =
                    parse_number(next_value(&mut args, "--sprite-size")?, "--sprite-size")?;
            } else if let Some(value) = arg.strip_prefix("--batch-max=") {
                options.batch_max = parse_number(value, "--batch-max")?;
            } else if arg == "--batch-max" {
                options.batch_max =
                    parse_number(next_value(&mut args, "--batch-max")?, "--batch-max")?;
            } else {
                return Err(format!(
                    "unknown argument '{arg}'; accepted: --sprites N, --frames N, \
                     --width N, --height N, --textures N, --overlay, --no-overlay, \
                     --use-atlas, --immediate, --centred-camera, --batch-max N, \
                     --sprite-size N"
                ));
            }
        }

        if options.width == 0 || options.height == 0 {
            return Err("--width and --height must be non-zero".to_owned());
        }
        if options.sprites == 0 {
            return Err("--sprites must be greater than zero".to_owned());
        }
        if options.textures == 0 {
            return Err("--textures must be greater than zero".to_owned());
        }
        if options.frames == Some(0) {
            return Err("--frames must be greater than zero".to_owned());
        }
        if !(options.sprite_size.is_finite() && options.sprite_size > 0.0) {
            return Err("--sprite-size must be a positive number".to_owned());
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

    // `Camera::new` centres the world origin in the viewport, so with sprites placed
    // at `0..width` three quarters of the field lands off-screen — which is what the
    // phase-15 baseline measured without saying so. Pinning the camera position to the
    // viewport centre makes world coordinates screen coordinates and the frame covers
    // the whole screen. `--centred-camera` restores the old placement so the two
    // tables can be compared like for like.
    let width = options.width as f32;
    let height = options.height as f32;
    if !options.centred_camera {
        engine.render_mut().set_camera(Camera::with_position(
            Rect::new(0.0, 0.0, width, height),
            width / 2.0,
            height / 2.0,
        ));
    }

    let (source, source_label) = build_source(&mut engine, &options)?;
    if options.batch_max > 0 {
        let current = engine.render().sprite_batch().config();
        engine.render_mut().set_batch_config(SpriteBatchConfig::new(
            current.initial_sprites,
            options.batch_max,
        ));
    }

    engine.push_scene(Box::new(SpriteField::new(
        source,
        options.sprites,
        width,
        height,
        options.immediate,
        options.sprite_size,
    )));

    let render = engine.render();
    println!(
        "profiler_demo: {} sprites, {source_label}, overlay {} \
         (F1 alterna, ESC sai) — backend {} (accelerated: {})",
        options.sprites,
        if engine.is_overlay_visible() {
            "ligado"
        } else {
            "desligado"
        },
        render.renderer_name(),
        render.renderer_is_accelerated(),
    );

    if let Some(frames) = options.frames {
        return benchmark(&mut engine, frames, options.sprites);
    }

    engine.run_current().map_err(|error| error.to_string())
}

/// Builds the scene's textures, either as separate uploads or packed into one atlas.
///
/// Returns the source and a human-readable description of it.
fn build_source(engine: &mut Engine, options: &Options) -> Result<(SpriteSource, String), String> {
    let images: Vec<Vec<u8>> = (0..options.textures).map(sprite_pixels).collect();

    if options.use_atlas {
        let names: Vec<String> = (0..images.len())
            .map(|index| format!("tex{index}"))
            .collect();
        let regions: Vec<(&str, u32, u32, &[u8])> = names
            .iter()
            .zip(&images)
            .map(|(name, pixels)| (name.as_str(), SOURCE_SIZE, SOURCE_SIZE, pixels.as_slice()))
            .collect();
        let atlas = TextureAtlas::from_rgba_images(engine.render(), &regions)
            .map_err(|error| error.to_string())?;
        let bounds: Vec<Rect> = atlas
            .names()
            .into_iter()
            .map(|name| atlas.region(&name).pixel_rect())
            .collect();
        let label = format!(
            "{} region(s) in one {}x{} atlas",
            bounds.len(),
            atlas.size().0,
            atlas.size().1
        );
        return Ok((
            SpriteSource::Atlas {
                texture: atlas.texture(),
                regions: bounds,
            },
            label,
        ));
    }

    let mut textures = Vec::with_capacity(images.len());
    for pixels in &images {
        textures.push(
            engine
                .render_mut()
                .create_texture_from_rgba(SOURCE_SIZE, SOURCE_SIZE, pixels)
                .map_err(|error| error.to_string())?,
        );
    }
    if textures.len() == 1 {
        Ok((SpriteSource::Single(textures[0]), "1 texture".to_owned()))
    } else {
        let label = format!("{} textures (interleaved)", textures.len());
        Ok((SpriteSource::Many(textures), label))
    }
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
        renderer: engine.render().renderer_name(),
        accelerated: engine.render().renderer_is_accelerated(),
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
    renderer: &'static str,
    accelerated: bool,
}

fn summary_line(measurement: &Measurement) -> String {
    format!(
        "profiler_demo: sprites={} frames={} wall={:.3}s fps={:.1} \
         frame_avg={:.3}ms frame_p99={:.3}ms cpu_avg={:.3}ms \
         render2d_avg={:.3}ms draw_calls={} renderer={} accelerated={}",
        measurement.sprites,
        measurement.frames,
        measurement.wall,
        measurement.fps,
        measurement.summary.average_ms(),
        measurement.summary.p99_ms(),
        measurement.cpu_avg_ms,
        measurement.render2d_avg_ms,
        measurement.stats.draw_calls,
        measurement.renderer,
        measurement.accelerated
    )
}

/// Builds a 16x16 RGBA sprite with a bright border, so every draw is visible.
///
/// `variant` shifts the interior colour, so images packed into one atlas stay
/// distinguishable on screen.
fn sprite_pixels(variant: u32) -> Vec<u8> {
    let size = SOURCE_SIZE;
    let mut pixels = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let border = x < 2 || y < 2 || x >= size - 2 || y >= size - 2;
            let colour = if border {
                [232, 240, 255, 255]
            } else {
                [
                    (40 + x * 10 + variant * 3) as u8,
                    (80 + y * 8) as u8,
                    (180 + variant * 7) as u8,
                    255,
                ]
            };
            let offset = ((y * size + x) * 4) as usize;
            pixels[offset..offset + 4].copy_from_slice(&colour);
        }
    }
    pixels
}

/// Where the sprites' pixels come from.
enum SpriteSource {
    /// One texture for every sprite, drawn from its whole area.
    Single(TextureHandle),
    /// Several textures, cycled per sprite: the worst case for the batcher.
    Many(Vec<TextureHandle>),
    /// One atlas, with sprites cycling through its regions.
    Atlas {
        texture: TextureHandle,
        regions: Vec<Rect>,
    },
}

/// A fixed number of sprites bouncing inside the window.
struct SpriteField {
    source: SpriteSource,
    bodies: Vec<Body>,
    width: f32,
    height: f32,
    /// Use `draw_texture_immediate`, the phase-15 path, instead of `draw_texture`.
    immediate: bool,
    /// Drawn side of every sprite, in pixels.
    sprite_size: f32,
}

impl SpriteField {
    fn new(
        source: SpriteSource,
        count: u32,
        width: f32,
        height: f32,
        immediate: bool,
        sprite_size: f32,
    ) -> Self {
        let mut seed = 0x1234_5678u32;
        let mut bodies = Vec::with_capacity(count as usize);
        for _ in 0..count {
            bodies.push(Body {
                x: random(&mut seed) * (width - sprite_size).max(0.0),
                y: random(&mut seed) * (height - sprite_size).max(0.0),
                vx: (random(&mut seed) - 0.5) * 120.0,
                vy: (random(&mut seed) - 0.5) * 120.0,
            });
        }
        Self {
            source,
            bodies,
            width,
            height,
            immediate,
            sprite_size,
        }
    }
}

impl Scene for SpriteField {
    fn update(&mut self, time: Time, _input: &Input, _audio: &mut AudioSystem) {
        let max_x = (self.width - self.sprite_size).max(0.0);
        let max_y = (self.height - self.sprite_size).max(0.0);
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
        // Positions stay in world space and go through the camera exactly like
        // `draw_sprite` does. With the default (centred) camera that puts three
        // quarters of the field off-screen, which is the phase-15 scene; `run` pins
        // the camera otherwise so the frame covers the whole screen. Either way the
        // batched and immediate arms see identical geometry.
        for (index, body) in self.bodies.iter().enumerate() {
            let origin = renderer.world_to_screen(Vec2::new(body.x, body.y));
            let dst = Rect::new(origin.x, origin.y, self.sprite_size, self.sprite_size);
            let (texture, src) = match &self.source {
                SpriteSource::Single(texture) => (*texture, Rect::ZERO),
                SpriteSource::Many(textures) if !textures.is_empty() => {
                    (textures[index % textures.len()], Rect::ZERO)
                }
                SpriteSource::Atlas { texture, regions } if !regions.is_empty() => {
                    (*texture, regions[index % regions.len()])
                }
                _ => continue,
            };
            let _ = if self.immediate {
                renderer.draw_texture_immediate(texture, dst, src)
            } else {
                renderer.draw_texture(texture, dst, src)
            };
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
            renderer: "software",
            accelerated: false,
        });

        assert!(line.contains("sprites=200"));
        assert!(line.contains("frames=600"));
        assert!(line.contains("draw_calls=200"));
        assert!(line.contains("cpu_avg=2.500ms"));
        assert!(line.contains("renderer=software"));
    }

    #[test]
    fn the_parser_accepts_texture_and_atlas_modes() {
        let options = parse(&["--textures", "10", "--use-atlas"]).expect("valid");
        assert_eq!(options.textures, 10);
        assert!(options.use_atlas);
        assert_eq!(Options::default().textures, 1);
        assert!(parse(&["--textures", "0"]).is_err());
    }

    #[test]
    fn the_parser_can_reproduce_the_phase_15_scene() {
        assert!(!Options::default().centred_camera);
        assert!(parse(&["--centred-camera"]).expect("valid").centred_camera);
    }

    #[test]
    fn the_parser_accepts_the_measurement_overrides() {
        let options =
            parse(&["--immediate", "--batch-max", "1024", "--sprite-size=8"]).expect("valid");
        assert!(options.immediate);
        assert_eq!(options.batch_max, 1024);
        assert!((options.sprite_size - 8.0).abs() < f32::EPSILON);
        assert!((Options::default().sprite_size - DEFAULT_SPRITE_SIZE).abs() < f32::EPSILON);

        assert!(parse(&["--sprite-size", "0"]).is_err());
        assert!(parse(&["--sprite-size", "-4"]).is_err());
        assert!(parse(&["--batch-max=oops"]).is_err());
    }
}
