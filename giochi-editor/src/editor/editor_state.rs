//! Everything the editor remembers between frames.
//!
//! `EditorState` is the *editor's* state, not the scene's: which file is open, what is
//! selected, which gizmo is armed, whether there are unsaved changes, and the viewport
//! camera. The scene itself is the engine's [`Scene`], loaded and saved through the
//! format in `giochi-engine-3d` — the editor never invents its own file format.

use std::path::{Path, PathBuf};

use giochi_engine_3d::scene::{Scene, SceneError};

use crate::editor::autosave::Autosave;
use crate::editor::command::{Command, CommandStack};
use crate::editor::dialog::FileDialog;
use crate::editor::orbit_camera::OrbitCamera;
use crate::panels::asset_browser::AssetBrowserState;
use crate::project::Project;
use crate::settings::EditorSettings;

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
/// `Q`/`W`/`E` arm select, translate and rotate — the same keys as every 3D package, so
/// muscle memory carries over. `R` arms scale in phase 35b.
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

    /// The tool bound to a key, if any.
    pub const fn from_key(key: char) -> Option<Self> {
        match key {
            'q' | 'Q' => Some(Self::Select),
            'w' | 'W' => Some(Self::Translate),
            'e' | 'E' => Some(Self::Rotate),
            'r' | 'R' => Some(Self::Scale),
            _ => None,
        }
    }
}

/// How far a duplicate is offset from its original, in its parent's space.
///
/// One unit along X and Z: far enough to see two objects rather than one, small enough that
/// the copy is still next to the original in a scene of a few metres.
const DUPLICATE_OFFSET: [f32; 3] = [1.0, 0.0, 1.0];

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

/// The answer to "save before quitting?".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuitChoice {
    /// Save, then quit.
    Save,
    /// Quit without saving.
    Discard,
    /// Keep editing.
    Cancel,
}

/// The state of the quit prompt while it is up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct QuitPrompt {
    /// Whether the "discard" button has been pressed once. The second press is what actually
    /// discards: throwing away unsaved work is the only destructive action in the editor, and
    /// a single misclick should not do it.
    pub discard_armed: bool,
}

/// The editor's session state.
///
/// Not `Clone`: the undo stack owns trait objects, and a cloned history would be a second
/// history of the same scene. Nothing in the editor needs to copy the session, and tests
/// compare the fields they mean (`scene`, `selected`, `dirty`). Not `PartialEq` either: the
/// asset browser holds egui texture handles.
#[derive(Debug)]
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
    /// The gizmo drag in progress, if any. One drag is one undo step (phase 37).
    pub drag: Option<crate::gizmos::Drag>,
    /// The last thing the editor did, shown in the menu bar.
    pub status: Status,
    /// The file dialog, while one is open.
    pub dialog: Option<FileDialog>,
    /// The open project, which owns `assets/` and `scenes/`.
    pub project: Option<Project>,
    /// The asset browser's filter, listing and preview.
    pub assets: AssetBrowserState,
    /// The undo/redo history. Every edit goes through it (rule 6 of the plan).
    pub commands: CommandStack,
    /// The recent list and the autosave interval, as last loaded from disk.
    pub settings: EditorSettings,
    /// Where `settings` lives, so the editor can write it back.
    pub settings_path: PathBuf,
    /// Whether a settings file has been read yet.
    ///
    /// Until it has, [`EditorState::save_settings`] writes nothing: an editor that has not
    /// looked at the disk should not create a config file, and a test that never loads
    /// settings must not touch the real home directory.
    settings_known: bool,
    /// The autosave timer.
    pub autosave: Autosave,
    /// The folder autosaves are written to. Normally the settings folder; a field so tests
    /// never write into the real home directory.
    pub autosave_dir: PathBuf,
    /// The "save before quitting?" prompt, while it is up.
    pub quit_prompt: Option<QuitPrompt>,
    /// Set when the user answered "save" on the quit prompt for a scene that had never been
    /// saved: once the save dialog produces a path, the editor quits.
    quit_after_save: bool,
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
            drag: None,
            status: Status::default(),
            dialog: None,
            project: None,
            assets: AssetBrowserState::new(),
            commands: CommandStack::default(),
            settings: EditorSettings::default(),
            settings_path: EditorSettings::default_path(),
            settings_known: false,
            autosave: Autosave::default(),
            autosave_dir: EditorSettings::config_dir(),
            quit_prompt: None,
            quit_after_save: false,
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
    ///
    /// Prefer [`EditorState::apply_command`]: an edit applied behind the stack's back cannot
    /// be undone, and this method cannot tell. It is kept for the handful of changes that are
    /// not edits (the scene's name, a project's settings) and for tests.
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Records that the scene on disk matches the editor.
    pub fn mark_saved(&mut self) {
        self.dirty = false;
        self.commands.mark_saved();
    }

    /// Applies `command` through the undo stack and reports it in the status bar.
    pub fn apply_command(&mut self, command: Box<dyn Command>) {
        self.apply_command_mergeable(command, false);
    }

    /// Applies `command`, merging it into the previous one when `merge` says the gesture is
    /// still going.
    pub fn apply_command_mergeable(&mut self, command: Box<dyn Command>, merge: bool) {
        let label = command.label();
        let Some(scene) = self.scene.as_mut() else {
            return;
        };
        self.commands.push_mergeable(scene, command, merge);
        self.dirty = self.commands.is_dirty();
        self.status = Status::info(label);
    }

    /// Undoes the most recent edit. Returns whether anything was undone.
    pub fn undo(&mut self) -> bool {
        let Some(scene) = self.scene.as_mut() else {
            return false;
        };
        let Some(label) = self.commands.undo(scene) else {
            self.status = Status::info("Nothing to undo");
            return false;
        };
        self.dirty = self.commands.is_dirty();
        self.status = Status::info(format!("Undo {label}"));
        true
    }

    /// Redoes the most recently undone edit. Returns whether anything was redone.
    pub fn redo(&mut self) -> bool {
        let Some(scene) = self.scene.as_mut() else {
            return false;
        };
        let Some(label) = self.commands.redo(scene) else {
            self.status = Status::info("Nothing to redo");
            return false;
        };
        self.dirty = self.commands.is_dirty();
        self.status = Status::info(format!("Redo {label}"));
        true
    }

    /// The label the Edit menu should show for Undo.
    pub fn undo_label(&self) -> Option<String> {
        self.commands.undo_label()
    }

    /// The label the Edit menu should show for Redo.
    pub fn redo_label(&self) -> Option<String> {
        self.commands.redo_label()
    }

    /// Asks the loop to stop after the current frame.
    ///
    /// With unsaved changes this opens the "save before quitting?" prompt instead, because a
    /// close button that silently discards an hour of work is the one bug an editor cannot
    /// ship.
    pub fn request_quit(&mut self) {
        if self.dirty && self.scene.is_some() {
            self.quit_prompt = Some(QuitPrompt::default());
        } else {
            self.quit_requested = true;
        }
    }

    /// Forces the loop to stop, whatever the answer to the prompt was.
    pub fn quit_now(&mut self) {
        self.quit_prompt = None;
        self.quit_requested = true;
        let _ = self.save_settings();
    }

    /// Answers the quit prompt.
    pub fn answer_quit(&mut self, choice: QuitChoice) {
        self.quit_prompt = None;
        match choice {
            QuitChoice::Save => match self.save_scene() {
                Ok(()) => self.quit_now(),
                Err(_) => {
                    // No path yet: ask for one, and quit once it is written.
                    self.quit_after_save = true;
                    self.save_dialog();
                }
            },
            QuitChoice::Discard => self.quit_now(),
            QuitChoice::Cancel => {
                self.quit_after_save = false;
                self.status = Status::info("Quit cancelled");
            }
        }
    }

    /// Loads the settings file (or the defaults, if it is not there yet).
    pub fn load_settings(&mut self) {
        self.settings = EditorSettings::load(&self.settings_path);
        self.settings_known = true;
        self.autosave.reset(self.settings.autosave_interval());
    }

    /// Writes the settings file.
    ///
    /// # Errors
    ///
    /// The filesystem's. A failure is logged rather than shown: not remembering a recent
    /// list is not a reason to interrupt anyone.
    pub fn save_settings(&self) -> std::io::Result<()> {
        if !self.settings_known {
            return Ok(());
        }
        self.settings.save(&self.settings_path)
    }

    /// Records `path` in the recent list and writes the settings file.
    pub fn remember(&mut self, kind: crate::settings::RecentKind, path: impl AsRef<Path>) {
        self.settings.remember(kind, path);
        if let Err(error) = self.save_settings() {
            log::warn!("could not write {}: {error}", self.settings_path.display());
        }
    }

    /// The entries `File > Open Recent` should list for `kind`.
    pub fn recent(&self, kind: crate::settings::RecentKind) -> Vec<crate::settings::RecentEntry> {
        self.settings.recent_of(kind).cloned().collect()
    }

    /// Drops the recent list, on disk too.
    pub fn clear_recent(&mut self) {
        let count = self.settings.clear_recent();
        if let Err(error) = self.save_settings() {
            log::warn!("could not write {}: {error}", self.settings_path.display());
        }
        self.status = Status::info(format!("Cleared {count} recent entries"));
    }

    /// Offers the autosave of the current scene, if it is newer than the file.
    pub fn find_autosave(&self) -> Option<PathBuf> {
        let name = self.scene_name().unwrap_or_else(|| "untitled".to_owned());
        crate::editor::autosave::find(self.scene_path.as_deref(), &name, &self.autosave_dir)
    }

    /// Loads the scene's autosave over the open scene.
    pub fn recover_autosave(&mut self) {
        let Some(path) = self.find_autosave() else {
            self.report(Status::info("No autosave to recover"));
            return;
        };
        match crate::editor::autosave::recover(&path) {
            Ok(scene) => {
                let first = scene.nodes.first().map(|node| node.id);
                self.frame_scene(&scene);
                self.scene = Some(scene);
                self.select(first);
                self.commands.clear();
                // Recovered work is by definition unsaved: the file on disk is older.
                self.dirty = true;
                self.status = Status::info(format!("Recovered {}", path.display()));
            }
            Err(error) => self.report(Status::error(format!("Could not recover: {error}"))),
        }
    }

    /// Runs the autosave timer. Returns whether a file was written.
    ///
    /// Called once per frame with the frame's `now`; the timer decides. Only a dirty scene
    /// with a real scene open is written, and the interval comes from the settings.
    pub fn tick_autosave(&mut self, now: std::time::Instant) -> bool {
        if !self.autosave.due(now) {
            return false;
        }
        if !self.dirty || self.scene.is_none() {
            return false;
        }
        let name = self.scene_name().unwrap_or_else(|| "untitled".to_owned());
        let scene_path = self.scene_path.clone();
        let Some(scene) = self.scene.as_ref() else {
            return false;
        };
        match crate::editor::autosave::write(scene, scene_path.as_deref(), &self.autosave_dir) {
            Ok(path) => {
                self.autosave.note(path.clone());
                log::info!("autosaved to {}", path.display());
                true
            }
            Err(error) => {
                log::warn!("autosave for {name} failed: {error}");
                false
            }
        }
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
        self.commands.clear();
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
        // A new file is a new history: undoing into the previous scene would be a surprise.
        self.commands.clear();
        self.mark_saved();
        self.remember(crate::settings::RecentKind::Scene, path);
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
        // A scene saved into the project is a scene of the project: `project.toml` is the
        // list a game (or the user) reads to find them.
        if let Some(project) = self.project.as_mut() {
            if project.register_scene(path) {
                if let Err(error) = project.save() {
                    log::warn!("could not update project.toml: {error}");
                }
            }
        }
        self.mark_saved();
        self.remember(crate::settings::RecentKind::Scene, path);
        self.status = Status::info(format!("Saved {}", path.display()));
        if self.quit_after_save {
            // The user asked to save and quit from a scene that had never been saved; the
            // dialog has now produced the path.
            self.quit_after_save = false;
            self.quit_now();
        }
        Ok(())
    }

    /// Where a scene dialog should start: the project's `scenes/`, or the cwd.
    pub fn scenes_directory(&self) -> PathBuf {
        match &self.project {
            Some(project) => project.scenes_dir(),
            None => default_dialog_directory(),
        }
    }

    /// Opens the "Open Scene" dialog in the project's scenes folder.
    pub fn open_dialog(&mut self) {
        self.dialog = Some(FileDialog::open(self.scenes_directory()));
    }

    /// Opens the "Save Scene As" dialog, pre-filled with the current name.
    pub fn save_dialog(&mut self) {
        let name = self
            .scene_name()
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "untitled".to_owned());
        self.dialog = Some(FileDialog::save(self.scenes_directory(), name));
    }

    /// Opens the "Open Project" dialog in the current directory.
    pub fn open_project_dialog(&mut self) {
        self.dialog = Some(FileDialog::open_project(default_dialog_directory()));
    }

    /// Opens a "New Project" dialog: the user picks the parent folder and types a name.
    pub fn new_project_dialog(&mut self) {
        self.dialog = Some(FileDialog::new_project(default_dialog_directory()));
    }

    /// Opens the "Import Asset" dialog in the current directory.
    pub fn import_dialog(&mut self) {
        self.dialog = Some(FileDialog::import(default_dialog_directory()));
    }

    /// Makes `project` the open project, refreshing the asset browser.
    ///
    /// Warnings from [`Project::validate`] go to the status line: a project with a missing
    /// scene is still usable, and refusing to open it would be worse than saying so.
    pub fn set_project(&mut self, project: Project) {
        let warnings = project.validate().unwrap_or_default();
        let root = project.root.clone();
        self.project = Some(project);
        // A project and a scene at the same path are two entries, so both menus can list it.
        self.remember(crate::settings::RecentKind::Project, root);
        self.assets.clear();
        self.assets.refresh(self.project.as_ref());
        self.status = match warnings.first() {
            Some(warning) => Status::warning(warning.clone()),
            None => Status::info(format!(
                "Project {}",
                self.project
                    .as_ref()
                    .map(|p| p.name.clone())
                    .unwrap_or_default()
            )),
        };
    }

    /// Creates a project under `parent/name` and opens it.
    pub fn create_project(&mut self, parent: impl AsRef<Path>, name: &str) -> Result<(), String> {
        let root = parent.as_ref().join(name.trim());
        let project = Project::create(&root, name)?;
        self.set_project(project);
        Ok(())
    }

    /// Opens an existing project directory.
    pub fn open_project(&mut self, root: impl AsRef<Path>) -> Result<(), String> {
        let project = Project::open(root)?;
        self.set_project(project);
        Ok(())
    }

    /// Imports `source` into the project's assets and refreshes the browser.
    pub fn import_asset(&mut self, source: impl AsRef<Path>) -> Result<PathBuf, String> {
        let project = self
            .project
            .as_mut()
            .ok_or_else(|| "open a project before importing assets".to_owned())?;
        let entry = project.import_asset(source.as_ref())?;
        self.assets.refresh(self.project.as_ref());
        self.assets.selected = Some(entry.path.clone());
        self.status = Status::info(format!("Imported {}", entry.label));
        Ok(entry.path)
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

    /// Creates a node of `kind` and selects it, as a child of the selection when exactly one
    /// node is selected.
    ///
    /// A child is created at its parent's origin: the world position of a new object under
    /// a group is the group's, and putting it at the kind's default would place it at those
    /// coordinates *inside* the group, which is almost never where it was wanted.
    pub fn create_node(&mut self, kind: crate::editor::node_edit::NewNodeKind) {
        let parent = if self.selected.len() == 1 {
            self.primary()
        } else {
            None
        };
        let position = if parent.is_some() {
            Some([0.0, 0.0, 0.0])
        } else {
            None
        };
        let Some(scene) = self.scene.as_mut() else {
            self.report(Status::warning("No scene open"));
            return;
        };
        match crate::editor::node_edit::create(scene, kind, parent, position) {
            Ok(node) => {
                let id = node.id;
                self.select(Some(id));
                self.apply_command(Box::new(crate::editor::command::AddNode::new(node)));
            }
            Err(error) => self.report(Status::warning(format!(
                "Could not add {}: {error}",
                kind.label()
            ))),
        }
    }

    /// Deletes every selected node and its subtree.
    pub fn delete_selected(&mut self) {
        if self.selected.is_empty() {
            return;
        }
        let selected = self.selected.clone();
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let command =
            crate::editor::command::RemoveNodes::capture(scene, &selected, "Delete node(s)");
        if command.nodes.is_empty() {
            self.report(Status::warning("Nothing to delete"));
            return;
        }
        let count = command.nodes.len();
        self.clear_selection();
        self.apply_command(Box::new(command));
        self.status = Status::info(format!("Deleted {count} node(s)"));
    }

    /// Duplicates every selected node, selecting the copies.
    ///
    /// A node whose ancestor is also selected is skipped: duplicating a group and its child
    /// separately would produce the child twice, once inside the copied group and once as
    /// its own copy.
    pub fn duplicate_selected(&mut self) {
        let selected = self.selected.clone();
        let Some(scene) = self.scene.as_mut() else {
            return;
        };
        let roots: Vec<EntityId> = selected
            .iter()
            .copied()
            .filter(|id| scene.node(*id).is_some())
            .filter(|id| {
                !selected
                    .iter()
                    .any(|other| other != id && scene.subtree(*other).contains(id))
            })
            .collect();
        if roots.is_empty() {
            return;
        }
        let mut copies = Vec::new();
        for id in roots {
            copies.extend(crate::editor::node_edit::duplicate(
                scene,
                id,
                DUPLICATE_OFFSET,
            ));
        }
        let created = copies.len();
        if created > 0 {
            // `duplicate` already added the nodes; the command must not add them twice, so
            // the stack is given an entry that knows how to take them away and put them back.
            let nodes: Vec<_> = copies
                .iter()
                .filter_map(|id| scene.node(*id).cloned())
                .collect();
            self.selected = copies;
            self.apply_command(Box::new(crate::editor::command::AddNodes::new(
                nodes,
                "Duplicate node(s)",
            )));
            self.status = Status::info(format!("Duplicated {created} node(s)"));
        }
    }

    /// Moves `id` under `new_parent`, reporting a refusal instead of panicking.
    pub fn reparent(&mut self, id: EntityId, new_parent: Option<EntityId>) {
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let before = scene.node(id).and_then(|node| node.parent);
        match crate::editor::node_edit::reparent(
            self.scene.as_mut().expect("scene"),
            id,
            new_parent,
        ) {
            Ok(()) => {
                if before == new_parent {
                    return;
                }
                // `reparent` already moved the node; the command records the pair so undo can
                // put it back where it was.
                self.apply_command(Box::new(crate::editor::command::Reparent {
                    id,
                    before,
                    after: new_parent,
                }));
                self.status = Status::info(match new_parent {
                    Some(parent) => format!("Moved {id} under {parent}"),
                    None => format!("Moved {id} to the root"),
                });
            }
            Err(error) => self.report(Status::warning(error)),
        }
    }

    /// Arms `mode`, cancelling a drag that belonged to the old one.
    pub fn set_mode(&mut self, mode: EditorMode) {
        if self.mode != mode {
            self.drag = None;
            self.mode = mode;
            self.status = Status::info(format!("{} tool", mode.name()));
        }
    }

    /// The node a gizmo should attach to: the primary selection, if exactly one object is
    /// selected and it exists in the scene.
    pub fn gizmo_target(&self) -> Option<&giochi_engine_3d::scene::SceneNode> {
        if self.selected.len() != 1 {
            return None;
        }
        let id = self.primary()?;
        self.scene.as_ref()?.node(id)
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

    fn state_with_scene() -> EditorState {
        let mut state = EditorState::new();
        state.new_scene();
        state
    }

    #[test]
    fn creating_a_node_adds_selects_and_dirties() {
        let mut state = state_with_scene();
        let before = state.scene.as_ref().expect("scene").nodes.len();
        state.create_node(crate::editor::node_edit::NewNodeKind::Sphere);
        let scene = state.scene.as_ref().expect("scene");
        assert_eq!(scene.nodes.len(), before + 1);
        let new_node = scene.nodes.last().expect("new");
        assert_eq!(new_node.name, "Sphere");
        assert_eq!(state.primary(), Some(new_node.id));
        assert!(state.dirty);
        assert!(
            state.status.text.contains("Sphere"),
            "{}",
            state.status.text
        );
    }

    #[test]
    fn a_second_node_of_the_same_kind_gets_a_numbered_name() {
        let mut state = state_with_scene();
        state.create_node(crate::editor::node_edit::NewNodeKind::Cube);
        state.create_node(crate::editor::node_edit::NewNodeKind::Cube);
        let scene = state.scene.as_ref().expect("scene");
        let names: Vec<&str> = scene
            .nodes
            .iter()
            .filter(|node| node.name.starts_with("Cube"))
            .map(|node| node.name.as_str())
            .collect();
        assert_eq!(names, ["Cube", "Cube 2", "Cube 3"]);
    }

    #[test]
    fn creating_with_one_node_selected_nests_under_it() {
        let mut state = state_with_scene();
        state.select(Some(EntityId(2)));
        state.create_node(crate::editor::node_edit::NewNodeKind::Group);
        let scene = state.scene.as_ref().expect("scene");
        let new_node = scene.nodes.last().expect("new");
        assert_eq!(new_node.parent, Some(EntityId(2)));
        assert_eq!(new_node.transform.position, [0.0, 0.0, 0.0]);
    }

    #[test]
    fn deleting_the_selection_removes_it_and_clears_it() {
        let mut state = state_with_scene();
        state.select(Some(EntityId(2)));
        state.delete_selected();
        let scene = state.scene.as_ref().expect("scene");
        assert!(scene.node(EntityId(2)).is_none());
        assert!(state.selected.is_empty());
        assert!(state.dirty);
        assert!(
            state.status.text.contains("Deleted"),
            "{}",
            state.status.text
        );
    }

    #[test]
    fn duplicating_selects_the_copies_and_not_the_originals() {
        let mut state = state_with_scene();
        state.select(Some(EntityId(1)));
        state.duplicate_selected();
        let scene = state.scene.as_ref().expect("scene");
        assert_eq!(scene.nodes.len(), 4);
        assert_eq!(state.selected.len(), 1);
        let copy = state.selected[0];
        assert_ne!(copy, EntityId(1));
        let node = scene.node(copy).expect("copy");
        assert_eq!(node.name, "Cube 2");
        assert_eq!(node.mesh, scene.node(EntityId(1)).expect("original").mesh);
    }

    #[test]
    fn duplicating_a_parent_and_its_child_copies_the_tree_once() {
        let mut state = state_with_scene();
        state.selected = vec![EntityId(2), EntityId(3)];
        state.duplicate_selected();
        let scene = state.scene.as_ref().expect("scene");
        // Two originals + two copies: the light and camera were not part of the selection.
        assert_eq!(scene.nodes.len(), 5);
        assert_eq!(
            state.selected.len(),
            2,
            "the copies of the parent and of its child, in one drag"
        );
    }

    #[test]
    fn reparenting_with_a_cycle_is_refused_with_a_warning() {
        let mut state = state_with_scene();
        // The Sun becomes a child of the Camera, then the Camera is asked to become a child
        // of the Sun.
        state.reparent(EntityId(3), Some(EntityId(2)));
        state.reparent(EntityId(2), Some(EntityId(3)));
        let scene = state.scene.as_ref().expect("scene");
        assert_eq!(
            scene.node(EntityId(3)).expect("sun").parent,
            Some(EntityId(2))
        );
        assert_eq!(scene.node(EntityId(2)).expect("camera").parent, None);
        assert_eq!(state.status.kind, StatusKind::Warning);
    }

    #[test]
    fn every_node_edit_is_undoable() {
        let mut state = state_with_scene();
        let before = state.scene.as_ref().expect("scene").nodes.len();

        state.clear_selection();
        state.create_node(crate::editor::node_edit::NewNodeKind::Cube);
        let created = state.primary().expect("created");
        assert_eq!(state.scene.as_ref().expect("scene").nodes.len(), before + 1);
        assert!(state.undo());
        assert!(state.scene.as_ref().expect("scene").node(created).is_none());
        assert!(state.redo());
        assert!(state.scene.as_ref().expect("scene").node(created).is_some());

        state.select(Some(created));
        state.duplicate_selected();
        let copies = state.selected.clone();
        assert_eq!(copies.len(), 1);
        assert!(state.undo());
        assert!(state
            .scene
            .as_ref()
            .expect("scene")
            .node(copies[0])
            .is_none());

        state.select(Some(created));
        state.delete_selected();
        assert!(state.scene.as_ref().expect("scene").node(created).is_none());
        assert!(state.undo());
        assert!(state.scene.as_ref().expect("scene").node(created).is_some());

        let parent_before = state
            .scene
            .as_ref()
            .expect("scene")
            .node(EntityId(2))
            .expect("camera")
            .parent;
        state.reparent(EntityId(2), Some(EntityId(1)));
        assert!(state.undo());
        assert_eq!(
            state
                .scene
                .as_ref()
                .expect("scene")
                .node(EntityId(2))
                .expect("camera")
                .parent,
            parent_before
        );
    }

    #[test]
    fn undoing_everything_leaves_the_scene_clean() {
        let mut state = state_with_scene();
        let before = state.scene.as_ref().expect("scene").nodes.len();
        state.clear_selection();
        state.create_node(crate::editor::node_edit::NewNodeKind::Light);
        state.create_node(crate::editor::node_edit::NewNodeKind::Sphere);
        assert!(state.dirty);
        while state.undo() {}
        assert_eq!(state.scene.as_ref().expect("scene").nodes.len(), before);
        assert!(!state.dirty, "back at the state the scene was created in");
        assert!(state.undo_label().is_none());
    }

    #[test]
    fn opening_a_scene_forgets_the_history_of_the_old_one() {
        let path = temp_path("history");
        let mut state = state_with_scene();
        state.save_scene_as(&path).expect("saved");
        state.clear_selection();
        state.create_node(crate::editor::node_edit::NewNodeKind::Cube);
        assert!(state.commands.can_undo());

        state.open_scene(&path).expect("opened");
        assert!(!state.commands.can_undo(), "a new file is a new history");
        assert!(!state.dirty);
        let _ = std::fs::remove_file(&path);
    }

    fn state_with_settings(dir: &std::path::Path) -> EditorState {
        let mut state = EditorState::new();
        state.settings_path = dir.join(crate::settings::SETTINGS_FILE);
        state.autosave_dir = dir.join("config");
        state.load_settings();
        state.new_scene();
        state
    }

    #[test]
    fn quitting_with_unsaved_changes_asks_instead_of_quitting() {
        let dir = std::env::temp_dir().join(format!("giochi-quit-{}", std::process::id()));
        let mut state = state_with_settings(&dir);
        state.mark_dirty();
        state.request_quit();
        assert!(!state.quit_requested(), "the prompt comes first");
        assert!(state.quit_prompt.is_some());

        state.answer_quit(crate::editor::editor_state::QuitChoice::Cancel);
        assert!(!state.quit_requested());
        assert!(state.quit_prompt.is_none());
        assert!(state.dirty, "cancelling must not touch the scene");
    }

    #[test]
    fn discarding_quits_without_saving() {
        let dir = std::env::temp_dir().join(format!("giochi-discard-{}", std::process::id()));
        let mut state = state_with_settings(&dir);
        state.mark_dirty();
        state.request_quit();
        state.answer_quit(crate::editor::editor_state::QuitChoice::Discard);
        assert!(state.quit_requested());
        assert!(state.quit_prompt.is_none());
    }

    #[test]
    fn a_clean_scene_quits_without_asking() {
        let dir = std::env::temp_dir().join(format!("giochi-clean-{}", std::process::id()));
        let mut state = state_with_settings(&dir);
        assert!(!state.dirty);
        state.request_quit();
        assert!(state.quit_requested());
        assert!(state.quit_prompt.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn quitting_from_an_unsaved_scene_asks_for_a_path_then_quits() {
        let dir = std::env::temp_dir().join(format!("giochi-unsaved-{}", std::process::id()));
        let mut state = state_with_settings(&dir);
        state.mark_dirty();
        state.request_quit();
        state.answer_quit(crate::editor::editor_state::QuitChoice::Save);
        assert!(!state.quit_requested(), "it needs a path first");
        assert!(state.dialog.is_some(), "the save dialog is open");

        let path = dir.join("saved.scene");
        state.save_scene_as(&path).expect("saved");
        assert!(state.quit_requested(), "and now it quits");
        assert!(!state.dirty);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn saving_from_a_scene_with_a_path_quits_straight_away() {
        let dir = std::env::temp_dir().join(format!("giochi-saved-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let mut state = state_with_settings(&dir);
        let path = dir.join("scene.scene");
        state.save_scene_as(&path).expect("saved");
        state.mark_dirty();
        state.request_quit();
        state.answer_quit(crate::editor::editor_state::QuitChoice::Save);
        assert!(state.quit_requested());
        assert!(!state.dirty);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn opening_and_saving_remember_the_path() {
        let dir = std::env::temp_dir().join(format!("giochi-recent-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let mut state = state_with_settings(&dir);
        let path = dir.join("remembered.scene");
        state.save_scene_as(&path).expect("saved");
        assert!(state
            .recent(crate::settings::RecentKind::Scene)
            .iter()
            .any(|entry| entry.path == path));
        // The list was written to disk, not only kept in memory.
        let reloaded = crate::settings::EditorSettings::load(&state.settings_path);
        assert!(reloaded.recent.iter().any(|entry| entry.path == path));

        state.clear_recent();
        assert!(state.recent(crate::settings::RecentKind::Scene).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_autosave_timer_only_writes_a_dirty_scene() {
        let dir = std::env::temp_dir().join(format!("giochi-autotick-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let mut state = state_with_settings(&dir);
        state.settings.autosave_seconds = 1;
        state.autosave.reset(state.settings.autosave_interval());
        let start = std::time::Instant::now();
        assert!(
            !state.tick_autosave(start),
            "the timer starts on the first tick"
        );
        assert!(
            !state.tick_autosave(start + std::time::Duration::from_secs(2)),
            "a clean scene is not written"
        );
        state.mark_dirty();
        assert!(state.tick_autosave(start + std::time::Duration::from_secs(4)));
        assert!(state.autosave.last_path().is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_scene_id_type_is_the_editor_id_type() {
        let id: EntityId = SceneNodeId::new(3);
        assert_eq!(id.raw(), 3);
    }
}
