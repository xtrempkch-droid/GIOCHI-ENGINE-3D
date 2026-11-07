//! Asset reading.
//!
//! The engine never allocates the buffer: the caller owns the memory and therefore
//! the size policy. Reading fails rather than truncating, and assets above
//! `GIOCHI_MAX_ASSET_SIZE` (64 MiB) are rejected outright (ADR-023's neighbour,
//! `docs/API_GUIDE.md` §9).

use core::ffi::c_char;

use giochi_engine::error::codes;

use crate::state::{self, guard};

/// Writes the size of `path` in bytes into `out_size`.
///
/// # C ABI
///
/// ```c
/// int32_t giochi_asset_size(const char* path, size_t* out_size);
/// ```
#[no_mangle]
pub extern "C" fn giochi_asset_size(path: *const c_char, out_size: *mut usize) -> i32 {
    guard(codes::INTERNAL, || {
        if out_size.is_null() {
            return state::invalid_argument("out_size must not be NULL");
        }
        let Some(path) = string(path) else {
            return codes::INVALID_ARGUMENT;
        };
        match giochi_engine::asset_size(&path) {
            Ok(size) => {
                // SAFETY: non-NULL, caller-owned writable `size_t`.
                unsafe { *out_size = size };
                codes::OK
            }
            Err(err) => state::fail(&err),
        }
    })
}

/// Reads up to `capacity` bytes of `path` into `buffer`.
///
/// Writes the number of bytes actually read into `out_read`, which may be NULL.
/// Fails with `GIOCHI_ERROR_ASSET` when the file is larger than `capacity`: the
/// engine never truncates.
///
/// # C ABI
///
/// ```c
/// int32_t giochi_asset_read(const char* path,
///                           uint8_t* buffer,
///                           size_t capacity,
///                           size_t* out_read);
/// ```
#[no_mangle]
pub extern "C" fn giochi_asset_read(
    path: *const c_char,
    buffer: *mut u8,
    capacity: usize,
    out_read: *mut usize,
) -> i32 {
    guard(codes::INTERNAL, || {
        if buffer.is_null() || capacity == 0 {
            return state::invalid_argument(
                "buffer must not be NULL and capacity must be non-zero",
            );
        }
        let Some(path) = string(path) else {
            return codes::INVALID_ARGUMENT;
        };
        // SAFETY: the contract requires `buffer` to point at `capacity` writable
        // bytes.
        let target = unsafe { core::slice::from_raw_parts_mut(buffer, capacity) };
        match giochi_engine::read_asset(&path, target) {
            Ok(read) => {
                // SAFETY: non-NULL-checked above; caller-owned writable `size_t`.
                unsafe {
                    if !out_read.is_null() {
                        *out_read = read;
                    }
                }
                codes::OK
            }
            Err(err) => state::fail(&err),
        }
    })
}

/// Reads a NUL-terminated path, reporting a NULL/invalid argument.
fn string(pointer: *const c_char) -> Option<String> {
    match state::cstr(pointer) {
        Some(text) => Some(text),
        None => {
            state::set_last_error("path must be a non-NULL, valid UTF-8 string");
            None
        }
    }
}
