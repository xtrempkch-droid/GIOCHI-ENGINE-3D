//! The scene file dialog, drawn with egui.
//!
//! # Why not `rfd`
//!
//! The plan listed `rfd` in phase 30 and phase 31. On this project's target it is the
//! wrong tool: `rfd`'s default backend on Linux is `xdg-desktop-portal`, and the console
//! this engine ships on (GIOCHI-OS) boots straight into the game with no portal, no
//! D-Bus session and no desktop. A dialog that only works on the developer's machine is
//! worse than no dialog. The GTK backend needs GTK3 in the image, which a KMSDRM
//! appliance does not have either.
//!
//! What the editor needs is small — pick a `.scene` file, or name one — so it draws the
//! dialog itself with the egui context it already owns. ADR-064 records the trade; the
//! moment a real portal is a requirement, `rfd` can be swapped in behind
//! [`FileDialog::show`] without touching the rest of the editor.

use std::path::{Path, PathBuf};

/// What the user did with the dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogOutcome {
    /// The dialog is still open.
    Pending,
    /// Open this path.
    Open(PathBuf),
    /// Save to this path.
    Save(PathBuf),
    /// Dismissed.
    Cancel,
}

/// Which action the dialog is collecting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogKind {
    /// Pick an existing file.
    Open,
    /// Name a file to write.
    Save,
}

impl DialogKind {
    /// The window title.
    pub const fn title(self) -> &'static str {
        match self {
            Self::Open => "Open Scene",
            Self::Save => "Save Scene As",
        }
    }

    /// The label of the confirming button.
    pub const fn confirm_label(self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::Save => "Save",
        }
    }
}

/// One row in the listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// What it points at.
    pub path: PathBuf,
    /// What the row shows.
    pub label: String,
    /// Whether it is a directory.
    pub is_dir: bool,
}

/// A file dialog over one directory, filtered to `.scene` files.
#[derive(Debug, Clone, PartialEq)]
pub struct FileDialog {
    /// Open or save.
    pub kind: DialogKind,
    /// The directory being listed.
    pub directory: PathBuf,
    /// The file name field. Always ends in `.scene` when the dialog is used.
    pub filename: String,
    /// The directory's contents: `..` first when there is a parent, then directories,
    /// then `.scene` files.
    pub entries: Vec<Entry>,
    /// Why the last refresh failed, if it did.
    pub error: Option<String>,
}

impl FileDialog {
    /// An "open" dialog starting in `directory`.
    pub fn open(directory: impl AsRef<Path>) -> Self {
        let mut dialog = Self {
            kind: DialogKind::Open,
            directory: directory.as_ref().to_path_buf(),
            filename: String::new(),
            entries: Vec::new(),
            error: None,
        };
        dialog.refresh();
        dialog
    }

    /// A "save as" dialog starting in `directory` with `filename` pre-filled.
    pub fn save(directory: impl AsRef<Path>, filename: impl Into<String>) -> Self {
        let mut dialog = Self {
            kind: DialogKind::Save,
            directory: directory.as_ref().to_path_buf(),
            filename: with_scene_extension(filename.into()),
            entries: Vec::new(),
            error: None,
        };
        dialog.refresh();
        dialog
    }

    /// Re-reads the directory.
    pub fn refresh(&mut self) {
        self.entries.clear();
        self.error = None;

        if let Some(parent) = self.directory.parent() {
            self.entries.push(Entry {
                path: parent.to_path_buf(),
                label: "..".to_owned(),
                is_dir: true,
            });
        }

        let read = match std::fs::read_dir(&self.directory) {
            Ok(read) => read,
            Err(error) => {
                self.error = Some(format!("{}: {error}", self.directory.display()));
                return;
            }
        };

        let mut directories = Vec::new();
        let mut files = Vec::new();
        for entry in read.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
            if is_dir {
                directories.push(Entry {
                    path,
                    label: format!("{name}/"),
                    is_dir: true,
                });
            } else if path
                .extension()
                .is_some_and(|extension| extension == "scene")
            {
                files.push(Entry {
                    path,
                    label: name,
                    is_dir: false,
                });
            }
        }
        directories.sort_by(|a, b| a.label.cmp(&b.label));
        files.sort_by(|a, b| a.label.cmp(&b.label));
        self.entries.append(&mut directories);
        self.entries.append(&mut files);
    }

    /// The path the dialog would act on right now, if the filename is usable.
    pub fn selected_path(&self) -> Option<PathBuf> {
        let name = self.filename.trim();
        if name.is_empty() {
            return None;
        }
        Some(self.directory.join(with_scene_extension(name.to_owned())))
    }

    /// Enters `entry`: a directory is listed, a file is chosen by an "open" dialog.
    pub fn activate(&mut self, entry: &Entry) -> DialogOutcome {
        if entry.is_dir {
            self.directory = entry.path.clone();
            self.refresh();
            DialogOutcome::Pending
        } else {
            self.filename = entry.label.clone();
            match self.kind {
                DialogKind::Open => DialogOutcome::Open(entry.path.clone()),
                DialogKind::Save => self
                    .selected_path()
                    .map_or(DialogOutcome::Pending, DialogOutcome::Save),
            }
        }
    }

    /// Draws the dialog and reports what the user did.
    pub fn show(&mut self, ctx: &egui::Context) -> DialogOutcome {
        let mut outcome = DialogOutcome::Pending;
        let title = self.kind.title().to_owned();
        let confirm = self.kind.confirm_label();

        egui::Window::new(title)
            .collapsible(false)
            .resizable(true)
            .default_width(560.0)
            .default_height(420.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("Up").clicked() {
                        if let Some(parent) = self.directory.parent() {
                            self.directory = parent.to_path_buf();
                            self.refresh();
                        }
                    }
                    if ui.button("Refresh").clicked() {
                        self.refresh();
                    }
                    ui.label(
                        egui::RichText::new(self.directory.display().to_string())
                            .small()
                            .monospace(),
                    );
                });
                ui.separator();

                if let Some(error) = &self.error {
                    ui.colored_label(egui::Color32::from_rgb(0xff, 0x6b, 0x6b), error);
                }

                egui::ScrollArea::vertical()
                    .max_height(260.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for entry in self.entries.clone() {
                            let label = if entry.is_dir {
                                egui::RichText::new(&entry.label).strong()
                            } else {
                                egui::RichText::new(&entry.label)
                            };
                            if ui
                                .selectable_label(self.filename == entry.label, label)
                                .clicked()
                            {
                                outcome = self.activate(&entry);
                            }
                        }
                        if self.entries.is_empty() && self.error.is_none() {
                            ui.label("no .scene files here");
                        }
                    });

                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("file");
                    ui.text_edit_singleline(&mut self.filename);
                    if ui.button(confirm).clicked() {
                        if let Some(path) = self.selected_path() {
                            outcome = match self.kind {
                                DialogKind::Open => {
                                    if path.exists() {
                                        DialogOutcome::Open(path)
                                    } else {
                                        self.error =
                                            Some(format!("{} does not exist", path.display()));
                                        DialogOutcome::Pending
                                    }
                                }
                                DialogKind::Save => DialogOutcome::Save(path),
                            };
                        } else {
                            self.error = Some("type a file name".to_owned());
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        outcome = DialogOutcome::Cancel;
                    }
                });
            });

        outcome
    }
}

/// Appends `.scene` unless the name already ends in it.
pub fn with_scene_extension(name: String) -> String {
    if name.to_ascii_lowercase().ends_with(".scene") {
        name
    } else {
        format!("{name}.scene")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("giochi-dialog-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("dir");
        path
    }

    #[test]
    fn the_extension_is_added_once() {
        assert_eq!(with_scene_extension("level".to_owned()), "level.scene");
        assert_eq!(
            with_scene_extension("level.scene".to_owned()),
            "level.scene"
        );
        assert_eq!(
            with_scene_extension("level.SCENE".to_owned()),
            "level.SCENE"
        );
    }

    #[test]
    fn a_new_dialog_lists_only_scene_files_and_directories() {
        let directory = folder("listing");
        std::fs::write(directory.join("a.scene"), "{}").expect("write");
        std::fs::write(directory.join("b.txt"), "hello").expect("write");
        std::fs::create_dir(directory.join("sub")).expect("mkdir");

        let dialog = FileDialog::open(&directory);
        let labels: Vec<&str> = dialog.entries.iter().map(|e| e.label.as_str()).collect();
        assert!(labels.contains(&"a.scene"));
        assert!(labels.contains(&"sub/"));
        assert!(!labels.contains(&"b.txt"), "{labels:?}");
        assert_eq!(labels.first(), Some(&".."), "parent first");
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn directories_sort_before_files() {
        let directory = folder("sorting");
        std::fs::write(directory.join("z.scene"), "{}").expect("write");
        std::fs::create_dir(directory.join("m")).expect("mkdir");

        let dialog = FileDialog::open(&directory);
        let sub = dialog
            .entries
            .iter()
            .position(|e| e.label == "m/")
            .expect("dir");
        let file = dialog
            .entries
            .iter()
            .position(|e| e.label == "z.scene")
            .expect("file");
        assert!(sub < file);
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn entering_a_directory_relistens_inside_it() {
        let directory = folder("enter");
        std::fs::create_dir(directory.join("sub")).expect("mkdir");
        std::fs::write(directory.join("sub").join("inner.scene"), "{}").expect("write");

        let mut dialog = FileDialog::open(&directory);
        let entry = dialog
            .entries
            .iter()
            .find(|e| e.label == "sub/")
            .expect("entry")
            .clone();
        assert_eq!(dialog.activate(&entry), DialogOutcome::Pending);
        assert_eq!(dialog.directory, directory.join("sub"));
        assert!(dialog.entries.iter().any(|e| e.label == "inner.scene"));
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn choosing_a_file_in_an_open_dialog_reports_it() {
        let directory = folder("choose");
        let target = directory.join("level.scene");
        std::fs::write(&target, "{}").expect("write");

        let mut dialog = FileDialog::open(&directory);
        let entry = dialog
            .entries
            .iter()
            .find(|e| e.label == "level.scene")
            .expect("entry")
            .clone();
        assert_eq!(dialog.activate(&entry), DialogOutcome::Open(target));
        assert_eq!(dialog.filename, "level.scene");
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn save_mode_produces_a_path_with_the_extension() {
        let directory = folder("saveas");
        let dialog = FileDialog::save(&directory, "my level");
        assert_eq!(dialog.filename, "my level.scene");
        assert_eq!(
            dialog.selected_path(),
            Some(directory.join("my level.scene"))
        );
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn an_empty_filename_has_no_path() {
        let directory = folder("empty");
        let dialog = FileDialog::open(&directory);
        assert_eq!(dialog.selected_path(), None);
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn listing_a_missing_directory_reports_an_error_and_keeps_the_parent() {
        let missing = std::env::temp_dir().join("giochi-dialog-does-not-exist");
        let _ = std::fs::remove_dir_all(&missing);
        let dialog = FileDialog::open(&missing);
        assert!(dialog.error.is_some());
        assert!(dialog.entries.iter().any(|e| e.label == ".."));
    }
}
