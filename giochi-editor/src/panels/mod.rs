//! Panels: one module per region of the editor's chrome.
//!
//! Phase 32 fills the hierarchy, phase 33 the inspector and phase 34 the asset browser.
//! Each panel draws itself and reports what the pointer did, and the layout applies the
//! result — that is what keeps "click a row" from spreading into every panel.

pub mod asset_browser;
pub mod hierarchy;
pub mod inspector;
