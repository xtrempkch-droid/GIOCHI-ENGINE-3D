//! Opaque texture handles and the registry that owns them.

use std::cell::RefCell;
use std::collections::HashMap;
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
    /// The file each texture was loaded from, when it came from one.
    ///
    /// Kept so [`TextureRegistry::reload`] can rebuild a texture without the caller
    /// having to remember the path. A texture created from memory has no entry, and
    /// reloading it is an error rather than a silent no-op.
    sources: RefCell<HashMap<TextureHandle, String>>,
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
            sources: RefCell::new(HashMap::new()),
            creator,
            _reserved: [0; 4],
        }
    }

    /// Loads the image at `path` (PNG/JPG/BMP/…) and returns its handle.
    ///
    /// The registry does not cache by path — that is
    /// [`ResourceManager`](crate::assets::ResourceManager)'s job. Returns
    /// [`GiochiError::Asset`] when the file is missing or not a supported image.
    ///
    /// The path is **remembered**, which is what makes [`TextureRegistry::reload`]
    /// possible without the caller keeping a table of its own.
    pub fn create_texture(&self, path: &str) -> Result<TextureHandle, GiochiError> {
        if path.trim().is_empty() {
            return Err(GiochiError::InvalidArgument("texture path is empty"));
        }

        let texture = self.load_texture_file(path)?;
        let handle = self.registry.insert(texture);
        self.sources.borrow_mut().insert(handle, path.to_owned());
        Ok(handle)
    }

    /// Decodes `path` into a texture, without registering it.
    fn load_texture_file(&self, path: &str) -> Result<Texture, GiochiError> {
        let mut texture = self
            .creator
            .load_texture(path)
            .map_err(|err| GiochiError::Asset(format!("failed to load texture '{path}': {err}")))?;

        // Blend mode is set before the texture is wrapped in an `Rc`, because
        // `Rc` grants shared access only.
        texture.set_blend_mode(BlendMode::Blend);
        Ok(texture)
    }

    /// The file `handle` was loaded from, or `None` for a generated texture.
    pub fn source_path(&self, handle: TextureHandle) -> Option<String> {
        self.sources.borrow().get(&handle).cloned()
    }

    /// Every live handle, in unspecified order.
    ///
    /// Used by the hot reloader, which has a path and needs the handles that came from
    /// it — the reverse of what [`Self::source_path`] answers.
    pub fn handles(&self) -> Vec<TextureHandle> {
        self.registry.handles()
    }

    /// Replaces the pixels of `handle` with the file it was loaded from.
    ///
    /// The **handle stays the same**, so every sprite, batch and script reference keeps
    /// working — that is the point of a reload. On failure the previous texture is
    /// left untouched and the error is returned, which is what makes an editor save a
    /// half-written file harmless.
    ///
    /// # Errors
    ///
    /// * [`GiochiError::InvalidArgument`] for a generated texture (no source path);
    /// * [`GiochiError::Asset`] when the file is missing or no longer decodes;
    /// * [`GiochiError::InvalidHandle`] when `handle` is unknown or currently resolved
    ///   by the renderer (an `Rc` clone outlives the swap).
    pub fn reload(&self, handle: TextureHandle) -> Result<(), GiochiError> {
        let Some(path) = self.source_path(handle) else {
            return Err(GiochiError::InvalidArgument(
                "texture was not loaded from a file, so it cannot be reloaded",
            ));
        };

        // Decode first, swap second: on a parse error nothing is touched.
        let replacement = self.load_texture_file(&path)?;
        self.registry
            .with_mut(handle, |texture| *texture = replacement)
            .ok_or(GiochiError::InvalidHandle(handle))
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

        let Some(expected) = rgba_buffer_len(width, height) else {
            return Err(GiochiError::InvalidArgument(
                "texture dimensions overflow the address space",
            ));
        };
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

    /// Creates a texture from an encoded image held in memory (PNG, JPEG, BMP, …).
    ///
    /// The format is sniffed from the bytes, so no size or format is needed. This
    /// is what `giochi_texture_create_memory` uses, and it is the only way to load
    /// an image on a target without a filesystem.
    pub fn create_texture_from_bytes(&self, bytes: &[u8]) -> Result<TextureHandle, GiochiError> {
        if bytes.is_empty() {
            return Err(GiochiError::InvalidArgument("image buffer is empty"));
        }

        let mut texture = self.creator.load_texture_bytes(bytes).map_err(|err| {
            GiochiError::Asset(format!("failed to decode texture from memory: {err}"))
        })?;
        texture.set_blend_mode(BlendMode::Blend);

        Ok(self.registry.insert(texture))
    }

    /// Releases the texture behind `handle`.
    ///
    /// Returns `false` when the handle is `0` or was already destroyed; a double
    /// destroy is reported, never a silent success.
    pub fn destroy_texture(&self, handle: TextureHandle) -> bool {
        let removed = self.registry.remove(handle);
        if removed {
            self.sources.borrow_mut().remove(&handle);
        }
        removed
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

    /// Replaces the pixels of an existing RGBA texture without reallocating it.
    ///
    /// `width` and `height` must match the size the texture was created with, and
    /// `pixels` must be exactly `width * height * 4` bytes. The debug overlay uses this
    /// to refresh its panel at 1 Hz: creating and destroying a texture on every refresh
    /// was measured at ~10 % of the frame on a Mali-G31, which is more than the profiler
    /// is allowed to cost (ADR-042).
    ///
    /// Returns [`GiochiError::InvalidHandle`] when the texture is unknown or currently
    /// shared with a renderer, so the caller can fall back to creating a new one.
    pub(crate) fn update_texture_rgba(
        &self,
        handle: TextureHandle,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<(), GiochiError> {
        if width == 0 || height == 0 {
            return Err(GiochiError::InvalidArgument(
                "texture width and height must be non-zero",
            ));
        }
        let Some(expected) = rgba_buffer_len(width, height) else {
            return Err(GiochiError::InvalidArgument(
                "texture dimensions overflow the address space",
            ));
        };
        if pixels.len() != expected {
            return Err(GiochiError::InvalidArgument(
                "pixel buffer must be exactly width * height * 4 bytes",
            ));
        }
        if self.dimensions(handle) != Some((width, height)) {
            return Err(GiochiError::InvalidArgument(
                "pixel buffer does not match the texture dimensions",
            ));
        }

        self.registry
            .with_mut(handle, |texture| {
                texture.update(None, pixels, width as usize * 4)
            })
            .ok_or(GiochiError::InvalidHandle(handle))?
            .map_err(|err| GiochiError::Render(format!("failed to update texture: {err}")))
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
        self.sources.borrow_mut().clear();
    }
}

impl Drop for TextureRegistry {
    fn drop(&mut self) {
        self.registry.clear();
    }
}

/// `width * height * 4`, or `None` when that cannot be addressed in memory.
fn rgba_buffer_len(width: u32, height: u32) -> Option<usize> {
    (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(4)
}

/// Initialises SDL_image exactly once per process.
///
/// The PNG and JPG decoders are **optional at build time**: a console image
/// built without `libpng`/`libjpeg` still links `libSDL2_image`, with the
/// per-format entry points compiled as "not supported" stubs. `IMG_Init` fails
/// when *any* requested flag cannot be satisfied, so asking for `PNG | JPG`
/// used to declare the whole subsystem dead on such a target — even though
/// BMP, TGA, PNM and QOI need no flags and kept working.
///
/// So the subsystem is opened with no flags at all, and each optional decoder
/// is then probed on its own, which is the only way to learn what this build
/// actually has. Measured on a Rockchip PX30 (Buildroot 2024.02) whose
/// `libSDL2_image` links only `libSDL2`/`libm`/`libc`.
fn ensure_image_subsystem() {
    static INIT: OnceLock<bool> = OnceLock::new();

    let ok = *INIT.get_or_init(|| {
        // No flags: registers the always-available decoders and only fails if
        // SDL_image itself is unusable.
        match sdl2::image::init(sdl2::image::InitFlag::empty()) {
            Ok(context) => {
                // The context must live for the whole process; leaking one
                // `Sdl2ImageContext` is the documented way to keep SDL_image
                // initialised for the engine's lifetime.
                std::mem::forget(context);
            }
            Err(err) => {
                log::error!("failed to initialise SDL_image: {err}");
                return false;
            }
        }

        for (flag, name) in [
            (sdl2::image::InitFlag::PNG, "PNG"),
            (sdl2::image::InitFlag::JPG, "JPG"),
        ] {
            match sdl2::image::init(flag) {
                Ok(context) => std::mem::forget(context),
                Err(_) => log::warn!(
                    "SDL_image was built without {name} support; \
                     {name} assets will not load on this target"
                ),
            }
        }

        true
    });

    if !ok {
        log::warn!("SDL_image is unavailable; texture loading from files will fail");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_size_arithmetic_refuses_to_wrap() {
        assert_eq!(rgba_buffer_len(2, 3), Some(24));
        assert_eq!(rgba_buffer_len(0, 3), Some(0));
        // `u32::MAX * u32::MAX * 4` does not fit a 64-bit address space, let alone a
        // 32-bit one; a debug build must answer "no" instead of panicking on overflow.
        assert_eq!(rgba_buffer_len(u32::MAX, u32::MAX), None);
    }

    #[test]
    fn texture_size_zeroes_its_reserved_tail() {
        let size = TextureSize::new(16, 32);

        assert_eq!((size.width, size.height), (16, 32));
        assert_eq!(size._reserved, [0; 4]);
    }
}
