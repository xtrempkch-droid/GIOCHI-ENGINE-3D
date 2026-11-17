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
use giochi_engine::{Color, Engine, EngineConfig, GiochiError};
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
}

impl Default for EditorOptions {
    fn default() -> Self {
        Self {
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
            frames: None,
            title: format!("giochi-editor v{VERSION}"),
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
    if let Err(error) = engine.render_mut().set_title(&state.window_title()) {
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

        engine.render_mut().clear(Color::rgb(8, 9, 13));
        let screen = engine.render().drawable_size();
        let output = backend.run(screen, dt, &egui_events, &mut |ctx| {
            theme.apply(ctx);
            editor::layout::draw(ctx, &mut state, &theme);
            engine.devtools_mut().draw_console_panel(ctx, screen);
        });
        egui_wants_keyboard = backend.context().wants_keyboard_input();
        backend.paint(engine.render_mut(), output);
        engine.render_mut().present();

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
