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
pub mod gizmos;
pub mod panels;

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
    // The icons are PNGs; SDL_image has to be up before the first `Surface::from_file`.
    // The engine initialises it when it loads a texture, but an editor that starts on an
    // empty scene never loads one, and the icons would fall back to letters for no reason.
    init_image_support();
    let mut backend = EguiBackend::new();
    // Loaded once: an `egui::TextureHandle` per icon lives for the session, and the
    // backend uploads each one on the first paint.
    let icons = editor::IconSet::load(backend.context(), editor::IconSet::default_directory());
    if !icons.is_complete() {
        log::warn!("some editor icons failed to load; falling back to letters");
    }
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
                &state.selected,
                &theme,
            );
        }

        let screen = engine.render().drawable_size();
        let mut metrics = editor::LayoutMetrics::default();
        let output = backend.run(screen, dt, &egui_events, &mut |ctx| {
            theme.apply(ctx);
            metrics = editor::layout::draw(ctx, &mut state, &theme, Some(&icons));
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

            apply_viewport_pick(&mut state, &metrics.input, rect);
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

/// Applies a click or double-click in the viewport to the selection.
///
/// Split out of the loop so the wiring is testable without SDL events: the picker and the
/// camera are pure, and this is the only place where "clicked here" becomes "selected
/// that".
fn apply_viewport_pick(
    state: &mut editor::EditorState,
    input: &editor::ViewportInput,
    rect: egui::Rect,
) {
    let Some(scene) = state.scene.clone() else {
        return;
    };

    if let Some(position) = input.double_click {
        if let Some(id) = gizmos::picker::pick(&scene, position, rect, &state.viewport_camera) {
            if let Some(node) = scene.node(id) {
                let (min, max) = editor::viewport::node_world_bounds(&scene, node);
                state.viewport_camera.frame_aabb(min, max);
            }
            state.select(Some(id));
        }
        return;
    }

    if let Some(position) = input.click {
        match gizmos::picker::pick(&scene, position, rect, &state.viewport_camera) {
            Some(id) if input.toggle_modifier => state.toggle_selection(id),
            Some(id) => state.select(Some(id)),
            None => state.clear_selection(),
        }
    }
}

/// Initialises SDL_image's PNG decoder, keeping the context alive for the process.
fn init_image_support() {
    match sdl2::image::init(sdl2::image::InitFlag::PNG) {
        Ok(context) => std::mem::forget(context),
        Err(error) => log::warn!("SDL_image PNG support unavailable: {error}"),
    }
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

    fn test_rect() -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0))
    }

    fn state_with_default_scene() -> editor::EditorState {
        let mut state = editor::EditorState::new();
        state.new_scene();
        // `new_scene` frames the scene; aim at the cube so the centre of the viewport is
        // over it.
        state.viewport_camera =
            editor::OrbitCamera::looking_at(glam::Vec3::new(0.0, 0.5, 0.0), 9.0);
        state
    }

    fn click(position: egui::Pos2) -> editor::ViewportInput {
        editor::ViewportInput {
            hovered: true,
            click: Some(position),
            ..editor::ViewportInput::default()
        }
    }

    #[test]
    fn clicking_the_cube_in_the_viewport_selects_it() {
        let mut state = state_with_default_scene();
        apply_viewport_pick(&mut state, &click(test_rect().center()), test_rect());
        assert_eq!(state.primary(), Some(editor::EntityId(1)));
    }

    #[test]
    fn clicking_empty_space_clears_the_selection() {
        let mut state = state_with_default_scene();
        state.select(Some(editor::EntityId(2)));
        apply_viewport_pick(&mut state, &click(egui::pos2(4.0, 4.0)), test_rect());
        assert!(state.selected.is_empty());
    }

    #[test]
    fn ctrl_click_in_the_viewport_toggles() {
        let mut state = state_with_default_scene();
        state.select(Some(editor::EntityId(2)));
        let mut input = click(test_rect().center());
        input.toggle_modifier = true;
        apply_viewport_pick(&mut state, &input, test_rect());
        assert_eq!(
            state.selected,
            vec![editor::EntityId(2), editor::EntityId(1)]
        );
    }

    #[test]
    fn a_double_click_frames_and_selects() {
        let mut state = state_with_default_scene();
        state.viewport_camera =
            editor::OrbitCamera::looking_at(glam::Vec3::new(0.0, 0.5, 0.0), 40.0);
        let input = editor::ViewportInput {
            hovered: true,
            double_click: Some(test_rect().center()),
            ..editor::ViewportInput::default()
        };
        apply_viewport_pick(&mut state, &input, test_rect());
        assert_eq!(state.primary(), Some(editor::EntityId(1)));
        assert!(
            state.viewport_camera.distance < 10.0,
            "framing pulls the camera in, got {}",
            state.viewport_camera.distance
        );
    }

    #[test]
    fn a_click_with_no_scene_is_ignored() {
        let mut state = editor::EditorState::new();
        apply_viewport_pick(&mut state, &click(test_rect().center()), test_rect());
        assert!(state.selected.is_empty());
    }

    #[test]
    fn the_default_window_matches_the_plan() {
        let options = EditorOptions::default();
        assert_eq!((options.width, options.height), (1600, 900));
        assert_eq!(options.frames, None);
        assert!(options.title.contains(VERSION));
    }
}
