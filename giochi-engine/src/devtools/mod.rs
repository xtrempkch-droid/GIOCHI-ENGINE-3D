//! Developer tools: an in-game console, debug geometry and an expanded overlay.
//!
//! # What this is for
//!
//! A game that only logs to a terminal cannot be inspected while it runs — and on a
//! handheld there is no terminal. Three tools, one feature flag:
//!
//! | Tool | Key | Type |
//! |---|---|---|
//! | console | `F2` | [`Console`] + [`CommandRegistry`] |
//! | debug geometry | `F3` | [`DebugDraw`] |
//! | expanded overlay | `F1` | [`overlay_extra`] appended to the phase-15 panel |
//!
//! # The cost when it is closed
//!
//! The console is the only thing here that needs egui, and egui only runs while the
//! console is open. Closed, a frame pays: one `Option` check, one key comparison for `F2`
//! and `F3`, and the debug draw buffer's `clear`. That is deliberate — R2's budget is
//! "disabled costs nothing", and "open" is a debug session where 2 % does not matter.
//!
//! # Commands
//!
//! Ten built-ins, listed by `help`. A game adds its own with
//! [`CommandRegistry::register`], closing over whatever state the command needs:
//!
//! ```no_run
//! # use std::cell::RefCell;
//! # use std::rc::Rc;
//! # let mut devtools = giochi_engine::devtools::DevTools::new();
//! let ammo = Rc::new(RefCell::new(30));
//! let refill = ammo.clone();
//! devtools.commands_mut().register("ammo", "shows or sets the ammo count", Box::new(
//!     move |args| {
//!         let mut ammo = refill.borrow_mut();
//!         if let Ok(value) = args.trim().parse::<i32>() {
//!             *ammo = value;
//!         }
//!         format!("ammo: {}", *ammo)
//!     },
//! ));
//! ```
//!
//! # What the engine does *not* decide for you
//!
//! `god`, `noclip` and `spawn` are **game** concepts. The console offers the switch and
//! the queue; the game reads them ([`DevTools::flags`], [`DevTools::take_spawn`]). An
//! engine that invented behaviour for them would be an engine that forces a game design.
//!
//! # C ABI
//!
//! Everything here is Rust-only until phase 24, which exposes `giochi_console_execute`,
//! `giochi_debug_draw_*` and `giochi_hot_reload_*` over the same state (R3).

pub mod commands;
pub mod console;
pub mod debug_draw;
pub mod overlay_extra;

use sdl2::event::Event;
use sdl2::keyboard::Keycode;

use crate::perf::{FrameStats, FrameTimeSummary};
use crate::render::RenderContext;
use crate::ui::egui_integration::translate_event;

pub use commands::{Builtin, CommandFn, CommandRegistry};
pub use console::{Console, ConsoleAction, ConsoleLine, LineKind, MAX_HISTORY, MAX_OUTPUT};
pub use debug_draw::DebugDraw;
pub use overlay_extra::OverlayExtra;

/// Largest multiplier `time_scale` accepts.
///
/// Beyond 8× a fixed 60 Hz step turns into 480 Hz of simulation per second, which is how a
/// debug command becomes a hang on a Cortex-A35. Values outside `[0, 8]` are clamped, and
/// `0` pauses — which is what a debugger's "freeze" is.
pub const MAX_TIME_SCALE: f32 = 8.0;

/// A pending `spawn` request, drained by the game.
#[repr(C)]
#[derive(Clone, PartialEq, Eq)]
pub struct SpawnRequest {
    kind: String,
    count: u32,
    _reserved: [u32; 4],
}

impl SpawnRequest {
    /// What to spawn, as the user typed it (`spawn cube 5` → `"cube"`).
    pub fn kind(&self) -> &str {
        &self.kind
    }

    /// How many, at least 1.
    pub fn count(&self) -> u32 {
        self.count
    }
}

impl std::fmt::Debug for SpawnRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "spawn {} x{}", self.kind, self.count)
    }
}

/// Game-facing switches the console toggles and the game reads.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DevFlags {
    /// `god` — the game decides what it means.
    pub god: bool,
    /// `noclip` — the game decides what it means.
    pub noclip: bool,
    /// Reserved for future ABI-compatible fields. Must be zero.
    pub _reserved: [u32; 4],
}

/// The devtools state: console, commands and debug geometry.
///
/// The engine owns one behind the `devtools` feature. A host that drives a
/// [`RenderContext`] itself can own one too and call [`DevTools::handle_event`],
/// [`DevTools::draw`] and [`DevTools::expanded_overlay`] in its own loop.
///
/// # C ABI
///
/// Not exposed; phase 24 reaches the console through `giochi_console_execute`.
#[repr(C)]
pub struct DevTools {
    console: Console,
    commands: CommandRegistry,
    debug_draw: DebugDraw,
    /// Events collected for the next egui frame.
    ///
    /// The engine owns the egui backend (the UI layer needs it too); the devtools own the
    /// events, because they are what the console's key handling produces.
    pending_events: Vec<egui::Event>,
    /// The expanded overlay's numbers.
    extra: OverlayExtra,
    /// `time_scale` multiplier the engine applies.
    time_scale: f32,
    /// Set by `reload_shaders`; the engine clears it.
    shader_reload_requested: bool,
    /// Set by `spawn`; the game clears it.
    spawn_request: Option<SpawnRequest>,
    /// Game switches.
    flags: DevFlags,
    /// Last frame's stats, for `fps` and `dump_frame_stats`.
    stats: FrameStats,
    summary: FrameTimeSummary,
    /// Last frame's delta, fed to egui.
    frame_time: f32,
    /// Whether the text field still needs focus on the next frame.
    needs_focus: bool,
    _reserved: [u32; 4],
}

impl std::fmt::Debug for DevTools {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DevTools")
            .field("console_open", &self.console.is_open())
            .field("commands", &self.commands.len())
            .field("debug_draw", &self.debug_draw.is_visible())
            .field("time_scale", &self.time_scale)
            .finish_non_exhaustive()
    }
}

impl Default for DevTools {
    fn default() -> Self {
        Self::new()
    }
}

impl DevTools {
    /// Creates the tools with the built-in command table.
    pub fn new() -> Self {
        let mut extra = OverlayExtra::new();
        let commands = CommandRegistry::with_builtins();
        extra.commands = commands.len() as u32;
        Self {
            console: Console::new(),
            commands,
            debug_draw: DebugDraw::new(),
            pending_events: Vec::new(),
            extra,
            time_scale: 1.0,
            shader_reload_requested: false,
            spawn_request: None,
            flags: DevFlags::default(),
            stats: FrameStats::default(),
            summary: FrameTimeSummary::default(),
            frame_time: 0.0,
            needs_focus: false,
            _reserved: [0; 4],
        }
    }

    // ---- the pieces ------------------------------------------------------

    /// The console.
    pub fn console(&self) -> &Console {
        &self.console
    }

    /// The console, mutably.
    pub fn console_mut(&mut self) -> &mut Console {
        &mut self.console
    }

    /// The command table.
    pub fn commands(&self) -> &CommandRegistry {
        &self.commands
    }

    /// The command table, mutably — where a game registers its own commands.
    pub fn commands_mut(&mut self) -> &mut CommandRegistry {
        &mut self.commands
    }

    /// The debug geometry buffer.
    pub fn debug_draw(&self) -> &DebugDraw {
        &self.debug_draw
    }

    /// The debug geometry buffer, mutably.
    pub fn debug_draw_mut(&mut self) -> &mut DebugDraw {
        &mut self.debug_draw
    }

    /// The game-facing switches.
    pub fn flags(&self) -> DevFlags {
        self.flags
    }

    /// The current simulation multiplier.
    pub fn time_scale(&self) -> f32 {
        self.time_scale
    }

    /// Sets the simulation multiplier, clamped to `[0, `[`MAX_TIME_SCALE`]`]`.
    pub fn set_time_scale(&mut self, scale: f32) {
        self.time_scale = if scale.is_finite() {
            scale.clamp(0.0, MAX_TIME_SCALE)
        } else {
            1.0
        };
        self.extra.time_scale = self.time_scale;
    }

    /// Whether `reload_shaders` was run since the last [`DevTools::take_shader_reload_request`].
    pub fn take_shader_reload_request(&mut self) -> bool {
        std::mem::take(&mut self.shader_reload_requested)
    }

    /// The pending spawn, if the console queued one.
    pub fn take_spawn(&mut self) -> Option<SpawnRequest> {
        self.spawn_request.take()
    }

    /// Tells the console how many entities are alive, for `entities`.
    pub fn set_entity_count(&mut self, count: u32) {
        self.extra.entities = count;
    }

    /// Tells the expanded overlay whether hot reload is on and how much it has done.
    pub fn set_hot_reload_state(&mut self, enabled: bool, applied: u64) {
        self.extra.hot_reload = enabled;
        self.extra.reloads_applied = applied;
    }

    /// Records the frame's statistics, for the `fps`/`dump_frame_stats` commands and for
    /// the expanded overlay.
    pub fn set_frame_stats(&mut self, stats: FrameStats, summary: FrameTimeSummary, dt: f32) {
        self.stats = stats;
        self.summary = summary;
        self.frame_time = dt.max(0.0);
        // Copied here rather than read in `overlay_lines`, which must stay allocation-free
        // of engine state so the panel can be built lazily.
        self.extra.draw_calls = stats.draw_calls;
        self.extra.sprites_drawn = stats.sprites_drawn;
        self.extra.meshes_drawn = stats.meshes_drawn;
    }

    /// Records just the frame delta, for the egui clock.
    pub fn set_frame_time(&mut self, dt: f32) {
        self.frame_time = dt.max(0.0);
    }

    /// The expanded overlay section.
    ///
    /// Allocates, so the engine calls it only when the panel is about to refresh (1 Hz),
    /// not once per frame.
    pub fn overlay_lines(&self) -> Vec<String> {
        overlay_extra::lines(&self.extra)
    }

    /// The values behind the expanded overlay, for a host that renders its own panel.
    pub fn overlay_extra(&self) -> OverlayExtra {
        self.extra
    }

    // ---- input -----------------------------------------------------------

    /// Feeds one SDL event. Returns `true` when the devtools consumed it.
    ///
    /// A consumed event must **not** reach [`crate::Input`]: typing `w` in the console
    /// must not walk the player forward. The engine filters on this return value.
    pub fn handle_event(&mut self, event: &Event) -> bool {
        if let Event::KeyDown {
            keycode: Some(key), ..
        } = event
        {
            match *key {
                Keycode::F2 => {
                    let open = self.console.toggle();
                    if open {
                        self.needs_focus = true;
                        self.console.push_line(
                            LineKind::Notice,
                            format!("console open; {} commands", self.commands.len()),
                        );
                    }
                    return true;
                }
                Keycode::F3 => {
                    let visible = self.debug_draw.toggle();
                    log::info!("debug draw {}", if visible { "on" } else { "off" });
                    return true;
                }
                _ => {}
            }
        }

        if !self.console.is_open() {
            return false;
        }

        // Keys the console owns: submit, complete, history, close.
        if let Event::KeyDown {
            keycode: Some(key), ..
        } = event
        {
            let candidates = self.commands.complete(self.console.completion_prefix());
            match self.console.handle_key(*key, &candidates) {
                ConsoleAction::Submit => {
                    if let Some(line) = self.console.submit() {
                        self.execute(&line);
                    }
                    return true;
                }
                ConsoleAction::Close => {
                    self.console.close();
                    return true;
                }
                ConsoleAction::Completed | ConsoleAction::Consumed => return true,
                ConsoleAction::Ignored => {}
            }
        }

        // Everything else that egui can use goes to egui, which decides whether it wants
        // it (a text field does, a pointer move does not).
        let wants_keyboard = self.console.is_open();
        let (translated, consumed) = translate_event(event, wants_keyboard);
        if let Some(translated) = translated {
            self.pending_events.push(translated);
        }
        consumed
    }

    // ---- commands --------------------------------------------------------

    /// Runs one console line and returns what it printed.
    ///
    /// Also appends the output to the scrollback, so a test and the panel see the same
    /// thing. Every line goes to `log::info!` as well: on a console without a screen, the
    /// log is the only place the output survives.
    pub fn execute(&mut self, line: &str) -> Vec<String> {
        let line = line.trim();
        if line.is_empty() {
            return Vec::new();
        }
        let (name, args) = match line.split_once(char::is_whitespace) {
            Some((name, args)) => (name, args.trim()),
            None => (line, ""),
        };

        if !self.commands.contains(name) {
            let message = format!("unknown command '{name}'; try 'help'");
            self.console.push_line(LineKind::Error, &message);
            log::info!("console: {message}");
            return vec![message];
        }

        // Read the built-in first: `run_builtin` takes `&mut self`, and a borrow of
        // `self.commands` cannot outlive it.
        let builtin = self.commands.builtin_for(name);
        let output = self.run_builtin(name, builtin, args);
        if !output.is_empty() {
            for line in &output {
                log::info!("console: {line}");
            }
            self.console.push_line(LineKind::Output, output.join("\n"));
        }
        output
    }

    /// Executes one built-in, or the game handler when there is no built-in.
    fn run_builtin(&mut self, name: &str, builtin: Builtin, args: &str) -> Vec<String> {
        match builtin {
            Builtin::None => match self.commands.run_handler(name, args) {
                Some(text) => text
                    .lines()
                    .filter(|line| !line.trim().is_empty())
                    .map(str::to_owned)
                    .collect(),
                // Registered but with no handler: a built-in that lost its `Builtin`
                // tag, which can only happen if a game replaced it with `None`.
                None => vec![format!("'{name}' has no handler")],
            },
            Builtin::Help => self.commands.help_lines(),
            Builtin::Clear => {
                self.console.clear();
                Vec::new()
            }
            Builtin::Fps => vec![
                format!("fps {:.1}", self.summary.fps),
                format!("frame {:.2} ms", self.stats.frame_time_ms()),
                format!("p99 {:.2} ms", self.summary.p99_ms()),
                format!(
                    "draw {}  sprites {}",
                    self.stats.draw_calls, self.stats.sprites_drawn
                ),
            ],
            Builtin::Entities => vec![format!("entities {}", self.extra.entities)],
            Builtin::TimeScale => self.command_time_scale(args),
            Builtin::God => {
                self.flags.god = !self.flags.god;
                self.extra.god_mode = self.flags.god;
                vec![format!("god {}", if self.flags.god { "on" } else { "off" })]
            }
            Builtin::Noclip => {
                self.flags.noclip = !self.flags.noclip;
                vec![format!(
                    "noclip {}",
                    if self.flags.noclip { "on" } else { "off" }
                )]
            }
            Builtin::Spawn => self.command_spawn(args),
            Builtin::ReloadShaders => {
                self.shader_reload_requested = true;
                vec!["shader reload requested".to_owned()]
            }
            Builtin::DumpFrameStats => vec![
                format!(
                    "fps {:.1}  frame {:.2} ms",
                    self.summary.fps,
                    self.stats.frame_time_ms()
                ),
                format!(
                    "p99 {:.2} ms  max {:.2} ms",
                    self.summary.p99_ms(),
                    self.summary.max_ms()
                ),
                format!(
                    "update {:.2} ms  render2d {:.2} ms  render3d {:.2} ms  audio {:.2} ms",
                    self.stats.update_time_ms(),
                    self.stats.render2d_time_ms(),
                    self.stats.render3d_time_ms(),
                    self.stats.audio_time_ms()
                ),
                format!(
                    "draw {}  sprites {}  meshes {}  verts {}",
                    self.stats.draw_calls,
                    self.stats.sprites_drawn,
                    self.stats.meshes_drawn,
                    self.stats.vertices_uploaded
                ),
            ],
        }
    }

    /// `time_scale [factor]`: with no argument it reports, with one it sets.
    fn command_time_scale(&mut self, args: &str) -> Vec<String> {
        if args.is_empty() {
            return vec![format!("time_scale {:.2}", self.time_scale)];
        }
        match args.parse::<f32>() {
            Ok(scale) if scale.is_finite() => {
                self.set_time_scale(scale);
                vec![format!("time_scale {:.2}", self.time_scale)]
            }
            Ok(_) => vec!["time_scale needs a finite number".to_owned()],
            Err(_) => vec![format!("'{args}' is not a number")],
        }
    }

    /// `spawn <kind> [count]`: queues the request for the game.
    fn command_spawn(&mut self, args: &str) -> Vec<String> {
        let mut parts = args.split_whitespace();
        let Some(kind) = parts.next() else {
            return vec!["usage: spawn <kind> [count]".to_owned()];
        };
        let count = match parts.next() {
            Some(text) => match text.parse::<u32>() {
                Ok(count) if count > 0 => count,
                _ => return vec![format!("'{text}' is not a positive count")],
            },
            None => 1,
        };
        self.spawn_request = Some(SpawnRequest {
            kind: kind.to_owned(),
            count,
            _reserved: [0; 4],
        });
        vec![format!("spawn queued: {kind} x{count}")]
    }
}

/// Drawing: debug geometry always, egui only while something needs it.
impl DevTools {
    /// Refreshes the figures the overlay shows and draws the buffered debug geometry.
    ///
    /// Call it **after** the scene has drawn and **before** `present`, and after the
    /// profiler has taken its snapshot so the tools do not inflate the numbers they report.
    pub fn draw(&mut self, render: &mut RenderContext) {
        // Two cheap calls, unconditional so the overlay is correct even when the console
        // has never been opened.
        self.extra.textures = render.texture_count() as u32;
        self.extra.texture_vram_bytes = render.texture_vram_bytes();
        self.extra.commands = self.commands.len() as u32;
        self.extra.console_open = self.console.is_open();
        self.extra.debug_draw = self.debug_draw.is_visible();
        self.extra.time_scale = self.time_scale;
        self.extra.god_mode = self.flags.god;

        // The debug geometry is the 2D renderer's own primitives, so it costs nothing
        // when nothing is buffered and needs no egui.
        self.debug_draw.render(render);
    }

    /// Takes the events collected since the last frame, for the engine's egui pass.
    ///
    /// Draining here rather than in the UI layer keeps the ownership clear: the console's
    /// key handling produces these, and whoever runs egui consumes them.
    pub fn take_pending_events(&mut self) -> Vec<egui::Event> {
        std::mem::take(&mut self.pending_events)
    }

    /// Throws away queued events. Called when nothing will run egui this frame.
    pub fn clear_pending_events(&mut self) {
        self.pending_events.clear();
    }

    /// The frame delta the console's egui clock advances by.
    pub fn frame_time(&self) -> f32 {
        self.frame_time
    }

    /// Whether the console needs the egui pass this frame.
    pub fn wants_ui(&self) -> bool {
        self.console.is_open()
    }

    /// Draws the console into an existing egui frame.
    ///
    /// `screen` is the drawable size in pixels; the panel is sized as a fraction of it so
    /// the game stays visible behind the console.
    pub fn draw_console_panel(&mut self, ctx: &egui::Context, screen: (u32, u32)) {
        if !self.console.is_open() {
            return;
        }
        self.draw_console(ctx, screen);
    }

    /// Builds and draws the console panel.
    fn draw_console(&mut self, ctx: &egui::Context, screen: (u32, u32)) {
        // Owned copies: the closure cannot hold a borrow of `self.console` while it also
        // needs `&mut` on the input string.
        let lines: Vec<(String, LineKind)> = self
            .console
            .output()
            .iter()
            .map(|line| (line.text().to_owned(), line.kind()))
            .collect();
        let hint = self.console.hint().map(str::to_owned);

        // The panel must not cover the whole screen: a console that hides the game is a
        // terminal, and the point of having it in-game is to see both.
        let max_height = (screen.1 as f32 * 0.6).max(120.0);

        egui::TopBottomPanel::bottom("giochi_devtools_console")
            .resizable(true)
            .default_height((screen.1 as f32 * 0.35).clamp(120.0, max_height))
            .max_height(max_height)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("CONSOLE").strong());
                    ui.label(format!(
                        "{} commands  F2 close  Tab complete",
                        self.commands.len()
                    ));
                });
                ui.separator();

                let input_height = ui.spacing().interact_size.y + 6.0;
                egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .auto_shrink([false, false])
                    .max_height((ui.available_height() - input_height * 2.0).max(40.0))
                    .show(ui, |ui| {
                        for (text, kind) in &lines {
                            let colour = match kind {
                                LineKind::Input => egui::Color32::from_rgb(150, 190, 255),
                                LineKind::Output => egui::Color32::from_rgb(210, 220, 235),
                                LineKind::Error => egui::Color32::from_rgb(255, 120, 120),
                                LineKind::Notice => egui::Color32::from_rgb(150, 210, 150),
                            };
                            ui.colored_label(colour, text);
                        }
                    });

                if let Some(hint) = &hint {
                    ui.label(
                        egui::RichText::new(hint)
                            .monospace()
                            .color(egui::Color32::from_rgb(170, 170, 140)),
                    );
                }

                let response = ui.add(
                    egui::TextEdit::singleline(self.console.input_mut())
                        .hint_text("command…")
                        .desired_width(f32::INFINITY)
                        .font(egui::TextStyle::Monospace),
                );
                if self.needs_focus {
                    response.request_focus();
                    self.needs_focus = false;
                }

                // egui publishes a "copy this" request through the platform output,
                // which `EguiBackend::paint` receives. It is logged there rather than
                // wired to SDL's clipboard: see `docs/DEVTOOLS.md` §7.
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: Keycode) -> Event {
        Event::KeyDown {
            timestamp: 0,
            window_id: 0,
            keycode: Some(code),
            scancode: None,
            keymod: sdl2::keyboard::Mod::NOMOD,
            repeat: false,
        }
    }

    #[test]
    fn a_new_devtools_is_closed_and_has_the_builtins() {
        let devtools = DevTools::new();
        assert!(!devtools.console().is_open());
        assert!(!devtools.debug_draw().is_visible());
        assert_eq!(devtools.commands().len(), 10);
        assert_eq!(devtools.time_scale(), 1.0);
        assert_eq!(devtools.overlay_extra().commands, 10);
    }

    #[test]
    fn f2_opens_and_closes_the_console_and_is_consumed() {
        let mut devtools = DevTools::new();
        assert!(devtools.handle_event(&key(Keycode::F2)));
        assert!(devtools.console().is_open());
        assert!(devtools.handle_event(&key(Keycode::F2)));
        assert!(!devtools.console().is_open());
    }

    #[test]
    fn f3_toggles_debug_draw_and_is_consumed_even_when_closed() {
        let mut devtools = DevTools::new();
        assert!(devtools.handle_event(&key(Keycode::F3)));
        assert!(devtools.debug_draw().is_visible());
        assert!(devtools.handle_event(&key(Keycode::F3)));
        assert!(!devtools.debug_draw().is_visible());
    }

    #[test]
    fn a_key_is_not_consumed_while_the_console_is_closed() {
        let mut devtools = DevTools::new();
        assert!(!devtools.handle_event(&key(Keycode::W)));
    }

    #[test]
    fn a_key_is_consumed_while_the_console_is_open() {
        let mut devtools = DevTools::new();
        devtools.handle_event(&key(Keycode::F2));
        assert!(
            devtools.handle_event(&key(Keycode::W)),
            "typing must not reach the game"
        );
    }

    #[test]
    fn enter_runs_the_line_and_echoes_it() {
        let mut devtools = DevTools::new();
        devtools.handle_event(&key(Keycode::F2));
        devtools.console_mut().set_input("entities");
        devtools.set_entity_count(7);
        assert!(devtools.handle_event(&key(Keycode::Return)));

        let text = devtools.console().output_text().join("\n");
        assert!(text.contains("> entities"), "{text}");
        assert!(text.contains("entities 7"), "{text}");
        assert_eq!(devtools.console().input(), "", "the line is cleared");
    }

    #[test]
    fn escape_closes_the_console() {
        let mut devtools = DevTools::new();
        devtools.handle_event(&key(Keycode::F2));
        assert!(devtools.handle_event(&key(Keycode::Escape)));
        assert!(!devtools.console().is_open());
    }

    #[test]
    fn tab_completes_from_the_registry() {
        let mut devtools = DevTools::new();
        devtools.handle_event(&key(Keycode::F2));
        devtools.console_mut().set_input("time");
        assert!(devtools.handle_event(&key(Keycode::Tab)));
        assert_eq!(devtools.console().input(), "time_scale ");
    }

    #[test]
    fn help_lists_every_command() {
        let mut devtools = DevTools::new();
        let output = devtools.execute("help");
        assert_eq!(output.len(), devtools.commands().len());
        assert!(output.iter().any(|line| line.starts_with("fps")));
        assert!(output.iter().any(|line| line.starts_with("spawn")));
    }

    #[test]
    fn an_unknown_command_is_an_error_and_not_a_panic() {
        let mut devtools = DevTools::new();
        let output = devtools.execute("nope");
        assert_eq!(output.len(), 1);
        assert!(output[0].contains("unknown command"));
        assert_eq!(
            devtools.console().output().last().map(|l| l.kind()),
            Some(LineKind::Error)
        );
    }

    #[test]
    fn a_blank_line_runs_nothing() {
        let mut devtools = DevTools::new();
        assert!(devtools.execute("   ").is_empty());
    }

    #[test]
    fn clear_empties_the_scrollback() {
        let mut devtools = DevTools::new();
        devtools.execute("help");
        assert!(devtools.console().output_len() > 1);
        devtools.execute("clear");
        assert_eq!(devtools.console().output_len(), 0);
    }

    #[test]
    fn time_scale_reports_sets_and_clamps() {
        let mut devtools = DevTools::new();
        assert_eq!(devtools.execute("time_scale")[0], "time_scale 1.00");

        assert_eq!(devtools.execute("time_scale 0.5")[0], "time_scale 0.50");
        assert_eq!(devtools.time_scale(), 0.5);

        // Above the ceiling, negative, and not a number.
        devtools.execute("time_scale 100");
        assert_eq!(devtools.time_scale(), MAX_TIME_SCALE);
        devtools.execute("time_scale -3");
        assert_eq!(devtools.time_scale(), 0.0);
        assert!(devtools.execute("time_scale fast")[0].contains("not a number"));
        // `inf` parses as a float but is refused: a debug command that accepts it turns
        // one fixed step into an infinite one. The previous value is kept.
        let before = devtools.time_scale();
        assert!(devtools.execute("time_scale inf")[0].contains("finite"));
        assert_eq!(devtools.time_scale(), before);
    }

    #[test]
    fn god_and_noclip_toggle_the_flags_and_the_overlay() {
        let mut devtools = DevTools::new();
        devtools.execute("god");
        assert!(devtools.flags().god);
        assert!(devtools.overlay_extra().god_mode);
        devtools.execute("god");
        assert!(!devtools.flags().god);

        devtools.execute("noclip");
        assert!(devtools.flags().noclip);
    }

    #[test]
    fn spawn_queues_a_request_the_game_drains_once() {
        let mut devtools = DevTools::new();
        let output = devtools.execute("spawn cube 5");
        assert!(output[0].contains("cube x5"));
        let request = devtools.take_spawn().expect("queued");
        assert_eq!(request.kind(), "cube");
        assert_eq!(request.count(), 5);
        assert!(devtools.take_spawn().is_none(), "the request is taken once");
    }

    #[test]
    fn spawn_defaults_to_one_and_rejects_a_bad_count() {
        let mut devtools = DevTools::new();
        devtools.execute("spawn cube");
        assert_eq!(devtools.take_spawn().map(|r| r.count()), Some(1));

        assert!(devtools.execute("spawn cube 0")[0].contains("not a positive count"));
        assert!(devtools.take_spawn().is_none());
        assert!(devtools.execute("spawn")[0].contains("usage"));
    }

    #[test]
    fn reload_shaders_only_records_a_request() {
        let mut devtools = DevTools::new();
        devtools.execute("reload_shaders");
        assert!(devtools.take_shader_reload_request());
        assert!(!devtools.take_shader_reload_request());
    }

    #[test]
    fn dump_frame_stats_reports_the_profiler_numbers() {
        let mut devtools = DevTools::new();
        let stats = FrameStats {
            draw_calls: 12,
            sprites_drawn: 9,
            ..FrameStats::default()
        };
        devtools.set_frame_stats(stats, FrameTimeSummary::default(), 0.016);
        let output = devtools.execute("dump_frame_stats");
        assert!(output.iter().any(|line| line.contains("draw 12")));
    }

    #[test]
    fn fps_reports_the_window_summary() {
        let mut devtools = DevTools::new();
        devtools.set_frame_stats(FrameStats::default(), FrameTimeSummary::default(), 0.016);
        let output = devtools.execute("fps");
        assert!(output[0].starts_with("fps "));
        assert_eq!(output.len(), 4);
    }

    #[test]
    fn a_game_command_runs_and_its_output_reaches_the_scrollback() {
        let mut devtools = DevTools::new();
        devtools.commands_mut().register(
            "heal",
            "restores health",
            Box::new(|args| format!("healed by {args}")),
        );
        let output = devtools.execute("heal 3");
        assert_eq!(output, vec!["healed by 3".to_owned()]);
        assert!(devtools
            .console()
            .output_text()
            .iter()
            .any(|line| line == "healed by 3"));
    }

    #[test]
    fn a_game_command_that_prints_nothing_adds_no_line() {
        let mut devtools = DevTools::new();
        devtools
            .commands_mut()
            .register("mute", "silent", Box::new(|_| String::new()));
        let before = devtools.console().output_len();
        assert!(devtools.execute("mute").is_empty());
        assert_eq!(devtools.console().output_len(), before);
    }

    #[test]
    fn the_overlay_lines_use_the_recorded_state() {
        let mut devtools = DevTools::new();
        devtools.set_entity_count(5);
        devtools.set_hot_reload_state(true, 3);
        devtools.execute("god");
        let lines = devtools.overlay_lines().join("\n");
        assert!(lines.contains("ENTITIES 5"), "{lines}");
        assert!(lines.contains("HOT_RELOAD ON RELOADS 3"), "{lines}");
        assert!(lines.contains("GOD ON"), "{lines}");
    }

    #[test]
    fn the_time_scale_accepts_zero_to_pause() {
        let mut devtools = DevTools::new();
        devtools.set_time_scale(0.0);
        assert_eq!(devtools.time_scale(), 0.0);
        devtools.set_time_scale(f32::NAN);
        assert_eq!(devtools.time_scale(), 1.0);
    }
}
