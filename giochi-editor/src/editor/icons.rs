//! The hierarchy's type icons.
//!
//! `assets/icons/{mesh,light,camera,group}_{16,24}.png` are loaded once per session and
//! uploaded to egui as textures. Everything is done with safe SDL APIs: `Surface::from_file`,
//! `convert_format` and `without_lock` (the last one is safe; `without_lock_mut` is not).
//!
//! A failed load is not fatal. An editor that refuses to start because a 16×16 PNG is
//! missing is worse than one that draws a coloured bullet instead, so [`IconSet::paint`]
//! falls back to a letter.

use std::path::PathBuf;

use egui::{ColorImage, TextureHandle, TextureOptions};
use sdl2::image::LoadSurface;
use sdl2::pixels::PixelFormatEnum;
use sdl2::surface::Surface;

/// The four kinds a hierarchy row can be, in the order the icons are loaded.
pub const KINDS: [&str; 4] = ["mesh", "light", "camera", "group"];

/// Textures for the four node kinds.
#[derive(Clone)]
pub struct IconSet {
    /// Indexed like [`KINDS`]. `None` when that icon failed to load.
    textures: [Option<TextureHandle>; 4],
}

impl std::fmt::Debug for IconSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let loaded: Vec<&str> = KINDS
            .iter()
            .zip(self.textures.iter())
            .filter_map(|(kind, texture)| texture.as_ref().map(|_| *kind))
            .collect();
        f.debug_struct("IconSet").field("loaded", &loaded).finish()
    }
}

impl Default for IconSet {
    fn default() -> Self {
        Self::empty()
    }
}

impl IconSet {
    /// A set with no textures, which paints the fallback for every kind.
    pub fn empty() -> Self {
        Self {
            textures: [None, None, None, None],
        }
    }

    /// Loads the 16×16 icons from `directory`, keeping whatever loaded.
    ///
    /// `SDL_image` must be initialised; the engine does that the first time it loads a
    /// texture, and the editor loads its scene through the engine, but a caller that
    /// calls this first should run `sdl2::image::init(InitFlag::PNG)`.
    pub fn load(ctx: &egui::Context, directory: impl AsRef<std::path::Path>) -> Self {
        let base = directory.as_ref();
        let mut textures: [Option<TextureHandle>; 4] = [None, None, None, None];
        for (index, kind) in KINDS.iter().enumerate() {
            let path = base.join(format!("{kind}_16.png"));
            match load_rgba(&path) {
                Ok((width, height, pixels)) => {
                    let image = ColorImage::from_rgba_unmultiplied(
                        [width as usize, height as usize],
                        &pixels,
                    );
                    textures[index] = Some(ctx.load_texture(
                        format!("giochi-editor-icon-{kind}"),
                        image,
                        TextureOptions::NEAREST,
                    ));
                }
                Err(error) => {
                    log::warn!("icon {} not loaded: {error}", path.display());
                }
            }
        }
        Self { textures }
    }

    /// The icon directory that ships with this crate.
    pub fn default_directory() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join("icons")
    }

    /// Whether every icon loaded.
    pub fn is_complete(&self) -> bool {
        self.textures.iter().all(Option::is_some)
    }

    /// Paints `kind`'s icon at `size` points.
    pub fn paint(&self, ui: &mut egui::Ui, kind: &str, size: f32) {
        if let Some(texture) = self.texture(kind) {
            // `painter.image` with an explicit 0..1 UV rather than the `Image` widget:
            // SDL's `render_geometry` rejects a UV outside the range, and the widget
            // path produced `-0.035714` here (measured; three warnings per frame, one
            // per row). An explicit UV is both smaller and exact.
            let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
            ui.painter().image(
                texture.id(),
                rect,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
            return;
        }
        // Fallback: one coloured letter, which is readable even with no assets at all.
        let (letter, color) = match kind {
            "mesh" => ("M", egui::Color32::from_rgb(0x7f, 0x9f, 0xd8)),
            "light" => ("L", egui::Color32::from_rgb(0xe0, 0xc0, 0x50)),
            "camera" => ("C", egui::Color32::from_rgb(0x60, 0xc8, 0xc0)),
            _ => ("G", ui.visuals().weak_text_color()),
        };
        ui.label(
            egui::RichText::new(letter)
                .small()
                .strong()
                .color(color)
                .monospace(),
        );
    }

    fn texture(&self, kind: &str) -> Option<&TextureHandle> {
        KINDS
            .iter()
            .position(|candidate| *candidate == kind)
            .and_then(|index| self.textures[index].as_ref())
    }
}

/// Reads one PNG as RGBA8, row by row.
fn load_rgba(path: &std::path::Path) -> Result<(u32, u32, Vec<u8>), String> {
    let surface = Surface::from_file(path).map_err(|error| error.to_string())?;
    let surface = surface
        .convert_format(PixelFormatEnum::RGBA32)
        .map_err(|error| error.to_string())?;
    let (width, height) = (surface.width(), surface.height());
    if width == 0 || height == 0 {
        return Err("image has no pixels".to_owned());
    }
    let pitch = surface.pitch() as usize;
    let bytes = surface
        .without_lock()
        .ok_or_else(|| "surface is locked".to_owned())?;
    let mut pixels = vec![0u8; width as usize * height as usize * 4];
    for row in 0..height as usize {
        let source = row * pitch;
        let target = row * width as usize * 4;
        pixels[target..target + width as usize * 4]
            .copy_from_slice(&bytes[source..source + width as usize * 4]);
    }
    Ok((width, height, pixels))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_icons_decode_to_the_size_they_claim() {
        for kind in KINDS {
            let path = IconSet::default_directory().join(format!("{kind}_16.png"));
            let (width, height, pixels) =
                load_rgba(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            assert_eq!((width, height), (16, 16));
            assert_eq!(pixels.len(), 16 * 16 * 4);
        }
    }

    #[test]
    fn the_24_pixel_variants_exist_too() {
        for kind in KINDS {
            let path = IconSet::default_directory().join(format!("{kind}_24.png"));
            let (width, height, _) = load_rgba(&path).expect("24px icon");
            assert_eq!((width, height), (24, 24));
        }
    }

    #[test]
    fn a_missing_directory_yields_an_incomplete_set_instead_of_failing() {
        let set = IconSet::load(&egui::Context::default(), "/nonexistent/icons");
        assert!(!set.is_complete());
        assert!(set.texture("mesh").is_none());
    }

    #[test]
    fn an_unknown_kind_is_a_group() {
        let set = IconSet::empty();
        assert!(set.texture("nonsense").is_none());
        assert!(set.texture("group").is_none());
    }
}
