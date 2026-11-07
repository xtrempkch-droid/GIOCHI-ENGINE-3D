//! # giochi-engine
//!
//! GIOCHI-ENGINE is a **2D-only** game engine core written in Rust on top of
//! SDL2. Its public surface is designed to be *C-shaped*: every value type is
//! `#[repr(C)]` with reserved space, every resource is an opaque `u64` handle,
//! and no `Rc`/`Arc`/`String`/`Vec` ever appears in a public struct field.
//!
//! The projected C form of every public type lives in
//! `docs/C_ABI_CONTRACT.md` (repository root). That document is authoritative:
//! types that cannot be expressed in C must be redesigned, not the contract.
//!
//! # Status
//!
//! **Complete.** All eight phases of the project are implemented: the platform layer
//! and the fixed-timestep loop, the 2D renderer with opaque texture handles, input,
//! the asset loader and the mixer, the scene stack and the [`core::engine::Engine`]
//! facade. The C ABI lives in `giochi-engine-capi` and the Lua binding in
//! `giochi-engine-lua`; neither changes anything here.
//!
//! There are no `todo!()` stubs left in this crate, and `tests/c_shape.rs` enforces the
//! C-shaped rules over the whole source tree.
//!
//! # Layering rules
//!
//! * [`core`] never imports `sdl2` directly — it goes through [`platform`].
//! * [`render`] does not know about [`scene`].
//! * [`math`] depends on nothing else inside the crate.
//! * `handle_registry` is crate-private.
//!
//! # Threading
//!
//! The engine is single-threaded. Public wrapper types are neither `Send` nor
//! `Sync` on purpose; handles are plain values and may be stored anywhere.
#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![warn(missing_debug_implementations)]

pub mod assets;
pub mod audio;
pub mod core;
pub mod error;
pub mod input;
pub mod math;
pub mod platform;
pub mod render;
pub mod scene;

pub(crate) mod handle_registry;

/// Semantic version of the engine, as declared in `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Major component of [`VERSION`].
pub const VERSION_MAJOR: u32 = 0;
/// Minor component of [`VERSION`].
pub const VERSION_MINOR: u32 = 1;
/// Patch component of [`VERSION`].
pub const VERSION_PATCH: u32 = 0;

/// Version of the C ABI described by `docs/C_ABI_CONTRACT.md`.
pub const ABI_VERSION: u32 = 1;

/// Returns the engine version packed as `major << 16 | minor << 8 | patch`.
///
/// This is the value exposed by `giochi_engine_version()` in the C ABI.
pub const fn version_packed() -> u32 {
    (VERSION_MAJOR << 16) | (VERSION_MINOR << 8) | VERSION_PATCH
}

pub use crate::assets::{asset_size, read_asset, ResourceManager, MAX_ASSET_SIZE};
pub use crate::audio::{AudioSystem, MusicHandle, MusicState, SoundHandle};
pub use crate::core::engine::{Engine, EngineConfig, RunStats, DEFAULT_CLEAR_COLOR};
pub use crate::core::game_loop::{GameLoop, FIXED_TIMESTEP, MAX_FRAME_TIME};
pub use crate::core::time::{FrameClock, Time};
pub use crate::error::{codes, GiochiError, GiochiResult};
pub use crate::input::{Input, KeyboardState, MouseState};
pub use crate::math::{Color, Rect, Vec2};
pub use crate::platform::sdl::{quit_signal, sdl_version, Platform, QuitSignal, VideoDriverChoice};
pub use crate::platform::window::{Window, WindowConfig};
pub use crate::render::{
    Camera, RenderContext, Sprite, TextureHandle, TextureRegistry, TextureSize,
};
pub use crate::scene::{EmptyScene, Scene, SceneHandle, SceneManager, SceneTransition};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_constants_match_package_version() {
        let expected = format!("{VERSION_MAJOR}.{VERSION_MINOR}.{VERSION_PATCH}");
        assert_eq!(VERSION, expected);
    }

    #[test]
    fn version_is_packed_as_major_minor_patch() {
        assert_eq!(version_packed(), 0x00_01_00);
    }
}
