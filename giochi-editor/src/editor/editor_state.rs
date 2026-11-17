//! Everything the editor remembers between frames.
//!
//! `EditorState` is the *editor's* state, not the scene's: which file is open, what is
//! selected, which gizmo is armed, whether there are unsaved changes, and the viewport
//! camera. The scene itself is the engine's [`Scene`], loaded and saved through the
//! format in `giochi-engine-3d` — the editor never invents its own file format.

use std::path::{Path, PathBuf};

use giochi_engine_3d::scene::{Scene, SceneError};

use crate::editor::dialog::FileDialog;
use crate::editor::orbit_camera::OrbitCamera;

/// Identifies one object in the edited scene.
///
/// The editor reuses the scene format's id instead of defining a second one: an id that
/// is saved in a file and an id that lives in an undo stack must not be two value spaces.
pub use giochi_engine_3d::scene::SceneNodeId as EntityId;

/// What a status message means, which decides the colour the menu bar draws it with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StatusKind {
    /// Nothing to report yet.
    #[default]
    Idle,
    /// Something worked.
    Info,
    /// Something worked, with a caveat.
    Warning,
    /// Something failed.
    Error,
}

/// The last thing the editor did, for the menu bar.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Status {
    /// The message.
    pub text: String,
    /// What it means.
    pub kind: StatusKind,
}

impl Status {
    /// A message of the given kind.
    pub fn new(kind: StatusKind, text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            kind,
        }
    }

    /// An informational message.
    pub fn info(text: impl Into<String>) -> Self {
        Self::new(StatusKind::Info, text)
    }

    /// An error message.
    pub fn error(text: impl Into<String>) -> Self {
        Self::new(StatusKind::Error, text)
    }

    /// A warning.
    pub fn warning(text: impl Into<String>) -> Self {
        Self::new(StatusKind::Warning, text)
    }
}

/// What dragging in the viewport does.
///
/// The toolbar and the `Q`/`W`/`E`/`R` shortcuts arrive with the gizmos (phases 35a and
/// 35b); phase 31 only carries the value so the rest of the editor can be written
/// against it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditorMode {
    /// Click selects, nothing is dragged.
    #[default]
    Select,
    /// The translate gizmo.
    Translate,
    /// The rotate gizmo.
    Rotate,
    /// The scale gizmo.
    Scale,
}

impl EditorMode {
    /// The name a toolbar or a log line uses.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Select => "select",
            Self::Translate => "translate",
            Self::Rotate => "rotate",
            Self::Scale => "scale",
        }
    }
}

/// Which of the panels are visible.
///
/// The `View` menu toggles these. A closed panel is not drawn at all rather than drawn
/// collapsed, so the central viewport gets the space back — which is the only reason to
/// close one while editing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelVisibility {
    /// The left panel.
    pub hierarchy: bool,
    /// The right panel.
    pub inspector: bool,
    /// The bottom panel.
    pub asset_browser: bool,
}

impl Default for PanelVisibility {
    fn default() -> Self {
        Self {
            hierarchy: true,
            inspector: true,
            asset_browser: true,
        }
    }
}

/// The editor's session state.
#[derive(Debug, Clone, PartialEq)]
pub struct EditorState {
    /// The scene file currently open, if any.
    pub scene_path: Option<PathBuf>,
    /// The scene being edited, or `None` before `File > New Scene` / `File > Open`.
    pub scene: Option<Scene>,
    /// Every selected object, in click order. Empty when nothing is selected.
    ///
    /// A `Vec` rather than an `Option`: Ctrl+click is a selection tool, and phase 33's
    /// inspector has to say "N objects selected". The first entry is the *primary*
    /// selection, which is the one the inspector shows and the gizmo attaches to.
    pub selected: Vec<EntityId>,
    /// The armed viewport tool.
    pub mode: EditorMode,
    /// Whether the scene has unsaved changes.
    pub dirty: bool,
    /// Which panels are shown.
    pub panels: PanelVisibility,
    /// The viewport's own camera. Never the scene's (`CameraDesc`).
    pub viewport_camera: OrbitCamera,
    /// The rectangle the viewport occupied on the previous frame.
    ///
    /// Drawing happens before egui runs (see [`crate::editor::viewport`]), so one
    /// frame's worth of rectangle is kept between frames. `None` on the first frame of a
    /// session.
    pub viewport_rect: Option<egui::Rect>,
    /// The last thing the editor did, shown in the menu bar.
    pub status: Status,
    /// The file dialog, while one is open.
    pub dialog: Option<FileDialog>,
    /// Set by the window close button, `Esc`, or `File > Exit`.
    quit_requested: bool,
}

impl Default for EditorState {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorState {
    /// A fresh editor with no scene open.
    pub fn new() -> Self {
        Self {
            scene_path: None,
            scene: None,
            selected: Vec::new(),
            mode: EditorMode::Select,
            dirty: false,
            panels: PanelVisibility::default(),
            viewport_camera: OrbitCamera::default(),
            viewport_rect: None,
            status: Status::default(),
            dialog: None,
            quit_requested: false,
        }
    }

    /// The primary selection: first in click order, the one the inspector shows.
    pub fn primary(&self) -> Option<EntityId> {
        self.selected.first().copied()
    }

    /// Whether `id` is selected.
    pub fn is_selected(&self, id: EntityId) -> bool {
        self.selected.contains(&id)
    }

    /// Replaces the selection with `entity`, or clears it with `None`.
    pub fn select(&mut self, entity: Option<EntityId>) {
        self.selected.clear();
        if let Some(entity) = entity {
            self.selected.push(entity);
        }
    }

    /// Adds `id` to the selection, or removes it when it was already there.
    ///
    /// This is Ctrl+click in the hierarchy and in the viewport.
    pub fn toggle_selection(&mut self, id: EntityId) {
        if let Some(index) = self.selected.iter().position(|selected| *selected == id) {
            self.selected.remove(index);
        } else {
            self.selected.push(id);
        }
    }

    /// Forgets the selection.
    pub fn clear_selection(&mut self) {
        self.selected.clear();
    }

    /// Records that the scene changed since the last save.
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Records that the scene on disk matches the editor.
    pub fn mark_saved(&mut self) {
        self.dirty = false;
    }

    /// Asks the loop to stop after the current frame.
    pub fn request_quit(&mut self) {
        self.quit_requested = true;
    }

    /// Whether the loop should stop.
    pub const fn quit_requested(&self) -> bool {
        self.quit_requested
    }

    /// Replaces the open scene with `File > New Scene`'s default and frames it.
    pub fn new_scene(&mut self) {
        let scene = Scene::default_scene();
        self.frame_scene(&scene);
        let first = scene.nodes.first().map(|node| node.id);
        self.scene = Some(scene);
        self.scene_path = None;
        self.select(first);
        self.mark_saved();
        self.status = Status::info("New scene");
    }

    /// Loads `path`, replacing whatever was open.
    ///
    /// # Errors
    ///
    /// The scene format's error, converted to a line the menu bar can show. The scene
    /// that was open stays open.
    pub fn open_scene(&mut self, path: impl AsRef<Path>) -> Result<(), String> {
        let path = path.as_ref();
        let scene = Scene::load_from_path(path).map_err(|error| describe(&error))?;
        self.frame_scene(&scene);
        let first = scene.nodes.first().map(|node| node.id);
        self.scene = Some(scene);
        self.scene_path = Some(path.to_path_buf());
        self.select(first);
        self.mark_saved();
        self.status = Status::info(format!("Opened {}", path.display()));
        Ok(())
    }

    /// Saves to the current path.
    ///
    /// # Errors
    ///
    /// `"no path yet; use Save As"` when the scene has never been saved; the caller
    /// opens the save dialog in that case.
    pub fn save_scene(&mut self) -> Result<(), String> {
        let path = self
            .scene_path
            .clone()
            .ok_or_else(|| "no path yet; use Save As".to_owned())?;
        self.save_scene_as(path)
    }

    /// Saves to `path`, which becomes the current path.
    ///
    /// # Errors
    ///
    /// The scene format's error, or `"no scene open"` when there is nothing to save.
    pub fn save_scene_as(&mut self, path: impl AsRef<Path>) -> Result<(), String> {
        let path = path.as_ref();
        let scene = self
            .scene
            .as_ref()
            .ok_or_else(|| "no scene open".to_owned())?;
        scene.save_to_path(path).map_err(|error| describe(&error))?;
        self.scene_path = Some(path.to_path_buf());
        self.mark_saved();
        self.status = Status::info(format!("Saved {}", path.display()));
        Ok(())
    }

    /// Opens the "Open Scene" dialog in the current directory.
    pub fn open_dialog(&mut self) {
        self.dialog = Some(FileDialog::open(default_dialog_directory()));
    }

    /// Opens the "Save Scene As" dialog, pre-filled with the current name.
    pub fn save_dialog(&mut self) {
        let name = self
            .scene_name()
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "untitled".to_owned());
        self.dialog = Some(FileDialog::save(default_dialog_directory(), name));
    }

    /// Saves, asking for a path when there is none yet.
    pub fn save_or_ask(&mut self) {
        if self.scene.is_none() {
            self.status = Status::warning("nothing to save");
            return;
        }
        match self.save_scene() {
            Ok(()) => {}
            Err(_) => self.save_dialog(),
        }
    }

    /// Records an outcome and logs it at the matching level.
    pub fn report(&mut self, status: Status) {
        log::log!(log::Level::from(status.kind), "{}", status.text);
        self.status = status;
    }

    /// Points the viewport camera at the scene's meshes.
    fn frame_scene(&mut self, scene: &Scene) {
        match scene.mesh_bounds() {
            Some((min, max)) => self.viewport_camera.frame_aabb(min, max),
            // An empty scene still has a grid to look at, so the camera keeps its
            // default framing instead of being pointed at nothing.
            None => self.viewport_camera = OrbitCamera::default(),
        }
    }

    /// The window title, with the `*` the plan asks for while there are unsaved changes.
    ///
    /// The engine's title buffer is 64 bytes (`ENGINE_TITLE_CAPACITY`), so the platform
    /// truncates a long path on a character boundary; the file *stem* rather than the
    /// full path is what keeps that rare.
    pub fn window_title(&self) -> String {
        match self.scene_name() {
            Some(name) => {
                let star = if self.dirty { "*" } else { "" };
                format!("giochi-editor — {name}{star}")
            }
            None => format!("giochi-editor v{}", crate::VERSION),
        }
    }

    /// The scene's display name: the file stem, or the scene's own name.
    pub fn scene_name(&self) -> Option<String> {
        self.scene_path
            .as_deref()
            .and_then(Path::file_stem)
            .map(|name| name.to_string_lossy().into_owned())
            .or_else(|| self.scene.as_ref().map(|scene| scene.name.clone()))
    }
}

fn describe(error: &SceneError) -> String {
    error.to_string()
}

/// Where a dialog opens when the editor has no better idea: the working directory, which
/// is where `cargo run` put the user.
fn default_dialog_directory() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

impl From<StatusKind> for log::Level {
    fn from(kind: StatusKind) -> Self {
        match kind {
            StatusKind::Error => log::Level::Error,
            StatusKind::Warning => log::Level::Warn,
            StatusKind::Idle | StatusKind::Info => log::Level::Info,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use giochi_engine_3d::scene::SceneNodeId;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "giochi-editor-state-{name}-{}.scene",
            std::process::id()
        ))
    }

    #[test]
    fn a_fresh_editor_has_no_scene_and_no_selection() {
        let state = EditorState::new();
        assert_eq!(state.scene_path, None);
        assert_eq!(state.scene, None);
        assert!(state.selected.is_empty());
        assert_eq!(state.mode, EditorMode::Select);
        assert!(!state.dirty);
        assert!(!state.quit_requested());
        assert_eq!(
            state.window_title(),
            format!("giochi-editor v{}", crate::VERSION)
        );
    }

    #[test]
    fn panels_start_visible_and_can_be_hidden() {
        let mut state = EditorState::new();
        assert!(state.panels.hierarchy);
        assert!(state.panels.inspector);
        assert!(state.panels.asset_browser);

        state.panels.inspector = false;
        assert!(!state.panels.inspector);
        assert!(state.panels.hierarchy, "closing one panel closes one panel");
    }

    #[test]
    fn the_title_tracks_the_scene_and_the_dirty_flag() {
        let mut state = EditorState::new();
        state.scene_path = Some(PathBuf::from("/tmp/track_easy.scene"));
        assert_eq!(state.scene_name().as_deref(), Some("track_easy"));
        assert_eq!(state.window_title(), "giochi-editor — track_easy");

        state.mark_dirty();
        assert_eq!(state.window_title(), "giochi-editor — track_easy*");

        state.mark_saved();
        assert!(!state.dirty);
        assert_eq!(state.window_title(), "giochi-editor — track_easy");
    }

    #[test]
    fn selecting_and_clearing_a_selection() {
        let mut state = EditorState::new();
        state.select(Some(EntityId::new(7)));
        assert_eq!(state.primary(), Some(EntityId(7)));
        assert!(state.is_selected(EntityId(7)));
        assert_eq!(EntityId(7).to_string(), "#7");
        assert_eq!(EntityId::new(7).raw(), 7);

        state.clear_selection();
        assert!(state.selected.is_empty());
        assert_eq!(state.primary(), None);
    }

    #[test]
    fn ctrl_click_toggles_membership_and_keeps_click_order() {
        let mut state = EditorState::new();
        state.select(Some(EntityId(1)));
        state.toggle_selection(EntityId(2));
        state.toggle_selection(EntityId(3));
        assert_eq!(state.selected, vec![EntityId(1), EntityId(2), EntityId(3)]);
        assert_eq!(state.primary(), Some(EntityId(1)));

        state.toggle_selection(EntityId(2));
        assert_eq!(state.selected, vec![EntityId(1), EntityId(3)]);
        assert!(!state.is_selected(EntityId(2)));
    }

    #[test]
    fn a_plain_select_replaces_the_whole_selection() {
        let mut state = EditorState::new();
        state.select(Some(EntityId(1)));
        state.toggle_selection(EntityId(2));
        state.select(Some(EntityId(9)));
        assert_eq!(state.selected, vec![EntityId(9)]);
    }

    #[test]
    fn every_mode_has_a_name() {
        for mode in [
            EditorMode::Select,
            EditorMode::Translate,
            EditorMode::Rotate,
            EditorMode::Scale,
        ] {
            assert!(!mode.name().is_empty());
        }
        assert_eq!(EditorMode::default(), EditorMode::Select);
    }

    #[test]
    fn quitting_is_sticky() {
        let mut state = EditorState::new();
        state.request_quit();
        assert!(state.quit_requested());
    }

    #[test]
    fn a_new_scene_has_the_three_objects_and_frames_them() {
        let mut state = EditorState::new();
        state.new_scene();
        let scene = state.scene.as_ref().expect("scene");
        assert_eq!(scene.nodes.len(), 3);
        assert!(!state.dirty, "a new scene is not a modification");
        assert_eq!(state.scene_path, None);
        assert!(state.viewport_camera.distance > 0.0);
        assert!(!state.status.text.is_empty());
        assert_eq!(
            state.primary(),
            Some(EntityId(1)),
            "the first object is selected so the inspector shows something"
        );
    }

    #[test]
    fn saving_without_a_path_asks_for_save_as() {
        let mut state = EditorState::new();
        state.new_scene();
        let error = state.save_scene().expect_err("no path");
        assert!(error.contains("Save As"), "{error}");
    }

    #[test]
    fn open_save_and_reopen_preserve_the_scene() {
        let path = temp_path("roundtrip");
        let mut state = EditorState::new();
        state.new_scene();
        state.save_scene_as(&path).expect("saved");
        assert!(!state.dirty);
        assert_eq!(state.scene_path.as_deref(), Some(path.as_path()));

        let expected = state.scene.clone().expect("scene");
        let mut other = EditorState::new();
        other.open_scene(&path).expect("opened");
        assert_eq!(other.scene.as_ref(), Some(&expected));
        assert!(!other.dirty);
        assert!(other.status.text.contains("Opened"));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn opening_a_missing_file_reports_the_path_and_keeps_the_old_scene() {
        let mut state = EditorState::new();
        state.new_scene();
        let before = state.scene.clone();
        let error = state
            .open_scene("/nonexistent/definitely/missing.scene")
            .expect_err("missing file");
        assert!(error.contains("missing.scene"), "{error}");
        assert_eq!(
            state.scene, before,
            "a failed open must not clear the scene"
        );
    }

    #[test]
    fn opening_a_foreign_file_reports_why() {
        let path = temp_path("foreign");
        std::fs::write(&path, r#"{"format": "not-giochi"}"#).expect("written");
        let mut state = EditorState::new();
        let error = state.open_scene(&path).expect_err("not a scene");
        assert!(error.contains("not a giochi scene"), "{error}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_scene_name_survives_save_as_and_titles_the_window() {
        let path = temp_path("titled");
        let mut state = EditorState::new();
        state.new_scene();
        state.save_scene_as(&path).expect("saved");
        state.mark_dirty();
        assert!(state.window_title().ends_with('*'));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn the_scene_id_type_is_the_editor_id_type() {
        let id: EntityId = SceneNodeId::new(3);
        assert_eq!(id.raw(), 3);
    }
}
