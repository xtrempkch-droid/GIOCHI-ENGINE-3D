//! Editor session state, theme, camera, viewport and file dialog.
//!
//! Phase 30 shipped the state, the theme and the layout; phase 31 adds the viewport and
//! everything it needs (a camera, a projection and a file dialog). Keeping them separate
//! is what lets phases 32–40 add panels and commands without rewriting the loop.

pub mod autosave;
pub mod command;
pub mod dialog;
pub mod editor_state;
pub mod icons;
pub mod layout;
pub mod node_edit;
pub mod orbit_camera;
pub mod theme;
pub mod viewport;

pub use command::{AddNode, AddNodes, Command, CommandStack, RemoveNodes, Reparent, SetNode};
pub use dialog::{DialogKind, DialogOutcome, FileDialog};
pub use editor_state::{EditorMode, EditorState, EntityId, PanelVisibility, Status, StatusKind};
pub use icons::IconSet;
pub use layout::{draw, LayoutMetrics};
pub use orbit_camera::OrbitCamera;
pub use theme::EditorTheme;
pub use viewport::ViewportInput;
