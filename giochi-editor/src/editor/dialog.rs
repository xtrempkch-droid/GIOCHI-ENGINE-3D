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

use crate::project::PROJECT_FILE;

/// What the user did with the dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogOutcome {
    /// The dialog is still open.
    Pending,
    /// Open this path (a file, or a directory for a project).
    Open(PathBuf),
    /// Save to this path.
    Save(PathBuf),
    /// Create a project folder `parent/name`.
    CreateProject {
        /// The folder the new project goes in.
        parent: PathBuf,
        /// The project's folder name.
        name: String,
    },
    /// Dismissed.
    Cancel,
}

/// What the dialog is for, which decides its title, its listing and what confirming means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogKind {
    /// Pick an existing `.scene`.
    OpenScene,
    /// Name a `.scene` to write.
    SaveScene,
    /// Pick a directory that contains `project.toml`.
    OpenProject,
    /// Pick a `.scene`-like file of any type, to copy into the project's assets.
    ImportAsset,
    /// Pick the parent folder and type the folder name of a new project.
    NewProject,
}

impl DialogKind {
    /// The window title.
    pub const fn title(self) -> &'static str {
        match self {
            Self::OpenScene => "Open Scene",
            Self::SaveScene => "Save Scene As",
            Self::OpenProject => "Open Project",
            Self::ImportAsset => "Import Asset",
            Self::NewProject => "New Project",
        }
    }

    /// The label of the confirming button.
    pub const fn confirm_label(self) -> &'static str {
        match self {
            Self::OpenScene | Self::OpenProject => "Open",
            Self::SaveScene => "Save",
            Self::ImportAsset => "Import",
            Self::NewProject => "Create",
        }
    }

    /// Whether the dialog has a name field.
    const fn needs_filename(self) -> bool {
        matches!(self, Self::SaveScene | Self::NewProject)
    }

    /// Whether a directory row enters the directory or is itself the answer.
    const fn directories_are_answers(self) -> bool {
        matches!(self, Self::OpenProject)
    }

    /// Whether the name field gets the `.scene` extension appended.
    const fn forces_scene_extension(self) -> bool {
        matches!(self, Self::SaveScene)
    }

    /// Which files the listing shows.
    fn shows(self, path: &Path) -> bool {
        match self {
            Self::OpenScene | Self::SaveScene => path
                .extension()
                .is_some_and(|extension| extension == "scene"),
            // A project name is a folder name; the dialog browses for its parent, so no
            // file is ever an answer.
            Self::NewProject => false,
            Self::ImportAsset => path.is_file(),
            // A project is confirmed by its folder, but showing `project.toml` makes the
            // folder's identity visible while browsing into it.
            Self::OpenProject => {
                path.is_file() && path.file_name().is_some_and(|name| name == PROJECT_FILE)
            }
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
    /// An "open scene" dialog starting in `directory`.
    pub fn open(directory: impl AsRef<Path>) -> Self {
        Self::new(DialogKind::OpenScene, directory, "")
    }

    /// A "save scene as" dialog starting in `directory` with `filename` pre-filled.
    pub fn save(directory: impl AsRef<Path>, filename: impl Into<String>) -> Self {
        Self::new(
            DialogKind::SaveScene,
            directory,
            with_scene_extension(filename.into()),
        )
    }

    /// An "open project" dialog starting in `directory`.
    pub fn open_project(directory: impl AsRef<Path>) -> Self {
        Self::new(DialogKind::OpenProject, directory, "")
    }

    /// An "import asset" dialog starting in `directory`.
    pub fn import(directory: impl AsRef<Path>) -> Self {
        Self::new(DialogKind::ImportAsset, directory, "")
    }

    /// A "new project" dialog: the parent folder is `directory`, the name is pre-filled.
    pub fn new_project(directory: impl AsRef<Path>) -> Self {
        Self::new(DialogKind::NewProject, directory, "my-project")
    }

    fn new(kind: DialogKind, directory: impl AsRef<Path>, filename: impl Into<String>) -> Self {
        let mut dialog = Self {
            kind,
            directory: directory.as_ref().to_path_buf(),
            filename: filename.into(),
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
            } else if self.kind.shows(&path) {
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
    ///
    /// `None` for a project dialog: what it produces is a folder name, not a file path
    /// (see [`DialogOutcome::CreateProject`]).
    pub fn selected_path(&self) -> Option<PathBuf> {
        if !self.kind.needs_filename() || self.kind == DialogKind::NewProject {
            return None;
        }
        let name = self.filename.trim();
        if name.is_empty() {
            return None;
        }
        let name = if self.kind.forces_scene_extension() {
            with_scene_extension(name.to_owned())
        } else {
            name.to_owned()
        };
        Some(self.directory.join(name))
    }

    /// Enters `entry`: a directory is listed (or chosen, for a project), a file is picked.
    pub fn activate(&mut self, entry: &Entry) -> DialogOutcome {
        if entry.is_dir {
            if self.kind.directories_are_answers() {
                return match crate::project::Project::open(&entry.path) {
                    Ok(_) => DialogOutcome::Open(entry.path.clone()),
                    Err(error) => {
                        self.error = Some(error);
                        DialogOutcome::Pending
                    }
                };
            }
            self.directory = entry.path.clone();
            self.refresh();
            DialogOutcome::Pending
        } else {
            self.filename = entry.label.clone();
            if self.kind.needs_filename() {
                self.selected_path()
                    .map_or(DialogOutcome::Pending, DialogOutcome::Save)
            } else {
                DialogOutcome::Open(entry.path.clone())
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
                            ui.label(match self.kind {
                                DialogKind::OpenProject => "no project.toml here",
                                DialogKind::ImportAsset => "empty folder",
                                DialogKind::NewProject => "pick the folder the project goes in",
                                _ => "no .scene files here",
                            });
                        }
                    });

                ui.separator();
                ui.horizontal(|ui| {
                    if self.kind.needs_filename() {
                        ui.label(if self.kind == DialogKind::NewProject {
                            "project folder name"
                        } else {
                            "file"
                        });
                        ui.text_edit_singleline(&mut self.filename);
                    } else if self.kind == DialogKind::OpenProject {
                        ui.label("choose the folder that contains project.toml");
                    }
                    if ui.button(confirm).clicked() {
                        outcome = match self.kind {
                            DialogKind::SaveScene => match self.selected_path() {
                                Some(path) => DialogOutcome::Save(path),
                                None => {
                                    self.error = Some("type a file name".to_owned());
                                    DialogOutcome::Pending
                                }
                            },
                            DialogKind::OpenScene | DialogKind::ImportAsset => {
                                match self.selected_path() {
                                    Some(path) if path.exists() => DialogOutcome::Open(path),
                                    Some(path) => {
                                        self.error =
                                            Some(format!("{} does not exist", path.display()));
                                        DialogOutcome::Pending
                                    }
                                    None => {
                                        self.error = Some("choose a file".to_owned());
                                        DialogOutcome::Pending
                                    }
                                }
                            }
                            DialogKind::OpenProject => {
                                match crate::project::Project::open(&self.directory) {
                                    Ok(_) => DialogOutcome::Open(self.directory.clone()),
                                    Err(error) => {
                                        self.error = Some(error);
                                        DialogOutcome::Pending
                                    }
                                }
                            }
                            DialogKind::NewProject => {
                                let name = self.filename.trim().to_owned();
                                if name.is_empty() {
                                    self.error = Some("type a project name".to_owned());
                                    DialogOutcome::Pending
                                } else if self.directory.join(&name).exists() {
                                    self.error = Some(format!(
                                        "{} already exists",
                                        self.directory.join(&name).display()
                                    ));
                                    DialogOutcome::Pending
                                } else {
                                    DialogOutcome::CreateProject {
                                        parent: self.directory.clone(),
                                        name,
                                    }
                                }
                            }
                        };
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
