//! Autosave and crash recovery.
//!
//! # What autosave is for, and what it is not
//!
//! It is not a substitute for `Ctrl+S`: the file it writes is a **separate** file with a
//! `.autosave` suffix, so a bad autosave can never overwrite the scene the user has been
//! saving deliberately. It exists for the one case that loses work — a crash, a power cut on
//! a handheld, a `SIGKILL` from an OOM killer — and for nothing else.
//!
//! # When it fires
//!
//! Every `interval` seconds, and only when the scene is **dirty**. Writing an unchanged scene
//! every minute would put the SD card through writes for no benefit, and on GIOCHI-OS the
//! project lives on an SD card.
//!
//! The clock is passed in (`tick(now)`) rather than read inside, so the tests can advance
//! time without sleeping. A test that sleeps for a minute per case is a test nobody runs.
//!
//! # Recovery
//!
//! The autosave sits next to its scene. On open, [`find`] returns it only when it is newer
//! than the scene file — an autosave older than the file it was made from describes work
//! that has already been saved, and offering it would invite the user to throw away newer
//! data. Recovery never happens automatically: the editor says the file is there and the
//! user chooses `File > Recover autosave`.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use giochi_engine_3d::scene::Scene;

use crate::settings::{autosave_path, EditorSettings};

/// The saved-autosave timer.
#[derive(Debug, Clone, Default)]
pub struct Autosave {
    interval: Option<Duration>,
    last: Option<Instant>,
    /// Where the most recent autosave went, for `File > Recover autosave`.
    last_path: Option<PathBuf>,
}

impl Autosave {
    /// A timer that fires every `interval`, or never when it is `None`.
    pub fn new(interval: Option<Duration>) -> Self {
        Self {
            interval,
            last: None,
            last_path: None,
        }
    }

    /// Restarts the timer, for a fresh scene or an interval change.
    pub fn reset(&mut self, interval: Option<Duration>) {
        self.interval = interval;
        self.last = None;
        self.last_path = None;
    }

    /// Whether an autosave is due at `now`, restarting the timer when it is.
    pub fn due(&mut self, now: Instant) -> bool {
        let Some(interval) = self.interval else {
            return false;
        };
        match self.last {
            Some(last) if now.saturating_duration_since(last) < interval => false,
            Some(_) => {
                self.last = Some(now);
                true
            }
            None => {
                // The first check starts the clock rather than firing immediately: opening a
                // scene and getting an autosave before touching anything is noise.
                self.last = Some(now);
                false
            }
        }
    }

    /// Records where an autosave just went.
    pub fn note(&mut self, path: PathBuf) {
        self.last_path = Some(path);
    }

    /// Where the last autosave was written.
    pub fn last_path(&self) -> Option<&Path> {
        self.last_path.as_deref()
    }

    /// The interval, for the status line.
    pub const fn interval(&self) -> Option<Duration> {
        self.interval
    }
}

/// Writes `scene` to its autosave path, returning where it went.
///
/// # Errors
///
/// The scene format's error, or the filesystem's. A failed autosave is logged and reported,
/// never fatal: losing the safety net must not lose the session.
pub fn write(
    scene: &Scene,
    scene_path: Option<&Path>,
    config_dir: &Path,
) -> Result<PathBuf, String> {
    let path = autosave_path(scene_path, &scene.name, &config_dir.join("autosave"));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    scene
        .save_to_path(&path)
        .map_err(|error| format!("{error:?}"))?;
    Ok(path)
}

/// The autosave worth offering for `scene_path`, if any.
///
/// Only newer-than-the-scene counts, and only when the scene has changes to lose. A scene
/// that was never saved has no reference point, so its autosave always counts.
pub fn find(scene_path: Option<&Path>, scene_name: &str, config_dir: &Path) -> Option<PathBuf> {
    let path = autosave_path(scene_path, scene_name, &config_dir.join("autosave"));
    let autosaved = std::fs::metadata(&path).ok()?;
    if autosaved.len() == 0 {
        return None;
    }
    let Some(scene_path) = scene_path else {
        return Some(path);
    };
    let scene = std::fs::metadata(scene_path).ok()?;
    let autosaved_at = autosaved.modified().ok()?;
    let scene_at = scene.modified().ok()?;
    (autosaved_at > scene_at).then_some(path)
}

/// Loads an autosave into a scene.
///
/// # Errors
///
/// The scene format's error: an autosave is a scene file, and a corrupt one is reported
/// rather than silently ignored.
pub fn recover(path: impl AsRef<Path>) -> Result<Scene, String> {
    Scene::load_from_path(path).map_err(|error| format!("{error:?}"))
}

/// The folder autosaves of never-saved scenes go to.
pub fn folder() -> PathBuf {
    EditorSettings::config_dir().join("autosave")
}

#[cfg(test)]
mod tests {
    use super::*;
    use giochi_engine_3d::scene::SceneNode;

    fn config() -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "giochi-autosave-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        path
    }

    #[test]
    fn the_timer_starts_the_clock_instead_of_firing_at_once() {
        let mut autosave = Autosave::new(Some(Duration::from_secs(60)));
        let start = Instant::now();
        assert!(!autosave.due(start));
        assert!(!autosave.due(start + Duration::from_secs(30)));
        assert!(autosave.due(start + Duration::from_secs(61)));
        assert!(!autosave.due(start + Duration::from_secs(62)));
        assert!(autosave.due(start + Duration::from_secs(122)));
    }

    #[test]
    fn no_interval_never_fires() {
        let mut autosave = Autosave::new(None);
        let start = Instant::now();
        assert!(!autosave.due(start));
        assert!(!autosave.due(start + Duration::from_secs(3600)));
    }

    #[test]
    fn resetting_the_timer_forgets_the_last_written_file() {
        let mut autosave = Autosave::new(Some(Duration::from_secs(1)));
        autosave.last_path = Some(PathBuf::from("/tmp/x.autosave"));
        autosave.reset(None);
        assert!(autosave.last_path().is_none());
        assert!(autosave.interval().is_none());
    }

    #[test]
    fn writing_an_autosave_produces_a_scene_that_loads() {
        let dir = config();
        let mut scene = Scene::new("track");
        scene.add_node(SceneNode::new(
            giochi_engine_3d::scene::SceneNodeId::new(0),
            "Cube",
        ));
        let path = write(&scene, None, &dir).expect("written");
        assert!(path.starts_with(dir.join("autosave")));
        let loaded = recover(&path).expect("recovered");
        assert_eq!(loaded.nodes.len(), 1);
        assert_eq!(loaded.name, "track");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_autosave_next_to_its_scene_is_found_when_it_is_newer() {
        let dir = config();
        std::fs::create_dir_all(&dir).expect("dir");
        let scene_path = dir.join("scene.scene");
        let mut scene = Scene::new("scene");
        scene.add_node(SceneNode::new(
            giochi_engine_3d::scene::SceneNodeId::new(0),
            "A",
        ));
        scene.save_to_path(&scene_path).expect("saved");
        // An autosave only counts when it is newer than the scene.
        std::thread::sleep(Duration::from_millis(20));
        let autosaved = write(&scene, Some(&scene_path), &dir).expect("autosaved");
        assert_eq!(autosaved, dir.join("scene.scene.autosave"));
        assert_eq!(
            find(Some(&scene_path), "scene", &dir),
            Some(autosaved.clone())
        );

        // Save the scene again afterwards: the autosave is now the older file.
        std::thread::sleep(Duration::from_millis(20));
        scene.save_to_path(&scene_path).expect("saved again");
        assert!(find(Some(&scene_path), "scene", &dir).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_scene_that_was_never_saved_offers_its_autosave() {
        let dir = config();
        let mut scene = Scene::new("untitled");
        scene.add_node(SceneNode::new(
            giochi_engine_3d::scene::SceneNodeId::new(0),
            "A",
        ));
        let path = write(&scene, None, &dir).expect("autosaved");
        assert_eq!(find(None, "untitled", &dir), Some(path));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_autosave_means_nothing_to_offer() {
        let dir = config();
        assert!(find(None, "untitled", &dir).is_none());
        assert!(find(Some(Path::new("/nothing/here.scene")), "here", &dir).is_none());
    }

    #[test]
    fn a_corrupt_autosave_is_reported_as_an_error() {
        let dir = config();
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("bad.autosave");
        std::fs::write(&path, "{ not json }").expect("written");
        assert!(recover(&path).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
