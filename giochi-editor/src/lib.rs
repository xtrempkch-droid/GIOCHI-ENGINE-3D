//! # giochi-editor
//!
//! The visual scene editor for GIOCHI-ENGINE — Salto B, phases 30–40.
//!
//! Phase 30 is the bootstrap: a resizable 1600×900 window with the editor's layout
//! (menu bar, Hierarchy, Inspector, Asset Browser and a viewport), a dark theme, and
//! the in-game console on `F2` reusing the engine's devtools. No scene is opened yet;
//! the panels say what phase fills them.
//!
//! ```text
//! cargo run -p giochi-editor                     # opens the editor
//! cargo run -p giochi-editor -- --frames 120     # renders 120 frames and exits
//! cargo run -p giochi-editor -- --scene track.scene
//! ```
//!
//! # Layering
//!
//! The editor **consumes** the engine. The one structural need it had — running an
//! egui frame from a host-owned loop — was met by making the engine's existing
//! `EguiBackend` public (ADR-062), not by forking the painter. Everything that is
//! editor logic lives in this crate.
//!
//! # Loop
//!
//! The editor owns its loop instead of calling [`giochi_engine::Engine::run_current`]:
//! an editor has no fixed timestep and no scene stack, and it needs to decide what
//! "the scene" is. The engine still owns the window, the renderer, input and the
//! devtools, so the two agree on where pixels go.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::time::Instant;

use giochi_engine::ui::{translate_event, EguiBackend};
use giochi_engine::{Engine, EngineConfig, GiochiError};
use sdl2::event::Event;
use sdl2::keyboard::Keycode;

pub mod editor;

/// Version of the editor, as declared in `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The size the plan asks the editor to open at.
pub const DEFAULT_WIDTH: u32 = 1600;
/// The size the plan asks the editor to open at.
pub const DEFAULT_HEIGHT: u32 = 900;

/// Options for one editor session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorOptions {
    /// Window width in physical pixels.
    pub width: u32,
    /// Window height in physical pixels.
    pub height: u32,
    /// Render this many frames and exit. `None` runs until the window closes.
    ///
    /// This is what a headless CI run uses; it is also how the manual check below is
    /// scripted.
    pub frames: Option<u64>,
    /// The window title the platform starts with.
    pub title: String,
    /// A scene to open before the first frame, as `--scene`.
    pub scene: Option<std::path::PathBuf>,
}

impl Default for EditorOptions {
    fn default() -> Self {
        Self {
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
            frames: None,
            title: format!("giochi-editor v{VERSION}"),
            scene: None,
        }
    }
}

/// What a finished session did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EditorRunStats {
    /// Display frames rendered.
    pub frames: u64,
}

/// Runs the editor until `options.frames` is reached or the user closes the window.
///
/// # Errors
///
/// Returns whatever [`Engine::new`] returned — there is no window to edit in without a
/// video driver — and any error from replacing the window title.
pub fn run(options: EditorOptions) -> Result<EditorRunStats, GiochiError> {
    log::info!("giochi-editor v{VERSION}");

    let config = EngineConfig::new(options.width, options.height, &options.title);
    let mut engine = Engine::new(config)?;
    // Phase 30 accepts a window whose layout survives a resize. SDL windows are fixed by
    // default; the engine's own loops never resize, so this is the editor's call.
    engine.render_mut().set_resizable(true);
    log::info!("video driver: {}", engine.platform().video_driver_name());

    let mut state = editor::EditorState::new();
    if let Some(path) = &options.scene {
        if let Err(error) = state.open_scene(path) {
            state.report(editor::Status::error(error));
        }
    }
    let mut window_title = state.window_title();
    if let Err(error) = engine.render_mut().set_title(&window_title) {
        log::warn!("could not set the editor title: {error}");
    }

    let theme = editor::EditorTheme::dark();
    let mut backend = EguiBackend::new();
    let mut egui_events: Vec<egui::Event> = Vec::new();
    let mut egui_wants_keyboard = false;
    let mut last_frame = Instant::now();
    let mut frames = 0u64;

    loop {
        let frame_started = Instant::now();
        let dt = frame_started.duration_since(last_frame).as_secs_f32();
        last_frame = frame_started;

        let raw_events = engine.platform().poll_events();
        egui_events.clear();
        for event in &raw_events {
            // The console sees keys first: `Esc` closes it, `F2` toggles it. A key it
            // consumed must not also close the editor.
            let consumed = engine.devtools_mut().handle_event(event);
            if !consumed {
                match event {
                    Event::Quit { .. } => state.request_quit(),
                    Event::KeyDown {
                        keycode: Some(Keycode::Escape),
                        ..
                    } => {
                        // A text field that wants the keyboard owns `Esc` too; phase 30
                        // has none, but the check is what keeps phase 36's inline rename
                        // from closing the editor.
                        if !egui_wants_keyboard {
                            state.request_quit();
                        }
                    }
                    _ => {}
                }
            }
            // The editor owns the keyboard all the time: `wants_keyboard = true` so
            // egui consumes the events, whether or not it has a focused text field.
            if let (Some(translated), _) = translate_event(event, true) {
                egui_events.push(translated);
            }
        }

        engine.render_mut().clear(theme.viewport_color());
        // The scene goes on the canvas **before** egui, using the rectangle the previous
        // frame's layout produced. egui then paints the panels around it, so a line
        // projected a pixel or two outside the viewport cannot land on a panel.
        if let Some(previous_rect) = state.viewport_rect {
            editor::viewport::draw(
                engine.render_mut(),
                previous_rect,
                state.scene.as_ref(),
                &state.viewport_camera,
                state.selected,
                &theme,
            );
        }

        let screen = engine.render().drawable_size();
        let mut metrics = editor::LayoutMetrics::default();
        let output = backend.run(screen, dt, &egui_events, &mut |ctx| {
            theme.apply(ctx);
            metrics = editor::layout::draw(ctx, &mut state, &theme);
            engine.devtools_mut().draw_console_panel(ctx, screen);
        });
        egui_wants_keyboard = backend.context().wants_keyboard_input();
        handle_shortcuts(backend.context(), &mut state, egui_wants_keyboard);
        backend.paint(engine.render_mut(), output);

        // The layout has just reported where the viewport is and what the pointer did
        // over it. Apply the camera part and remember the rectangle for the next frame,
        // which is drawn before egui runs.
        state.viewport_rect = metrics.viewport_rect;
        if let Some(rect) = metrics.viewport_rect {
            if metrics.input.moves_the_camera() {
                metrics
                    .input
                    .apply_to_camera(&mut state.viewport_camera, rect.height());
            }
        }
        engine.render_mut().present();

        if state.window_title() != window_title {
            window_title = state.window_title();
            if let Err(error) = engine.render_mut().set_title(&window_title) {
                log::warn!("could not set the editor title: {error}");
            }
        }

        frames += 1;
        if state.quit_requested() || options.frames.is_some_and(|limit| frames >= limit) {
            break;
        }
    }

    // Hand the egui textures back while the registry is still alive (the same rule the
    // engine follows for its overlay).
    backend.release(engine.render_mut());
    log::info!("giochi-editor: {frames} frame(s) rendered");

    Ok(EditorRunStats { frames })
}

/// Applies the editor's keyboard shortcuts, unless egui wants the keyboard.
///
/// `Ctrl+Shift+S` is checked before `Ctrl+S`: egui's `consume_key` ignores extra
/// modifiers, so the other order would make "Save As" also save.
fn handle_shortcuts(ctx: &egui::Context, state: &mut editor::EditorState, wants_keyboard: bool) {
    if wants_keyboard {
        return;
    }
    let modifiers = egui::Modifiers::COMMAND;
    let with_shift = egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT);
    let mut save_as = false;
    let mut save = false;
    let mut open = false;
    let mut new = false;
    ctx.input_mut(|input| {
        save_as = input.consume_key(with_shift, egui::Key::S);
        save = input.consume_key(modifiers, egui::Key::S);
        open = input.consume_key(modifiers, egui::Key::O);
        new = input.consume_key(modifiers, egui::Key::N);
    });
    if save_as {
        state.save_dialog();
    } else if save {
        state.save_or_ask();
    } else if open {
        state.open_dialog();
    } else if new {
        state.new_scene();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_window_matches_the_plan() {
        let options = EditorOptions::default();
        assert_eq!((options.width, options.height), (1600, 900));
        assert_eq!(options.frames, None);
        assert!(options.title.contains(VERSION));
    }
}
