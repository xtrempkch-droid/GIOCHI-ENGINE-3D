//! Low-level asset file helpers.
//!
//! The loader never allocates a buffer that crosses the API boundary: callers
//! pass their own `&mut [u8]`, which keeps the helpers C-shaped and lets the
//! engine control asset-size limits.

use crate::error::GiochiError;

/// Largest asset the loader will accept, in bytes (64 MiB).
pub const MAX_ASSET_SIZE: usize = 64 * 1024 * 1024;

/// Returns the size of the asset at `path` in bytes.
///
/// Implemented in phase 4.
pub fn asset_size(_path: &str) -> Result<usize, GiochiError> {
    todo!("fase 4: medir o asset em disco")
}

/// Reads the asset at `path` into `buffer` and returns how many bytes were
/// written.
///
/// `buffer` must be at least [`asset_size`] bytes long; otherwise the read fails
/// with [`GiochiError::Asset`] instead of truncating silently.
///
/// Implemented in phase 4.
pub fn read_asset(_path: &str, _buffer: &mut [u8]) -> Result<usize, GiochiError> {
    todo!("fase 4: ler o asset para um buffer do chamador")
}
