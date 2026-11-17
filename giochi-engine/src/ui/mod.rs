//! In-game UI: widgets, themes and the egui backend.
//!
//! Three layers, smallest dependence first:
//!
//! | Module | Knows about | Used by |
//! |---|---|---|
//! | [`widgets`] | nothing but the engine's math types | a game's menus |
//! | [`theme`] | egui, for the palette and text styles it applies to a context | the widget layer |
//! | [`egui_integration`] | egui + `sdl2::render` | the engine, once per frame |
//!
//! A game calls [`widgets::Button`], [`widgets::Slider`] and friends, never an `egui` type.
//! That is what phase 24 can expose through the C ABI, and what keeps a game's source
//! independent of the egui version.
//!
//! [`egui_integration`] is public because a host that owns its own loop — the visual
//! editor does — needs the same painter the engine uses. A game should not touch it.
//!
//! See `docs/UI.md` and ADR-052 (why the backend is hand-written), ADR-055 (why the
//! facade exists at all) and ADR-062 (why the backend became a public host API).

pub mod egui_integration;
pub mod theme;
pub mod widgets;

pub use egui_integration::{translate_event, EguiBackend};
pub use theme::{Theme, ThemeKind};
pub use widgets::{
    Button, Checkbox, ComboBox, Image, ProgressBar, Separator, Slider, TextInput, Ui,
};
