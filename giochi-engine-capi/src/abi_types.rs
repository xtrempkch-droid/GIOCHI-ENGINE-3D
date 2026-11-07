//! Types and constants that exist only at the C boundary.
//!
//! Everything a `giochi_*` function takes or returns is either a primitive, an
//! opaque `u64` handle, or a `#[repr(C)]` type from `giochi-engine` used verbatim.
//! The two exceptions live here:
//!
//! * [`GiochiEngineConfig`] — the engine's `EngineConfig` with `char title[64]`
//!   instead of `uint8_t title[64]`, so C users can pass it to `strncpy` and
//!   friends without a cast. The layout is identical and asserted in
//!   `tests/rust_ffi.rs`.
//! * [`GiochiSceneCallbacks`] — a callback table, which has no Rust counterpart.
//!
//! Error codes are also defined here as `i32` constants so `cbindgen` emits them
//! as `#define`s in `giochi.h`.

use core::ffi::{c_char, c_void};

use giochi_engine::core::engine::ENGINE_TITLE_CAPACITY;
use giochi_engine::error::codes;
use giochi_engine::EngineConfig;

// The C header spells the title buffer as a literal so it needs no companion define;
// this keeps the two in step.
const _: () = assert!(ENGINE_TITLE_CAPACITY == 64);

/// Version of the C ABI implemented by this library.
///
/// Must stay equal to `giochi_abi_version()` and to `docs/C_ABI_CONTRACT.md`.
pub const GIOCHI_ABI_VERSION: u32 = 2;

/// Largest asset the loader accepts, in bytes (64 MiB).
pub const GIOCHI_MAX_ASSET_SIZE: usize = 64 * 1024 * 1024;

// Built-in shaders, for `giochi_shader_builtin` (`docs/C_ABI_CONTRACT.md` §10.6). The
// numbers are in the header, so they must never be renumbered.

/// Solid colour with one Lambert term. The phase 11 default.
pub const GIOCHI_SHADER_FLAT: i32 = 0;
/// `texture2D` sampling with the same light term.
pub const GIOCHI_SHADER_TEXTURED: i32 = 1;
/// Directional Blinn-Phong. Reads the engine's light, which part II does not expose yet.
pub const GIOCHI_SHADER_PHONG: i32 = 2;

// The numeric error codes are part of the ABI and must never be renumbered
// (`docs/C_ABI_CONTRACT.md` §4). They are written as literals so `cbindgen` can put
// them in the header as `#define`s — it cannot evaluate a path expression — and the
// assertions below keep them equal to the engine's own table.

/// Success.
pub const GIOCHI_ERROR_OK: i32 = 0;
/// The handle was `0` or referred to a resource that no longer exists.
pub const GIOCHI_ERROR_INVALID_HANDLE: i32 = -1;
/// A caller-supplied argument was out of range or malformed.
pub const GIOCHI_ERROR_INVALID_ARGUMENT: i32 = -2;
/// SDL2/platform initialisation or windowing failure.
pub const GIOCHI_ERROR_PLATFORM: i32 = -3;
/// Rendering failure.
pub const GIOCHI_ERROR_RENDER: i32 = -4;
/// Asset loading failure.
pub const GIOCHI_ERROR_ASSET: i32 = -5;
/// Audio subsystem failure.
pub const GIOCHI_ERROR_AUDIO: i32 = -6;
/// Scene misuse, or a subsystem used where it is not available.
pub const GIOCHI_ERROR_SCENE: i32 = -7;
/// The engine was not initialised when the call required it.
pub const GIOCHI_ERROR_NOT_INITIALIZED: i32 = -8;
/// The operation is not supported by this build or ABI version.
pub const GIOCHI_ERROR_UNSUPPORTED: i32 = -9;
/// Unexpected internal failure, including a caught panic.
pub const GIOCHI_ERROR_INTERNAL: i32 = -99;

// `giochi_engine::ABI_VERSION` is the version of **part I** — the 2D surface — and part I
// never changes. `GIOCHI_ABI_VERSION` is the version of the whole C ABI, which moved to 2
// when part II (3D) shipped. So the invariant is "at least", not "equal": a consumer
// checks `giochi_abi_version()` against the number it was built with, and part I callers
// written against 1 keep working unchanged.
const _: () = assert!(GIOCHI_ABI_VERSION >= giochi_engine::ABI_VERSION);
const _: () = assert!(GIOCHI_MAX_ASSET_SIZE == giochi_engine::MAX_ASSET_SIZE);
const _: () = assert!(GIOCHI_ERROR_OK == codes::OK);
const _: () = assert!(GIOCHI_ERROR_INVALID_HANDLE == codes::INVALID_HANDLE);
const _: () = assert!(GIOCHI_ERROR_INVALID_ARGUMENT == codes::INVALID_ARGUMENT);
const _: () = assert!(GIOCHI_ERROR_PLATFORM == codes::PLATFORM);
const _: () = assert!(GIOCHI_ERROR_RENDER == codes::RENDER);
const _: () = assert!(GIOCHI_ERROR_ASSET == codes::ASSET);
const _: () = assert!(GIOCHI_ERROR_AUDIO == codes::AUDIO);
const _: () = assert!(GIOCHI_ERROR_SCENE == codes::SCENE);
const _: () = assert!(GIOCHI_ERROR_NOT_INITIALIZED == codes::NOT_INITIALIZED);
const _: () = assert!(GIOCHI_ERROR_UNSUPPORTED == codes::UNSUPPORTED);
const _: () = assert!(GIOCHI_ERROR_INTERNAL == codes::INTERNAL);

/// Engine configuration, as the C ABI sees it.
///
/// Identical in layout to `giochi_engine::EngineConfig`; `title` is `char[64]`
/// rather than `uint8_t[64]` so a C caller can treat it as a string.
///
/// # C ABI
///
/// ```c
/// typedef struct GiochiEngineConfig {
///     uint32_t width;
///     uint32_t height;
///     uint32_t target_fps;
///     uint32_t vsync;
///     char     title[64];
///     uint32_t _reserved[8];
/// } GiochiEngineConfig;
/// ```
///
/// Size: 112 bytes, alignment: 4 bytes.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct GiochiEngineConfig {
    /// Window width, in physical pixels.
    pub width: u32,
    /// Window height, in physical pixels.
    pub height: u32,
    /// Target simulation rate in Hz; `0` keeps the engine default (60).
    pub target_fps: u32,
    /// `0` disables vertical sync, any other value enables it.
    pub vsync: u32,
    /// NUL-terminated UTF-8 window title.
    pub title: [c_char; 64],
    /// Reserved for future ABI-compatible fields. Must be zero.
    pub _reserved: [u32; 8],
}

impl GiochiEngineConfig {
    /// Converts to the engine's configuration type.
    ///
    /// Invalid UTF-8 in `title` is replaced lossily; the title is truncated on a
    /// character boundary by `EngineConfig::set_title`.
    pub(crate) fn to_engine(self) -> EngineConfig {
        let bytes: Vec<u8> = self
            .title
            .iter()
            .take_while(|&&byte| byte != 0)
            .map(|&byte| byte as u8)
            .collect();
        let title = String::from_utf8_lossy(&bytes);

        let mut config = EngineConfig::new(self.width, self.height, &title);
        config.target_fps = self.target_fps;
        config.vsync = u32::from(self.vsync != 0);
        config
    }
}

/// A scene expressed as callbacks plus an opaque `user_data` pointer.
///
/// The engine copies this table in `giochi_scene_create`; the pointers are stored
/// and called until the scene handle is destroyed, so `user_data` and the function
/// pointers must stay valid for that long.
///
/// # C ABI
///
/// ```c
/// typedef struct GiochiSceneCallbacks {
///     void* user_data;
///     void (*on_enter)(void* user_data);
///     void (*on_exit)(void* user_data);
///     void (*update)(void* user_data, const GiochiTime* t);
///     void (*render)(void* user_data);
///     uint32_t _reserved[4];
/// } GiochiSceneCallbacks;
/// ```
///
/// Size: 56 bytes, alignment: 8 bytes.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct GiochiSceneCallbacks {
    /// Passed back to every callback unchanged. May be NULL.
    pub user_data: *mut c_void,
    /// Called once before the scene's first frame. May be NULL.
    pub on_enter: Option<extern "C" fn(*mut c_void)>,
    /// Called once after the scene's last frame. May be NULL.
    pub on_exit: Option<extern "C" fn(*mut c_void)>,
    /// Called once per fixed simulation step. Required.
    pub update: Option<extern "C" fn(*mut c_void, *const giochi_engine::Time)>,
    /// Called once per display frame. Required.
    pub render: Option<extern "C" fn(*mut c_void)>,
    /// Reserved for future ABI-compatible fields. Must be zero.
    pub _reserved: [u32; 4],
}

impl GiochiSceneCallbacks {
    /// Returns `true` when the table has the two callbacks the engine requires.
    pub(crate) fn is_complete(&self) -> bool {
        self.update.is_some() && self.render.is_some()
    }
}
