//! Projects: a folder that owns `assets/`, `scenes/` and a `project.toml`.
//!
//! # Why a project at all
//!
//! Phase 31 could open any `.scene` anywhere, which is fine for a demo and wrong for
//! making a game: the editor has to know where assets live (so a drag-and-drop can write
//! a relative path the game can resolve), where scenes belong, and which scenes exist.
//! Phase 34 makes that explicit and keeps it in one small file.
//!
//! ```toml
//! name = "my-project"
//! scenes = ["scenes/default.scene"]
//! ```
//!
//! # Relative, always
//!
//! A scene and a material store paths relative to the project root, never absolute ones.
//! A project copied to another machine (or to the console) must still load, and an
//! absolute path from the developer's home directory would break every one of them.
//! [`AssetEntry`] therefore carries both the path on disk and the relative path that goes
//! into the scene.
//!
//! # glTF is recognised but not loadable
//!
//! The filter shows `.gltf`/`.glb` because they are what a project will contain, and
//! because hiding them would make a user think the import failed. Dropping one reports,
//! by name, that the loader belongs to the pending Production Pack (ADR-063) and that
//! `.obj` is what works today. Silently doing nothing would be worse than either.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The file a project directory is identified by.
pub const PROJECT_FILE: &str = "project.toml";
/// Where imported assets go.
pub const ASSETS_DIR: &str = "assets";
/// Where scenes go.
pub const SCENES_DIR: &str = "scenes";

/// What an asset is, which decides its filter bucket and where an import lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AssetKind {
    /// Everything.
    #[default]
    All,
    /// Meshes: `.obj` loads; `.gltf`/`.glb` are recognised (see the module docs).
    Model,
    /// Images.
    Texture,
    /// Audio.
    Sound,
    /// Scene files.
    Scene,
    /// Anything else a project carries.
    Other,
}

impl AssetKind {
    /// The bucket's label, as the filter row shows it.
    pub const fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Model => "Models",
            Self::Texture => "Textures",
            Self::Sound => "Sounds",
            Self::Scene => "Scenes",
            Self::Other => "Other",
        }
    }

    /// The filter buckets, in the order the row shows them.
    pub const FILTERS: [AssetKind; 5] = [
        AssetKind::All,
        AssetKind::Model,
        AssetKind::Texture,
        AssetKind::Sound,
        AssetKind::Scene,
    ];

    /// The subdirectory an asset of this kind is imported into, under `assets/`.
    pub const fn folder(self) -> &'static str {
        match self {
            Self::Model => "models",
            Self::Texture => "textures",
            Self::Sound => "sounds",
            Self::Scene => "scenes",
            Self::All | Self::Other => "misc",
        }
    }

    /// Whether an entry of this kind is shown by `filter`.
    pub fn passes(self, filter: AssetKind) -> bool {
        filter == AssetKind::All || filter == self
    }

    /// The kind of a file, from its extension.
    pub fn of(path: &Path) -> AssetKind {
        let extension = path
            .extension()
            .map(|extension| extension.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        match extension.as_str() {
            "obj" | "gltf" | "glb" => AssetKind::Model,
            "png" | "jpg" | "jpeg" | "bmp" => AssetKind::Texture,
            "wav" | "ogg" | "mp3" | "flac" => AssetKind::Sound,
            "scene" => AssetKind::Scene,
            _ => AssetKind::Other,
        }
    }

    /// Whether this project can load the format today.
    ///
    /// `false` for `.gltf`/`.glb`, which are the one hole the module documents.
    pub fn is_loadable(self, path: &Path) -> bool {
        if self != AssetKind::Model {
            return true;
        }
        path.extension()
            .map(|extension| extension.eq_ignore_ascii_case("obj"))
            .unwrap_or(false)
    }
}

/// One file found under the project's `assets/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetEntry {
    /// Where it is on disk.
    pub path: PathBuf,
    /// The path a scene stores: relative to the project root, with `/` separators.
    pub relative: String,
    /// What the row shows: the path under `assets/`.
    pub label: String,
    /// Its bucket.
    pub kind: AssetKind,
    /// Size in bytes, for the preview line.
    pub size: u64,
}

impl AssetEntry {
    /// Whether the current engine can load it (see [`AssetKind::is_loadable`]).
    pub fn is_loadable(&self) -> bool {
        self.kind.is_loadable(&self.path)
    }
}

/// A project directory and the scenes it owns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectFile {
    /// The project's name.
    pub name: String,
    /// Scene files, relative to the root.
    #[serde(default)]
    pub scenes: Vec<String>,
}

/// An open project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    /// The directory that contains `project.toml`.
    pub root: PathBuf,
    /// Its name, from the file.
    pub name: String,
    /// Scene paths, relative to `root`.
    pub scenes: Vec<String>,
}

impl Project {
    /// Creates the structure under `root` and writes `project.toml`.
    ///
    /// The directory may exist (an empty folder the user just made) but a `project.toml`
    /// already there is an error rather than an overwrite.
    pub fn create(root: impl AsRef<Path>, name: &str) -> Result<Self, String> {
        let root = root.as_ref().to_path_buf();
        let name = name.trim();
        if name.is_empty() {
            return Err("the project needs a name".to_owned());
        }
        let file = root.join(PROJECT_FILE);
        if file.exists() {
            return Err(format!("{} already exists", file.display()));
        }
        std::fs::create_dir_all(root.join(ASSETS_DIR))
            .map_err(|error| format!("{}: {error}", root.display()))?;
        std::fs::create_dir_all(root.join(SCENES_DIR))
            .map_err(|error| format!("{}: {error}", root.display()))?;

        let project = Self {
            root,
            name: name.to_owned(),
            scenes: Vec::new(),
        };
        project.save()?;
        Ok(project)
    }

    /// Opens an existing project directory.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, String> {
        let root = root.as_ref().to_path_buf();
        let file = root.join(PROJECT_FILE);
        let text = std::fs::read_to_string(&file)
            .map_err(|error| format!("{}: {error}", file.display()))?;
        let parsed: ProjectFile = toml::from_str(&text)
            .map_err(|error| format!("{} is not a valid project file: {error}", file.display()))?;
        if parsed.name.trim().is_empty() {
            return Err(format!("{} has no project name", file.display()));
        }
        Ok(Self {
            root,
            name: parsed.name,
            scenes: parsed.scenes,
        })
    }

    /// Writes `project.toml`.
    pub fn save(&self) -> Result<(), String> {
        let file = self.root.join(PROJECT_FILE);
        let parsed = ProjectFile {
            name: self.name.clone(),
            scenes: self.scenes.clone(),
        };
        let text = toml::to_string_pretty(&parsed)
            .map_err(|error| format!("could not serialise the project: {error}"))?;
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("{}: {error}", parent.display()))?;
        }
        std::fs::write(&file, text).map_err(|error| format!("{}: {error}", file.display()))
    }

    /// `<root>/assets`.
    pub fn assets_dir(&self) -> PathBuf {
        self.root.join(ASSETS_DIR)
    }

    /// `<root>/scenes`.
    pub fn scenes_dir(&self) -> PathBuf {
        self.root.join(SCENES_DIR)
    }

    /// The scene paths that exist, as absolute paths.
    pub fn existing_scenes(&self) -> Vec<PathBuf> {
        self.scenes
            .iter()
            .map(|scene| self.root.join(scene))
            .filter(|path| path.is_file())
            .collect()
    }

    /// Adds `path` to the scene list if it is new, and returns whether it changed.
    ///
    /// A path outside the project root is stored as given: the editor does not forbid
    /// editing a scene that lives elsewhere, it just does not claim to own it.
    pub fn register_scene(&mut self, path: &Path) -> bool {
        let relative = self.relative_to_root(path);
        if self.scenes.iter().any(|scene| scene == &relative) {
            return false;
        }
        self.scenes.push(relative);
        true
    }

    /// The path the scene file should store for `path`: relative to the root when it is
    /// inside it, with `/` separators, and otherwise the path as given.
    pub fn relative_to_root(&self, path: &Path) -> String {
        let Ok(relative) = path.strip_prefix(&self.root) else {
            // Outside the root: keep the path as given, with forward slashes so the file
            // is the same on any platform. `components()` would turn the leading `/` into
            // a component and join it back as `//`.
            return path.to_string_lossy().replace('\\', "/");
        };
        relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/")
    }

    /// Copies `source` into the project's asset folder and returns the new entry.
    ///
    /// A name that is already taken becomes `name-1.ext`, `name-2.ext`, … — importing the
    /// same file twice must not overwrite the first copy a scene already points at.
    pub fn import_asset(&self, source: &Path) -> Result<AssetEntry, String> {
        if !source.is_file() {
            return Err(format!("{} is not a file", source.display()));
        }
        let kind = AssetKind::of(source);
        if kind == AssetKind::Other {
            return Err(format!(
                "{}: unsupported asset type (models: obj/gltf/glb, textures: png/jpg, \
                 sounds: wav/ogg, scenes: scene)",
                source.display()
            ));
        }
        let folder = self.assets_dir().join(kind.folder());
        std::fs::create_dir_all(&folder)
            .map_err(|error| format!("{}: {error}", folder.display()))?;

        let file_name = source
            .file_name()
            .ok_or_else(|| format!("{} has no file name", source.display()))?
            .to_string_lossy()
            .into_owned();
        let destination = unique_path(&folder, &file_name);

        std::fs::copy(source, &destination).map_err(|error| {
            format!(
                "could not copy {} to {}: {error}",
                source.display(),
                destination.display()
            )
        })?;

        let (relative, label) = self.describe(&destination);
        let size = std::fs::metadata(&destination)
            .map(|meta| meta.len())
            .unwrap_or(0);
        Ok(AssetEntry {
            path: destination,
            relative,
            label,
            kind,
            size,
        })
    }

    /// The `(relative, label)` pair for a path inside the project.
    ///
    /// `relative` is what a scene stores (`assets/textures/grass.png`); `label` is what a
    /// row shows (`textures/grass.png`).
    pub fn describe(&self, path: &Path) -> (String, String) {
        let relative = self.relative_to_root(path);
        let label = relative
            .strip_prefix(&format!("{ASSETS_DIR}/"))
            .unwrap_or(&relative)
            .to_owned();
        (relative, label)
    }

    /// Lists the project's assets, filtered and sorted directories-last.
    ///
    /// `max_entries` bounds the walk so a project that accidentally contains `node_modules`
    /// cannot freeze the panel. Hidden entries are skipped.
    pub fn scan_assets(&self, filter: AssetKind, max_entries: usize) -> Vec<AssetEntry> {
        let mut entries = Vec::new();
        let assets = self.assets_dir();
        walk(&assets, self, filter, max_entries, &mut entries);
        entries.sort_by(|left, right| left.label.cmp(&right.label));
        entries
    }

    /// Checks the invariants the panel relies on, and reports missing scenes as warnings.
    pub fn validate(&self) -> Result<Vec<String>, String> {
        let mut warnings = Vec::new();
        if !self.assets_dir().is_dir() {
            warnings.push(format!(
                "{} does not exist; importing will create it",
                self.assets_dir().display()
            ));
        }
        if !self.scenes_dir().is_dir() {
            warnings.push(format!(
                "{} does not exist; saving will create it",
                self.scenes_dir().display()
            ));
        }
        for scene in &self.scenes {
            if !self.root.join(scene).is_file() {
                warnings.push(format!("scene {scene} listed in project.toml is missing"));
            }
        }
        let mut seen = BTreeSet::new();
        for scene in &self.scenes {
            if !seen.insert(scene.clone()) {
                warnings.push(format!("scene {scene} is listed twice"));
            }
        }
        Ok(warnings)
    }
}

fn walk(
    directory: &Path,
    project: &Project,
    filter: AssetKind,
    max_entries: usize,
    out: &mut Vec<AssetEntry>,
) {
    if out.len() >= max_entries {
        return;
    }
    let Ok(read) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in read.flatten() {
        if out.len() >= max_entries {
            return;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
        if is_dir {
            walk(&path, project, filter, max_entries, out);
            continue;
        }
        let kind = AssetKind::of(&path);
        if kind == AssetKind::Other || !kind.passes(filter) {
            continue;
        }
        let (relative, label) = project.describe(&path);
        let size = entry.metadata().map(|meta| meta.len()).unwrap_or(0);
        out.push(AssetEntry {
            path,
            relative,
            label,
            kind,
            size,
        });
    }
}

/// `folder/name.ext`, or the first free `folder/name-N.ext`.
fn unique_path(folder: &Path, file_name: &str) -> PathBuf {
    let candidate = folder.join(file_name);
    if !candidate.exists() {
        return candidate;
    }
    let path = Path::new(file_name);
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "asset".to_owned());
    let extension = path
        .extension()
        .map(|extension| extension.to_string_lossy().into_owned());
    for suffix in 1..10_000 {
        let name = match &extension {
            Some(extension) => format!("{stem}-{suffix}.{extension}"),
            None => format!("{stem}-{suffix}"),
        };
        let candidate = folder.join(name);
        if !candidate.exists() {
            return candidate;
        }
    }
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("giochi-project-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        root
    }

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("dir");
        }
        std::fs::write(path, contents).expect("write");
    }

    #[test]
    fn creating_a_project_makes_the_folders_and_the_file() {
        let root = temp_root("create");
        let project = Project::create(&root, "gorilla").expect("created");
        assert_eq!(project.name, "gorilla");
        assert!(root.join(ASSETS_DIR).is_dir());
        assert!(root.join(SCENES_DIR).is_dir());
        assert!(root.join(PROJECT_FILE).is_file());

        let reopened = Project::open(&root).expect("reopened");
        assert_eq!(reopened, project);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_project_without_a_name_is_refused() {
        let root = temp_root("nameless");
        assert!(Project::create(&root, "   ").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn creating_over_an_existing_project_is_refused() {
        let root = temp_root("existing");
        Project::create(&root, "one").expect("first");
        let error = Project::create(&root, "two").expect_err("second");
        assert!(error.contains("already exists"), "{error}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn opening_a_missing_project_names_the_file() {
        let root = temp_root("missing");
        let error = Project::open(&root).expect_err("missing");
        assert!(error.contains(PROJECT_FILE), "{error}");
    }

    #[test]
    fn opening_a_malformed_project_explains_why() {
        let root = temp_root("malformed");
        write(&root.join(PROJECT_FILE), "name = [not a string");
        let error = Project::open(&root).expect_err("malformed");
        assert!(error.contains("not a valid project file"), "{error}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_unknown_field_in_the_project_file_is_refused() {
        let root = temp_root("unknown-field");
        write(&root.join(PROJECT_FILE), "name = \"x\"\nowner = \"me\"\n");
        let error = Project::open(&root).expect_err("unknown field");
        assert!(error.contains("not a valid project file"), "{error}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_round_trip_keeps_the_scene_list() {
        let root = temp_root("roundtrip");
        let mut project = Project::create(&root, "levels").expect("created");
        project.scenes = vec!["scenes/a.scene".to_owned(), "scenes/b.scene".to_owned()];
        project.save().expect("saved");
        let reopened = Project::open(&root).expect("reopened");
        assert_eq!(reopened.scenes, project.scenes);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn registering_a_scene_is_idempotent_and_relative() {
        let root = temp_root("register");
        let mut project = Project::create(&root, "scenes").expect("created");
        let scene = root.join(SCENES_DIR).join("level.scene");
        assert!(project.register_scene(&scene));
        assert_eq!(project.scenes, vec!["scenes/level.scene"]);
        assert!(!project.register_scene(&scene), "second time is a no-op");
        assert_eq!(project.scenes.len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_scene_outside_the_root_keeps_the_path_it_was_given() {
        let root = temp_root("outside");
        let project = Project::create(&root, "outside").expect("created");
        let outside = Path::new("/tmp/elsewhere/level.scene");
        assert_eq!(
            project.relative_to_root(outside),
            "/tmp/elsewhere/level.scene"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn importing_copies_into_the_kind_folder_and_keeps_the_name() {
        let root = temp_root("import");
        let project = Project::create(&root, "import").expect("created");
        let source = temp_root("import-source").join("grass.png");
        write(&source, "not really a png");

        let entry = project.import_asset(&source).expect("imported");
        assert_eq!(entry.kind, AssetKind::Texture);
        assert!(entry
            .path
            .starts_with(project.assets_dir().join("textures")));
        assert_eq!(entry.label, "textures/grass.png");
        assert_eq!(entry.relative, "assets/textures/grass.png");
        assert!(entry.is_loadable());
        assert!(entry.path.is_file());

        let _ = std::fs::remove_file(&source);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn importing_the_same_file_twice_does_not_overwrite_the_first() {
        let root = temp_root("import-twice");
        let project = Project::create(&root, "twice").expect("created");
        let source = temp_root("import-twice-source").join("hit.wav");
        write(&source, "waves");

        let first = project.import_asset(&source).expect("first");
        let second = project.import_asset(&source).expect("second");
        assert_ne!(first.path, second.path);
        assert_eq!(second.label, "sounds/hit-1.wav");
        assert!(first.path.is_file() && second.path.is_file());

        let _ = std::fs::remove_file(&source);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn importing_an_unsupported_file_names_the_supported_kinds() {
        let root = temp_root("import-bad");
        let project = Project::create(&root, "bad").expect("created");
        let source = temp_root("import-bad-source").join("notes.txt");
        write(&source, "text");
        let error = project.import_asset(&source).expect_err("unsupported");
        assert!(error.contains("unsupported asset type"), "{error}");
        assert!(error.contains("obj"), "{error}");
        let _ = std::fs::remove_file(&source);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn scanning_filters_by_kind_and_skips_hidden_files() {
        let root = temp_root("scan");
        let project = Project::create(&root, "scan").expect("created");
        write(&root.join("assets/models/car.obj"), "o car");
        write(&root.join("assets/models/car.gltf"), "{}");
        write(&root.join("assets/textures/floor.png"), "png");
        write(&root.join("assets/sounds/engine.ogg"), "ogg");
        write(&root.join("assets/.hidden.png"), "png");
        write(&root.join("assets/notes.txt"), "text");

        let all = project.scan_assets(AssetKind::All, 100);
        let labels: Vec<&str> = all.iter().map(|entry| entry.label.as_str()).collect();
        assert!(labels.contains(&"models/car.obj"));
        assert!(labels.contains(&"models/car.gltf"));
        assert!(labels.contains(&"textures/floor.png"));
        assert!(labels.contains(&"sounds/engine.ogg"));
        assert!(!labels.contains(&".hidden.png"));
        assert!(!labels.contains(&"notes.txt"), "Other is not an asset");

        let models = project.scan_assets(AssetKind::Model, 100);
        assert_eq!(models.len(), 2);
        assert!(models.iter().all(|entry| entry.kind == AssetKind::Model));

        let textures = project.scan_assets(AssetKind::Texture, 100);
        assert_eq!(textures.len(), 1);
        assert_eq!(textures[0].label, "textures/floor.png");

        let sounds = project.scan_assets(AssetKind::Sound, 100);
        assert_eq!(sounds.len(), 1);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn scanning_respects_the_entry_limit() {
        let root = temp_root("scan-limit");
        let project = Project::create(&root, "limit").expect("created");
        for index in 0..20 {
            write(&root.join(format!("assets/textures/t{index}.png")), "png");
        }
        let entries = project.scan_assets(AssetKind::All, 5);
        assert_eq!(entries.len(), 5);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_gltf_is_recognised_but_reported_as_not_loadable() {
        let gltf = Path::new("assets/models/car.gltf");
        assert_eq!(AssetKind::of(gltf), AssetKind::Model);
        assert!(!AssetKind::Model.is_loadable(gltf));
        assert!(AssetKind::Model.is_loadable(Path::new("assets/models/car.OBJ")));
    }

    #[test]
    fn every_kind_has_a_folder_and_a_label() {
        for kind in AssetKind::FILTERS {
            assert!(!kind.label().is_empty());
        }
        assert_eq!(AssetKind::Model.folder(), "models");
        assert_eq!(AssetKind::Texture.folder(), "textures");
        assert_eq!(AssetKind::Sound.folder(), "sounds");
        assert_eq!(AssetKind::Scene.folder(), "scenes");
    }

    #[test]
    fn the_filter_passes_all_or_the_exact_kind() {
        assert!(AssetKind::Model.passes(AssetKind::All));
        assert!(AssetKind::Model.passes(AssetKind::Model));
        assert!(!AssetKind::Model.passes(AssetKind::Texture));
    }

    #[test]
    fn validate_reports_missing_scenes_and_folders_as_warnings() {
        let root = temp_root("validate");
        let project = Project::create(&root, "validate").expect("created");
        let mut project = project;
        project.scenes = vec![
            "scenes/missing.scene".to_owned(),
            "scenes/missing.scene".to_owned(),
        ];
        let warnings = project.validate().expect("no fatal error");
        assert!(warnings.iter().any(|warning| warning.contains("missing")));
        assert!(warnings.iter().any(|warning| warning.contains("twice")));

        let _ = std::fs::remove_dir_all(&root);
        let empty = Project {
            root: root.clone(),
            name: "gone".to_owned(),
            scenes: Vec::new(),
        };
        let warnings = empty.validate().expect("warnings");
        assert!(warnings.iter().any(|warning| warning.contains(ASSETS_DIR)));
        assert!(warnings.iter().any(|warning| warning.contains(SCENES_DIR)));
    }

    #[test]
    fn existing_scenes_skips_paths_that_are_not_files() {
        let root = temp_root("existing-scenes");
        let mut project = Project::create(&root, "s").expect("created");
        project.scenes = vec!["scenes/a.scene".to_owned(), "scenes/b.scene".to_owned()];
        write(&root.join("scenes/a.scene"), "{}");
        let existing = project.existing_scenes();
        assert_eq!(existing.len(), 1);
        assert!(existing[0].ends_with("a.scene"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
