//! Low-level asset file helpers.
//!
//! The loader never allocates a buffer that crosses the API boundary: callers pass
//! their own `&mut [u8]`, which keeps the helpers C-shaped and lets the engine (or
//! a C consumer) control the asset-size limit.

use std::fs;
use std::path::Path;

use crate::error::GiochiError;

/// Largest asset the loader will accept, in bytes (64 MiB).
///
/// Exposed to C as `GIOCHI_MAX_ASSET_SIZE`.
pub const MAX_ASSET_SIZE: usize = 64 * 1024 * 1024;

/// Returns the size of the asset at `path` in bytes.
///
/// Fails with [`GiochiError::Asset`] for a missing file, a permission problem, a
/// path that is not a regular file, or an asset larger than [`MAX_ASSET_SIZE`].
pub fn asset_size(path: &str) -> Result<usize, GiochiError> {
    if path.trim().is_empty() {
        return Err(GiochiError::InvalidArgument("asset path is empty"));
    }

    let metadata = fs::metadata(path)
        .map_err(|err| GiochiError::Asset(format!("failed to stat asset '{path}': {err}")))?;

    if !metadata.is_file() {
        return Err(GiochiError::Asset(format!(
            "asset '{path}' is not a regular file"
        )));
    }

    let size = usize::try_from(metadata.len()).map_err(|_| {
        GiochiError::Asset(format!("asset '{path}' is too large to address in memory"))
    })?;

    if size > MAX_ASSET_SIZE {
        return Err(GiochiError::Asset(format!(
            "asset '{path}' is {size} bytes, above the {MAX_ASSET_SIZE}-byte limit"
        )));
    }

    Ok(size)
}

/// Reads the asset at `path` into `buffer` and returns how many bytes were
/// written.
///
/// `buffer` must be at least [`asset_size`] bytes long; otherwise the read fails
/// with [`GiochiError::Asset`] instead of truncating silently.
pub fn read_asset(path: &str, buffer: &mut [u8]) -> Result<usize, GiochiError> {
    let size = asset_size(path)?;

    if buffer.len() < size {
        return Err(GiochiError::Asset(format!(
            "buffer of {} bytes is too small for asset '{path}' ({size} bytes)",
            buffer.len()
        )));
    }

    let bytes = fs::read(path)
        .map_err(|err| GiochiError::Asset(format!("failed to read asset '{path}': {err}")))?;

    // `size` is a snapshot. A file that grows between the stat and the read would make
    // `bytes` longer than both `size` and the caller's buffer, and the unconditional
    // `buffer[..bytes.len()]` this function used to perform indexed straight past the
    // end of a buffer the caller sized from `asset_size` — a panic reachable from C
    // (`giochi_asset_read`) and from the Lua host. The mismatch is an error instead.
    copy_verified_asset(path, size, buffer, bytes)
}

/// Copies `bytes` into `buffer` after verifying it still has the size the caller was told.
///
/// Split out of [`read_asset`] so the size-change race is testable without racing two
/// syscalls against a writer.
fn copy_verified_asset(
    path: &str,
    expected_size: usize,
    buffer: &mut [u8],
    bytes: Vec<u8>,
) -> Result<usize, GiochiError> {
    if bytes.len() != expected_size {
        return Err(GiochiError::Asset(format!(
            "asset '{path}' changed size between stat and read: expected {expected_size} \
             bytes, read {}",
            bytes.len()
        )));
    }
    if bytes.len() > buffer.len() {
        // Defensive: every caller pre-checks the buffer against the same `expected_size`,
        // so this cannot fire through the public path; it keeps the invariant local.
        return Err(GiochiError::Asset(format!(
            "buffer of {} bytes is too small for asset '{path}' ({expected_size} bytes)",
            buffer.len()
        )));
    }

    let read = bytes.len();
    buffer[..read].copy_from_slice(&bytes);
    Ok(read)
}

/// Reads the whole asset into a new `Vec<u8>`.
///
/// Rust-only convenience: the vector never crosses the ABI boundary, which is why
/// this helper exists here and not as a `giochi_*` function.
pub fn read_asset_vec(path: &str) -> Result<Vec<u8>, GiochiError> {
    let size = asset_size(path)?;
    let bytes = fs::read(path)
        .map_err(|err| GiochiError::Asset(format!("failed to read asset '{path}': {err}")))?;

    // Same race as `read_asset`: a file that grew after the stat would come back as a
    // buffer longer than the size this function is documented to enforce, silently
    // lifting the `MAX_ASSET_SIZE` cap. This used to be a `debug_assert`, which a
    // release build — the one that ships — compiled out entirely.
    if bytes.len() != size {
        return Err(GiochiError::Asset(format!(
            "asset '{path}' changed size between stat and read: expected {size} bytes, \
             read {}",
            bytes.len()
        )));
    }

    Ok(bytes)
}

/// Returns the file name of `path` without its directory, for diagnostics.
pub fn asset_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_file(name: &str, contents: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("giochi-loader-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join(name);
        let mut file = fs::File::create(&path).expect("temp file");
        file.write_all(contents).expect("write temp file");
        path
    }

    #[test]
    fn size_of_a_real_file_is_reported() {
        let path = temp_file("size.bin", b"12345");

        assert_eq!(asset_size(path.to_str().unwrap()).expect("size"), 5);
    }

    #[test]
    fn missing_files_are_asset_errors() {
        let error = asset_size("/nonexistent/giochi/missing.bin").expect_err("must fail");

        assert_eq!(error.code(), crate::error::codes::ASSET);
        assert!(error.to_string().contains("failed to stat asset"));
    }

    #[test]
    fn empty_paths_are_invalid_arguments() {
        let error = asset_size("   ").expect_err("must fail");

        assert_eq!(error.code(), crate::error::codes::INVALID_ARGUMENT);
    }

    #[test]
    fn directories_are_rejected() {
        let error = asset_size(std::env::temp_dir().to_str().unwrap()).expect_err("must fail");

        assert!(
            error.to_string().contains("not a regular file"),
            "unexpected: {error}"
        );
    }

    #[test]
    fn read_asset_fills_the_caller_buffer() {
        let path = temp_file("read.bin", b"hello asset");
        let mut buffer = [0u8; 32];

        let read = read_asset(path.to_str().unwrap(), &mut buffer).expect("read");

        assert_eq!(read, 11);
        assert_eq!(&buffer[..read], b"hello asset");
    }

    #[test]
    fn a_short_buffer_fails_instead_of_truncating() {
        let path = temp_file("short.bin", b"0123456789");
        let mut buffer = [0u8; 4];

        let error = read_asset(path.to_str().unwrap(), &mut buffer).expect_err("must fail");

        assert!(
            error.to_string().contains("too small"),
            "unexpected: {error}"
        );
        assert_eq!(buffer, [0u8; 4], "the buffer must be left untouched");
    }

    #[test]
    fn read_asset_vec_returns_the_contents() {
        let path = temp_file("vec.bin", b"giochi");

        assert_eq!(
            read_asset_vec(path.to_str().unwrap()).expect("read"),
            b"giochi".to_vec()
        );
    }

    #[test]
    fn a_file_that_changed_size_between_stat_and_read_is_an_error_not_a_panic() {
        // The race itself cannot be driven through the public function — the two syscalls
        // are back to back — so the check is exercised the way a grown file would hit it:
        // a `bytes` buffer longer than the size `asset_size` reported. The old code sliced
        // `buffer[..bytes.len()]` unconditionally, so this exact condition indexed past the
        // caller's buffer and panicked inside `giochi_asset_read` / the Lua host.
        let path = temp_file("grown.bin", b"12345");
        // Sized from the stat, exactly as a real caller does: `asset_size` said 3.
        let mut buffer = [0u8; 3];

        // Simulate the race: the stat saw 3 bytes and the read returned 5.
        let expected = 3usize;
        let bytes = b"12345".to_vec();
        let error = copy_verified_asset(path.to_str().unwrap(), expected, &mut buffer, bytes)
            .expect_err("a size mismatch must be an asset error");

        assert_eq!(error.code(), crate::error::codes::ASSET);
        assert!(
            error.to_string().contains("changed size"),
            "unexpected: {error}"
        );
        assert_eq!(buffer, [0u8; 3], "the buffer must be left untouched");
    }

    #[test]
    fn a_read_shorter_than_the_stat_is_also_reported() {
        // A file that shrank between the syscalls: the old code silently returned fewer
        // bytes than `asset_size` promised, which a caller could misread as a read error.
        let path = temp_file("shrunk.bin", b"12345");
        let mut buffer = [0u8; 5];

        let error = copy_verified_asset(path.to_str().unwrap(), 5, &mut buffer, b"12".to_vec())
            .expect_err("a size mismatch must be an asset error");

        assert!(
            error.to_string().contains("changed size"),
            "unexpected: {error}"
        );
    }

    #[test]
    fn asset_name_strips_directories() {
        assert_eq!(asset_name("assets/sprites/player.png"), "player.png");
        assert_eq!(asset_name("player.png"), "player.png");
        assert_eq!(asset_name(""), "");
    }
}
