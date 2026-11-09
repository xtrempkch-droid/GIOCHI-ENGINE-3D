//! 2D rendering on top of SDL2's `render` API.
//!
//! Scope is deliberately 2D-only: no EGL, GLES, shaders, vertex buffers, meshes
//! or render pipelines are introduced here. Textures are opaque handles, and the
//! renderer never exposes an SDL object to user code.

pub mod atlas;
pub mod batch;
pub mod camera;
pub mod renderer;
pub mod sprite;
pub mod texture;

pub use atlas::{AtlasRegion, TextureAtlas};
pub use batch::{SpriteBatch, SpriteBatchConfig};
pub use camera::Camera;
pub use renderer::RenderContext;
pub use sprite::Sprite;
pub use texture::{TextureHandle, TextureRegistry, TextureSize};
