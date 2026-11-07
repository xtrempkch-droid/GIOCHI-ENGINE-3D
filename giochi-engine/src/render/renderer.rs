//! The 2D rendering context.

use std::rc::Rc;

use sdl2::render::{TextureCreator, WindowCanvas};
use sdl2::video::Window as SdlWindow;
use sdl2::video::WindowContext;

use crate::error::GiochiError;
use crate::math::{Color, Rect, Vec2};
use crate::platform::sdl::SdlContext;
use crate::platform::window::Window;
use crate::render::camera::Camera;
use crate::render::sprite::Sprite;
use crate::render::texture::{TextureHandle, TextureRegistry};

/// The engine's 2D render context.
///
/// Thin wrapper over the SDL2 canvas: it owns the drawing target and offers the
/// only drawing primitives the engine has. `draw_rect` and `draw_texture` work in
/// **screen** space; `draw_sprite` treats the sprite rectangle as **world** space
/// and applies the current [`Camera`].
///
/// Creating the context consumes the [`Window`], because SDL binds the renderer to
/// its window; the context takes over the window-level operations it can provide
/// (`size`, `drawable_size`, `set_title`).
///
/// # C ABI
///
/// Not exposed. C consumers call `giochi_render_*`, which operate on the engine's
/// single context (ADR-012).
#[repr(C)]
pub struct RenderContext {
    // Dropped before `_context`: the canvas must be destroyed while SDL is still
    // initialised.
    canvas: WindowCanvas,
    textures: TextureRegistry,
    camera: Camera,
    _context: Rc<SdlContext>,
    _reserved: [u32; 4],
}

impl std::fmt::Debug for RenderContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderContext")
            .field("size", &self.size())
            .field("textures", &self.textures.len())
            .field("camera", &self.camera)
            .finish_non_exhaustive()
    }
}

impl RenderContext {
    /// Creates the context for `window`, without vsync.
    pub fn new(window: Window) -> Result<RenderContext, GiochiError> {
        Self::with_vsync(window, false)
    }

    /// Creates the context for `window`, optionally waiting for vblank on
    /// [`RenderContext::present`].
    pub fn with_vsync(window: Window, vsync: bool) -> Result<RenderContext, GiochiError> {
        let (sdl_window, context) = window.into_parts();
        let canvas = build_canvas(sdl_window, vsync)?;
        let size = canvas.window().size();
        let textures = TextureRegistry::from_creator(canvas.texture_creator());

        Ok(RenderContext {
            canvas,
            textures,
            camera: Camera::new(Rect::new(0.0, 0.0, size.0 as f32, size.1 as f32)),
            _context: context,
            _reserved: [0; 4],
        })
    }

    /// The texture registry owned by this context.
    ///
    /// Note that holding this reference blocks `&mut self` calls (`clear`,
    /// `draw_*`, `present`), so prefer the delegating methods below in draw loops.
    pub fn textures(&self) -> &TextureRegistry {
        &self.textures
    }

    /// Loads a texture and returns its handle. See [`TextureRegistry::create_texture`].
    pub fn create_texture(&self, path: &str) -> Result<TextureHandle, GiochiError> {
        self.textures.create_texture(path)
    }

    /// Creates a texture from raw RGBA8 pixels.
    /// See [`TextureRegistry::create_texture_from_rgba`].
    pub fn create_texture_from_rgba(
        &self,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<TextureHandle, GiochiError> {
        self.textures
            .create_texture_from_rgba(width, height, pixels)
    }

    /// Releases a texture. Returns `false` for an invalid or already-freed handle.
    pub fn destroy_texture(&self, texture: TextureHandle) -> bool {
        self.textures.destroy_texture(texture)
    }

    /// Pixel size of a texture, or `None` for an invalid handle.
    pub fn texture_size(&self, texture: TextureHandle) -> Option<(u32, u32)> {
        self.textures.dimensions(texture)
    }

    /// Number of live textures.
    pub fn texture_count(&self) -> usize {
        self.textures.len()
    }

    /// Drawable size in physical pixels.
    pub fn size(&self) -> (u32, u32) {
        self.canvas.window().size()
    }

    /// Size SDL renders into, in physical pixels (differs from [`Self::size`] on
    /// high-DPI displays).
    pub fn drawable_size(&self) -> (u32, u32) {
        self.canvas.window().drawable_size()
    }

    /// Replaces the window title.
    pub fn set_title(&mut self, title: &str) -> Result<(), GiochiError> {
        self.canvas
            .window_mut()
            .set_title(title)
            .map_err(|err| GiochiError::Platform(format!("invalid window title: {err}")))
    }

    /// Sets the camera used by [`RenderContext::draw_sprite`].
    pub fn set_camera(&mut self, camera: Camera) {
        self.camera = camera;
    }

    /// Returns the camera currently in use.
    pub fn camera(&self) -> Camera {
        self.camera
    }

    /// Fits the current camera's viewport to the window, keeping its position and
    /// zoom. Called at construction and available for resize handling.
    pub fn fit_camera_to_window(&mut self) {
        let (width, height) = self.size();
        self.camera.viewport = Rect::new(0.0, 0.0, width as f32, height as f32);
    }

    /// Clears the whole target with `color`.
    pub fn clear(&mut self, color: Color) {
        self.canvas.set_draw_color(to_sdl_color(color));
        self.canvas.clear();
    }

    /// Fills `rect` (screen space) with `color`.
    ///
    /// Empty or degenerate rectangles are a no-op rather than an error.
    pub fn draw_rect(&mut self, rect: Rect, color: Color) {
        let Some(dst) = to_sdl_rect(rect) else {
            return;
        };
        self.canvas.set_draw_color(to_sdl_color(color));
        if let Err(err) = self.canvas.fill_rect(dst) {
            log::error!("draw_rect failed: {err}");
        }
    }

    /// Copies `src` of `texture` into `dst` (both screen space).
    ///
    /// A zero-sized `src` means "the whole texture". An invalid handle or an empty
    /// destination is reported and nothing is drawn.
    pub fn draw_texture(
        &mut self,
        texture: TextureHandle,
        dst: Rect,
        src: Rect,
    ) -> Result<(), GiochiError> {
        let texture = self
            .textures
            .resolve(texture)
            .ok_or(GiochiError::InvalidHandle(texture))?;

        let Some(dst) = to_sdl_rect(dst) else {
            return Ok(());
        };
        let src = to_sdl_rect(src);

        self.canvas
            .copy(&texture, src, dst)
            .map_err(|err| GiochiError::Render(format!("draw_texture failed: {err}")))
    }

    /// Draws `sprite`, converting its world-space rectangle through the camera.
    pub fn draw_sprite(&mut self, sprite: &Sprite) -> Result<(), GiochiError> {
        let dst = self.camera.world_rect_to_screen(sprite.world_rect());
        self.draw_texture(sprite.texture, dst, Rect::ZERO)
    }

    /// Converts a world-space point to screen space with the current camera.
    pub fn world_to_screen(&self, world: Vec2) -> Vec2 {
        self.camera.world_to_screen(world)
    }

    /// Presents the back buffer on screen.
    pub fn present(&mut self) {
        self.canvas.present();
    }

    /// Reads back `rect` from the current target as RGBA8 bytes.
    ///
    /// Intended for tests and screenshot tooling: it forces a GPU→CPU copy and must
    /// not be used in a frame loop. Returns `row-major` pixels, 4 bytes each.
    pub fn read_pixels(&self, rect: Rect) -> Result<Vec<u8>, GiochiError> {
        let Some(rect) = to_sdl_rect(rect) else {
            return Ok(Vec::new());
        };

        self.canvas
            .read_pixels(Some(rect), sdl2::pixels::PixelFormatEnum::RGBA32)
            .map_err(|err| GiochiError::Render(format!("read_pixels failed: {err}")))
    }

    pub(crate) fn texture_creator(&self) -> TextureCreator<WindowContext> {
        self.canvas.texture_creator()
    }
}

fn build_canvas(window: SdlWindow, vsync: bool) -> Result<WindowCanvas, GiochiError> {
    let mut builder = window.into_canvas();
    if vsync {
        builder = builder.present_vsync();
    }

    builder
        .build()
        .map_err(|err| GiochiError::Render(format!("failed to create the 2D renderer: {err}")))
}

/// Converts an engine color into SDL's color type.
fn to_sdl_color(color: Color) -> sdl2::pixels::Color {
    sdl2::pixels::Color::RGBA(color.r, color.g, color.b, color.a)
}

/// Converts a float rectangle into SDL's integer rectangle.
///
/// Returns `None` when the rectangle has no area after rounding, which lets
/// callers treat "nothing to draw" as a no-op. Non-finite components become `0`,
/// and the result is clamped to the `i32` range SDL uses.
fn to_sdl_rect(rect: Rect) -> Option<sdl2::rect::Rect> {
    let x = to_sdl_coord(rect.x);
    let y = to_sdl_coord(rect.y);
    let w = to_sdl_extent(rect.w);
    let h = to_sdl_extent(rect.h);

    if w <= 0 || h <= 0 {
        return None;
    }

    Some(sdl2::rect::Rect::new(x, y, w as u32, h as u32))
}

fn to_sdl_coord(value: f32) -> i32 {
    if !value.is_finite() {
        return 0;
    }
    value.round().clamp(i32::MIN as f32, i32::MAX as f32) as i32
}

fn to_sdl_extent(value: f32) -> i32 {
    if !value.is_finite() || value <= 0.0 {
        return 0;
    }
    value.round().clamp(0.0, i32::MAX as f32) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_map_channel_by_channel() {
        let color = to_sdl_color(Color::new(1, 2, 3, 4));

        assert_eq!((color.r, color.g, color.b, color.a), (1, 2, 3, 4));
    }

    #[test]
    fn rects_are_rounded_to_integers() {
        let rect = to_sdl_rect(Rect::new(1.4, 2.6, 10.5, 20.4)).expect("non-empty rect");

        assert_eq!((rect.x(), rect.y(), rect.w, rect.h), (1, 3, 11, 20));
    }

    #[test]
    fn empty_and_degenerate_rects_are_skipped() {
        for rect in [
            Rect::new(0.0, 0.0, 0.0, 10.0),
            Rect::new(0.0, 0.0, 10.0, 0.0),
            Rect::new(0.0, 0.0, -5.0, 10.0),
            Rect::new(0.0, 0.0, 0.4, 0.4),
        ] {
            assert!(to_sdl_rect(rect).is_none(), "{rect:?} must be skipped");
        }
    }

    #[test]
    fn non_finite_rects_do_not_panic() {
        let rect = to_sdl_rect(Rect::new(f32::NAN, f32::INFINITY, 10.0, 10.0))
            .expect("finite extents survive");

        assert_eq!((rect.x(), rect.y()), (0, 0));
        assert_eq!((rect.w, rect.h), (10, 10));
    }

    #[test]
    fn huge_coordinates_are_clamped_instead_of_wrapping() {
        let rect = to_sdl_rect(Rect::new(1e30, -1e30, 1e30, 1e30)).expect("clamped rect");

        assert!(rect.x() > 0);
        assert!(rect.y() < 0);
    }
}
