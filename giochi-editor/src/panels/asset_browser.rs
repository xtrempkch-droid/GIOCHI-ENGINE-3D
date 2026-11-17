//! The asset browser: the project's files, a preview, and dragging them into the scene.
//!
//! # What a drop means
//!
//! | Dropped on | Payload | Effect |
//! |---|---|---|
//! | viewport | `.obj` model | adds a node at the ground point under the cursor |
//! | inspector's material slot | texture | sets `MaterialDesc::texture` |
//! | anywhere else | anything | nothing; the drag is cancelled |
//!
//! A `.gltf`/`.glb` is listed and draggable, and dropping one reports in the status bar
//! that its loader belongs to the pending Production Pack and that `.obj` is what works.
//! That is the honest version of "drag a glTF to the viewport instantiates it" in a
//! repository whose 3D crate has no glTF loader (ADR-063): the alternative is a drop that
//! silently does nothing.
//!
//! # The listing is a snapshot
//!
//! The panel rescans the folder when the project opens, after an import, or when
//! **Refresh** is pressed — not every frame. A per-frame `read_dir` of an asset folder on
//! a handheld SD card is a stutter waiting to happen, and nothing else in the editor
//! writes into `assets/`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use egui::TextureHandle;

use crate::editor::{EditorState, EditorTheme};
use crate::project::{AssetEntry, AssetKind, Project};

/// What is being dragged from the browser.
///
/// `Send + Sync` because egui stores it in its context; it is three owned values and a
/// [`PathBuf`], which is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetDrag {
    /// Where the file is on disk.
    pub path: PathBuf,
    /// The path a scene stores, relative to the project root.
    pub relative: String,
    /// What it is.
    pub kind: AssetKind,
}

impl AssetDrag {
    /// A drag payload for `entry`.
    pub fn of(entry: &AssetEntry) -> Self {
        Self {
            path: entry.path.clone(),
            relative: entry.relative.clone(),
            kind: entry.kind,
        }
    }

    /// Whether the engine can load it today (`.gltf`/`.glb` cannot).
    pub fn is_loadable(&self) -> bool {
        self.kind.is_loadable(&self.path)
    }

    /// The name to give a node created from this asset: the file stem.
    pub fn node_name(&self) -> String {
        self.path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "asset".to_owned())
    }
}

/// The browser's own state: filter, listing and the last preview.
///
/// `Clone` because [`crate::editor::EditorState`] is cloned (phase 39's play-in-editor
/// will copy the editing state), and `TextureHandle` is `Clone`. Not `PartialEq`: two
/// browsers holding the same paths but different GPU handles are not "equal" in any sense
/// a caller could act on.
#[derive(Clone, Default)]
pub struct AssetBrowserState {
    /// The active filter bucket.
    pub filter: AssetKind,
    /// The listing, refreshed on demand.
    pub entries: Vec<AssetEntry>,
    /// The entry the preview shows.
    pub selected: Option<PathBuf>,
    /// Textures decoded for the preview, keyed by path.
    previews: HashMap<PathBuf, (SystemTime, TextureHandle)>,
    /// Set once so the first frame with a project scans it.
    scanned: bool,
}

impl std::fmt::Debug for AssetBrowserState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AssetBrowserState")
            .field("filter", &self.filter)
            .field("entries", &self.entries.len())
            .field("selected", &self.selected)
            .field("previews", &self.previews.len())
            .field("scanned", &self.scanned)
            .finish()
    }
}

impl AssetBrowserState {
    /// A browser showing everything.
    pub fn new() -> Self {
        Self {
            filter: AssetKind::All,
            ..Self::default()
        }
    }

    /// Re-reads the project's assets.
    pub fn refresh(&mut self, project: Option<&Project>) {
        self.entries = match project {
            Some(project) => project.scan_assets(self.filter, 2_000),
            None => Vec::new(),
        };
        self.scanned = true;
        if let Some(selected) = &self.selected {
            if !self.entries.iter().any(|entry| &entry.path == selected) {
                self.selected = None;
            }
        }
    }

    /// Changes the filter and rescans.
    pub fn set_filter(&mut self, project: Option<&Project>, filter: AssetKind) {
        if self.filter != filter {
            self.filter = filter;
            self.refresh(project);
        }
    }

    /// Scans on the first frame that has a project, so the panel is correct on open.
    pub fn scan_if_needed(&mut self, project: Option<&Project>) {
        if !self.scanned && project.is_some() {
            self.refresh(project);
        }
    }

    /// Forgets the listing, for a project that was just closed.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.selected = None;
        self.previews.clear();
        self.scanned = false;
    }
}

/// Draws the browser and returns a drag payload when the user starts dragging an entry.
///
/// The caller owns what to do with the payload; the panel only reports it.
pub fn draw(ui: &mut egui::Ui, state: &mut EditorState, theme: &EditorTheme) -> Option<AssetDrag> {
    let project = state.project.clone();
    state.assets.scan_if_needed(project.as_ref());

    let mut refresh = false;
    let mut filter = state.assets.filter;

    ui.horizontal_wrapped(|ui| {
        for bucket in AssetKind::FILTERS {
            if ui
                .selectable_label(filter == bucket, bucket.label())
                .clicked()
            {
                filter = bucket;
            }
        }
        ui.separator();
        if ui.button("Refresh").clicked() {
            refresh = true;
        }
        if ui.button("Import Asset…").clicked() {
            state.import_dialog();
        }
        if let Some(project) = &project {
            ui.label(
                egui::RichText::new(format!(
                    "{} — {} item(s)",
                    project.name,
                    state.assets.entries.len()
                ))
                .small()
                .color(theme.muted),
            );
        }
    });
    if filter != state.assets.filter {
        state.assets.set_filter(project.as_ref(), filter);
    }
    if refresh {
        state.assets.refresh(project.as_ref());
    }
    ui.separator();

    let Some(project) = project else {
        ui.label(
            egui::RichText::new("No project open. File > New Project… or Open Project…")
                .small()
                .color(theme.muted),
        );
        return None;
    };

    if state.assets.entries.is_empty() {
        ui.label(
            egui::RichText::new(format!(
                "No assets yet. Import Asset… copies a file into {}.",
                project.assets_dir().display()
            ))
            .small()
            .color(theme.muted),
        );
        return None;
    }

    let mut drag = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for entry in state.assets.entries.clone() {
                let is_selected = state.assets.selected.as_deref() == Some(entry.path.as_path());
                let label = format!(
                    "{}  {}{}",
                    entry.kind.label(),
                    entry.label,
                    if entry.is_loadable() {
                        ""
                    } else {
                        "  (no loader)"
                    }
                );
                let dragged = ui
                    .dnd_drag_source(
                        egui::Id::new(("giochi-asset", &entry.relative)),
                        AssetDrag::of(&entry),
                        |ui| {
                            ui.selectable_label(is_selected, label).clicked();
                        },
                    )
                    .response;
                if dragged.clicked() {
                    state.assets.selected = Some(entry.path.clone());
                }
                if dragged.drag_started() {
                    drag = Some(AssetDrag::of(&entry));
                }
            }
        });

    preview(ui, state, &project, theme);
    drag
}

/// Draws the preview of the selected asset.
fn preview(ui: &mut egui::Ui, state: &mut EditorState, project: &Project, theme: &EditorTheme) {
    let Some(path) = state.assets.selected.clone() else {
        return;
    };
    let Some(entry) = state
        .assets
        .entries
        .iter()
        .find(|entry| entry.path == path)
        .cloned()
    else {
        return;
    };

    ui.separator();
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(&entry.label).small().strong());
        ui.label(
            egui::RichText::new(format!("{} B", entry.size))
                .small()
                .color(theme.muted),
        );
    });

    match entry.kind {
        AssetKind::Texture => {
            match texture_preview(ui.ctx(), &mut state.assets, &entry.path, theme) {
                Some(texture) => {
                    let size = texture.size_vec2();
                    let scale = (120.0 / size.x.max(1.0))
                        .min(120.0 / size.y.max(1.0))
                        .min(1.0);
                    ui.add(egui::Image::new(&texture).fit_to_exact_size(size * scale));
                }
                None => {
                    ui.label(
                        egui::RichText::new("could not decode this image")
                            .small()
                            .color(theme.muted),
                    );
                }
            }
        }
        AssetKind::Model => draw_wire_cube(ui, theme),
        AssetKind::Scene => match giochi_engine_3d::scene::Scene::load_from_path(&entry.path) {
            Ok(scene) => {
                ui.label(
                    egui::RichText::new(format!(
                        "{} — {} object(s)",
                        scene.name,
                        scene.nodes.len()
                    ))
                    .small()
                    .color(theme.muted),
                );
            }
            Err(error) => {
                ui.label(
                    egui::RichText::new(error.to_string())
                        .small()
                        .color(theme.danger),
                );
            }
        },
        AssetKind::Sound => {
            ui.label(
                egui::RichText::new("audio: play it from the game, not here")
                    .small()
                    .color(theme.muted),
            );
        }
        AssetKind::All | AssetKind::Other => {}
    }

    if !entry.is_loadable() {
        ui.label(
            egui::RichText::new(format!(
                "{} is recognised, but the glTF loader is part of the pending Production \
                 Pack. Import an .obj to place a model.",
                path.extension()
                    .map(|extension| extension.to_string_lossy().to_uppercase())
                    .unwrap_or_default()
            ))
            .small()
            .color(egui::Color32::from_rgb(0xd8, 0xa8, 0x40)),
        );
    }
    let _ = project;
}

/// The "wire cube" the plan asks for as a mesh preview: three faces of a cube in lines.
fn draw_wire_cube(ui: &mut egui::Ui, theme: &EditorTheme) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(72.0, 72.0), egui::Sense::hover());
    let painter = ui.painter();
    let centre = rect.center();
    let radius = 24.0;
    let front = [
        centre + egui::vec2(-radius, -radius * 0.6),
        centre + egui::vec2(radius, -radius * 0.6),
        centre + egui::vec2(radius, radius * 0.6),
        centre + egui::vec2(-radius, radius * 0.6),
    ];
    let offset = egui::vec2(radius * 0.5, -radius * 0.5);
    let stroke = egui::Stroke::new(1.0_f32, theme.accent);
    painter.rect_stroke(
        egui::Rect::from_points(&front),
        egui::CornerRadius::ZERO,
        egui::Stroke::new(1.0_f32, theme.muted),
        egui::StrokeKind::Middle,
    );
    for corner in front {
        painter.line_segment([corner, corner + offset], stroke);
    }
    let back: Vec<egui::Pos2> = front.iter().map(|corner| *corner + offset).collect();
    for index in 0..back.len() {
        painter.line_segment([back[index], back[(index + 1) % back.len()]], stroke);
    }
}

fn texture_preview(
    ctx: &egui::Context,
    browser: &mut AssetBrowserState,
    path: &Path,
    theme: &EditorTheme,
) -> Option<TextureHandle> {
    let modified = std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH);
    if let Some((stamp, texture)) = browser.previews.get(path) {
        if *stamp == modified {
            return Some(texture.clone());
        }
    }
    let (width, height, pixels) = load_rgba(path).ok()?;
    let image =
        egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], &pixels);
    let texture = ctx.load_texture(
        format!("giochi-asset-preview-{}", path.display()),
        image,
        egui::TextureOptions::LINEAR,
    );
    let _ = theme;
    browser
        .previews
        .insert(path.to_path_buf(), (modified, texture.clone()));
    Some(texture)
}

/// Reads an image as RGBA8 with safe SDL calls, row by row (the same shape as the icon
/// loader: `convert_format` to RGBA32, then `without_lock`).
fn load_rgba(path: &Path) -> Result<(u32, u32, Vec<u8>), String> {
    use sdl2::image::LoadSurface;
    use sdl2::pixels::PixelFormatEnum;
    use sdl2::surface::Surface;

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
    use crate::project::Project;

    fn root(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("giochi-browser-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        path
    }

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("dir");
        }
        std::fs::write(path, contents).expect("write");
    }

    #[test]
    fn a_drag_payload_carries_a_node_name_and_the_relative_path() {
        let entry = AssetEntry {
            path: PathBuf::from("/p/assets/models/race_car.obj"),
            relative: "assets/models/race_car.obj".to_owned(),
            label: "models/race_car.obj".to_owned(),
            kind: AssetKind::Model,
            size: 10,
        };
        let drag = AssetDrag::of(&entry);
        assert_eq!(drag.node_name(), "race_car");
        assert_eq!(drag.relative, entry.relative);
        assert_eq!(drag.kind, AssetKind::Model);
    }

    #[test]
    fn the_browser_scans_once_a_project_exists() {
        let root = root("scan-once");
        let project = Project::create(&root, "p").expect("created");
        write(&root.join("assets/textures/a.png"), "x");

        let mut browser = AssetBrowserState::new();
        browser.scan_if_needed(None);
        assert!(browser.entries.is_empty(), "nothing to scan yet");

        browser.scan_if_needed(Some(&project));
        assert_eq!(browser.entries.len(), 1);

        // A file added afterwards is not picked up until a refresh: the listing is a
        // snapshot, not a per-frame read_dir.
        write(&root.join("assets/textures/b.png"), "x");
        browser.scan_if_needed(Some(&project));
        assert_eq!(browser.entries.len(), 1);
        browser.refresh(Some(&project));
        assert_eq!(browser.entries.len(), 2);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn changing_the_filter_rescans_and_drops_a_selection_that_no_longer_matches() {
        let root = root("filter");
        let project = Project::create(&root, "p").expect("created");
        write(&root.join("assets/textures/a.png"), "x");
        write(&root.join("assets/models/a.obj"), "o");

        let mut browser = AssetBrowserState::new();
        browser.refresh(Some(&project));
        assert_eq!(browser.entries.len(), 2);

        browser.selected = Some(root.join("assets/models/a.obj"));
        browser.set_filter(Some(&project), AssetKind::Texture);
        assert_eq!(browser.entries.len(), 1);
        assert_eq!(browser.entries[0].kind, AssetKind::Texture);
        assert_eq!(browser.selected, None, "a hidden selection is cleared");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn closing_the_project_clears_the_listing() {
        let root = root("clear");
        let project = Project::create(&root, "p").expect("created");
        write(&root.join("assets/textures/a.png"), "x");

        let mut browser = AssetBrowserState::new();
        browser.refresh(Some(&project));
        assert_eq!(browser.entries.len(), 1);
        browser.selected = Some(root.join("assets/textures/a.png"));

        browser.clear();
        assert!(browser.entries.is_empty());
        assert_eq!(browser.selected, None);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_default_filter_is_all() {
        let browser = AssetBrowserState::new();
        assert_eq!(browser.filter, AssetKind::All);
        assert!(browser.entries.is_empty());
    }
}
