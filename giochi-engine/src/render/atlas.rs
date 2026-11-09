//! Texture atlases: several images packed into one texture.
//!
//! A [`TextureAtlas`] loads a list of image files, packs them with a one-pixel gutter
//! and registers the result as a single texture. Every region then draws with one
//! texture handle, so a scene built from many small images collapses to one batched
//! draw call (see [`SpriteBatch`](crate::render::SpriteBatch)).
//!
//! # Why one pixel of padding
//!
//! Sampling a texture near a region's edge with bilinear filtering reads the neighbour's
//! pixels and produces a visible seam (bleeding). One transparent pixel between images
//! is enough for the linear filter SDL uses; the region's UV rectangle covers only the
//! image, never the gutter. See ADR-043.
//!
//! # Lifetime
//!
//! The atlas owns a normal texture handle. Destroying it is explicit, like every other
//! resource in the engine: `render.destroy_texture(atlas.texture())`. The atlas value
//! itself holds no OS resources.

use std::collections::HashMap;
use std::path::Path;

use sdl2::image::LoadSurface;
use sdl2::pixels::PixelFormatEnum;
use sdl2::surface::Surface;

use crate::error::GiochiError;
use crate::math::Rect;
use crate::render::renderer::RenderContext;
use crate::render::texture::TextureHandle;

/// Transparent gutter around every region, in pixels.
pub const ATLAS_PADDING: u32 = 1;

/// Largest atlas edge accepted, in pixels.
///
/// 4096 is the smallest maximum texture size the engine's targets guarantee (GLES 2.0
/// requires at least 2048; every console GPU here reports 4096).
pub const ATLAS_MAX_SIZE: u32 = 4096;

/// Smallest atlas width the packer starts from, in pixels.
const ATLAS_MIN_WIDTH: u32 = 256;

/// A region of an atlas, in both UV and pixel coordinates.
///
/// `u`/`v` are normalised (`0.0..=1.0`) and address the image itself, excluding the
/// padding; `x`/`y`/`width`/`height` are the same rectangle in texture pixels, ready to
/// pass as the `src` of [`RenderContext::draw_texture`].
///
/// # C ABI
///
/// Not exposed in ABI v2; phase 18c can project it if atlases become part of the C
/// surface. The Rust shape is already `#[repr(C)]`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct AtlasRegion {
    /// Normalised left edge.
    pub u: f32,
    /// Normalised top edge.
    pub v: f32,
    /// Normalised width.
    pub uv_width: f32,
    /// Normalised height.
    pub uv_height: f32,
    /// Left edge in texture pixels.
    pub x: u32,
    /// Top edge in texture pixels.
    pub y: u32,
    /// Width in texture pixels.
    pub width: u32,
    /// Height in texture pixels.
    pub height: u32,
    /// Reserved for future ABI-compatible fields. Must be zero.
    pub _reserved: [u32; 4],
}

impl AtlasRegion {
    /// The region's rectangle in texture pixels, for
    /// [`RenderContext::draw_texture`].
    pub fn pixel_rect(&self) -> Rect {
        Rect::new(
            self.x as f32,
            self.y as f32,
            self.width as f32,
            self.height as f32,
        )
    }

    /// Whether this is the empty region returned for an unknown name.
    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }
}

/// Several images packed into one texture.
///
/// # C ABI
///
/// Not exposed; C consumers draw the underlying handle with `giochi_render_draw_texture`
/// and a `src` rectangle, exactly as Rust users do.
#[repr(C)]
pub struct TextureAtlas {
    texture: TextureHandle,
    width: u32,
    height: u32,
    entries: Vec<Entry>,
    index: HashMap<String, usize>,
    _reserved: [u32; 4],
}

struct Entry {
    name: String,
    region: AtlasRegion,
}

impl std::fmt::Debug for TextureAtlas {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextureAtlas")
            .field("texture", &self.texture)
            .field("size", &(self.width, self.height))
            .field("regions", &self.entries.len())
            .finish_non_exhaustive()
    }
}

impl TextureAtlas {
    /// Loads `paths` and packs them into one texture registered in `render`.
    ///
    /// Region names are the file stems (`"player.png"` → `"player"`). Names must be
    /// unique. The packer uses a shelf layout, sorts images by height and rounds the
    /// atlas up to a power of two; a set that cannot fit in
    /// [`ATLAS_MAX_SIZE`] is an [`GiochiError::Asset`] rather than a partially loaded
    /// atlas.
    pub fn from_images(
        render: &RenderContext,
        paths: &[&str],
    ) -> Result<TextureAtlas, GiochiError> {
        if paths.is_empty() {
            return Err(GiochiError::InvalidArgument(
                "an atlas needs at least one image",
            ));
        }

        let mut images = Vec::with_capacity(paths.len());
        for path in paths {
            if path.trim().is_empty() {
                return Err(GiochiError::InvalidArgument("atlas image path is empty"));
            }
            let name = Path::new(path)
                .file_stem()
                .and_then(|stem| stem.to_str())
                .ok_or(GiochiError::Asset(format!(
                    "atlas image '{path}' has no usable file name"
                )))?
                .to_owned();
            images.push((name, read_rgba(path)?));
        }

        Self::build(render, images)
    }

    /// Packs in-memory RGBA8 images into one texture registered in `render`.
    ///
    /// This is [`TextureAtlas::from_images`] for generated content: `images` is a list of
    /// `(name, width, height, pixels)`, with `pixels` exactly `width * height * 4` bytes
    /// in RGBA order. The same packing, padding and validation rules apply. Region names
    /// must be unique.
    ///
    /// ```no_run
    /// # use giochi_engine::{RenderContext, TextureAtlas};
    /// # fn example(render: &RenderContext) -> Result<(), Box<dyn std::error::Error>> {
    /// let white = [255u8; 4 * 4 * 4];
    /// let atlas = TextureAtlas::from_rgba_images(render, &[("white", 4, 4, &white)])?;
    /// let source = atlas.region("white").pixel_rect();
    /// # let _ = source;
    /// # Ok(())
    /// # }
    /// ```
    pub fn from_rgba_images(
        render: &RenderContext,
        images: &[(&str, u32, u32, &[u8])],
    ) -> Result<TextureAtlas, GiochiError> {
        Self::build(render, convert_rgba_images(images)?)
    }

    /// Shared tail of the constructors: pack, upload, index.
    fn build(
        render: &RenderContext,
        images: Vec<(String, RgbaImage)>,
    ) -> Result<TextureAtlas, GiochiError> {
        let sizes: Vec<(u32, u32)> = images
            .iter()
            .map(|(_, image)| (image.width, image.height))
            .collect();
        let layout = pack(&sizes, ATLAS_PADDING, ATLAS_MAX_SIZE).ok_or_else(|| {
            GiochiError::Asset(format!(
                "{} images do not fit in a {ATLAS_MAX_SIZE}x{ATLAS_MAX_SIZE} atlas",
                images.len()
            ))
        })?;

        let mut pixels = vec![0u8; (layout.width * layout.height * 4) as usize];
        for ((_, image), (x, y)) in images.iter().zip(&layout.positions) {
            image.blit_into(&mut pixels, layout.width, *x, *y);
        }

        let texture = render.create_texture_from_rgba(layout.width, layout.height, &pixels)?;

        let mut entries = Vec::with_capacity(images.len());
        let mut index = HashMap::with_capacity(images.len());
        for ((name, image), (x, y)) in images.into_iter().zip(layout.positions) {
            if index.contains_key(&name) {
                render.textures().destroy_texture(texture);
                return Err(GiochiError::Asset(format!(
                    "duplicate atlas region name '{name}'"
                )));
            }
            let region = AtlasRegion {
                u: x as f32 / layout.width as f32,
                v: y as f32 / layout.height as f32,
                uv_width: image.width as f32 / layout.width as f32,
                uv_height: image.height as f32 / layout.height as f32,
                x,
                y,
                width: image.width,
                height: image.height,
                _reserved: [0; 4],
            };
            index.insert(name.clone(), entries.len());
            entries.push(Entry { name, region });
        }

        Ok(TextureAtlas {
            texture,
            width: layout.width,
            height: layout.height,
            entries,
            index,
            _reserved: [0; 4],
        })
    }

    /// The texture every region lives in.
    pub fn texture(&self) -> TextureHandle {
        self.texture
    }

    /// Atlas size in pixels.
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Number of regions.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the atlas holds no regions.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns the region called `name`, or an empty region.
    ///
    /// The lookup goes through a map built once at construction; an unknown name is not
    /// an error because a missing sprite is a content bug, and a game that crashes on
    /// one bad asset is worse than one that draws nothing.
    pub fn region(&self, name: &str) -> AtlasRegion {
        self.index
            .get(name)
            .map(|index| self.entries[*index].region)
            .unwrap_or_default()
    }

    /// Whether the atlas has a region called `name`.
    pub fn contains(&self, name: &str) -> bool {
        self.index.contains_key(name)
    }

    /// Every region name, in load order.
    pub fn names(&self) -> Vec<String> {
        self.entries
            .iter()
            .map(|entry| entry.name.clone())
            .collect()
    }
}

/// One image decoded to RGBA8.
#[derive(Debug)]
struct RgbaImage {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl RgbaImage {
    /// Copies the image into `destination` at `(left, top)`, rows padded to
    /// `destination_width`.
    fn blit_into(&self, destination: &mut [u8], destination_width: u32, left: u32, top: u32) {
        let source_stride = self.width as usize * 4;
        let destination_stride = destination_width as usize * 4;
        for row in 0..self.height as usize {
            let source = row * source_stride;
            let target = (top as usize + row) * destination_stride + left as usize * 4;
            destination[target..target + source_stride]
                .copy_from_slice(&self.pixels[source..source + source_stride]);
        }
    }
}

/// Validates the `(name, width, height, pixels)` list of
/// [`TextureAtlas::from_rgba_images`] and copies each buffer into an [`RgbaImage`].
///
/// Split out from the constructor so the rules can be tested without a window.
fn convert_rgba_images(
    images: &[(&str, u32, u32, &[u8])],
) -> Result<Vec<(String, RgbaImage)>, GiochiError> {
    if images.is_empty() {
        return Err(GiochiError::InvalidArgument(
            "an atlas needs at least one image",
        ));
    }

    let mut converted = Vec::with_capacity(images.len());
    for (name, width, height, pixels) in images {
        if name.trim().is_empty() {
            return Err(GiochiError::InvalidArgument("atlas region name is empty"));
        }
        if *width == 0 || *height == 0 {
            return Err(GiochiError::InvalidArgument(
                "an atlas region cannot have zero width or height",
            ));
        }
        let Some(expected) = rgba_buffer_len(*width, *height) else {
            return Err(GiochiError::InvalidArgument(
                "an atlas region's dimensions overflow the address space",
            ));
        };
        if pixels.len() != expected {
            return Err(GiochiError::InvalidArgument(
                "an atlas region needs exactly width * height * 4 bytes of RGBA8",
            ));
        }
        converted.push((
            (*name).to_owned(),
            RgbaImage {
                width: *width,
                height: *height,
                pixels: pixels.to_vec(),
            },
        ));
    }

    Ok(converted)
}

/// `width * height * 4`, or `None` when that cannot be addressed in memory.
fn rgba_buffer_len(width: u32, height: u32) -> Option<usize> {
    (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(4)
}

/// Loads and converts one image to RGBA8.
fn read_rgba(path: &str) -> Result<RgbaImage, GiochiError> {
    let surface = Surface::from_file(path)
        .map_err(|err| GiochiError::Asset(format!("failed to load atlas image '{path}': {err}")))?;
    let surface = surface
        .convert_format(PixelFormatEnum::RGBA32)
        .map_err(|err| {
            GiochiError::Asset(format!("failed to convert atlas image '{path}': {err}"))
        })?;

    let width = surface.width();
    let height = surface.height();
    if width == 0 || height == 0 {
        return Err(GiochiError::Asset(format!(
            "atlas image '{path}' has no pixels"
        )));
    }
    let pitch = surface.pitch() as usize;
    let bytes = surface
        .without_lock()
        .ok_or_else(|| GiochiError::Asset(format!("atlas image '{path}' is locked")))?;

    let mut pixels = vec![0u8; width as usize * height as usize * 4];
    for row in 0..height as usize {
        let source = row * pitch;
        let target = row * width as usize * 4;
        pixels[target..target + width as usize * 4]
            .copy_from_slice(&bytes[source..source + width as usize * 4]);
    }

    Ok(RgbaImage {
        width,
        height,
        pixels,
    })
}

/// Result of packing: the atlas size and each image's top-left corner.
#[derive(Debug, PartialEq, Eq)]
struct Layout {
    width: u32,
    height: u32,
    positions: Vec<(u32, u32)>,
}

/// Packs `sizes` into shelves, in the order given.
///
/// Images are sorted by height (tallest first) to keep shelves tight; `positions` comes
/// back in the caller's order. Returns `None` when an image cannot fit, or when the
/// packed height would exceed `max_size`.
fn pack(sizes: &[(u32, u32)], padding: u32, max_size: u32) -> Option<Layout> {
    if sizes.is_empty() {
        return None;
    }
    if sizes.iter().any(|(w, h)| *w == 0 || *h == 0) {
        return None;
    }
    if sizes.iter().any(|(w, h)| *w > max_size || *h > max_size) {
        // Checked before the additions below: a caller can pass an image whose width is
        // near `u32::MAX`, and `w + padding * 2` would panic in a debug build before the
        // "does not fit" answer was reached.
        return None;
    }

    let widest = sizes
        .iter()
        .map(|(width, _)| width + padding * 2)
        .max()
        .unwrap_or(0);
    let width = next_power_of_two(widest.max(ATLAS_MIN_WIDTH));
    if width > max_size {
        return None;
    }

    let mut order: Vec<usize> = (0..sizes.len()).collect();
    order.sort_by_key(|index| std::cmp::Reverse(sizes[*index].1));

    let mut positions = vec![(0u32, 0u32); sizes.len()];
    let mut x = padding;
    let mut y = padding;
    let mut shelf_height = 0;
    let mut used_height = padding;

    for index in order {
        let (image_width, image_height) = sizes[index];
        if x + image_width + padding > width {
            y += shelf_height + padding;
            x = padding;
            shelf_height = 0;
        }
        if y + image_height + padding > max_size {
            return None;
        }
        positions[index] = (x, y);
        x += image_width + padding;
        shelf_height = shelf_height.max(image_height);
        used_height = used_height.max(y + image_height + padding);
    }

    Some(Layout {
        width,
        height: next_power_of_two(used_height).min(max_size),
        positions,
    })
}

fn next_power_of_two(value: u32) -> u32 {
    value.max(1).checked_next_power_of_two().unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packing_places_every_image_inside_the_atlas() {
        let sizes = [(32, 32), (64, 16), (16, 48), (8, 8)];
        let layout = pack(&sizes, ATLAS_PADDING, ATLAS_MAX_SIZE).expect("fits");

        assert_eq!(layout.positions.len(), sizes.len());
        assert!(layout.width.is_power_of_two());
        assert!(layout.height.is_power_of_two());
        for ((width, height), (x, y)) in sizes.iter().zip(&layout.positions) {
            assert!(*x >= ATLAS_PADDING && *y >= ATLAS_PADDING);
            assert!(*x + width + ATLAS_PADDING <= layout.width);
            assert!(*y + height + ATLAS_PADDING <= layout.height);
        }
    }

    #[test]
    fn packed_images_do_not_overlap_and_keep_the_padding() {
        let sizes = [(20, 20), (20, 20), (20, 20), (20, 20), (20, 20), (20, 20)];
        let layout = pack(&sizes, ATLAS_PADDING, ATLAS_MAX_SIZE).expect("fits");

        let mut rects: Vec<(u32, u32, u32, u32)> = sizes
            .iter()
            .zip(&layout.positions)
            .map(|((w, h), (x, y))| (*x, *y, *w, *h))
            .collect();
        rects.sort_unstable();

        for (index, a) in rects.iter().enumerate() {
            for b in &rects[index + 1..] {
                let separated = a.0 + a.2 + ATLAS_PADDING <= b.0
                    || b.0 + b.2 + ATLAS_PADDING <= a.0
                    || a.1 + a.3 + ATLAS_PADDING <= b.1
                    || b.1 + b.3 + ATLAS_PADDING <= a.1;
                assert!(
                    separated,
                    "images {a:?} and {b:?} overlap or lose their padding"
                );
            }
        }
    }

    #[test]
    fn positions_come_back_in_the_original_order() {
        let sizes = [(8, 64), (64, 8), (16, 16)];
        let layout = pack(&sizes, ATLAS_PADDING, ATLAS_MAX_SIZE).expect("fits");

        // The tallest image is placed first, but its position is reported first too.
        assert_eq!(layout.positions.len(), 3);
        assert_eq!(layout.positions[0].0, ATLAS_PADDING);
        assert_eq!(layout.positions[0].1, ATLAS_PADDING);
    }

    #[test]
    fn an_image_too_large_or_an_empty_list_cannot_be_packed() {
        assert!(pack(&[], ATLAS_PADDING, ATLAS_MAX_SIZE).is_none());
        assert!(pack(&[(ATLAS_MAX_SIZE + 1, 8)], ATLAS_PADDING, ATLAS_MAX_SIZE).is_none());
        // Near `u32::MAX`, `width + padding * 2` used to overflow before the size check.
        assert!(pack(&[(u32::MAX, 8)], ATLAS_PADDING, ATLAS_MAX_SIZE).is_none());
        assert!(pack(&[(8, u32::MAX)], ATLAS_PADDING, ATLAS_MAX_SIZE).is_none());
        assert!(pack(&[(0, 8)], ATLAS_PADDING, ATLAS_MAX_SIZE).is_none());
        assert!(pack(&[(8, 0)], ATLAS_PADDING, ATLAS_MAX_SIZE).is_none());
    }

    #[test]
    fn the_height_grows_with_the_number_of_shelves() {
        let one = pack(&[(16, 16)], ATLAS_PADDING, ATLAS_MAX_SIZE).expect("fits");
        let many = pack(&[(16, 16); 200], ATLAS_PADDING, ATLAS_MAX_SIZE).expect("fits");

        assert!(many.height > one.height);
        assert!(many.positions.iter().all(|(x, y)| *x >= 1 && *y >= 1));
    }

    #[test]
    fn a_default_region_is_empty_and_has_no_pixels() {
        let region = AtlasRegion::default();

        assert!(region.is_empty());
        assert_eq!(region.pixel_rect(), Rect::ZERO);
        assert_eq!(region._reserved, [0; 4]);
    }

    #[test]
    fn a_region_reports_its_pixel_rectangle() {
        let region = AtlasRegion {
            u: 0.25,
            v: 0.5,
            uv_width: 0.125,
            uv_height: 0.25,
            x: 16,
            y: 32,
            width: 8,
            height: 16,
            _reserved: [0; 4],
        };

        assert!(!region.is_empty());
        assert_eq!(region.pixel_rect(), Rect::new(16.0, 32.0, 8.0, 16.0));
    }

    #[test]
    fn next_power_of_two_saturates_instead_of_panicking() {
        assert_eq!(next_power_of_two(1), 1);
        assert_eq!(next_power_of_two(255), 256);
        assert_eq!(next_power_of_two(u32::MAX), u32::MAX);
    }

    /// `convert_rgba_images` is every rule of `from_rgba_images` except the upload, so
    /// it can be asserted without a window.
    #[test]
    fn the_in_memory_constructor_rejects_a_bad_pixel_buffer() {
        let too_short = convert_rgba_images(&[("a", 4, 4, &[0; 63])]).expect_err("short buffer");
        assert!(matches!(too_short, GiochiError::InvalidArgument(_)));

        let empty_side = convert_rgba_images(&[("a", 0, 4, &[])]).expect_err("zero width");
        assert!(matches!(empty_side, GiochiError::InvalidArgument(_)));

        let no_name = convert_rgba_images(&[("  ", 4, 4, &[0; 64])]).expect_err("blank name");
        assert!(matches!(no_name, GiochiError::InvalidArgument(_)));

        let nothing = convert_rgba_images(&[]).expect_err("no images");
        assert!(matches!(nothing, GiochiError::InvalidArgument(_)));
    }

    #[test]
    fn the_in_memory_constructor_keeps_the_pixels_and_the_order() {
        let a = vec![1u8; 4 * 4 * 4];
        let b = vec![2u8; 8 * 2 * 4];
        let images = convert_rgba_images(&[("a", 4, 4, a.as_slice()), ("b", 8, 2, b.as_slice())])
            .expect("valid");

        assert_eq!(images.len(), 2);
        assert_eq!(images[0].0, "a");
        assert_eq!((images[0].1.width, images[0].1.height), (4, 4));
        assert_eq!(images[0].1.pixels, a);
        assert_eq!(images[1].0, "b");
        assert_eq!((images[1].1.width, images[1].1.height), (8, 2));
        assert_eq!(images[1].1.pixels, b);
    }
}
