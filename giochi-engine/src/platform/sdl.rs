//! SDL2 initialisation.
//!
//! On the production target (Linux consoles) the engine renders straight to the
//! DRM/KMS device through SDL2's `kmsdrm` video driver. The driver is selected
//! through the environment so developers can keep using X11/Wayland locally:
//!
//! * `SDL_VIDEODRIVER=kmsdrm` — force the KMSDRM backend (set automatically by
//!   [`init_sdl`] only when the variable is absent, preserving explicit user
//!   choices such as `SDL_VIDEODRIVER=x11` on a desktop);
//! * `SDL_KMSDRM_REQUIRE_DRM_MASTER=1` — require DRM master (needed when the
//!   engine owns the console and must page-flip directly);
//! * `SDL_KMSDRM_DEVICE_INDEX=<n>` — pick a specific `/dev/dri/cardN`;
//! * `SDL_VIDEO_EGL_DRIVER` — EGL driver name used by the KMSDRM backend;
//! * `SDL_VIDEO_GL_DRIVER` — GL/GLES library used for the EGL context.
//!
//! The 2D renderer uses SDL2's `render` API (software/GL under the hood); no GL
//! symbol is referenced from Rust.

use crate::error::GiochiError;

/// Environment variable used to select the SDL2 video driver.
pub const SDL_VIDEODRIVER_ENV: &str = "SDL_VIDEODRIVER";

/// Video driver used when [`SDL_VIDEODRIVER_ENV`] is not set.
pub const KMSDRM_VIDEO_DRIVER: &str = "kmsdrm";

/// Returns the SDL2 version linked into the engine as `(major, minor, patch)`.
///
/// Useful for diagnostics: the engine builds SDL2 from source (`bundled`), so
/// this is the version the binary actually contains, not the one installed on
/// the system.
pub fn sdl_version() -> (u32, u32, u32) {
    let version = sdl2::version::version();
    (
        u32::from(version.major),
        u32::from(version.minor),
        u32::from(version.patch),
    )
}

/// Initialises the SDL2 subsystems used by the engine.
///
/// Implemented in phase 1 (video/timer/events/controller, KMSDRM hints and a
/// descriptive error when `/dev/dri` is not accessible).
pub fn init_sdl() -> Result<(), GiochiError> {
    todo!("fase 1: inicializar SDL2 e aplicar hints de KMSDRM")
}
