//! Ball balance — a game built to *feel* input latency.
//!
//! A suspended platform, a ball resting on it, and one job: tilt the platform to keep the
//! ball from rolling off. Every input mistake shows up immediately as the ball drifting,
//! which is what makes it a latency test — a delay between the stick moving and the
//! platform tilting appears as the ball going somewhere you did not ask.
//!
//! Three things are drawn so the sources of lag can be told apart:
//!
//! * **the input marker** (blue cube, right) follows the stick with *no physics at all*.
//!   If it lags your thumb, the delay is in input or display, not in the game;
//! * **the platform** tilts from the same input, one frame behind the marker;
//! * **the ball** adds the simulation on top of that.
//!
//! The cube on the left is a hitch indicator: green while the last frame fitted in 60 Hz,
//! amber approaching the budget, red past it. A stutter is visible without a console.
//!
//! ```text
//! cargo run -p giochi-engine-3d --example ball_balance
//! cargo run -p giochi-engine-3d --example ball_balance -- --frames 600 --trace 120
//! cargo run -p giochi-engine-3d --example ball_balance -- --frames 300 --auto
//! cargo run -p giochi-engine-3d --example ball_balance -- --frames 3 --quadro out.bmp
//! ```
//!
//! Controls: left stick, or WASD / arrows. `R` resets the round, ESC quits.

use std::process::ExitCode;

use giochi_engine::input::gamepad::{AXIS_LEFT_X, AXIS_LEFT_Y};
use giochi_engine::input::keyboard::{
    KEY_A, KEY_D, KEY_DOWN, KEY_LEFT, KEY_RIGHT, KEY_S, KEY_UP, KEY_W,
};
use giochi_engine::input::Input;
use giochi_engine::platform::sdl::{quit_signal, QuitSignal};
use giochi_engine::platform::window::{Window, WindowConfig};
use giochi_engine::platform::Platform;
use giochi_engine_3d::context::{self, GlesVersion};
use giochi_engine_3d::game::{BalanceConfig, BalanceGame, BalanceView};
use giochi_engine_3d::math3d::{Color3D, Transform3D, Vec3};
use giochi_engine_3d::shader::BuiltinShader;
use giochi_engine_3d::RenderContext3D;
use sdl2::keyboard::Keycode;

/// Below this the stick counts as centred: a worn analogue stick never reads exactly 0.
const DEAD_ZONE: f32 = 0.12;

#[derive(Debug, Default)]
struct Options {
    /// Stop after this many frames.
    frames: Option<u64>,
    /// Print a per-frame line for the first N frames, for measuring latency.
    trace: Option<u64>,
    /// Capture the last frame to this BMP.
    quadro: Option<String>,
    /// Drive the input from the built-in script instead of the hardware.
    auto: bool,
}

impl Options {
    /// Parses the command line, rejecting anything it does not understand.
    ///
    /// It used to ignore unknown flags and turn an unparseable number into `None`, which
    /// for `--frames` means "no limit": a typo such as `--frames=3oo` produced an
    /// unattended run that never terminates, and in CI that is a job timeout rather than
    /// a message. Every uncertainty is an error now.
    fn parse() -> Result<Self, String> {
        let mut options = Self::default();
        let mut args = std::env::args().skip(1);

        while let Some(arg) = args.next() {
            if let Some(value) = arg.strip_prefix("--frames=") {
                options.frames = Some(parse_number(value, "--frames")?);
            } else if arg == "--frames" {
                options.frames = Some(parse_number(
                    next_value(&mut args, "--frames")?,
                    "--frames",
                )?);
            } else if let Some(value) = arg.strip_prefix("--trace=") {
                options.trace = Some(parse_number(value, "--trace")?);
            } else if arg == "--trace" {
                options.trace = Some(parse_number(next_value(&mut args, "--trace")?, "--trace")?);
            } else if let Some(value) = arg.strip_prefix("--quadro=") {
                options.quadro = Some(value.to_string());
            } else if arg == "--quadro" {
                options.quadro = Some(next_value(&mut args, "--quadro")?.to_string());
            } else if arg == "--auto" {
                options.auto = true;
            } else {
                return Err(format!("unknown argument '{arg}'; accepted: --frames N, --trace N, --auto, --quadro PATH"));
            }
        }

        Ok(options)
    }
}

/// Returns the value that follows a flag, or an error naming it.
fn next_value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    args.next().ok_or_else(|| {
        format!("{flag} needs a value; accepted: --frames N, --trace N, --auto, --quadro PATH")
    })
}

/// Parses a flag's value, naming the flag when it is not a number.
fn parse_number<T: std::str::FromStr>(value: impl AsRef<str>, flag: &str) -> Result<T, String> {
    let value = value.as_ref();
    value
        .parse()
        .map_err(|_| format!("{flag} expects a number, got '{value}'"))
}

/// Reads the stick, falling back to the keyboard when no gamepad is attached.
fn read_input(input: &Input) -> (f32, f32) {
    let gamepad = input.gamepad();
    if gamepad.is_connected() {
        // SDL reports negative Y for "stick up"; the near direction is +Z, so it is kept.
        return (
            apply_dead_zone(gamepad.axis(AXIS_LEFT_X)),
            apply_dead_zone(gamepad.axis(AXIS_LEFT_Y)),
        );
    }

    let keyboard = input.keyboard();
    let mut x = 0.0;
    let mut z = 0.0;
    if keyboard.is_down(KEY_D) || keyboard.is_down(KEY_RIGHT) {
        x += 1.0;
    }
    if keyboard.is_down(KEY_A) || keyboard.is_down(KEY_LEFT) {
        x -= 1.0;
    }
    if keyboard.is_down(KEY_S) || keyboard.is_down(KEY_DOWN) {
        z += 1.0;
    }
    if keyboard.is_down(KEY_W) || keyboard.is_down(KEY_UP) {
        z -= 1.0;
    }

    (x, z)
}

fn apply_dead_zone(value: f32) -> f32 {
    if value.abs() < DEAD_ZONE {
        0.0
    } else {
        value
    }
}

/// Frame-time statistics, so a stutter is a number and not a feeling.
#[derive(Debug, Default)]
struct FrameStats {
    samples: Vec<f32>,
    worst: f32,
    over_budget: u32,
}

impl FrameStats {
    fn record(&mut self, millis: f32) {
        self.samples.push(millis);
        if millis > self.worst {
            self.worst = millis;
        }
        // 60 Hz is 16.67 ms; count anything meaningfully past it.
        if millis > 20.0 {
            self.over_budget += 1;
        }
    }

    fn percentile(&self, fraction: f32) -> f32 {
        if self.samples.is_empty() {
            return 0.0;
        }
        let mut sorted = self.samples.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let index = ((sorted.len() as f32 * fraction) as usize).min(sorted.len() - 1);
        sorted[index]
    }

    fn summary(&self) -> String {
        if self.samples.is_empty() {
            return "no frames".to_owned();
        }
        let mean = self.samples.iter().sum::<f32>() / self.samples.len() as f32;
        format!(
            "frame work: mean {mean:.2} ms, p50 {:.2}, p95 {:.2}, p99 {:.2}, worst {:.2}; \
             {}/{} frames over 20 ms",
            self.percentile(0.50),
            self.percentile(0.95),
            self.percentile(0.99),
            self.worst,
            self.over_budget,
            self.samples.len()
        )
    }

    /// Colour for the hitch indicator.
    fn indicator(&self) -> Color3D {
        match self.samples.last().copied().unwrap_or(0.0) {
            ms if ms < 16.7 => Color3D::new(0.2, 0.85, 0.3, 1.0),
            ms if ms < 25.0 => Color3D::new(0.95, 0.8, 0.2, 1.0),
            _ => Color3D::new(0.95, 0.25, 0.2, 1.0),
        }
    }
}

fn main() -> ExitCode {
    env_logger::init();

    // Both steps produce the same kind of message, and a bad argument is reported the same
    // way a failed run is: one line on stderr and a non-zero exit.
    match Options::parse()
        .map_err(Box::<dyn std::error::Error>::from)
        .and_then(run)
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("erro: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(options: Options) -> Result<(), Box<dyn std::error::Error>> {
    let platform = Platform::new()?;

    let requested = GlesVersion::from_env();
    context::configure_attributes(&platform, requested)?;

    let window = Window::new_opengl(&platform, WindowConfig::new(1280, 720), "giochi balance")?;
    let mut render = RenderContext3D::new(context::create(&window)?);
    println!(
        "GLES {} on {}, driver {}",
        render.gles_version().major(),
        render.adapter(),
        platform.video_driver_name()
    );

    let config = BalanceConfig::default();
    let platform_mesh = render.create_plane_mesh(config.platform_size, 6)?;
    let ball_mesh = render.create_sphere_mesh(config.ball_radius, 16, 24)?;
    let marker_mesh = render.create_cube_mesh(0.45)?;
    let shader = render.create_builtin_shader(BuiltinShader::Flat)?;
    println!(
        "mesh: platform {} triangles, ball {} vertices, marker {} triangles",
        render.mesh_index_count(platform_mesh) / 3,
        render.mesh_vertex_count(ball_mesh),
        render.mesh_index_count(marker_mesh) / 3
    );

    render.set_clear_color(Color3D::new(0.07, 0.09, 0.14, 1.0));

    // The camera and the two indicators live in `game::BalanceView`, not here, because
    // whether they are on screen is a checkable property and nothing was checking it: the
    // markers were originally placed by eye against a 16:9 window, and on the console's 4:3
    // panel the hitch indicator sat entirely outside the frame and the input marker left it
    // at full deflection. `BalanceView` is tested at both aspect ratios.
    let view = BalanceView::new(config.platform_size / 2.0);
    let camera = view.camera().with_clip_planes(0.1, 100.0);

    let mut input = Input::new(&platform);
    let mut game = BalanceGame::new(config);
    let mut stats = FrameStats::default();

    let mut frames: u64 = 0;
    let mut previous_falls = 0;
    let mut last_input = (0.0_f32, 0.0_f32);
    let mut previous_frame = std::time::Instant::now();

    let frame_budget_ms = if options.frames.is_some() {
        0
    } else {
        1000 / 60
    };

    loop {
        let frame_started = std::time::Instant::now();
        // Real frame time drives the physics, so the game runs at the same speed whatever
        // the console manages. In a fixed-frames run the step is pinned instead, to keep
        // the traces comparable between machines.
        let delta = if options.frames.is_some() {
            1.0 / 60.0
        } else {
            frame_started
                .duration_since(previous_frame)
                .as_secs_f32()
                .clamp(1.0 / 240.0, 0.25)
        };
        previous_frame = frame_started;

        let events = platform.poll_events();
        for event in &events {
            if quit_signal(event) != QuitSignal::None {
                let meshes = [platform_mesh, ball_mesh, marker_mesh];
                finish(&mut render, meshes, shader, &game, &stats, frames);
                return Ok(());
            }
            if let sdl2::event::Event::KeyDown {
                keycode: Some(Keycode::R),
                repeat: false,
                ..
            } = event
            {
                game.respawn();
                println!("reset");
            }
        }
        input.update(&events);

        let stick = if options.auto {
            BalanceGame::scripted_input(frames)
        } else {
            read_input(&input)
        };

        game.update(stick, delta);

        if game.falls() > previous_falls {
            previous_falls = game.falls();
            println!(
                "caiu ({}) aos {:.1} s; melhor {:.1} s",
                game.falls(),
                game.survived(),
                game.best()
            );
        }

        // Ask the window, not the renderer: the renderer only learns the drawable size
        // inside `begin_frame`, so reading it here would give the previous frame's size —
        // and nothing at all on the first frame.
        let (width, height) = window.drawable_size();
        let camera = camera.with_drawable_size(width, height);

        render.begin_frame(&window);

        let tilt = game.tilt();
        let ball = game.ball();
        render.draw_mesh_colored(
            platform_mesh,
            shader,
            &Transform3D::from_rotation(tilt),
            &camera,
            Color3D::new(0.30, 0.34, 0.42, 1.0),
        )?;

        // Pale while there is room, then amber and red as the rim approaches. Pale rather
        // than green so it never reads as the hitch indicator, which is the only green
        // thing on screen.
        let ball_colour = if game.is_falling() {
            Color3D::new(0.92, 0.16, 0.12, 1.0)
        } else {
            let danger = ball.edge_proximity(&config);
            let heat = ((danger - 0.55) / 0.45).clamp(0.0, 1.0);
            Color3D::new(
                0.98 - 0.06 * heat,
                0.94 - 0.78 * heat,
                0.80 - 0.68 * heat,
                1.0,
            )
        };
        render.draw_mesh_colored(
            ball_mesh,
            shader,
            &Transform3D::from_translation(ball.world_position(tilt, game.fall_drop(), &config))
                .with_rotation(ball.spin()),
            &camera,
            ball_colour,
        )?;

        // The raw input marker: no physics, no smoothing. Whatever lag it shows is input
        // or display lag, never the simulation.
        render.draw_mesh_colored(
            marker_mesh,
            shader,
            &Transform3D::from_translation(view.input_marker_position(stick)),
            &camera,
            Color3D::new(0.35, 0.75, 1.0, 1.0),
        )?;

        // The hitch indicator.
        render.draw_mesh_colored(
            marker_mesh,
            shader,
            &Transform3D::from_translation(view.hitch_marker_position()),
            &camera,
            stats.indicator(),
        )?;

        if options
            .quadro
            .as_deref()
            .is_some_and(|_| options.frames.is_some_and(|limit| frames + 1 >= limit))
        {
            let path = options.quadro.clone().unwrap_or_default();
            let pixels = render.read_pixels(width, height)?;
            write_bmp(&path, width, height, &pixels)?;
            println!("quadro: {path} ({width}x{height})");
        }

        // Measured **before** the swap, and this is the whole point of the indicator.
        // `end_frame` swaps the buffers, and on KMSDRM that blocks until vsync: reading
        // the clock after it charges every frame the 16.7 ms it waited, so the hitch
        // indicator sits permanently on its own threshold and reports a lag that is
        // just the display refreshing. What a player needs to know is how long the game
        // took to produce the frame, not how long the panel took to show it.
        let frame_millis = frame_started.elapsed().as_secs_f32() * 1000.0;

        render.end_frame(&window);

        let error = render.take_gl_error();
        if error != 0 {
            return Err(format!("GL error 0x{error:04x} on frame {frames}").into());
        }

        stats.record(frame_millis);

        // The trace is the measurement: one line per frame with the input, the tilt and
        // where the ball is. Input changing on frame N and the ball reacting on frame N
        // is correct; reacting on frame N+3 is a bug worth finding.
        if options.trace.is_some_and(|limit| frames < limit) {
            let (bx, bz) = ball.position();
            let (vx, vz) = ball.velocity();
            println!(
                "f{frames:04} work {frame_millis:5.2}ms in ({:+.2},{:+.2}) tilt ({:+.3},{:+.3}) \
                 ball ({bx:+.3},{bz:+.3}) v ({vx:+.2},{vz:+.2})",
                stick.0,
                stick.1,
                -tilt.rotate_vec3(Vec3::new(1.0, 0.0, 0.0)).y,
                tilt.rotate_vec3(Vec3::new(0.0, 0.0, 1.0)).y,
            );
            if (stick.0 - last_input.0).abs() > 0.2 || (stick.1 - last_input.1).abs() > 0.2 {
                println!("     input changed at frame {frames}");
            }
            last_input = stick;
        }

        frames += 1;

        if options.frames.is_some_and(|limit| frames >= limit) {
            let meshes = [platform_mesh, ball_mesh, marker_mesh];
            finish(&mut render, meshes, shader, &game, &stats, frames);
            return Ok(());
        }

        if frame_budget_ms > 0 {
            // Pacing is the one place the post-swap time is the right number: the swap
            // has already spent whatever the display wanted, and only the remainder up
            // to the budget is ours to sleep. Using the pre-swap figure here would stack
            // a delay on top of the vsync wait and halve the frame rate.
            let spent = frame_started.elapsed().as_millis() as u32;
            if spent < frame_budget_ms {
                platform.delay_ms(frame_budget_ms - spent);
            }
        }
    }
}

fn finish(
    render: &mut RenderContext3D,
    meshes: [u64; 3],
    shader: u64,
    game: &BalanceGame,
    stats: &FrameStats,
    frames: u64,
) {
    println!("{frames} frames rendered, no GL error");
    println!("{}", stats.summary());
    println!(
        "melhor equilíbrio {:.1} s, {} quedas; bola em ({:+.2},{:+.2})",
        game.best(),
        game.falls(),
        game.ball().position().0,
        game.ball().position().1
    );

    for mesh in meshes {
        render.destroy_mesh(mesh);
    }
    render.destroy_shader(shader);
    println!(
        "released: {} meshes and {} shaders left",
        render.mesh_count(),
        render.shader_count()
    );
}

/// Writes RGBA8 pixels as a 32-bit BMP, bottom-up.
fn write_bmp(path: &str, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    let expected = (width as usize) * (height as usize) * 4;
    if rgba.len() != expected {
        return Err(format!(
            "capture has {} bytes; expected {expected}",
            rgba.len()
        ));
    }

    // BMP stores BGRA ordered bottom-up, which is exactly the order glReadPixels returns,
    // so only the channel order changes.
    let mut pixels = Vec::with_capacity(expected);
    for pixel in rgba.chunks_exact(4) {
        pixels.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
    }

    let header = 54usize;
    let mut bmp = Vec::with_capacity(header + pixels.len());
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&((header + pixels.len()) as u32).to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&(header as u32).to_le_bytes());
    bmp.extend_from_slice(&40u32.to_le_bytes());
    bmp.extend_from_slice(&(width as i32).to_le_bytes());
    bmp.extend_from_slice(&(height as i32).to_le_bytes());
    bmp.extend_from_slice(&1u16.to_le_bytes());
    bmp.extend_from_slice(&32u16.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&(pixels.len() as u32).to_le_bytes());
    bmp.extend_from_slice(&2835i32.to_le_bytes());
    bmp.extend_from_slice(&2835i32.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&pixels);

    std::fs::write(path, &bmp).map_err(|err| format!("failed to write '{path}': {err}"))
}
