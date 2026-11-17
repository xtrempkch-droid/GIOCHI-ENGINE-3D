//! What the editor remembers between sessions: the recent list, and where autosaves go.
//!
//! # Where it lives
//!
//! `$XDG_CONFIG_HOME/giochi-editor/settings.toml`, or `~/.config/giochi-editor/settings.toml`
//! on a machine without XDG. `GIOCHI_EDITOR_CONFIG_DIR` overrides the folder, which is what
//! the tests and a portable install use. On GIOCHI-OS the home directory is on the same SD
//! card as the projects, so a config file there costs nothing extra.
//!
//! # Why this is not in the scene
//!
//! Recent files are a property of *this editor on this machine*, not of the scene. A scene
//! that carried a list of other scenes would be wrong the moment it was copied, and the
//! format's `deny_unknown_fields` would make an older editor refuse to open it.
//!
//! # Why the file is written whole
//!
//! It is a hundred bytes. Writing a merge of the old and the new would need to know which
//! side is newer for every field, and there is exactly one writer.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The settings file's name inside the config folder.
pub const SETTINGS_FILE: &str = "settings.toml";
/// How many recent entries are remembered.
pub const MAX_RECENT: usize = 10;
/// Seconds between autosaves, by default.
pub const DEFAULT_AUTOSAVE_SECONDS: u64 = 60;

/// What kind of thing a recent entry points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RecentKind {
    /// A `.scene` file.
    Scene,
    /// A project folder with `project.toml`.
    Project,
}

/// One entry in the recent list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecentEntry {
    /// What it is.
    pub kind: RecentKind,
    /// Where it is. Absolute: a relative path in a settings file means relative to whatever
    /// the editor's working directory happens to be next time.
    pub path: PathBuf,
}

impl RecentEntry {
    /// The last path component, for a menu that does not want a full path per row.
    pub fn label(&self) -> String {
        self.path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.display().to_string())
    }
}

/// The settings file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditorSettings {
    /// Most recent first.
    #[serde(default)]
    pub recent: Vec<RecentEntry>,
    /// Seconds between autosaves. Zero disables them.
    #[serde(default = "default_autosave_seconds")]
    pub autosave_seconds: u64,
}

fn default_autosave_seconds() -> u64 {
    DEFAULT_AUTOSAVE_SECONDS
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            recent: Vec::new(),
            autosave_seconds: DEFAULT_AUTOSAVE_SECONDS,
        }
    }
}

impl EditorSettings {
    /// The folder settings and autosaves live in.
    ///
    /// `GIOCHI_EDITOR_CONFIG_DIR` wins, then `XDG_CONFIG_HOME`, then `~/.config`. When even
    /// `HOME` is unset (a systemd unit with no environment, say) the current directory is
    /// used rather than failing: an editor that refuses to start because it cannot guess a
    /// config path is worse than one that leaves a file behind.
    pub fn config_dir() -> PathBuf {
        if let Some(dir) = std::env::var_os("GIOCHI_EDITOR_CONFIG_DIR") {
            return PathBuf::from(dir);
        }
        if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(dir).join("giochi-editor");
        }
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(".config").join("giochi-editor");
        }
        PathBuf::from(".giochi-editor")
    }

    /// The settings file's full path.
    pub fn default_path() -> PathBuf {
        Self::config_dir().join(SETTINGS_FILE)
    }

    /// Loads from `path`, or returns the defaults when the file is missing.
    ///
    /// A file that exists but does not parse is reported and **replaced by the defaults**
    /// rather than refused: losing the recent list is a nuisance, and refusing to start
    /// because of it is a bug report.
    pub fn load(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref();
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Self::default();
            }
            Err(error) => {
                log::warn!("could not read {}: {error}", path.display());
                return Self::default();
            }
        };
        match toml::from_str(&text) {
            Ok(settings) => settings,
            Err(error) => {
                log::warn!(
                    "{} is not a settings file ({error}); starting with defaults",
                    path.display()
                );
                Self::default()
            }
        }
    }

    /// Writes the file, creating the folder. Written to a temporary file and renamed, so a
    /// crash mid-write cannot leave a half file that the next start refuses.
    pub fn save(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(self)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        let temporary = path.with_extension("toml.tmp");
        std::fs::write(&temporary, text)?;
        std::fs::rename(&temporary, path)?;
        Ok(())
    }

    /// Records `path` as the most recent entry of its kind.
    ///
    /// The path is made absolute first: two entries for the same file, one relative and one
    /// absolute, would look like two files and open one.
    pub fn remember(&mut self, kind: RecentKind, path: impl AsRef<Path>) {
        let path = absolute(path.as_ref());
        self.recent
            .retain(|entry| entry.path != path || entry.kind != kind);
        self.recent.insert(0, RecentEntry { kind, path });
        self.recent.truncate(MAX_RECENT);
    }

    /// Drops every entry. Returns how many were dropped.
    pub fn clear_recent(&mut self) -> usize {
        let count = self.recent.len();
        self.recent.clear();
        count
    }

    /// The entries of one kind, most recent first.
    pub fn recent_of(&self, kind: RecentKind) -> impl Iterator<Item = &RecentEntry> {
        self.recent.iter().filter(move |entry| entry.kind == kind)
    }

    /// The autosave interval, or `None` when autosaving is off.
    pub fn autosave_interval(&self) -> Option<std::time::Duration> {
        (self.autosave_seconds > 0).then(|| std::time::Duration::from_secs(self.autosave_seconds))
    }
}

/// `path` as an absolute path, without touching the filesystem.
///
/// `std::fs::canonicalize` would resolve symlinks and fail on a file that does not exist any
/// more — which is exactly the case a recent list has to survive.
pub fn absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    std::env::current_dir()
        .map(|dir| dir.join(path))
        .unwrap_or_else(|_| path.to_path_buf())
}

/// Where the autosave of `scene_path` goes.
///
/// Next to the scene, as `<name>.autosave`, because that is where the user will look and
/// because it makes the file travel with the project. A scene that has never been saved has
/// no directory to live in, so its autosave goes to `fallback` with the scene's in-memory
/// name (`untitled`).
pub fn autosave_path(scene_path: Option<&Path>, scene_name: &str, fallback: &Path) -> PathBuf {
    match scene_path {
        Some(path) => {
            let mut name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "scene.scene".to_owned());
            name.push_str(".autosave");
            path.with_file_name(name)
        }
        None => fallback.join(format!("{scene_name}.autosave")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "giochi-settings-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        path
    }

    #[test]
    fn a_missing_file_gives_the_defaults() {
        let settings = EditorSettings::load(temp("missing").join("nope.toml"));
        assert!(settings.recent.is_empty());
        assert_eq!(settings.autosave_seconds, DEFAULT_AUTOSAVE_SECONDS);
    }

    #[test]
    fn a_round_trip_keeps_the_list_in_order() {
        let path = temp("roundtrip").join(SETTINGS_FILE);
        let mut settings = EditorSettings::default();
        settings.remember(RecentKind::Scene, "/tmp/a.scene");
        settings.remember(RecentKind::Project, "/tmp/p");
        settings.save(&path).expect("saved");

        let loaded = EditorSettings::load(&path);
        assert_eq!(loaded, settings);
        assert_eq!(loaded.recent[0].kind, RecentKind::Project);
        assert_eq!(loaded.recent[1].kind, RecentKind::Scene);
        let _ = std::fs::remove_dir_all(path.parent().expect("parent"));
    }

    #[test]
    fn remembering_the_same_file_moves_it_to_the_top_instead_of_duplicating() {
        let mut settings = EditorSettings::default();
        settings.remember(RecentKind::Scene, "/tmp/a.scene");
        settings.remember(RecentKind::Scene, "/tmp/b.scene");
        settings.remember(RecentKind::Scene, "/tmp/a.scene");
        assert_eq!(settings.recent.len(), 2);
        assert_eq!(settings.recent[0].path, PathBuf::from("/tmp/a.scene"));
    }

    #[test]
    fn a_scene_and_a_project_at_the_same_path_are_two_entries() {
        // Unlikely but legal: a directory called `x.scene` holding a project.
        let mut settings = EditorSettings::default();
        settings.remember(RecentKind::Scene, "/tmp/x");
        settings.remember(RecentKind::Project, "/tmp/x");
        assert_eq!(settings.recent.len(), 2);
    }

    #[test]
    fn the_list_is_capped() {
        let mut settings = EditorSettings::default();
        for index in 0..MAX_RECENT + 5 {
            settings.remember(RecentKind::Scene, format!("/tmp/{index}.scene"));
        }
        assert_eq!(settings.recent.len(), MAX_RECENT);
        assert_eq!(
            settings.recent[0].path,
            PathBuf::from(format!("/tmp/{}.scene", MAX_RECENT + 4))
        );
    }

    #[test]
    fn relative_paths_are_stored_absolute() {
        let mut settings = EditorSettings::default();
        settings.remember(RecentKind::Scene, "assets/scenes/default.scene");
        assert!(settings.recent[0].path.is_absolute());
        assert!(settings.recent[0]
            .path
            .ends_with("assets/scenes/default.scene"));
    }

    #[test]
    fn clearing_empties_the_list_and_says_how_many_went() {
        let mut settings = EditorSettings::default();
        settings.remember(RecentKind::Scene, "/tmp/a.scene");
        settings.remember(RecentKind::Project, "/tmp/b");
        assert_eq!(settings.clear_recent(), 2);
        assert!(settings.recent.is_empty());
    }

    #[test]
    fn the_two_kinds_are_filtered_apart() {
        let mut settings = EditorSettings::default();
        settings.remember(RecentKind::Scene, "/tmp/a.scene");
        settings.remember(RecentKind::Project, "/tmp/p");
        settings.remember(RecentKind::Scene, "/tmp/b.scene");
        let scenes: Vec<_> = settings
            .recent_of(RecentKind::Scene)
            .map(|entry| entry.label())
            .collect();
        assert_eq!(scenes, ["b.scene", "a.scene"]);
        let projects: Vec<_> = settings.recent_of(RecentKind::Project).collect();
        assert_eq!(projects.len(), 1);
    }

    #[test]
    fn a_zero_interval_disables_autosave() {
        let mut settings = EditorSettings::default();
        assert_eq!(
            settings.autosave_interval(),
            Some(std::time::Duration::from_secs(DEFAULT_AUTOSAVE_SECONDS))
        );
        settings.autosave_seconds = 0;
        assert!(settings.autosave_interval().is_none());
    }

    #[test]
    fn a_corrupt_file_is_reported_and_replaced_by_the_defaults() {
        let path = temp("corrupt").join(SETTINGS_FILE);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
        std::fs::write(&path, "this is not toml = = =").expect("written");
        let settings = EditorSettings::load(&path);
        assert_eq!(settings, EditorSettings::default());
        let _ = std::fs::remove_dir_all(path.parent().expect("parent"));
    }

    #[test]
    fn a_scene_that_was_never_saved_autosaves_into_the_fallback_folder() {
        let fallback = PathBuf::from("/tmp/config/autosave");
        let path = autosave_path(None, "untitled", &fallback);
        assert_eq!(path, fallback.join("untitled.autosave"));
    }

    #[test]
    fn a_saved_scene_autosaves_next_to_itself() {
        let path = autosave_path(
            Some(Path::new("/p/scenes/track.scene")),
            "track",
            Path::new("/cfg"),
        );
        assert_eq!(path, PathBuf::from("/p/scenes/track.scene.autosave"));
    }
}
