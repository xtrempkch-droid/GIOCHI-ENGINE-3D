//! Asset cache shared by the whole engine.

use crate::error::GiochiError;
use crate::render::{TextureHandle, TextureRegistry};

/// Caches loaded resources so that requesting the same path twice returns the
/// same handle instead of uploading a second copy to the GPU.
///
/// # C ABI
///
/// Not exposed. The C ABI's `giochi_texture_create` goes through the engine's
/// single resource manager, so two C callers loading the same path share one
/// texture.
#[repr(C)]
#[derive(Debug, Default)]
pub struct ResourceManager {
    _reserved: [u32; 4],
}

impl ResourceManager {
    /// Creates an empty cache.
    pub const fn new() -> Self {
        Self { _reserved: [0; 4] }
    }

    /// Loads `path` through `registry`, or returns the handle already cached for
    /// it.
    ///
    /// Implemented in phase 4.
    pub fn load_texture(
        &mut self,
        _registry: &TextureRegistry,
        _path: &str,
    ) -> Result<TextureHandle, GiochiError> {
        todo!("fase 4: cache de texturas por caminho")
    }

    /// Number of distinct paths currently cached.
    ///
    /// Implemented in phase 4.
    pub fn cached_texture_count(&self) -> usize {
        todo!("fase 4: contagem de texturas em cache")
    }
}
