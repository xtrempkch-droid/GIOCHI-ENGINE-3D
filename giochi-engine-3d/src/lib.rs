//! GIOCHI-ENGINE 3D — OpenGL ES 2.0 over EGL, parallel to the 2D engine.
//!
//! # What this crate is
//!
//! A **separate** renderer. The 2D engine (`giochi-engine`) keeps using
//! `sdl2::render`; this crate drives GLES directly through EGL via SDL2's window.
//! A game picks one. There is no unified mode and no swapping a backend at runtime
//! (ADR-037).
//!
//! What is shared: window creation, input, audio and the `u64` handle discipline.
//! What is not: `sdl2::render` and anything built on it.
//!
//! # GLES 2.0 by default
//!
//! GLES 2.0 is the ARM64 floor — RK3326 handhelds and the R35S class of device ship
//! drivers that cannot be trusted with 3.0 features. GLES 3.0 is opt-in with
//! `GIOCHI_GL_VERSION=3`, and anything 3.0-only must be guarded by
//! [`RenderContext3D::supports_gles3`] (ADR-038).
//!
//! # Example
//!
//! ```no_run
//! use giochi_engine::platform::window::WindowConfig;
//! use giochi_engine::platform::Platform;
//! use giochi_engine_3d::context::{self, GlesVersion};
//! use giochi_engine_3d::math3d::Color3D;
//! use giochi_engine_3d::RenderContext3D;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let platform = Platform::new()?;
//! // The GL attributes are read when the *window* is created, so this comes first.
//! context::configure_attributes(&platform, GlesVersion::Gles2)?;
//! let window = giochi_engine::platform::window::Window::new_opengl(
//!     &platform,
//!     WindowConfig::new(1280, 720),
//!     "3D",
//! )?;
//! let mut render = RenderContext3D::new(context::create(&window)?);
//!
//! render.clear(&window, Color3D::SKY);
//! # Ok(())
//! # }
//! ```
#![warn(missing_docs)]
#![warn(rust_2018_idioms)]

pub mod context;
pub mod game;
#[cfg(feature = "hot_reload")]
pub mod hot_reload;
pub mod math3d;
pub mod mesh;
pub mod render3d;
pub mod shader;

mod error;
mod registry;

pub use error::{Error3D, Result3D};
pub use render3d::{
    sanitise_scale, scaled_size, DirectionalLight, InstancedRenderer, MeshHandle, RenderContext3D,
    ShaderHandle, TextureHandle, Upscaler, MAX_SCALE, MIN_SCALE,
};

/// Semantic version of the crate, as declared in `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_matches_the_workspace() {
        // Compared against the 2D crate rather than a literal: a bump in the workspace
        // manifest reaches both through `version.workspace = true`, and a hardcoded
        // `"0.2.0"` here is exactly the line a release forgets to touch — it did, on the
        // 0.2.1 bump.
        assert_eq!(VERSION, giochi_engine::VERSION);
    }
}
