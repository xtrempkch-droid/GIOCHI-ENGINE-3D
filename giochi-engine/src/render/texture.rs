//! Opaque texture handles and the registry that owns them.

use std::sync::OnceLock;

use sdl2::image::LoadTexture;
use sdl2::render::{BlendMode, Texture, TextureCreator};
use sdl2::video::WindowContext;

use crate::error::GiochiError;
use crate::handle_registry::HandleRegistry;
use crate::render::renderer::RenderContext;

/// Opaque texture handle. `0` is invalid/null.
///
/// # C ABI
///
/// ```c
/// typedef uint64_t GiochiTexture;
/// ```
pub type TextureHandle = u64;

/// Pixel size of a texture.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextureSize {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Reserved for future ABI-compatible fields. Must be zero.
    pub _reserved: [u32; 4],
}

impl TextureSize {
    /// Creates a size; `_reserved` is zeroed.
    pub const fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            _reserved: [0; 4],
        }
    }
}

/// Owns every texture created by the engine.
///
/// Textures are created and destroyed explicitly (`create_texture` /
/// `destroy_texture`); there is no RAII across the ABI boundary. The handle is
/// only an index into this registry — the SDL texture itself is freed as soon as
/// the handle is destroyed, and the test suite checks that the live count returns
/// to zero.
///
/// The SDL texture creator is held *inside* the registry so that
/// [`TextureRegistry::create_texture`] takes only a path: a creator argument would
/// force a generic or lifetime-bearing public signature (see `docs/DECISIONS.md`,
/// ADR-008 and ADR-017).
///
/// # C ABI
///
/// Not exposed. C consumers use `giochi_texture_create`/`giochi_texture_destroy`
/// on the engine's single registry.
#[repr(C)]
pub struct TextureRegistry {
    registry: HandleRegistry<Texture>,
    creator: TextureCreator<WindowContext>,
    _reserved: [u32; 4],
}

impl std::fmt::Debug for TextureRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextureRegistry")
            .field("live", &self.len())
            .finish_non_exhaustive()
    }
}

impl TextureRegistry {
    /// Creates an empty registry bound to `render_context`'s renderer.
    ///
    /// The registry keeps the renderer alive through the shared creator, so it is
    /// safe for it to outlive the `RenderContext` value — but a texture may only
    /// be drawn with the canvas it was created for, which in this engine is always
    /// the single window canvas.
    pub fn new(render_context: &RenderContext) -> Self {
        Self::from_creator(render_context.texture_creator())
    }

    /// Creates an empty registry from a raw SDL texture creator.
    pub(crate) fn from_creator(creator: TextureCreator<WindowContext>) -> Self {
        ensure_image_subsystem();

        Self {
            registry: HandleRegistry::new(),
            creator,
            _reserved: [0; 4],
        }
    }

    /// Loads the image at `path` (PNG/JPG/BMP/…) and returns its handle.
    ///
    /// The path is read once; the registry does not cache by path (that is the
    /// `ResourceManager`'s job in phase 4). Returns [`GiochiError::Asset`] when
    /// the file is missing or not a supported image.
    pub fn create_texture(&self, path: &str) -> Result<TextureHandle, GiochiError> {
        if path.trim().is_empty() {
            return Err(GiochiError::InvalidArgument("texture path is empty"));
        }

        let mut texture = self
            .creator
            .load_texture(path)
            .map_err(|err| GiochiError::Asset(format!("failed to load texture '{path}': {err}")))?;

        // Blend mode is set before the texture is wrapped in an `Rc`, because
        // `Rc` grants shared access only.
        texture.set_blend_mode(BlendMode::Blend);

        Ok(self.registry.insert(texture))
    }

    /// Creates a texture of `width`x`height` from raw RGBA8 pixels.
    ///
    /// `pixels` must be exactly `width * height * 4` bytes. Useful for generated
    /// graphics and for tests, which must not depend on image files on disk.
    pub fn create_texture_from_rgba(
        &self,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<TextureHandle, GiochiError> {
        if width == 0 || height == 0 {
            return Err(GiochiError::InvalidArgument(
                "texture width and height must be non-zero",
            ));
        }

        let expected = width as usize * height as usize * 4;
        if pixels.len() != expected {
            return Err(GiochiError::InvalidArgument(
                "pixel buffer must be exactly width * height * 4 bytes",
            ));
        }

        let mut buffer = pixels.to_vec();
        let surface = sdl2::surface::Surface::from_data(
            &mut buffer,
            width,
            height,
            width * 4,
            sdl2::pixels::PixelFormatEnum::RGBA32,
        )
        .map_err(|err| GiochiError::Asset(format!("failed to build texture surface: {err}")))?;

        let mut texture = self
            .creator
            .create_texture_from_surface(&surface)
            .map_err(|err| GiochiError::Asset(format!("failed to upload texture: {err}")))?;
        texture.set_blend_mode(BlendMode::Blend);

        Ok(self.registry.insert(texture))
    }

    /// Releases the texture behind `handle`.
    ///
    /// Returns `false` when the handle is `0` or was already destroyed; a double
    /// destroy is reported, never a silent success.
    pub fn destroy_texture(&self, handle: TextureHandle) -> bool {
        self.registry.remove(handle)
    }

    /// Returns `true` when `handle` refers to a live texture.
    pub fn contains(&self, handle: TextureHandle) -> bool {
        self.registry.contains(handle)
    }

    /// Returns the pixel size behind `handle`, or `None` for an invalid handle.
    pub fn dimensions(&self, handle: TextureHandle) -> Option<(u32, u32)> {
        let texture = self.registry.get(handle)?;
        let query = texture.query();
        Some((query.width, query.height))
    }

    /// Returns the pixel size behind `handle` in C-shaped form.
    pub fn size(&self, handle: TextureHandle) -> Option<TextureSize> {
        self.dimensions(handle)
            .map(|(width, height)| TextureSize::new(width, height))
    }

    /// Resolves `handle` for the renderer. Crate-private: the SDL type must never
    /// appear in a public signature.
    pub(crate) fn resolve(&self, handle: TextureHandle) -> Option<std::rc::Rc<Texture>> {
        self.registry.get(handle)
    }

    /// Number of live textures. Used by tests to prove there is no leak.
    pub fn len(&self) -> usize {
        self.registry.len()
    }

    /// Returns `true` when no texture is registered.
    pub fn is_empty(&self) -> bool {
        self.registry.is_empty()
    }

    /// Destroys every texture. Used on shutdown.
    pub fn clear(&self) {
        self.registry.clear();
    }
}

impl Drop for TextureRegistry {
    fn drop(&mut self) {
        self.registry.clear();
    }
}

/// Initialises SDL_image exactly once per process.
///
/// `IMG_LoadTexture` needs the PNG/JPG decoders registered; failing to initialise
/// them is reported by the load call itself, so the result is not propagated here
/// and is logged instead.
fn ensure_image_subsystem() {
    static INIT: OnceLock<bool> = OnceLock::new();

    let ok = *INIT.get_or_init(|| {
        match sdl2::image::init(sdl2::image::InitFlag::PNG | sdl2::image::InitFlag::JPG) {
            Ok(context) => {
                // The context must live for the whole process; leaking one
                // `Sdl2ImageContext` is the documented way to keep SDL_image
                // initialised for the engine's lifetime.
                std::mem::forget(context);
                true
            }
            Err(err) => {
                log::error!("failed to initialise SDL_image: {err}");
                false
            }
        }
    });

    if !ok {
        log::warn!("SDL_image is unavailable; texture loading from files will fail");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texture_size_zeroes_its_reserved_tail() {
        let size = TextureSize::new(16, 32);

        assert_eq!((size.width, size.height), (16, 32));
        assert_eq!(size._reserved, [0; 4]);
    }
}
