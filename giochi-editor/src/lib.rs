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
pub mod project;

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
    /// A project directory to open before the first frame, as `--project`.
    pub project: Option<std::path::PathBuf>,
}

impl Default for EditorOptions {
    fn default() -> Self {
        Self {
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
            frames: None,
            title: format!("giochi-editor v{VERSION}"),
            scene: None,
            project: None,
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
    if let Some(path) = &options.project {
        if let Err(error) = state.open_project(path) {
            state.report(editor::Status::error(error));
        }
    }
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
            editor::viewport::draw_with_gizmo(
                engine.render_mut(),
                previous_rect,
                state.scene.as_ref(),
                &state.viewport_camera,
                &state.selected,
                &theme,
                editor::viewport::GizmoView {
                    mode: state.mode,
                    active: state.drag.as_ref().map(|drag| drag.handle),
                },
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
        if let (Some(drag), Some(rect)) = (&metrics.dropped_asset, metrics.viewport_rect) {
            let position = metrics
                .input
                .click
                .or_else(|| backend.context().pointer_hover_pos())
                .unwrap_or_else(|| rect.center());
            apply_asset_drop(&mut state, drag, rect, position);
        }
        if let Some(rect) = metrics.viewport_rect {
            if metrics.input.moves_the_camera() {
                metrics
                    .input
                    .apply_to_camera(&mut state.viewport_camera, rect.height());
            }

            let camera = state.viewport_camera;
            apply_viewport_interaction(&mut state, &metrics.input, rect, &camera);
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

/// Instantiates an asset dropped on the viewport.
///
/// Only `.obj` meshes become nodes today. Everything else reports why not, by name: a
/// drop that silently does nothing is the failure mode this function exists to avoid.
fn apply_asset_drop(
    state: &mut editor::EditorState,
    drag: &panels::asset_browser::AssetDrag,
    rect: egui::Rect,
    position: egui::Pos2,
) -> bool {
    use giochi_engine_3d::scene::{MeshSource, SceneNode, SceneNodeId};

    if drag.kind != project::AssetKind::Model {
        state.report(editor::Status::warning(format!(
            "{} is a {}; drop it on the matching slot instead",
            drag.node_name(),
            drag.kind.label().to_lowercase()
        )));
        return false;
    }
    if !drag.is_loadable() {
        state.report(editor::Status::warning(format!(
            "{}: the glTF loader is part of the pending Production Pack (ADR-063); \
             use an .obj model",
            drag.node_name()
        )));
        return false;
    }
    let Some(scene) = state.scene.as_mut() else {
        state.report(editor::Status::warning(
            "open or create a scene before dropping an asset",
        ));
        return false;
    };

    // Where the cursor points on the floor (y = 0). A miss (the camera looking at the
    // horizon) falls back to the orbit target, so the object always lands somewhere the
    // user can see instead of at a division-by-zero.
    let position = gizmos::picker::ground_point(position, rect, &state.viewport_camera)
        .unwrap_or_else(|| state.viewport_camera.target.to_array());

    let mut node = SceneNode::new(SceneNodeId::new(0), drag.node_name());
    node.mesh = Some(MeshSource::Obj(drag.relative.clone()));
    node.transform.position = position;
    let node = scene.add_node(node);
    let id = node.id;
    state.select(Some(id));
    state.mark_dirty();
    state.report(editor::Status::info(format!(
        "Added {} at ({:.2}, {:.2}, {:.2})",
        node.name, position[0], position[1], position[2]
    )));
    true
}

/// Applies a frame of viewport interaction: finish a drag, continue one, start one, or
/// fall back to selection.
///
/// The order is what makes dragging feel right: while a drag is alive the pointer never
/// selects anything, because the same press that grabbed the arrow would otherwise also
/// pick whatever node is behind it.
fn apply_viewport_interaction(
    state: &mut editor::EditorState,
    input: &editor::ViewportInput,
    rect: egui::Rect,
    camera: &editor::OrbitCamera,
) {
    // 1. A drag in progress: follow the pointer; on release, end it.
    if let Some(mut drag) = state.drag.take() {
        if input.released {
            if let Some(scene) = state.scene.as_mut() {
                if let Some(node) = scene.node(drag.node).cloned() {
                    if node.transform != drag.start {
                        // One drag, one modification: the dirty flag is set here, not per
                        // frame, and phase 37 will push exactly this before/after pair.
                        state.mark_dirty();
                        let description = if drag.is_scale() {
                            format!(
                                "{} ×{:.2}",
                                drag.description(),
                                gizmos::scale::factor_from(drag.start, node.transform, drag.handle)
                            )
                        } else {
                            drag.description()
                        };
                        state.status = editor::Status::info(description);
                    }
                }
            }
            return;
        }

        if let Some(position) = input.pointer {
            if let Some(ray) = gizmos::picker::ray_through(position, rect, camera) {
                drag.advance(ray);
                let transform = drag.transform_for(ray);
                if let Some(scene) = state.scene.as_mut() {
                    if let Some(node) = scene.node_mut(drag.node) {
                        node.transform = transform;
                    }
                }
            }
        }
        state.drag = Some(drag);
        return;
    }

    // 2. A press on a handle starts a drag, in preference to selecting a node.
    if let Some(position) = input.pressed.or(input.double_click) {
        if let Some(started) = begin_gizmo_drag(state, position, rect, camera) {
            state.drag = Some(started);
            return;
        }
    }

    apply_viewport_pick(state, input, rect);
}

/// Starts a gizmo drag if the press landed on a handle.
fn begin_gizmo_drag(
    state: &mut editor::EditorState,
    position: egui::Pos2,
    rect: egui::Rect,
    camera: &editor::OrbitCamera,
) -> Option<gizmos::Drag> {
    use giochi_engine_3d::scene::SceneNodeId;

    if state.mode == editor::EditorMode::Select {
        return None;
    }
    let node = state.gizmo_target().cloned()?;
    let scene = state.scene.as_ref()?;
    let ray = gizmos::picker::ray_through(position, rect, camera)?;

    let world = editor::viewport::world_transform(scene, &node);
    let center = glam::Vec3::new(world.position[0], world.position[1], world.position[2]);
    let geometry = editor::viewport::gizmo_geometry(scene, &node, center, camera, rect);

    let parent = parent_space(scene, &node);
    let id: SceneNodeId = node.id;

    match state.mode {
        editor::EditorMode::Translate => {
            let handle = gizmos::translate::pick(ray, geometry, gizmos::AXIS_TOLERANCE_POINTS)?;
            gizmos::translate::begin(id, node.transform, handle, ray, geometry, parent)
        }
        editor::EditorMode::Rotate => {
            let handle = gizmos::rotate::pick(
                ray,
                geometry,
                gizmos::RING_POINTS,
                gizmos::AXIS_TOLERANCE_POINTS,
                camera,
            )?;
            gizmos::rotate::begin(id, node.transform, handle, ray, geometry, parent)
        }
        editor::EditorMode::Scale => {
            let handle = gizmos::scale::pick(ray, geometry, gizmos::AXIS_TOLERANCE_POINTS)?;
            gizmos::scale::begin(id, node.transform, handle, ray, geometry, parent)
        }
        editor::EditorMode::Select => None,
    }
}

/// The parent's world rotation and scale, for a node's local transform.
fn parent_space(
    scene: &giochi_engine_3d::scene::Scene,
    node: &giochi_engine_3d::scene::SceneNode,
) -> gizmos::ParentSpace {
    let Some(parent_id) = node.parent else {
        return gizmos::ParentSpace::default();
    };
    let Some(parent) = scene.node(parent_id) else {
        return gizmos::ParentSpace::default();
    };
    let world = editor::viewport::world_transform(scene, parent);
    gizmos::ParentSpace {
        rotation: glam::Quat::from_array(world.rotation),
        scale: glam::Vec3::new(world.scale[0], world.scale[1], world.scale[2]),
    }
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
    let mut import = false;
    ctx.input_mut(|input| {
        save_as = input.consume_key(with_shift, egui::Key::S);
        save = input.consume_key(modifiers, egui::Key::S);
        open = input.consume_key(modifiers, egui::Key::O);
        new = input.consume_key(modifiers, egui::Key::N);
        import = input.consume_key(modifiers, egui::Key::I);
    });
    // Node editing. `Delete` and `Backspace` both delete: the first is what a keyboard
    // says, the second is what a laptop user's fingers do.
    let mut delete = false;
    let mut duplicate = false;
    ctx.input_mut(|input| {
        delete = input.consume_key(egui::Modifiers::NONE, egui::Key::Delete)
            || input.consume_key(egui::Modifiers::NONE, egui::Key::Backspace);
        duplicate = input.consume_key(modifiers, egui::Key::D);
    });

    // Bare letters, no modifiers: the tool keys are the only unmodified shortcuts, so
    // there is no ambiguity with the Ctrl combinations above.
    let mut tool = None;
    ctx.input_mut(|input| {
        for (key, mode) in [
            (egui::Key::Q, editor::EditorMode::Select),
            (egui::Key::W, editor::EditorMode::Translate),
            (egui::Key::E, editor::EditorMode::Rotate),
            (egui::Key::R, editor::EditorMode::Scale),
        ] {
            if input.consume_key(egui::Modifiers::NONE, key) {
                tool = Some(mode);
            }
        }
    });

    if delete {
        state.delete_selected();
    } else if duplicate {
        state.duplicate_selected();
    } else if save_as {
        state.save_dialog();
    } else if save {
        state.save_or_ask();
    } else if let Some(mode) = tool {
        state.set_mode(mode);
    } else if import {
        state.import_dialog();
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

    /// The screen position of the tip of a translate arrow, which is guaranteed to be on
    /// the handle: the geometry puts the tip exactly `length` from the centre.
    fn arrow_tip(state: &editor::EditorState, handle: gizmos::GizmoHandle) -> egui::Pos2 {
        let rect = test_rect();
        let scene = state.scene.as_ref().expect("scene");
        let node = scene.node(state.primary().expect("node")).expect("node");
        let world = editor::viewport::world_transform(scene, node);
        let center = glam::Vec3::new(world.position[0], world.position[1], world.position[2]);
        let geometry = gizmos::GizmoGeometry::for_camera(center, &state.viewport_camera, rect);
        let view = glam::Mat4::from_cols_array(
            &state
                .viewport_camera
                .view_projection(rect.width() / rect.height()),
        );
        let axis = handle.axis().expect("a direction handle");
        editor::viewport::project_point(geometry.tip(axis), &view, rect).expect("on screen")
    }

    fn press(position: egui::Pos2) -> editor::ViewportInput {
        editor::ViewportInput {
            hovered: true,
            pointer: Some(position),
            pressed: Some(position),
            ..editor::ViewportInput::default()
        }
    }

    #[test]
    fn dragging_a_move_arrow_moves_the_node_by_the_dragged_distance() {
        let mut state = state_with_default_scene();
        state.set_mode(editor::EditorMode::Translate);
        let rect = test_rect();
        let camera = state.viewport_camera;
        let start = state
            .scene
            .as_ref()
            .expect("scene")
            .node(editor::EntityId(1))
            .expect("cube")
            .transform
            .position;

        let grab = arrow_tip(&state, gizmos::GizmoHandle::X);
        apply_viewport_interaction(&mut state, &press(grab), rect, &camera);
        assert!(state.drag.is_some(), "the press grabbed the X arrow");

        // Drag along the arrow's screen direction; the node must follow the axis only.
        let moved = grab + egui::vec2(60.0, 0.0);
        let input = editor::ViewportInput {
            hovered: true,
            pointer: Some(moved),
            ..editor::ViewportInput::default()
        };
        apply_viewport_interaction(&mut state, &input, rect, &camera);

        let after = state
            .scene
            .as_ref()
            .expect("scene")
            .node(editor::EntityId(1))
            .expect("cube")
            .transform
            .position;
        assert!(after[0] > start[0] + 0.5, "{start:?} -> {after:?}");
        assert!(
            (after[1] - start[1]).abs() < 1e-4,
            "no vertical drift: {after:?}"
        );
        assert!(
            (after[2] - start[2]).abs() < 1e-4,
            "no depth drift: {after:?}"
        );
        assert!(
            !state.dirty,
            "a drag in progress is not a committed edit yet"
        );

        let release = editor::ViewportInput {
            hovered: true,
            pointer: Some(moved),
            released: true,
            ..editor::ViewportInput::default()
        };
        apply_viewport_interaction(&mut state, &release, rect, &camera);
        assert!(state.drag.is_none(), "release ends the drag");
        assert!(state.dirty, "and that commits it");
        assert!(state.status.text.contains("X"), "{}", state.status.text);
    }

    fn press_key(state: &mut editor::EditorState, key: egui::Key) {
        press_key_with(state, key, egui::Modifiers::NONE);
    }

    fn press_key_with(state: &mut editor::EditorState, key: egui::Key, modifiers: egui::Modifiers) {
        let ctx = egui::Context::default();
        let mut raw = egui::RawInput {
            modifiers,
            ..egui::RawInput::default()
        };
        raw.events.push(egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        });
        let _ = ctx.run(raw, |ctx| handle_shortcuts(ctx, state, false));
    }

    #[test]
    fn delete_and_ctrl_d_edit_the_selection() {
        let mut state = state_with_default_scene();
        state.select(Some(editor::EntityId(2)));
        press_key(&mut state, egui::Key::Delete);
        assert!(
            state
                .scene
                .as_ref()
                .expect("scene")
                .node(editor::EntityId(2))
                .is_none(),
            "Delete removed the light"
        );

        state.select(Some(editor::EntityId(1)));
        press_key_with(&mut state, egui::Key::D, egui::Modifiers::COMMAND);
        assert_eq!(
            state.scene.as_ref().expect("scene").nodes.len(),
            3,
            "the cube was duplicated, so two cubes plus the camera"
        );
        assert_eq!(state.selected.len(), 1);
        assert_ne!(state.primary(), Some(editor::EntityId(1)));
    }

    #[test]
    fn backspace_deletes_like_delete_does() {
        let mut state = state_with_default_scene();
        state.select(Some(editor::EntityId(3)));
        press_key(&mut state, egui::Key::Backspace);
        assert!(state
            .scene
            .as_ref()
            .expect("scene")
            .node(editor::EntityId(3))
            .is_none());
    }

    #[test]
    fn the_tool_keys_arm_and_disarm_the_gizmos() {
        let mut state = state_with_default_scene();
        press_key(&mut state, egui::Key::W);
        assert_eq!(state.mode, editor::EditorMode::Translate);
        press_key(&mut state, egui::Key::E);
        assert_eq!(state.mode, editor::EditorMode::Rotate);
        press_key(&mut state, egui::Key::R);
        assert_eq!(state.mode, editor::EditorMode::Scale);
        press_key(&mut state, egui::Key::Q);
        assert_eq!(state.mode, editor::EditorMode::Select);
    }

    #[test]
    fn a_tool_key_does_not_fire_while_a_text_field_has_the_keyboard() {
        // The console and the file dialog take text; `w` there must be a `w`.
        let ctx = egui::Context::default();
        let mut state = state_with_default_scene();
        let mut raw = egui::RawInput::default();
        raw.events.push(egui::Event::Key {
            key: egui::Key::W,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        let _ = ctx.run(raw, |ctx| handle_shortcuts(ctx, &mut state, true));
        assert_eq!(state.mode, editor::EditorMode::Select);
    }

    #[test]
    fn switching_tools_cancels_a_drag_that_belonged_to_the_old_one() {
        // A drag is only meaningful for the tool that started it: half a translate finished
        // with the rotate gizmo would apply a rotation computed from a translation grab.
        let mut state = state_with_default_scene();
        state.set_mode(editor::EditorMode::Translate);
        let rect = test_rect();
        let camera = state.viewport_camera;
        let grab = arrow_tip(&state, gizmos::GizmoHandle::X);
        apply_viewport_interaction(&mut state, &press(grab), rect, &camera);
        assert!(state.drag.is_some());
        state.set_mode(editor::EditorMode::Rotate);
        assert!(state.drag.is_none());
    }

    #[test]
    fn dragging_a_scale_cube_changes_only_that_axis() {
        let mut state = state_with_default_scene();
        state.set_mode(editor::EditorMode::Scale);
        let rect = test_rect();
        let camera = state.viewport_camera;
        let start = state
            .scene
            .as_ref()
            .expect("scene")
            .node(editor::EntityId(1))
            .expect("cube")
            .transform;

        let grab = arrow_tip(&state, gizmos::GizmoHandle::X);
        apply_viewport_interaction(&mut state, &press(grab), rect, &camera);
        assert!(state.drag.is_some(), "the press grabbed the X cube");

        let out = grab + egui::vec2(80.0, 0.0);
        let input = editor::ViewportInput {
            hovered: true,
            pointer: Some(out),
            ..editor::ViewportInput::default()
        };
        apply_viewport_interaction(&mut state, &input, rect, &camera);
        let release = editor::ViewportInput {
            hovered: true,
            pointer: Some(out),
            released: true,
            ..editor::ViewportInput::default()
        };
        apply_viewport_interaction(&mut state, &release, rect, &camera);

        let after = state
            .scene
            .as_ref()
            .expect("scene")
            .node(editor::EntityId(1))
            .expect("cube")
            .transform;
        assert!(after.scale[0] > start.scale[0] + 0.2, "{:?}", after.scale);
        assert_eq!(after.scale[1], start.scale[1]);
        assert_eq!(after.scale[2], start.scale[2]);
        assert_eq!(after.position, start.position, "scaling moves nothing");
        assert!(state.dirty);
        assert!(state.status.text.contains('×'), "{}", state.status.text);
    }

    #[test]
    fn the_uniform_cube_scales_every_axis() {
        let mut state = state_with_default_scene();
        state.set_mode(editor::EditorMode::Scale);
        let rect = test_rect();
        let camera = state.viewport_camera;
        let start = state
            .scene
            .as_ref()
            .expect("scene")
            .node(editor::EntityId(1))
            .expect("cube")
            .transform
            .scale;

        let center = rect.center();
        apply_viewport_interaction(&mut state, &press(center), rect, &camera);
        assert!(
            matches!(
                state.drag.as_ref().map(|drag| drag.handle),
                Some(gizmos::GizmoHandle::Center)
            ),
            "the press grabbed the uniform cube"
        );

        let out = center + egui::vec2(120.0, 0.0);
        let input = editor::ViewportInput {
            hovered: true,
            pointer: Some(out),
            ..editor::ViewportInput::default()
        };
        apply_viewport_interaction(&mut state, &input, rect, &camera);

        let after = state
            .scene
            .as_ref()
            .expect("scene")
            .node(editor::EntityId(1))
            .expect("cube")
            .transform
            .scale;
        assert!(after[0] > start[0] + 0.2, "{after:?}");
        assert!((after[1] / after[0] - start[1] / start[0]).abs() < 1e-4);
        assert!((after[2] / after[0] - start[2] / start[0]).abs() < 1e-4);
    }

    #[test]
    fn a_press_off_the_handles_falls_back_to_selection() {
        let mut state = state_with_default_scene();
        state.set_mode(editor::EditorMode::Translate);
        let rect = test_rect();
        let camera = state.viewport_camera;
        // The cube's centre projects to the middle of the viewport, where the centre square
        // lives; aim at the bottom-left corner, far from every handle.
        let empty = egui::pos2(30.0, 560.0);
        let before = state.selected.clone();
        apply_viewport_interaction(&mut state, &press(empty), rect, &camera);
        assert!(state.drag.is_none(), "nothing was grabbed");
        assert_eq!(state.selected, before, "selecting is a click, not a press");
    }

    #[test]
    fn the_select_tool_ignores_the_handles() {
        let mut state = state_with_default_scene();
        let rect = test_rect();
        let camera = state.viewport_camera;
        let grab = arrow_tip(&state, gizmos::GizmoHandle::X);
        apply_viewport_interaction(&mut state, &press(grab), rect, &camera);
        assert!(state.drag.is_none(), "no handles are drawn in select mode");
        assert_eq!(state.primary(), Some(editor::EntityId(1)));
    }

    fn model_drag(name: &str, relative: &str) -> panels::asset_browser::AssetDrag {
        panels::asset_browser::AssetDrag {
            path: std::path::PathBuf::from(name),
            relative: relative.to_owned(),
            kind: project::AssetKind::Model,
        }
    }

    #[test]
    fn dropping_an_obj_on_the_viewport_adds_a_node_at_the_ground_point() {
        let mut state = state_with_default_scene();
        let drag = model_drag(
            "/p/assets/models/race_car.obj",
            "assets/models/race_car.obj",
        );
        let rect = test_rect();
        assert!(apply_asset_drop(&mut state, &drag, rect, rect.center()));

        let scene = state.scene.as_ref().expect("scene");
        assert_eq!(scene.nodes.len(), 4, "the default three plus the drop");
        let node = scene.nodes.last().expect("new node");
        assert_eq!(node.name, "race_car");
        assert_eq!(
            node.mesh,
            Some(giochi_engine_3d::scene::MeshSource::Obj(
                "assets/models/race_car.obj".to_owned()
            ))
        );
        assert_eq!(node.transform.position[1], 0.0, "it lands on the floor");
        assert_eq!(state.primary(), Some(node.id), "and it is selected");
        assert!(state.dirty);
        assert!(state.status.text.contains("Added"), "{}", state.status.text);
    }

    #[test]
    fn two_drops_land_in_different_places() {
        let mut state = state_with_default_scene();
        let rect = test_rect();
        let drag = model_drag("/p/assets/models/box.obj", "assets/models/box.obj");
        apply_asset_drop(&mut state, &drag, rect, egui::pos2(300.0, 500.0));
        apply_asset_drop(&mut state, &drag, rect, egui::pos2(500.0, 500.0));

        let scene = state.scene.as_ref().expect("scene");
        let positions: Vec<[f32; 3]> = scene
            .nodes
            .iter()
            .filter(|node| node.name == "box")
            .map(|node| node.transform.position)
            .collect();
        assert_eq!(positions.len(), 2);
        assert_ne!(positions[0], positions[1], "a pile would be a bug");
    }

    #[test]
    fn dropping_a_gltf_explains_the_missing_loader_instead_of_doing_nothing() {
        let mut state = state_with_default_scene();
        let drag = model_drag("/p/assets/models/car.gltf", "assets/models/car.gltf");
        let rect = test_rect();
        assert!(!apply_asset_drop(&mut state, &drag, rect, rect.center()));
        assert_eq!(state.scene.as_ref().map(|s| s.nodes.len()), Some(3));
        assert!(state.status.text.contains("glTF"), "{}", state.status.text);
        assert_eq!(state.status.kind, editor::StatusKind::Warning);
    }

    #[test]
    fn dropping_a_texture_on_the_viewport_says_where_it_goes() {
        let mut state = state_with_default_scene();
        let drag = panels::asset_browser::AssetDrag {
            path: std::path::PathBuf::from("/p/assets/textures/grass.png"),
            relative: "assets/textures/grass.png".to_owned(),
            kind: project::AssetKind::Texture,
        };
        let rect = test_rect();
        assert!(!apply_asset_drop(&mut state, &drag, rect, rect.center()));
        assert_eq!(state.scene.as_ref().map(|s| s.nodes.len()), Some(3));
        assert!(
            state.status.text.contains("texture"),
            "{}",
            state.status.text
        );
    }

    #[test]
    fn dropping_an_asset_with_no_scene_says_to_open_one() {
        let mut state = editor::EditorState::new();
        let drag = model_drag("/p/assets/models/box.obj", "assets/models/box.obj");
        let rect = test_rect();
        assert!(!apply_asset_drop(&mut state, &drag, rect, rect.center()));
        assert!(state.status.text.contains("scene"), "{}", state.status.text);
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
