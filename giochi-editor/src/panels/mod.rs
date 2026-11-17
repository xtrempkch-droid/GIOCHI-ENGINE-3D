//! Panels: one module per region of the editor's chrome.
//!
//! Phase 32 fills the hierarchy; phases 33 and 34 add the inspector and the asset browser
//! beside it. Each panel draws itself and reports what the pointer did, and the layout
//! applies the result — that is what keeps "click a row" from spreading into every panel.

pub mod hierarchy;
