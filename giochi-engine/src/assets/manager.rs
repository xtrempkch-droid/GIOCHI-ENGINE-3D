//! Asset cache shared by the whole engine.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::assets::loader;
use crate::error::GiochiError;
use crate::render::{TextureHandle, TextureRegistry};

/// Caches loaded resources so that requesting the same path twice returns the same
/// handle instead of uploading a second copy to the GPU.
///
/// The cache is keyed by the **path as given**: `"assets/player.png"` and
/// `"./assets/player.png"` are two entries. Normalising would mean touching the
/// filesystem again, which defeats the point of the cache.
///
/// `i32` is deliberately absent from the Rust methods: they return
/// [`GiochiResult`](crate::error::GiochiResult). At the boundary, `giochi_texture_create`
/// returns the handle directly (or `0` on failure) and goes through
/// [`ResourceManager::load_texture`], so two C callers loading the same path share one
/// texture.
///
/// # C ABI
///
/// Not exposed. C consumers call `giochi_texture_create`, which goes through the
/// engine's single resource manager, so two C callers loading the same path share
/// one texture.
#[repr(C)]
#[derive(Debug, Default)]
pub struct ResourceManager {
    textures: RefCell<HashMap<String, TextureHandle>>,
    _reserved: [u32; 4],
}

impl ResourceManager {
    /// Creates an empty cache.
    pub fn new() -> Self {
        Self {
            textures: RefCell::new(HashMap::new()),
            _reserved: [0; 4],
        }
    }

    /// Loads `path` through `registry`, or returns the handle already cached for it.
    ///
    /// Two loads of the same path produce **one** texture upload: the second call
    /// returns the same handle and the registry's live count stays at one.
    ///
    /// A path whose cached handle has been destroyed behind the manager's back is
    /// reloaded and re-cached, so a `destroy_texture` cannot leave the manager
    /// handing out dead handles.
    pub fn load_texture(
        &mut self,
        registry: &TextureRegistry,
        path: &str,
    ) -> Result<TextureHandle, GiochiError> {
        if let Some(handle) = self.cached_handle(registry, path) {
            log::debug!("texture cache hit: {path} -> {handle}");
            return Ok(handle);
        }

        let handle = registry.create_texture(path)?;
        self.textures.borrow_mut().insert(path.to_owned(), handle);
        log::debug!("texture cache miss: {path} -> {handle}");
        Ok(handle)
    }

    /// Returns the cached handle for `path` that is still alive in `registry`.
    fn cached_handle(&self, registry: &TextureRegistry, path: &str) -> Option<TextureHandle> {
        let cached = *self.textures.borrow().get(path)?;
        if registry.contains(cached) {
            Some(cached)
        } else {
            // The texture was destroyed outside the manager: drop the stale entry.
            self.textures.borrow_mut().remove(path);
            None
        }
    }

    /// Looks up `path` without loading anything.
    ///
    /// Returns `None` for an unknown path *and* for a handle that is no longer
    /// alive in `registry`.
    pub fn get_texture(&self, registry: &TextureRegistry, path: &str) -> Option<TextureHandle> {
        self.cached_handle(registry, path)
    }

    /// Returns `true` when `path` has a live cached texture.
    pub fn is_texture_cached(&self, registry: &TextureRegistry, path: &str) -> bool {
        self.cached_handle(registry, path).is_some()
    }

    /// The cached path for `handle`, if the manager loaded it.
    ///
    /// The reverse of [`ResourceManager::get_texture`], which the hot reloader needs:
    /// the file watcher reports a path, and the renderer reloads by handle.
    pub fn path_for_handle(&self, handle: TextureHandle) -> Option<String> {
        if handle == 0 {
            return None;
        }
        self.textures
            .borrow()
            .iter()
            .find(|(_, cached)| **cached == handle)
            .map(|(path, _)| path.clone())
    }

    /// Reloads the texture cached for `path` from disk, keeping its handle.
    ///
    /// Returns the handle on success. Returns [`GiochiError::InvalidArgument`] when the
    /// path is not cached — the running game never loaded it, so there is nothing to
    /// refresh and nothing to invent.
    ///
    /// A failed reload **evicts** the cache entry: the file on disk is now the truth,
    /// and the next `load_texture` must report the real error rather than handing back a
    /// handle to an image that no longer matches the file.
    pub fn reload_texture(
        &mut self,
        registry: &TextureRegistry,
        path: &str,
    ) -> Result<TextureHandle, GiochiError> {
        let Some(handle) = self.cached_handle(registry, path) else {
            return Err(GiochiError::InvalidArgument(
                "texture is not cached, so there is nothing to reload",
            ));
        };

        match registry.reload(handle) {
            Ok(()) => {
                log::debug!("reloaded cached texture '{path}' -> {handle}");
                Ok(handle)
            }
            Err(err) => {
                self.textures.borrow_mut().remove(path);
                Err(err)
            }
        }
    }

    /// Reloads the texture behind `handle` from the path it was cached under.
    ///
    /// # Errors
    ///
    /// [`GiochiError::InvalidHandle`] when no cached path maps to `handle`, plus
    /// whatever [`ResourceManager::reload_texture`] reports.
    pub fn reload_texture_handle(
        &mut self,
        registry: &TextureRegistry,
        handle: TextureHandle,
    ) -> Result<(), GiochiError> {
        let Some(path) = self.path_for_handle(handle) else {
            return Err(GiochiError::InvalidHandle(handle));
        };
        self.reload_texture(registry, &path).map(|_| ())
    }

    /// Number of distinct paths currently cached.
    ///
    /// Counts entries whose texture may already have been destroyed; use
    /// [`ResourceManager::live_texture_count`] to count only live ones.
    pub fn cached_texture_count(&self) -> usize {
        self.textures.borrow().len()
    }

    /// Number of cached paths whose texture is still alive in `registry`.
    pub fn live_texture_count(&self, registry: &TextureRegistry) -> usize {
        self.textures
            .borrow()
            .values()
            .filter(|handle| registry.contains(**handle))
            .count()
    }

    /// Drops every cached entry **and** destroys the cached textures.
    ///
    /// Explicit `destroy_texture` calls remain the supported way to free a single
    /// texture; this is the whole-cache shortcut used on shutdown.
    pub fn clear(&mut self, registry: &TextureRegistry) {
        let entries: Vec<TextureHandle> =
            self.textures.borrow_mut().drain().map(|(_, h)| h).collect();
        for handle in entries {
            registry.destroy_texture(handle);
        }
    }

    /// Reads an asset through [`loader::read_asset_vec`].
    ///
    /// Present so gameplay code has one entry point for "all the files the engine
    /// touches"; the loader stays free of cache state.
    pub fn read_asset(&self, path: &str) -> Result<Vec<u8>, GiochiError> {
        loader::read_asset_vec(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The cache keys off `TextureRegistry`, which needs a live SDL renderer, so
    // the cache-hit/miss behaviour is covered by `tests/assets_audio_smoke.rs`.
    // What can be checked without a renderer lives here.

    #[test]
    fn a_new_manager_is_empty() {
        let manager = ResourceManager::new();

        assert_eq!(manager.cached_texture_count(), 0);
        assert_eq!(manager._reserved, [0; 4]);
    }

    #[test]
    fn default_matches_new() {
        let manager = ResourceManager::default();

        assert_eq!(manager.cached_texture_count(), 0);
    }

    #[test]
    fn reading_a_missing_asset_fails_with_an_asset_error() {
        let manager = ResourceManager::new();

        let error = manager
            .read_asset("/nonexistent/giochi/none.png")
            .expect_err("must fail");

        assert_eq!(error.code(), crate::error::codes::ASSET);
    }
}
