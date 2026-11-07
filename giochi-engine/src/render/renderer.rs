//! The 2D rendering context.

use crate::error::GiochiError;
use crate::math::{Color, Rect};
use crate::platform::window::Window;
use crate::render::texture::TextureHandle;

/// The engine's 2D render context.
///
/// Thin wrapper over the SDL2 canvas: it owns the drawing target and offers the
/// only drawing primitives the engine has. All coordinates are in physical
/// pixels of the window; use [`crate::render::Camera`] to convert world space to
/// screen space.
///
/// # C ABI
///
/// Not exposed. C consumers call `giochi_render_*` functions, which operate on
/// the engine's single context.
#[repr(C)]
#[derive(Debug)]
pub struct RenderContext {
    _reserved: [u32; 4],
}

impl RenderContext {
    /// Creates the context for `window`.
    ///
    /// Implemented in phase 2.
    pub fn new(_window: &Window) -> Result<RenderContext, GiochiError> {
        todo!("fase 2: criar o canvas SDL2")
    }

    /// Clears the whole target with `color`.
    ///
    /// Implemented in phase 2.
    pub fn clear(&mut self, _color: Color) -> Result<(), GiochiError> {
        todo!("fase 2: limpar o canvas")
    }

    /// Fills `rect` with `color`.
    ///
    /// Implemented in phase 2.
    pub fn draw_rect(&mut self, _rect: Rect, _color: Color) -> Result<(), GiochiError> {
        todo!("fase 2: desenhar retangulo")
    }

    /// Copies `src` of `texture` into `dst`.
    ///
    /// `src` is in texture coordinates; a zero-sized `src` means "the whole
    /// texture". `dst` is in screen coordinates.
    ///
    /// Implemented in phase 2.
    pub fn draw_texture(
        &mut self,
        _texture: TextureHandle,
        _dst: Rect,
        _src: Rect,
    ) -> Result<(), GiochiError> {
        todo!("fase 2: desenhar textura")
    }

    /// Presents the back buffer on screen.
    ///
    /// Implemented in phase 2.
    pub fn present(&mut self) {
        todo!("fase 2: apresentar o frame")
    }
}
