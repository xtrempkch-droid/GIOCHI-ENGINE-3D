//! A textured 2D sprite.

use crate::render::texture::TextureHandle;

/// A sprite: a texture drawn at a position with an explicit size.
///
/// # C ABI
///
/// ```c
/// typedef struct GiochiSprite {
///     uint64_t texture;
///     float    x, y, w, h;
///     uint32_t _reserved[8];
/// } GiochiSprite;
/// ```
///
/// Size: 56 bytes, alignment: 8 bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sprite {
    /// Texture drawn by this sprite.
    pub texture: TextureHandle,
    /// Destination X, in world coordinates.
    pub x: f32,
    /// Destination Y, in world coordinates.
    pub y: f32,
    /// Destination width.
    pub w: f32,
    /// Destination height.
    pub h: f32,
    /// Reserved for future ABI-compatible fields. Must be zero.
    pub _reserved: [u32; 8],
}

impl Sprite {
    /// Creates a sprite; `_reserved` is zeroed.
    pub const fn new(texture: TextureHandle, x: f32, y: f32, w: f32, h: f32) -> Self {
        Self {
            texture,
            x,
            y,
            w,
            h,
            _reserved: [0; 8],
        }
    }
}
