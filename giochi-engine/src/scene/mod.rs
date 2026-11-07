//! Scene stack: the unit of game state the engine drives.

// The file name is mandated by the workspace layout in the project prompt.
#[allow(clippy::module_inception)]
pub mod scene;
pub mod state;

pub use scene::{Scene, SceneHandle};
pub use state::SceneManager;
