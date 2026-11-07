//! # giochi-engine-capi
//!
//! The C ABI for GIOCHI-ENGINE, built as both `cdylib` (`libgiochi.so`) and
//! `staticlib` (`libgiochi.a`).
//!
//! The complete surface is specified in `docs/C_ABI_CONTRACT.md` and implemented
//! in phase 7. Phase 0 exposes only the two version queries so that consumers can
//! already probe a library build.
//!
//! Conventions used by every function that will be added here:
//!
//! * return `i32`: `0` on success, a negative code from
//!   `giochi_engine::error::codes` on failure;
//! * resources are `uint64_t` handles, `0` meaning "null";
//! * no Rust type (no `Rc`, `String`, `Result`, panic) crosses the boundary;
//! * every pointer parameter is validated before use.
#![allow(unsafe_code)]
#![warn(missing_docs)]

use giochi_engine::ABI_VERSION;

/// Returns the version of the C ABI implemented by this library.
///
/// Currently `1`. Consumers should check this value at start-up.
#[no_mangle]
pub extern "C" fn giochi_abi_version() -> u32 {
    ABI_VERSION
}

/// Returns the engine version packed as `major << 16 | minor << 8 | patch`.
///
/// For version `0.1.0` this returns `0x0000_0100`.
#[no_mangle]
pub extern "C" fn giochi_engine_version() -> u32 {
    giochi_engine::version_packed()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abi_version_matches_contract() {
        assert_eq!(giochi_abi_version(), 1);
    }

    #[test]
    fn engine_version_is_packed() {
        let packed = giochi_engine_version();
        let major = (packed >> 16) & 0xFF;
        let minor = (packed >> 8) & 0xFF;
        let patch = packed & 0xFF;

        assert_eq!(
            (major, minor, patch),
            (
                giochi_engine::VERSION_MAJOR,
                giochi_engine::VERSION_MINOR,
                giochi_engine::VERSION_PATCH
            )
        );
    }
}
