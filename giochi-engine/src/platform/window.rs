//! Main window wrapper.

use crate::error::GiochiError;

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
    /// `0` = windowed, any other value = fullscreen desktop (needed on KMSDRM).
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
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self::new(1280, 720)
    }
}

/// The engine's single OS window.
///
/// Wraps the SDL2 window; owns it for the whole engine lifetime.
#[repr(C)]
#[derive(Debug)]
pub struct Window {
    _reserved: [u32; 4],
}

impl Window {
    /// Creates the window described by `config`, with the given title.
    ///
    /// Implemented in phase 1.
    pub fn new(_config: WindowConfig, _title: &str) -> Result<Window, GiochiError> {
        todo!("fase 1: criar a janela SDL2")
    }

    /// Returns the drawable size in physical pixels.
    ///
    /// Implemented in phase 1.
    pub fn size(&self) -> (u32, u32) {
        todo!("fase 1: consultar o tamanho da janela")
    }
}
