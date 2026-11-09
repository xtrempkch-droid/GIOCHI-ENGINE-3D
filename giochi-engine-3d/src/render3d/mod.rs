//! 3D rendering.
//!
//! Separate from `giochi_engine::render` on purpose: that one is a `sdl2::render`
//! wrapper and this one drives GLES directly. They cannot share a canvas, so they do
//! not share a module (ADR-037).

mod instanced;
mod light;
mod renderer;
pub mod upscale;

pub use instanced::{InstancedRenderer, DEFAULT_INSTANCE_CAPACITY};
pub use light::DirectionalLight;
pub use renderer::{MeshHandle, RenderContext3D, ShaderHandle};
pub use upscale::{sanitise_scale, scaled_size, Upscaler, MAX_SCALE, MIN_SCALE};
