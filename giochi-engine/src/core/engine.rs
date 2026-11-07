//! Engine facade and configuration.
//!
//! [`Engine`] is the idiomatic Rust entry point: it owns the platform, the renderer,
//! input, audio and the scene stack, and drives them through the fixed-timestep
//! loop:
//!
//! ```no_run
//! use giochi_engine::{Engine, EngineConfig, EmptyScene};
//!
//! # fn main() -> Result<(), giochi_engine::GiochiError> {
//! let mut engine = Engine::new(EngineConfig::default())?;
//! engine.run(Box::new(EmptyScene::new()))?;
//! # Ok(())
//! # }
//! ```
//!
//! The C ABI exposes the same functionality as a singleton
//! (`giochi_engine_init` / `giochi_engine_run` / `giochi_engine_shutdown`); see
//! `docs/C_ABI_CONTRACT.md`.

use crate::assets::ResourceManager;
use crate::audio::AudioSystem;
use crate::core::game_loop::{GameLoop, MAX_FRAME_TIME};
use crate::core::time::{FrameClock, Time};
use crate::error::GiochiError;
use crate::input::Input;
use crate::math::Color;
use crate::platform::window::WindowConfig;
use crate::platform::{quit_signal, Platform};
use crate::render::RenderContext;
use crate::scene::{Scene, SceneManager, SceneTransition};

/// Capacity of [`EngineConfig::title`], in bytes, including the trailing NUL.
pub const ENGINE_TITLE_CAPACITY: usize = 64;

/// Default background colour: near-black with a slight blue cast.
pub const DEFAULT_CLEAR_COLOR: Color = Color::rgb(12, 14, 22);

/// Configuration passed to [`Engine::new`].
///
/// The title is a fixed-size NUL-terminated UTF-8 buffer instead of a `String` so the
/// struct keeps a stable C layout (see `docs/DECISIONS.md`, ADR-007).
///
/// # C ABI
///
/// ```c
/// typedef struct GiochiEngineConfig {
///     uint32_t width;
///     uint32_t height;
///     uint32_t target_fps;
///     uint32_t vsync;              /* 0 = off, non-zero = on */
///     char     title[64];          /* NUL-terminated UTF-8, truncated if longer */
///     uint32_t _reserved[8];
/// } GiochiEngineConfig;
/// ```
///
/// Size: 112 bytes, alignment: 4 bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineConfig {
    /// Window width in physical pixels.
    pub width: u32,
    /// Window height in physical pixels.
    pub height: u32,
    /// Target simulation rate in frames per second.
    pub target_fps: u32,
    /// `0` disables vertical sync, any other value enables it.
    pub vsync: u32,
    /// NUL-terminated UTF-8 window title.
    pub title: [u8; ENGINE_TITLE_CAPACITY],
    /// Reserved for future ABI-compatible fields. Must be zero.
    pub _reserved: [u32; 8],
}

impl EngineConfig {
    /// Creates a configuration with vsync enabled and a 60 Hz target.
    ///
    /// `title` is truncated on a UTF-8 character boundary if it does not fit.
    pub fn new(width: u32, height: u32, title: &str) -> Self {
        let mut buffer = [0u8; ENGINE_TITLE_CAPACITY];
        copy_title(title, &mut buffer);
        Self {
            width,
            height,
            target_fps: 60,
            vsync: 1,
            title: buffer,
            _reserved: [0; 8],
        }
    }

    /// Sets the window title.
    pub fn with_title(mut self, title: &str) -> Self {
        self.set_title(title);
        self
    }

    /// Copies `title` into the fixed-size buffer, truncating on a character
    /// boundary and always leaving room for the trailing NUL.
    pub fn set_title(&mut self, title: &str) {
        self.title = [0u8; ENGINE_TITLE_CAPACITY];
        copy_title(title, &mut self.title);
    }

    /// Returns the configured title as a string slice.
    ///
    /// Returns `""` if the buffer is not valid UTF-8 (for example because a C caller
    /// wrote arbitrary bytes into it).
    pub fn title_str(&self) -> &str {
        let end = self
            .title
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(self.title.len());
        std::str::from_utf8(&self.title[..end]).unwrap_or("")
    }

    /// Whether vsync was requested.
    pub fn is_vsync_enabled(&self) -> bool {
        self.vsync != 0
    }

    /// The window configuration matching these dimensions.
    pub fn window_config(&self) -> WindowConfig {
        WindowConfig::new(self.width, self.height)
    }
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self::new(1280, 720, "GIOCHI-ENGINE")
    }
}

fn copy_title(title: &str, buffer: &mut [u8; ENGINE_TITLE_CAPACITY]) {
    let bytes = title.as_bytes();
    let mut length = bytes.len().min(ENGINE_TITLE_CAPACITY - 1);
    while length > 0 && !title.is_char_boundary(length) {
        length -= 1;
    }
    buffer[..length].copy_from_slice(&bytes[..length]);
}

/// Statistics for one `run` invocation.
///
/// Returned by [`Engine::run_stats`] so a caller (and the tests) can assert that the
/// loop actually ran, at what rate, and that nothing leaked.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RunStats {
    /// Number of display frames rendered.
    pub frames: u64,
    /// Number of fixed simulation steps executed.
    pub steps: u64,
    /// Wall-clock duration of the run, in seconds.
    pub elapsed: f32,
    /// Reserved for future ABI-compatible fields. Must be zero.
    pub _reserved: [u32; 4],
}

impl RunStats {
    /// Average display frames per second over the run, or `0.0` for a zero-length
    /// run.
    pub fn fps(&self) -> f32 {
        effective_fps(self.frames, self.elapsed)
    }

    /// Average simulation steps per second over the run, or `0.0` for a zero-length
    /// run.
    pub fn step_rate(&self) -> f32 {
        effective_fps(self.steps, self.elapsed)
    }

    /// Wall-clock time per frame, in milliseconds.
    pub fn frame_time_ms(&self) -> f32 {
        if self.frames == 0 || !self.elapsed.is_finite() || self.elapsed <= 0.0 {
            0.0
        } else {
            self.elapsed * 1000.0 / self.frames as f32
        }
    }
}

fn effective_fps(count: u64, elapsed: f32) -> f32 {
    if count == 0 || !elapsed.is_finite() || elapsed <= 0.0 {
        0.0
    } else {
        count as f32 / elapsed
    }
}

/// The engine facade.
///
/// Owns every subsystem in dependency order:
///
/// ```text
/// Platform → Window → RenderContext → AudioSystem / Input → SceneManager
/// ```
///
/// Fields drop in reverse declaration order, so the canvas and the mixer go before
/// the SDL context — which is what SDL requires.
///
/// # C ABI
///
/// `Engine` is never passed across the FFI boundary: C consumers drive the single
/// engine instance through `giochi_engine_*` functions (ADR-012).
#[repr(C)]
pub struct Engine {
    platform: Platform,
    render: RenderContext,
    audio: AudioSystem,
    input: Input,
    scenes: SceneManager,
    game_loop: GameLoop,
    fixed_timestep: f32,
    clock: FrameClock,
    time: Time,
    clear_color: Color,
    stats: RunStats,
    frame_limit: Option<u64>,
    _reserved: [u32; 4],
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("size", &self.render.size())
            .field("driver", &self.platform.video_driver_name())
            .field("audio", &self.audio.is_available())
            .field("scenes", &self.scenes.depth())
            .field("stats", &self.stats)
            .finish_non_exhaustive()
    }
}

impl Engine {
    /// Initialises the platform, the window, the renderer, input and audio.
    ///
    /// A missing audio device is **not** an error: the mixer starts in silent mode
    /// (ADR-021). A missing video driver is, because there would be nothing to draw
    /// into.
    pub fn new(config: EngineConfig) -> Result<Engine, GiochiError> {
        if config.width == 0 || config.height == 0 {
            return Err(GiochiError::InvalidArgument(
                "engine width and height must be non-zero",
            ));
        }

        let platform = Platform::new()?;
        let window = platform.create_window(config.window_config(), config.title_str())?;
        let render = RenderContext::with_vsync(window, config.is_vsync_enabled())?;
        let audio = AudioSystem::new(&platform)?;
        let input = Input::new(&platform);

        // A target rate between 1 and 999 Hz becomes the fixed timestep; anything
        // else keeps the documented 60 Hz default.
        let fixed_timestep = if (1..1000).contains(&config.target_fps) {
            GameLoop::new()
                .with_fixed_timestep(1.0 / config.target_fps as f32)
                .map(|game_loop| game_loop.fixed_timestep())
                .unwrap_or(crate::core::game_loop::FIXED_TIMESTEP)
        } else {
            crate::core::game_loop::FIXED_TIMESTEP
        };
        let game_loop = GameLoop::new()
            .with_fixed_timestep(fixed_timestep)
            .unwrap_or_else(|_| GameLoop::new());

        let (actual_width, actual_height) = render.size();
        if actual_width != config.width || actual_height != config.height {
            // KMSDRM is fullscreen-only: it binds the panel's native mode and
            // ignores the requested size, so the drawable is not what was asked
            // for. Saying so once beats debugging a scene laid out for a screen
            // that does not exist.
            log::info!(
                "requested {}x{}, backend opened {}x{}",
                config.width,
                config.height,
                actual_width,
                actual_height
            );
        }

        log::info!(
            "engine {:?}: {}x{} via '{}', audio {}",
            config.title_str(),
            actual_width,
            actual_height,
            platform.video_driver_name(),
            if audio.is_available() {
                "ativo"
            } else {
                "silencioso"
            }
        );

        Ok(Engine {
            platform,
            render,
            audio,
            input,
            scenes: SceneManager::new(),
            game_loop,
            fixed_timestep,
            clock: FrameClock::new(),
            time: Time::new(),
            clear_color: DEFAULT_CLEAR_COLOR,
            stats: RunStats::default(),
            frame_limit: None,
            _reserved: [0; 4],
        })
    }

    /// Initialises an engine and immediately pushes `scene` on the stack.
    pub fn with_scene(config: EngineConfig, scene: Box<dyn Scene>) -> Result<Engine, GiochiError> {
        let mut engine = Engine::new(config)?;
        engine.scenes.push(scene);
        Ok(engine)
    }

    /// Runs `scene` until it asks to quit or the user closes the window.
    ///
    /// Replaces whatever the stack held.
    pub fn run(&mut self, scene: Box<dyn Scene>) -> Result<(), GiochiError> {
        self.scenes.replace(scene);
        self.run_current()
    }

    /// Runs exactly one iteration of the loop with the supplied frame time.
    ///
    /// This is the loop body of [`Engine::run_current`] without the sleep, and is
    /// what a host that owns its own pacing (an editor, a test harness, the C ABI's
    /// `giochi_engine_step`) uses:
    ///
    /// ```text
    /// poll events → input → fixed steps → clear → render → present → transition
    /// ```
    ///
    /// `frame_time` is the wall-clock time the caller wants the simulation to
    /// believe elapsed; it is clamped like any other frame. Nothing is slept here,
    /// so the caller decides the rate.
    ///
    /// Unlike [`Engine::run_current`] this does **not** reset [`Engine::run_stats`],
    /// so a host stepping in its own loop gets cumulative statistics. It also does
    /// not require [`Engine::is_running`] to be true; a scene that has asked to quit
    /// is left quit, and the caller decides whether to call
    /// [`Engine::reset_quit`].
    pub fn step(&mut self, frame_time: f32) -> Result<(), GiochiError> {
        let frame_time = clamp_frame_time(frame_time);
        self.run_frame(frame_time)?;
        self.stats.elapsed += frame_time;
        Ok(())
    }

    /// Runs the current scene until it quits, the window closes, or `Escape` is
    /// pressed.
    ///
    /// This is the whole loop:
    ///
    /// 1. poll SDL events → `QuitSignal` → [`GameLoop`];
    /// 2. `input.update(&events)`, before anything reads input;
    /// 3. `game_loop.advance(frame_time)` → N fixed steps, each `time.advance` +
    ///    `scene.update`;
    /// 4. `render`, `present`;
    /// 5. apply the scene's [`SceneTransition`];
    /// 6. sleep the remainder of the frame budget.
    ///
    /// An empty stack is not an error: the loop runs and draws the clear colour,
    /// which is what a "loading" state needs.
    pub fn run_current(&mut self) -> Result<(), GiochiError> {
        self.stats = RunStats::default();
        let started = std::time::Instant::now();
        let mut transitions = 0u32;

        while self.game_loop.is_running() {
            let frame_started = std::time::Instant::now();
            let frame_time = self.clock.tick();

            let outcome = self.run_frame(frame_time)?;
            match outcome {
                FrameOutcome::Continue => self.platform.delay_ms(
                    self.game_loop
                        .sleep_ms(frame_started.elapsed().as_secs_f32()),
                ),
                FrameOutcome::Transitioned => {
                    transitions += 1;
                    // A scene change must not be counted as a stall: restart the
                    // frame budget so the new scene's first frame is not starved.
                    self.clock.reset();
                }
                FrameOutcome::Stop => break,
            }

            // A scene that transitions every frame would spin forever; that is a
            // bug in the scene, but the engine reports it rather than hanging.
            if transitions > MAX_TRANSITIONS {
                return Err(GiochiError::Scene(format!(
                    "scene requested more than {MAX_TRANSITIONS} transitions in one run; \
                     a scene must return Continue between transitions"
                )));
            }
        }

        self.stats.elapsed = started.elapsed().as_secs_f32();
        log::info!(
            "run encerrado: {} frames, {} passos, {:.1} fps",
            self.stats.frames,
            self.stats.steps,
            self.stats.fps()
        );
        Ok(())
    }

    /// One iteration of the loop: poll, update, clear, render, present, transition.
    ///
    /// Shared by [`Engine::run_current`] and [`Engine::step`] so the two can never
    /// disagree about the order of a frame.
    fn run_frame(&mut self, frame_time: f32) -> Result<FrameOutcome, GiochiError> {
        let events = self.platform.poll_events();
        self.input.update(&events);
        for event in &events {
            self.game_loop.handle_quit_signal(quit_signal(event));
        }

        let steps = self.game_loop.advance(frame_time);
        for _ in 0..steps {
            self.time.advance(self.game_loop.fixed_timestep());
            let time = self.time;
            if let Some(scene) = self.scenes.current_mut() {
                scene.update(time, &self.input, &mut self.audio);
            }
            self.stats.steps += 1;
        }

        self.render.clear(self.clear_color);
        if let Some(scene) = self.scenes.current_mut() {
            scene.render(&mut self.render);
        }
        self.render.present();
        self.stats.frames += 1;

        if self
            .frame_limit
            .is_some_and(|limit| self.stats.frames >= limit)
        {
            return Ok(FrameOutcome::Stop);
        }

        let transition = self
            .scenes
            .current_mut()
            .map(|scene| scene.transition())
            .unwrap_or(SceneTransition::Continue);
        if transition == SceneTransition::Continue {
            Ok(FrameOutcome::Continue)
        } else if self.apply_transition(transition) {
            Ok(FrameOutcome::Transitioned)
        } else {
            Ok(FrameOutcome::Stop)
        }
    }

    /// Applies a scene transition. Returns `false` when the loop must stop.
    fn apply_transition(&mut self, transition: SceneTransition) -> bool {
        match transition {
            SceneTransition::Continue => true,
            SceneTransition::Quit => {
                // The transition must leave the engine in a coherent state: a caller
                // that reads `is_running()` after `run_current` has to see `false`,
                // not "the loop broke out but still claims to be running".
                self.game_loop.request_quit();
                false
            }
            SceneTransition::Pop => {
                self.scenes.pop();
                true
            }
            SceneTransition::Replace | SceneTransition::Push => {
                // The engine has no way to create a scene from a transition value;
                // a scene requests these by replacing itself through
                // `Engine::push_scene`/`replace_scene` instead.
                log::warn!(
                    "{transition:?} não pode ser aplicado pelo engine; use \
                     push_scene/replace_scene para trocar de cena"
                );
                true
            }
        }
    }

    /// Pushes `scene` on top of the stack; it becomes active on the next frame.
    pub fn push_scene(&mut self, scene: Box<dyn Scene>) {
        self.scenes.push(scene);
    }

    /// Replaces the top scene; the new one becomes active on the next frame.
    pub fn replace_scene(&mut self, scene: Box<dyn Scene>) {
        self.scenes.replace(scene);
    }

    /// Pops the top scene, returning to the one below.
    pub fn pop_scene(&mut self) -> bool {
        self.scenes.pop().is_some()
    }

    /// Asks the engine to stop after the current frame.
    pub fn request_quit(&mut self) {
        self.game_loop.request_quit();
    }

    /// Returns whether the loop is still running.
    pub fn is_running(&self) -> bool {
        self.game_loop.is_running()
    }

    /// Re-arms the loop so a later `run_current` call executes again.
    ///
    /// Used by tests and by an editor-style host that runs one scene, returns to its
    /// own UI and later runs another.
    pub fn reset_quit(&mut self) {
        self.game_loop = GameLoop::new()
            .with_fixed_timestep(self.fixed_timestep)
            .unwrap_or_else(|_| GameLoop::new());
        self.clock.reset();
    }

    /// The fixed simulation step in seconds.
    pub fn fixed_timestep(&self) -> f32 {
        self.fixed_timestep
    }

    /// Statistics for the last (or current) run.
    pub fn run_stats(&self) -> RunStats {
        self.stats
    }

    /// Stops the loop automatically after `frames` rendered frames.
    ///
    /// `None` (the default) runs until the user quits. This is what makes a demo or
    /// an example verifiable from a script without a human pressing `Escape`, and it
    /// is the mechanism behind every `--frames N` flag in the repository.
    pub fn set_frame_limit(&mut self, frames: u64) {
        self.frame_limit = Some(frames);
    }

    /// Clears the automatic frame limit.
    pub fn clear_frame_limit(&mut self) {
        self.frame_limit = None;
    }

    /// The configured frame limit, if any.
    pub fn frame_limit(&self) -> Option<u64> {
        self.frame_limit
    }

    /// The current simulation clock.
    pub fn time(&self) -> Time {
        self.time
    }

    /// Colour used to clear the screen before each frame.
    pub fn clear_color(&self) -> Color {
        self.clear_color
    }

    /// Sets the clear colour used before each frame.
    pub fn set_clear_color(&mut self, color: Color) {
        self.clear_color = color;
    }

    /// The renderer, for scenes that need more than the `render` callback gives them.
    pub fn render(&self) -> &RenderContext {
        &self.render
    }

    /// The renderer, mutably.
    pub fn render_mut(&mut self) -> &mut RenderContext {
        &mut self.render
    }

    /// The audio system.
    pub fn audio(&self) -> &AudioSystem {
        &self.audio
    }

    /// The audio system, mutably.
    pub fn audio_mut(&mut self) -> &mut AudioSystem {
        &mut self.audio
    }

    /// The input snapshot for the current frame.
    pub fn input(&self) -> &Input {
        &self.input
    }

    /// The scene stack.
    pub fn scenes(&self) -> &SceneManager {
        &self.scenes
    }

    /// The scene stack, mutably.
    pub fn scenes_mut(&mut self) -> &mut SceneManager {
        &mut self.scenes
    }

    /// The resource cache used by the renderer.
    pub fn resources(&self) -> &ResourceManager {
        self.render.resources()
    }

    /// The platform layer.
    pub fn platform(&self) -> &Platform {
        &self.platform
    }

    /// The fixed-timestep policy.
    pub fn game_loop(&self) -> &GameLoop {
        &self.game_loop
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        // Explicit teardown in dependency order; no RAII promise is made to the
        // caller, but the engine itself must not leak handles.
        self.scenes.clear();
        self.audio.clear();
        self.render.clear_resource_cache();
        self.render.textures().clear();
    }
}

/// Upper bound on scene transitions per run before the engine reports a bug.
const MAX_TRANSITIONS: u32 = 10_000;

/// What [`Engine::run_frame`] decided about the loop.
enum FrameOutcome {
    /// Nothing happened: keep going.
    Continue,
    /// The scene changed the stack; the frame budget restarts.
    Transitioned,
    /// The loop must stop (quit, empty transition target, or frame limit).
    Stop,
}

/// One fixed simulation step, exposed for tests that assert on `MAX_FRAME_TIME`.
pub fn clamp_frame_time(frame_time: f32) -> f32 {
    if frame_time.is_finite() && frame_time > 0.0 {
        frame_time.min(MAX_FRAME_TIME)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_keeps_dimensions_and_enables_vsync() {
        let config = EngineConfig::new(320, 240, "game");

        assert_eq!(config.width, 320);
        assert_eq!(config.height, 240);
        assert_eq!(config.target_fps, 60);
        assert_eq!(config.vsync, 1);
        assert!(config.is_vsync_enabled());
        assert_eq!(config.title_str(), "game");
        assert_eq!(config._reserved, [0; 8]);
    }

    #[test]
    fn long_titles_are_truncated_and_stay_valid_utf8() {
        let title = "á".repeat(100);
        let config = EngineConfig::new(1, 1, &title);

        assert_eq!(config.title[ENGINE_TITLE_CAPACITY - 1], 0);
        assert!(config.title_str().len() < ENGINE_TITLE_CAPACITY - 1);
        assert!(config.title_str().chars().all(|c| c == 'á'));
    }

    #[test]
    fn titles_with_multibyte_chars_cut_cleanly() {
        // 32 'á' = 64 bytes: one over the buffer, so it must be cut to 31.
        let config = EngineConfig::new(1, 1, &"á".repeat(32));

        assert_eq!(config.title_str().chars().count(), 31);
    }

    #[test]
    fn set_title_replaces_previous_value() {
        let mut config = EngineConfig::default();
        config.set_title("novo");

        assert_eq!(config.title_str(), "novo");
    }

    #[test]
    fn invalid_utf8_in_the_title_reports_an_empty_string() {
        let mut config = EngineConfig::default();
        config.title[0] = 0xFF;
        config.title[1] = 0xFE;

        assert_eq!(config.title_str(), "");
    }

    #[test]
    fn with_title_is_chainable() {
        let config = EngineConfig::default().with_title("chain");

        assert_eq!(config.title_str(), "chain");
    }

    #[test]
    fn window_config_matches_the_engine_dimensions() {
        let config = EngineConfig::new(400, 300, "x");
        let window = config.window_config();

        assert_eq!((window.width, window.height), (400, 300));
        assert_eq!(window._reserved, [0; 8]);
    }

    #[test]
    fn a_zero_target_fps_is_accepted_as_uncapped() {
        // The engine treats 0 as "keep the default timestep".
        let config = EngineConfig {
            target_fps: 0,
            ..EngineConfig::default()
        };

        assert_eq!(config.target_fps, 0);
    }

    #[test]
    fn run_stats_report_zero_rates_for_an_empty_run() {
        let stats = RunStats::default();

        assert_eq!(stats.fps(), 0.0);
        assert_eq!(stats.step_rate(), 0.0);
        assert_eq!(stats.frame_time_ms(), 0.0);
        assert_eq!(stats._reserved, [0; 4]);
    }

    #[test]
    fn run_stats_compute_rates_from_the_totals() {
        let stats = RunStats {
            frames: 120,
            steps: 120,
            elapsed: 2.0,
            _reserved: [0; 4],
        };

        assert_eq!(stats.fps(), 60.0);
        assert_eq!(stats.step_rate(), 60.0);
        assert!((stats.frame_time_ms() - 16.666_668).abs() < 0.01);
    }

    #[test]
    fn run_stats_ignore_non_finite_elapsed() {
        let stats = RunStats {
            frames: 10,
            steps: 10,
            elapsed: f32::NAN,
            _reserved: [0; 4],
        };

        assert_eq!(stats.fps(), 0.0);
        assert_eq!(stats.frame_time_ms(), 0.0);
    }

    #[test]
    fn frame_time_clamping_matches_the_loop_policy() {
        assert_eq!(clamp_frame_time(-1.0), 0.0);
        assert_eq!(clamp_frame_time(f32::NAN), 0.0);
        assert_eq!(clamp_frame_time(10.0), MAX_FRAME_TIME);
        assert_eq!(clamp_frame_time(0.01), 0.01);
    }

    #[test]
    fn the_default_clear_color_is_opaque() {
        assert_eq!(DEFAULT_CLEAR_COLOR.a, 255);
    }

    // The default clear colour must stay dark; a bright one would wash out sprites.
    const _: () = assert!(DEFAULT_CLEAR_COLOR.r < 32);
    const _: () = assert!(DEFAULT_CLEAR_COLOR.g < 32);
    const _: () = assert!(DEFAULT_CLEAR_COLOR.b < 40);

    #[test]
    fn a_zero_sized_engine_is_rejected_before_touching_sdl() {
        let error = Engine::new(EngineConfig::new(0, 720, "x")).expect_err("must fail");

        assert_eq!(error.code(), crate::error::codes::INVALID_ARGUMENT);
    }
}
