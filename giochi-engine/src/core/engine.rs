//! Engine facade and configuration.
//!
//! [`Engine`] is the idiomatic Rust entry point: it owns the platform, renderer,
//! input and audio subsystems and drives a [`Scene`] through the fixed-timestep
//! loop. The C ABI exposes the same functionality as a singleton
//! (`giochi_engine_init` / `giochi_engine_run` / `giochi_engine_shutdown`);
//! see `docs/C_ABI_CONTRACT.md`.

use crate::error::GiochiError;
use crate::scene::Scene;

/// Capacity of [`EngineConfig::title`], in bytes, including the trailing NUL.
pub const ENGINE_TITLE_CAPACITY: usize = 64;

/// Configuration passed to [`Engine::new`].
///
/// The title is a fixed-size NUL-terminated UTF-8 buffer instead of a `String`
/// so the struct keeps a stable C layout (see `docs/DECISIONS.md`, ADR-007).
///
/// # C ABI
///
/// ```c
/// typedef struct GiochiEngineConfig {
///     uint32_t width;
///     uint32_t height;
///     uint32_t target_fps;
///     uint32_t vsync;              /* 0 = off, non-zero = on */
///     char     title[64];          /* NUL-terminated UTF-8, truncated if longer */
///     uint32_t _reserved[8];
/// } GiochiEngineConfig;
/// ```
///
/// Size: 112 bytes, alignment: 4 bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineConfig {
    /// Window width in physical pixels.
    pub width: u32,
    /// Window height in physical pixels.
    pub height: u32,
    /// Target simulation rate in frames per second.
    pub target_fps: u32,
    /// `0` disables vertical sync, any other value enables it.
    pub vsync: u32,
    /// NUL-terminated UTF-8 window title.
    pub title: [u8; ENGINE_TITLE_CAPACITY],
    /// Reserved for future ABI-compatible fields. Must be zero.
    pub _reserved: [u32; 8],
}

impl EngineConfig {
    /// Creates a configuration with vsync enabled and a 60 Hz target.
    ///
    /// `title` is truncated on a UTF-8 character boundary if it does not fit.
    pub fn new(width: u32, height: u32, title: &str) -> Self {
        let mut buffer = [0u8; ENGINE_TITLE_CAPACITY];
        copy_title(title, &mut buffer);
        Self {
            width,
            height,
            target_fps: 60,
            vsync: 1,
            title: buffer,
            _reserved: [0; 8],
        }
    }

    /// Copies `title` into the fixed-size buffer, truncating on a character
    /// boundary and always leaving room for the trailing NUL.
    pub fn set_title(&mut self, title: &str) {
        self.title = [0u8; ENGINE_TITLE_CAPACITY];
        copy_title(title, &mut self.title);
    }

    /// Returns the configured title as a string slice.
    ///
    /// Returns `""` if the buffer is not valid UTF-8 (for example because a C
    /// caller wrote arbitrary bytes into it).
    pub fn title_str(&self) -> &str {
        let end = self
            .title
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(self.title.len());
        std::str::from_utf8(&self.title[..end]).unwrap_or("")
    }
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self::new(1280, 720, "GIOCHI-ENGINE")
    }
}

fn copy_title(title: &str, buffer: &mut [u8; ENGINE_TITLE_CAPACITY]) {
    let bytes = title.as_bytes();
    let mut length = bytes.len().min(ENGINE_TITLE_CAPACITY - 1);
    while length > 0 && !title.is_char_boundary(length) {
        length -= 1;
    }
    buffer[..length].copy_from_slice(&bytes[..length]);
}

/// The engine facade.
///
/// # C ABI
///
/// `Engine` is never passed across the FFI boundary: C consumers drive the
/// single engine instance through `giochi_engine_*` functions.
#[repr(C)]
#[derive(Debug)]
pub struct Engine {
    _reserved: [u32; 4],
}

impl Engine {
    /// Initialises SDL2, the window, the renderer, input and audio.
    ///
    /// Implemented in phase 5.
    pub fn new(_config: EngineConfig) -> Result<Engine, GiochiError> {
        todo!("fase 5: construir a fachada Engine")
    }

    /// Runs `scene` until the user quits or the scene requests it.
    ///
    /// Implemented in phase 5.
    pub fn run(&mut self, _scene: Box<dyn Scene>) -> Result<(), GiochiError> {
        todo!("fase 5: rodar o loop principal com a cena")
    }

    /// Asks the engine to stop after the current frame.
    ///
    /// Implemented in phase 5.
    pub fn request_quit(&mut self) {
        todo!("fase 5: sinalizar encerramento da engine")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_keeps_dimensions_and_enables_vsync() {
        let config = EngineConfig::new(320, 240, "game");

        assert_eq!(config.width, 320);
        assert_eq!(config.height, 240);
        assert_eq!(config.target_fps, 60);
        assert_eq!(config.vsync, 1);
        assert_eq!(config.title_str(), "game");
        assert_eq!(config._reserved, [0; 8]);
    }

    #[test]
    fn long_titles_are_truncated_and_stay_valid_utf8() {
        let title = "á".repeat(100);
        let config = EngineConfig::new(1, 1, &title);

        assert_eq!(config.title[ENGINE_TITLE_CAPACITY - 1], 0);
        assert!(config.title_str().len() < ENGINE_TITLE_CAPACITY - 1);
        assert!(config.title_str().chars().all(|c| c == 'á'));
    }

    #[test]
    fn set_title_replaces_previous_value() {
        let mut config = EngineConfig::default();
        config.set_title("novo");

        assert_eq!(config.title_str(), "novo");
    }
}
