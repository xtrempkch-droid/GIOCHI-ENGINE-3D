//! 3D rendering.
//!
//! Separate from `giochi_engine::render` on purpose: that one is a `sdl2::render`
//! wrapper and this one drives GLES directly. They cannot share a canvas, so they do
//! not share a module (ADR-037).

mod renderer;

pub use renderer::RenderContext3D;
