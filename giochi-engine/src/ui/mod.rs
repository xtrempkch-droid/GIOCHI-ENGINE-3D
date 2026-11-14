//! In-game UI: egui on top of the engine's renderers.
//!
//! Phase 20 introduces the backend (this module) for the debug console; phase 21 adds the
//! widget facade, themes and layouts on top of it. The backend is deliberately private:
//! what a game sees is [`crate::devtools`] and, from phase 21, the C-shaped widget
//! facade — never an `egui` type in an engine signature.
//!
//! See `docs/DEVTOOLS.md` and ADR-052 for why the integration is hand-written instead of
//! using `egui-sdl2`.

pub(crate) mod egui_integration;
