//! Version and diagnostics.
//!
//! These three functions are the only part of the ABI that is safe to call before
//! `giochi_engine_init`, and `giochi_get_last_error` is the only one that may be
//! called from any thread.

use core::ffi::c_char;

use crate::abi_types::GIOCHI_ABI_VERSION;
use crate::state;

/// Returns the version of the C ABI implemented by this library.
///
/// Consumers should check it at start-up against the version they were built for and
/// accept anything newer, because every part of the ABI is additive:
///
/// ```c
/// if (giochi_abi_version() < 1) { /* built against a newer library */ }
/// ```
///
/// Part I is 1, part II (3D) added 2, part III (frame statistics) made it 3, and
/// part IV (networking) made it 4.
///
/// # C ABI
///
/// ```c
/// uint32_t giochi_abi_version(void);
/// ```
#[no_mangle]
pub extern "C" fn giochi_abi_version() -> u32 {
    GIOCHI_ABI_VERSION
}

/// Returns the engine version packed as `major << 16 | minor << 8 | patch`.
///
/// For version `0.4.0` this returns `0x0004_0000`.
///
/// # C ABI
///
/// ```c
/// uint32_t giochi_engine_version(void);
/// ```
#[no_mangle]
pub extern "C" fn giochi_engine_version() -> u32 {
    giochi_engine::version_packed()
}

/// Returns the message of the last failure on this thread, or `""` if the last call
/// succeeded.
///
/// The pointer is owned by the engine and stays valid until the next `giochi_*` call
/// on the same thread. It is never NULL and must not be freed or modified.
///
/// This function deliberately does **not** clear or set the error: it reports it.
///
/// # C ABI
///
/// ```c
/// const char* giochi_get_last_error(void);
/// ```
#[no_mangle]
pub extern "C" fn giochi_get_last_error() -> *const c_char {
    state::last_error_pointer()
}
