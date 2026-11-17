//! Panels: one module per region of the editor's chrome.
//!
//! Phase 32 fills the hierarchy and phase 33 the inspector; phase 34 adds the asset
//! browser beside them. Each panel draws itself and reports what the pointer did, and the
//! layout applies the result — that is what keeps "click a row" from spreading into every
//! panel.

pub mod hierarchy;
pub mod inspector;
