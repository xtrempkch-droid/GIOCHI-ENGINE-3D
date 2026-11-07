//! Platform layer: SDL2 initialisation and the window wrapper.
//!
//! This is the **only** module allowed to import `sdl2`. The core, input, render
//! and audio modules talk to the platform through these wrappers so that the
//! dependency can be swapped or mocked without touching gameplay code.

pub mod sdl;
pub mod window;

pub use sdl::{init_sdl, sdl_version, SDL_VIDEODRIVER_ENV};
pub use window::{Window, WindowConfig};
