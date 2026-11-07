//! Opaque texture handles and the registry that owns them.

use crate::error::GiochiError;

/// Opaque texture handle. `0` is invalid/null.
///
/// # C ABI
///
/// ```c
/// typedef uint64_t GiochiTexture;
/// ```
pub type TextureHandle = u64;

/// Owns every GPU texture created by the engine.
///
/// Textures are created and destroyed explicitly (`create_texture` /
/// `destroy_texture`); there is no RAII across the boundary. The registry keeps
/// the SDL texture alive for as long as its handle is registered and frees it as
/// soon as the handle is destroyed.
///
/// The SDL texture creator is held *inside* the registry so that
/// [`TextureRegistry::create_texture`] takes only a path — a creator argument
/// would force a generic or lifetime-bearing public signature (see
/// `docs/DECISIONS.md`, ADR-008).
///
/// # C ABI
///
/// `TextureRegistry` is not exposed; C consumers use `giochi_texture_create` and
/// `giochi_texture_destroy`.
#[repr(C)]
#[derive(Debug, Default)]
pub struct TextureRegistry {
    _reserved: [u32; 4],
}

impl TextureRegistry {
    /// Creates an empty registry.
    pub const fn new() -> Self {
        Self { _reserved: [0; 4] }
    }

    /// Loads the image at `path` and returns its handle.
    ///
    /// Implemented in phase 2.
    pub fn create_texture(&self, _path: &str) -> Result<TextureHandle, GiochiError> {
        todo!("fase 2: carregar textura e registrar o handle")
    }

    /// Releases the texture behind `handle`.
    ///
    /// Returns `false` when the handle is `0` or already destroyed.
    ///
    /// Implemented in phase 2.
    pub fn destroy_texture(&self, _handle: TextureHandle) -> bool {
        todo!("fase 2: destruir textura pelo handle")
    }

    /// Number of live textures. Used by tests to prove there is no leak.
    ///
    /// Implemented in phase 2.
    pub fn len(&self) -> usize {
        todo!("fase 2: contagem de texturas vivas")
    }

    /// Returns `true` when no texture is registered.
    ///
    /// Implemented in phase 2.
    pub fn is_empty(&self) -> bool {
        todo!("fase 2: contagem de texturas vivas")
    }
}
