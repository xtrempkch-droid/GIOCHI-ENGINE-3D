//! A textured 2D sprite.

use crate::math::Rect;
use crate::render::texture::TextureHandle;

/// A sprite: a texture drawn at a position with an explicit size.
///
/// The rectangle is in **world** coordinates: [`crate::render::RenderContext::draw_sprite`]
/// converts it through the current camera. The whole texture is drawn; use
/// [`crate::render::RenderContext::draw_texture`] to draw a sub-rectangle.
///
/// # C ABI
///
/// ```c
/// typedef struct GiochiSprite {
///     GiochiTexture texture;
///     float         x, y, w, h;
///     uint32_t      _reserved[8];
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
    /// Destination width, in world units.
    pub w: f32,
    /// Destination height, in world units.
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

    /// The sprite's rectangle in world space.
    pub fn world_rect(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }

    /// Moves the sprite by `(dx, dy)` world units.
    pub fn translate(&mut self, dx: f32, dy: f32) {
        self.x += dx;
        self.y += dy;
    }

    /// Returns `true` when the sprite has a valid texture and a non-empty area.
    pub fn is_drawable(&self) -> bool {
        self.texture != 0 && self.w > 0.0 && self.h > 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_zeroes_the_reserved_tail() {
        let sprite = Sprite::new(7, 1.0, 2.0, 3.0, 4.0);

        assert_eq!(sprite._reserved, [0; 8]);
        assert_eq!(sprite.world_rect(), Rect::new(1.0, 2.0, 3.0, 4.0));
    }

    #[test]
    fn translate_moves_in_world_space() {
        let mut sprite = Sprite::new(1, 10.0, 20.0, 5.0, 5.0);

        sprite.translate(-2.5, 3.0);

        assert_eq!((sprite.x, sprite.y), (7.5, 23.0));
    }

    #[test]
    fn sprites_without_a_texture_or_area_are_not_drawable() {
        assert!(!Sprite::new(0, 0.0, 0.0, 10.0, 10.0).is_drawable());
        assert!(!Sprite::new(1, 0.0, 0.0, 0.0, 10.0).is_drawable());
        assert!(!Sprite::new(1, 0.0, 0.0, 10.0, -1.0).is_drawable());
        assert!(Sprite::new(1, 0.0, 0.0, 10.0, 10.0).is_drawable());
    }
}
