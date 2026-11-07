//! Platform layer: SDL2 initialisation and the window wrapper.
//!
//! This is the **only** module allowed to import `sdl2`. The core, input, render
//! and audio modules talk to the platform through these wrappers so that the
//! dependency can be swapped or mocked without touching gameplay code.

pub mod sdl;
pub mod window;

pub use sdl::{
    has_drm_device, quit_signal, sdl_version, Platform, QuitSignal, VideoDriverChoice,
    DRM_DIRECTORY, HINT_KMSDRM_DEVICE_INDEX, HINT_KMSDRM_REQUIRE_DRM_MASTER, HINT_VIDEO_EGL_DRIVER,
    HINT_VIDEO_GL_DRIVER, KMSDRM_VIDEO_DRIVER, SDL_VIDEODRIVER_ENV,
};
pub use window::{Window, WindowConfig};
