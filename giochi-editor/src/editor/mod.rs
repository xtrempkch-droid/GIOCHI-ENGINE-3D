//! Editor session state, theme and panel layout.
//!
//! Phase 30 ships these three modules. The panels' *contents* arrive in phases 32–34;
//! keeping the state and the layout separate now is what lets them be added one at a
//! time without rewriting the loop.

pub mod editor_state;
pub mod layout;
pub mod theme;

pub use editor_state::{EditorMode, EditorState, EntityId, PanelVisibility};
pub use layout::{draw, LayoutMetrics};
pub use theme::EditorTheme;
