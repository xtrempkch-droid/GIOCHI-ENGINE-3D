//! Engine error type and the numeric codes used at the C ABI boundary.
//!
//! Two error representations coexist by design (see `docs/DECISIONS.md`,
//! ADR-003):
//!
//! * **Inside Rust** every fallible operation returns [`GiochiResult`], whose
//!   error is the [`GiochiError`] enum (`thiserror`).
//! * **At the C boundary** every function returns an `i32`: `0` on success and a
//!   negative code from [`codes`] on failure, with the human-readable message
//!   available through `giochi_get_last_error()`.
//!
//! [`GiochiError`] never crosses the FFI boundary.

/// Numeric error codes returned by `giochi_*` functions.
///
/// The numeric values are part of the C ABI: they must not be renumbered.
pub mod codes {
    /// Success.
    pub const OK: i32 = 0;

    /// The handle passed in was `0` or was not registered.
    pub const INVALID_HANDLE: i32 = -1;

    /// A caller-supplied argument was out of range or malformed.
    pub const INVALID_ARGUMENT: i32 = -2;

    /// SDL2/platform initialisation or windowing failure.
    pub const PLATFORM: i32 = -3;

    /// Rendering failure (canvas, texture or blending).
    pub const RENDER: i32 = -4;

    /// Asset loading failure (missing file, unsupported format).
    pub const ASSET: i32 = -5;

    /// Audio subsystem failure.
    pub const AUDIO: i32 = -6;

    /// Scene stack misuse (empty stack, unknown scene handle).
    pub const SCENE: i32 = -7;

    /// The engine was not initialised when the call required it.
    pub const NOT_INITIALIZED: i32 = -8;

    /// The operation is not supported by this build or ABI version.
    pub const UNSUPPORTED: i32 = -9;

    /// Unexpected internal failure.
    pub const INTERNAL: i32 = -99;
}

/// Result alias used throughout the engine's Rust API.
pub type GiochiResult<T> = Result<T, GiochiError>;

/// Errors produced by the engine's Rust-facing API.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum GiochiError {
    /// A resource handle was `0` or referred to a resource that no longer exists.
    #[error("invalid handle: {0}")]
    InvalidHandle(u64),

    /// A caller-supplied argument was out of range or malformed.
    #[error("invalid argument: {0}")]
    InvalidArgument(&'static str),

    /// The engine must be initialised before this operation.
    #[error("engine is not initialised")]
    NotInitialized,

    /// SDL2 or platform-level failure (video driver, KMSDRM device, window).
    #[error("platform error: {0}")]
    Platform(String),

    /// Rendering failure (canvas, texture or blending).
    #[error("render error: {0}")]
    Render(String),

    /// Asset loading failure (missing file, unsupported format).
    #[error("asset error: {0}")]
    Asset(String),

    /// Audio subsystem failure.
    #[error("audio error: {0}")]
    Audio(String),

    /// Scene stack misuse (empty stack, unknown scene handle).
    #[error("scene error: {0}")]
    Scene(String),

    /// The operation is not supported by this build or ABI version.
    #[error("unsupported operation: {0}")]
    Unsupported(&'static str),
}

impl GiochiError {
    /// Maps the error to the numeric code returned at the C ABI boundary.
    pub fn code(&self) -> i32 {
        match self {
            GiochiError::InvalidHandle(_) => codes::INVALID_HANDLE,
            GiochiError::InvalidArgument(_) => codes::INVALID_ARGUMENT,
            GiochiError::NotInitialized => codes::NOT_INITIALIZED,
            GiochiError::Platform(_) => codes::PLATFORM,
            GiochiError::Render(_) => codes::RENDER,
            GiochiError::Asset(_) => codes::ASSET,
            GiochiError::Audio(_) => codes::AUDIO,
            GiochiError::Scene(_) => codes::SCENE,
            GiochiError::Unsupported(_) => codes::UNSUPPORTED,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_code_is_negative_or_zero() {
        for code in [
            codes::OK,
            codes::INVALID_HANDLE,
            codes::INVALID_ARGUMENT,
            codes::PLATFORM,
            codes::RENDER,
            codes::ASSET,
            codes::AUDIO,
            codes::SCENE,
            codes::NOT_INITIALIZED,
            codes::UNSUPPORTED,
            codes::INTERNAL,
        ] {
            assert!(code <= 0, "error codes must be <= 0, got {code}");
        }
    }

    #[test]
    fn error_codes_are_unique() {
        let all = [
            codes::OK,
            codes::INVALID_HANDLE,
            codes::INVALID_ARGUMENT,
            codes::PLATFORM,
            codes::RENDER,
            codes::ASSET,
            codes::AUDIO,
            codes::SCENE,
            codes::NOT_INITIALIZED,
            codes::UNSUPPORTED,
            codes::INTERNAL,
        ];
        for (i, a) in all.iter().enumerate() {
            for b in all.iter().skip(i + 1) {
                assert_ne!(a, b, "duplicate error code: {a}");
            }
        }
    }

    #[test]
    fn error_maps_to_its_code() {
        assert_eq!(GiochiError::InvalidHandle(7).code(), codes::INVALID_HANDLE);
        assert_eq!(GiochiError::NotInitialized.code(), codes::NOT_INITIALIZED);
        assert_eq!(GiochiError::Unsupported("3d").code(), codes::UNSUPPORTED);
    }
}
