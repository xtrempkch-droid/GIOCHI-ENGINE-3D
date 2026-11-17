//! Everything the editor remembers between frames.
//!
//! `EditorState` is the *editor's* state, not the scene's: which file is open, what is
//! selected, which gizmo is armed, whether there are unsaved changes. The scene itself
//! arrives in phase 31 and the selection becomes a scene entity then — which is why
//! [`EntityId`] is a stable value and not a pointer (phase 38 saves it in the scene
//! metadata).

use std::fmt;
use std::path::{Path, PathBuf};

/// Identifies one object in the edited scene.
///
/// A plain `u64` rather than an index into a `Vec`: indices shift when a node is deleted
/// or reparented, and an undo stack that stores indices undoes the wrong node. The scene
/// format assigns these and phase 38 persists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntityId(pub u64);

impl EntityId {
    /// Wraps a raw identifier.
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// The raw identifier, for the scene format.
    pub const fn raw(self) -> u64 {
        self.0
    }
}

impl fmt::Display for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// What dragging in the viewport does.
///
/// The toolbar and the `Q`/`W`/`E`/`R` shortcuts arrive with the gizmos (phases 35a and
/// 35b); phase 30 only carries the value so the rest of the editor can be written
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

/// Which of the placeholder panels are visible.
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
    /// The selected object, if any. Phase 32 turns this into a multi-selection.
    pub selected: Option<EntityId>,
    /// The armed viewport tool.
    pub mode: EditorMode,
    /// Whether the scene has unsaved changes.
    pub dirty: bool,
    /// Which placeholder panels are shown.
    pub panels: PanelVisibility,
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
            selected: None,
            mode: EditorMode::Select,
            dirty: false,
            panels: PanelVisibility::default(),
            quit_requested: false,
        }
    }

    /// The current selection, replacing whatever was selected.
    pub fn select(&mut self, entity: Option<EntityId>) {
        self.selected = entity;
    }

    /// Forgets the selection.
    pub fn clear_selection(&mut self) {
        self.selected = None;
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

    /// The file stem of the open scene, if there is one.
    pub fn scene_name(&self) -> Option<String> {
        self.scene_path
            .as_deref()
            .and_then(Path::file_stem)
            .map(|name| name.to_string_lossy().into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_editor_has_no_scene_and_no_selection() {
        let state = EditorState::new();
        assert_eq!(state.scene_path, None);
        assert_eq!(state.selected, None);
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
        assert_eq!(state.selected, Some(EntityId(7)));
        assert_eq!(EntityId(7).to_string(), "#7");
        assert_eq!(EntityId::new(7).raw(), 7);

        state.clear_selection();
        assert_eq!(state.selected, None);
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
}
