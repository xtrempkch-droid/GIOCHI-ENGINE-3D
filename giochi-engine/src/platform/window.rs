//! Main window wrapper.

use std::fmt;
use std::rc::Rc;

use crate::error::GiochiError;
use crate::platform::sdl::{Platform, SdlContext};

/// Descriptor for creating the main window.
///
/// # C ABI
///
/// The window is created by `giochi_engine_init` in the C ABI; this struct has
/// no direct C counterpart and exists only for the Rust facade.
///
/// Size: 44 bytes, alignment: 4 bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowConfig {
    /// Width in physical pixels.
    pub width: u32,
    /// Height in physical pixels.
    pub height: u32,
    /// `0` = windowed, any other value = fullscreen desktop (required on KMSDRM,
    /// which has no window manager).
    pub fullscreen: u32,
    /// Reserved for future ABI-compatible fields. Must be zero.
    pub _reserved: [u32; 8],
}

impl WindowConfig {
    /// Creates a windowed configuration.
    pub const fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            fullscreen: 0,
            _reserved: [0; 8],
        }
    }

    /// Returns the same configuration marked fullscreen.
    pub const fn with_fullscreen(mut self) -> Self {
        self.fullscreen = 1;
        self
    }

    /// Returns the same configuration with an explicit mode.
    pub const fn fullscreen(mut self, fullscreen: bool) -> Self {
        self.fullscreen = if fullscreen { 1 } else { 0 };
        self
    }
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self::new(1280, 720)
    }
}

/// The engine's single OS window.
///
/// Owns the SDL window and keeps the SDL context alive, so it is safe to drop the
/// [`Platform`] that created it first. The window has no renderer of its own;
/// phase 2 attaches the 2D [`crate::render::RenderContext`] to it.
#[repr(C)]
pub struct Window {
    inner: sdl2::video::Window,
    /// Keeps the SDL video subsystem alive for at least as long as the window.
    _context: Rc<SdlContext>,
    _reserved: [u32; 4],
}

impl fmt::Debug for Window {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Window")
            .field("size", &self.size())
            .field("fullscreen", &self.is_fullscreen())
            .finish_non_exhaustive()
    }
}

impl Window {
    /// Creates the window described by `config`.
    ///
    /// `config.fullscreen` maps to SDL's "fullscreen desktop" mode, which is the
    /// only mode KMSDRM supports.
    pub fn new(
        platform: &Platform,
        config: WindowConfig,
        title: &str,
    ) -> Result<Window, GiochiError> {
        if config.width == 0 || config.height == 0 {
            return Err(GiochiError::InvalidArgument(
                "window width and height must be non-zero",
            ));
        }

        let video = platform.sdl_context().video();
        let mut builder = video.window(title, config.width, config.height);

        if config.fullscreen != 0 {
            builder.fullscreen_desktop();
        } else {
            builder.position_centered();
        }

        let inner = builder
            .build()
            .map_err(|err| GiochiError::Platform(format!("failed to create window: {err}")))?;

        Ok(Window {
            inner,
            _context: Rc::clone(platform.sdl_context()),
            _reserved: [0; 4],
        })
    }

    /// Returns the drawable size in physical pixels.
    pub fn size(&self) -> (u32, u32) {
        self.inner.size()
    }

    /// Returns the size SDL will actually render into, in physical pixels.
    ///
    /// Differs from [`Window::size`] on high-DPI displays.
    pub fn drawable_size(&self) -> (u32, u32) {
        self.inner.drawable_size()
    }

    /// Replaces the window title.
    pub fn set_title(&mut self, title: &str) -> Result<(), GiochiError> {
        self.inner
            .set_title(title)
            .map_err(|err| GiochiError::Platform(format!("invalid window title: {err}")))
    }

    /// Returns `true` when the window is in fullscreen desktop mode.
    pub fn is_fullscreen(&self) -> bool {
        let flag = sdl2::sys::SDL_WindowFlags::SDL_WINDOW_FULLSCREEN_DESKTOP as u32;
        self.inner.window_flags() & flag != 0
    }

    /// Underlying SDL window, for the renderer created in phase 2.
    // Consumed by `RenderContext::new` in phase 2; until then it is only a
    // seam, so the dead-code lint would be noise.
    #[allow(dead_code)]
    pub(crate) fn sdl_window(&self) -> &sdl2::video::Window {
        &self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_windowed_1280x720() {
        let config = WindowConfig::default();

        assert_eq!((config.width, config.height), (1280, 720));
        assert_eq!(config.fullscreen, 0);
        assert_eq!(config._reserved, [0; 8]);
    }

    #[test]
    fn with_fullscreen_sets_the_flag_without_touching_reserved() {
        let config = WindowConfig::new(640, 480).with_fullscreen();

        assert_eq!(config.fullscreen, 1);
        assert_eq!(config._reserved, [0; 8]);
    }

    #[test]
    fn fullscreen_toggle_is_reversible() {
        let config = WindowConfig::new(640, 480)
            .fullscreen(true)
            .fullscreen(false);

        assert_eq!(config.fullscreen, 0);
    }
}
