//! The filesystem watcher: `notify` plus the coalescing that a game loop needs.
//!
//! `notify` reports raw events from its own thread. A game cannot act on them there
//! — SDL and the renderer are single-threaded and not `Send` — so the thread only
//! pushes into a channel and the engine drains it once per frame on the main thread.
//! That split is the whole reason this module exists instead of a direct callback.
//!
//! Between the raw event and the action there is a **debounce window**. Every editor
//! save is more than one filesystem event (write, `chmod`, `rename` over the original),
//! and reloading a shader three times because the editor wrote it three times costs a
//! GL compile each time. Events for the same path inside the window collapse into one,
//! and the window is measured from the **last** event, so a slow save still produces a
//! single reload.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use crate::error::GiochiError;
use crate::hot_reload::{HotReloadEvent, HotReloadKind};

/// How long a path must be quiet before its events are reported.
///
/// 150 ms is the plan's number, and it is a latency/robustness trade-off rather than a
/// measurement: long enough that a save tool's write-then-rename arrives as one event,
/// short enough that the whole cycle (debounce plus the next frame) stays well under
/// the 200 ms the phase promises.
pub const DEFAULT_DEBOUNCE: Duration = Duration::from_millis(150);

/// One filesystem notification as it came from `notify`, before coalescing.
#[derive(Debug, Clone)]
struct RawChange {
    path: PathBuf,
    kind: HotReloadKind,
}

/// A path seen inside the current debounce window.
#[derive(Debug, Clone, Copy)]
struct Pending {
    kind: HotReloadKind,
    /// When the most recent raw event for this path arrived.
    at: Instant,
}

/// Watches files and directories and reports settled changes on the main thread.
///
/// ```no_run
/// use std::path::PathBuf;
/// use giochi_engine::hot_reload::FileWatcher;
///
/// let mut watcher = FileWatcher::new();
/// watcher
///     .watch_files(&[PathBuf::from("shaders/quad.frag")])
///     .expect("inotify must accept the path");
///
/// // Once per frame:
/// for event in watcher.poll() {
///     println!("{:?} {}", event.kind(), event.path().display());
/// }
/// ```
///
/// # Files versus directories
///
/// [`FileWatcher::watch_files`] watches each file's **parent directory** and filters
/// to the names it was given. That is not an optimisation: `inotify` follows the
/// inode, so a watch installed on a file stops reporting the moment the editor saves
/// by writing a temporary file and renaming it over the original — which is what most
/// editors do. Watching the directory survives that.
///
/// [`FileWatcher::watch_dirs`] watches recursively and reports every file underneath,
/// which is what an asset tree wants.
///
/// # C ABI
///
/// Not exposed. The C ABI drives hot reload through `giochi_hot_reload_*`, which acts
/// on the engine's single watcher (ADR-048).
#[repr(C)]
pub struct FileWatcher {
    /// `None` until the first successful watch, and when `notify` refused to start.
    inner: Option<RecommendedWatcher>,
    /// Kept so the backend's callback can clone it.
    sender: Sender<RawChange>,
    receiver: Receiver<RawChange>,
    /// Files named explicitly by [`FileWatcher::watch_files`], used to filter events.
    files: Vec<PathBuf>,
    /// Directories watched recursively. Events under any of them are reported as-is.
    dirs: Vec<PathBuf>,
    /// Events seen but not yet reported, keyed by path.
    pending: HashMap<PathBuf, Pending>,
    debounce: Duration,
    _reserved: [u32; 4],
}

impl std::fmt::Debug for FileWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileWatcher")
            .field("active", &self.inner.is_some())
            .field("files", &self.files.len())
            .field("dirs", &self.dirs.len())
            .field("pending", &self.pending.len())
            .field("debounce", &self.debounce)
            .finish_non_exhaustive()
    }
}

impl Default for FileWatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl FileWatcher {
    /// Creates a watcher with the default debounce. Nothing is watched yet.
    pub fn new() -> Self {
        Self::with_debounce(DEFAULT_DEBOUNCE)
    }

    /// Creates a watcher with an explicit debounce window.
    ///
    /// [`DEFAULT_DEBOUNCE`] is the right answer for a human editing files; a shorter
    /// window is mostly useful for tests, and [`Duration::ZERO`] disables coalescing
    /// in time (each raw event is reported by the next poll).
    pub fn with_debounce(debounce: Duration) -> Self {
        let (sender, receiver) = channel();
        Self {
            inner: None,
            sender,
            receiver,
            files: Vec::new(),
            dirs: Vec::new(),
            pending: HashMap::new(),
            debounce,
            _reserved: [0; 4],
        }
    }

    /// The debounce window in use.
    pub fn debounce(&self) -> Duration {
        self.debounce
    }

    /// Whether a backend was successfully created and is watching something.
    pub fn is_active(&self) -> bool {
        self.inner.is_some()
    }

    /// Number of distinct paths being watched: named files plus directories.
    pub fn watched_count(&self) -> usize {
        self.files.len() + self.dirs.len()
    }

    /// The files named by [`FileWatcher::watch_files`], as absolute canonical paths.
    ///
    /// Absolute because that is what the backend reports: `notify` resolves the path it
    /// watches, and an event whose path were compared against the relative name the
    /// caller passed in would never match. Callers that need to map an event back to
    /// their own relative names should compare canonicalised paths.
    pub fn watched_files(&self) -> Vec<PathBuf> {
        self.files.clone()
    }

    /// Watches each of `paths`, in the sense of "report changes to this file".
    ///
    /// Returns how many *new* paths were added. A parent directory that does not exist
    /// is an error: silently watching nothing is how a hot-reload setup appears to work
    /// while never reloading.
    ///
    /// # Errors
    ///
    /// [`GiochiError::Asset`] when `notify` cannot start or refuses a directory, or
    /// when a path has no parent at all.
    pub fn watch_files(&mut self, paths: &[PathBuf]) -> Result<usize, GiochiError> {
        let mut added = 0usize;
        for path in paths {
            // Absolute before anything else: `notify` resolves the path it watches, so
            // every event comes back absolute. Comparing an absolute event against a
            // relative name here silently matched nothing — a watcher that looked alive
            // and never fired.
            let absolute = absolute_path(path)?;
            if self.files.iter().any(|known| known == &absolute) {
                continue;
            }
            let Some(parent) = absolute.parent() else {
                return Err(GiochiError::Asset(format!(
                    "'{}' has no parent directory to watch",
                    absolute.display()
                )));
            };

            self.ensure_backend()?;
            // A directory already watched recursively already reports this file; a
            // second, non-recursive watch on it would deliver every event twice.
            let already_recursive = self.dirs.iter().any(|dir| parent.starts_with(dir));
            if !already_recursive {
                self.add_watch(parent, RecursiveMode::NonRecursive)?;
            }
            self.files.push(absolute);
            added += 1;
        }
        Ok(added)
    }

    /// Watches each of `paths` recursively, reporting every file underneath.
    ///
    /// Returns how many *new* directories were added.
    ///
    /// # Errors
    ///
    /// [`GiochiError::Asset`] when `notify` cannot start or refuses a directory.
    pub fn watch_dirs(&mut self, paths: &[PathBuf]) -> Result<usize, GiochiError> {
        let mut added = 0usize;
        for path in paths {
            let absolute = std::fs::canonicalize(path).map_err(|err| {
                GiochiError::Asset(format!("cannot watch '{}': {err}", path.display()))
            })?;
            if self.dirs.iter().any(|known| known == &absolute) {
                continue;
            }
            self.ensure_backend()?;
            self.add_watch(&absolute, RecursiveMode::Recursive)?;
            self.dirs.push(absolute);
            added += 1;
        }
        Ok(added)
    }

    /// Stops watching everything. The backend is dropped, so its thread ends too.
    pub fn clear(&mut self) {
        self.inner = None;
        self.files.clear();
        self.dirs.clear();
        self.pending.clear();
    }

    /// Reports the changes that have settled, in path order.
    ///
    /// Call it once per frame. It never blocks: it drains whatever the watch thread has
    /// produced and returns immediately, which is what keeps a hot reload from becoming
    /// a frame-time spike. Events still inside their debounce window stay pending and
    /// are returned by a later call.
    pub fn poll(&mut self) -> Vec<HotReloadEvent> {
        self.collect_pending();
        self.take_settled(Instant::now())
    }

    /// Moves everything the backend produced into `pending`, restarting each window.
    fn collect_pending(&mut self) {
        // `try_recv` stops at the first empty slot, so this cannot spin.
        while let Ok(change) = self.receiver.try_recv() {
            if !self.is_relevant(&change.path) {
                continue;
            }
            let at = Instant::now();
            // `Deleted` followed by `Created` inside one window is the rename dance an
            // editor performs when saving: what the user did is "modify this file".
            let kind = match self.pending.get(&change.path) {
                Some(previous)
                    if matches!(
                        (previous.kind, change.kind),
                        (HotReloadKind::Deleted, HotReloadKind::Created)
                    ) =>
                {
                    HotReloadKind::Modified
                }
                _ => change.kind,
            };
            self.pending.insert(change.path, Pending { kind, at });
        }
    }

    /// Removes and returns the pending events whose window closed by `now`.
    ///
    /// Sorted by path because `HashMap` iteration order is unspecified: a caller
    /// reloads in the returned order, and a frame's work should be reproducible.
    fn take_settled(&mut self, now: Instant) -> Vec<HotReloadEvent> {
        let debounce = self.debounce;
        let mut settled: Vec<PathBuf> = self
            .pending
            .iter()
            .filter(|(_, pending)| now.saturating_duration_since(pending.at) >= debounce)
            .map(|(path, _)| path.clone())
            .collect();
        settled.sort();

        let mut events = Vec::with_capacity(settled.len());
        for path in settled {
            if let Some(pending) = self.pending.remove(&path) {
                events.push(HotReloadEvent::new(path, pending.kind));
            }
        }
        events
    }

    /// Whether `path` is something this watcher was asked about.
    fn is_relevant(&self, path: &Path) -> bool {
        if self.files.iter().any(|watched| watched == path) {
            return true;
        }
        self.dirs.iter().any(|dir| path.starts_with(dir))
    }

    /// Registers `path` with the backend. The backend must exist.
    fn add_watch(&mut self, path: &Path, mode: RecursiveMode) -> Result<(), GiochiError> {
        let Some(watcher) = self.inner.as_mut() else {
            return Err(GiochiError::Asset(
                "file watcher backend is not running".to_owned(),
            ));
        };

        watcher.watch(path, mode).map_err(|err| {
            GiochiError::Asset(format!("failed to watch '{}': {err}", path.display()))
        })?;
        log::debug!("hot reload: watching '{}'", path.display());
        Ok(())
    }

    /// Creates the backend on first use, with a callback that only forwards.
    fn ensure_backend(&mut self) -> Result<(), GiochiError> {
        if self.inner.is_some() {
            return Ok(());
        }

        let sender = self.sender.clone();
        let watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
            match result {
                Ok(event) => {
                    let Some(kind) = classify(&event.kind) else {
                        return;
                    };
                    for path in event.paths {
                        // `send` fails only once the receiver is gone, which means the
                        // engine dropped the watcher. Nothing left to report then.
                        let _ = sender.send(RawChange { path, kind });
                    }
                }
                // A watch can fail after the fact, and `ENOSPC` on the inotify watch
                // limit is the classic failure on ARM64. Log it: the alternative is a
                // reload feature that silently stops working.
                Err(err) => log::warn!("hot reload: watch backend error: {err}"),
            }
        })
        .map_err(|err| GiochiError::Asset(format!("failed to start file watcher: {err}")))?;

        self.inner = Some(watcher);
        log::debug!("hot reload: backend started");
        Ok(())
    }
}

/// Resolves `path` to an absolute, canonical path even when it does not exist yet.
///
/// A `Created` event is about a file that was not there when the watch was installed, so
/// `fs::canonicalize` on the file itself fails; the parent directory is resolved instead
/// and the file name appended.
fn absolute_path(path: &Path) -> Result<PathBuf, GiochiError> {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return Ok(canonical);
    }

    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    let parent = if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    };
    let parent = std::fs::canonicalize(parent).map_err(|err| {
        GiochiError::Asset(format!(
            "cannot resolve the directory of '{}': {err}",
            path.display()
        ))
    })?;

    Ok(match path.file_name() {
        Some(name) => parent.join(name),
        None => parent,
    })
}

/// Maps a `notify` event kind onto the three things the engine can act on.
///
/// Access events are dropped: reading a file is not a change, and a watcher that
/// reloaded on read would reload its own reload.
fn classify(kind: &notify::EventKind) -> Option<HotReloadKind> {
    use notify::event::{CreateKind, ModifyKind, RemoveKind};

    match kind {
        notify::EventKind::Create(CreateKind::File | CreateKind::Any | CreateKind::Other) => {
            Some(HotReloadKind::Created)
        }
        notify::EventKind::Modify(
            ModifyKind::Data(_)
            | ModifyKind::Metadata(_)
            | ModifyKind::Name(_)
            | ModifyKind::Any
            | ModifyKind::Other,
        ) => Some(HotReloadKind::Modified),
        notify::EventKind::Remove(RemoveKind::File | RemoveKind::Any | RemoveKind::Other) => {
            Some(HotReloadKind::Deleted)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_watcher_watches_nothing() {
        let watcher = FileWatcher::new();
        assert!(!watcher.is_active());
        assert_eq!(watcher.watched_count(), 0);
        assert_eq!(watcher.debounce(), DEFAULT_DEBOUNCE);
    }

    #[test]
    fn poll_without_a_backend_returns_nothing() {
        let mut watcher = FileWatcher::new();
        assert!(watcher.poll().is_empty());
    }

    #[test]
    fn classify_ignores_access_events_and_the_catch_all() {
        use notify::event::AccessKind;

        assert_eq!(classify(&notify::EventKind::Access(AccessKind::Read)), None);
        assert_eq!(classify(&notify::EventKind::Any), None);
        assert_eq!(classify(&notify::EventKind::Other), None);
    }

    #[test]
    fn classify_maps_the_three_actions() {
        use notify::event::{CreateKind, DataChange, ModifyKind, RemoveKind};

        assert_eq!(
            classify(&notify::EventKind::Create(CreateKind::File)),
            Some(HotReloadKind::Created)
        );
        assert_eq!(
            classify(&notify::EventKind::Modify(ModifyKind::Data(
                DataChange::Content
            ))),
            Some(HotReloadKind::Modified)
        );
        assert_eq!(
            classify(&notify::EventKind::Remove(RemoveKind::File)),
            Some(HotReloadKind::Deleted)
        );
    }

    #[test]
    fn a_named_file_is_relevant_only_exactly() {
        let mut watcher = FileWatcher::new();
        watcher.files.push(PathBuf::from("assets/player.png"));
        assert!(watcher.is_relevant(Path::new("assets/player.png")));
        assert!(!watcher.is_relevant(Path::new("assets/player.bmp")));
    }

    #[test]
    fn a_watched_directory_accepts_any_descendant() {
        let mut watcher = FileWatcher::new();
        watcher.dirs.push(PathBuf::from("assets"));
        assert!(watcher.is_relevant(Path::new("assets/player.png")));
        assert!(watcher.is_relevant(Path::new("assets/ui/font.png")));
        assert!(!watcher.is_relevant(Path::new("src/player.png")));
    }

    #[test]
    fn an_event_is_not_reported_before_its_window_closes() {
        let mut watcher = FileWatcher::with_debounce(DEFAULT_DEBOUNCE);
        let at = Instant::now();
        watcher.pending.insert(
            "a.frag".into(),
            Pending {
                kind: HotReloadKind::Modified,
                at,
            },
        );

        assert!(watcher.take_settled(at).is_empty());
        assert_eq!(
            watcher.pending.len(),
            1,
            "it must stay pending, not be lost"
        );
    }

    #[test]
    fn an_event_is_reported_once_its_window_closes() {
        let mut watcher = FileWatcher::with_debounce(DEFAULT_DEBOUNCE);
        let at = Instant::now();
        watcher.pending.insert(
            "a.frag".into(),
            Pending {
                kind: HotReloadKind::Modified,
                at,
            },
        );

        let settled = watcher.take_settled(at + DEFAULT_DEBOUNCE);
        assert_eq!(settled.len(), 1);
        assert_eq!(settled[0].path(), Path::new("a.frag"));
        assert_eq!(settled[0].kind(), HotReloadKind::Modified);
        assert!(watcher.pending.is_empty());
    }

    #[test]
    fn a_zero_debounce_reports_immediately() {
        let mut watcher = FileWatcher::with_debounce(Duration::ZERO);
        let at = Instant::now();
        watcher.pending.insert(
            "a.png".into(),
            Pending {
                kind: HotReloadKind::Created,
                at,
            },
        );
        assert_eq!(watcher.take_settled(at).len(), 1);
    }

    #[test]
    fn settled_events_come_out_in_path_order() {
        let mut watcher = FileWatcher::with_debounce(DEFAULT_DEBOUNCE);
        let at = Instant::now();
        for name in ["c.png", "a.png", "b.png"] {
            watcher.pending.insert(
                name.into(),
                Pending {
                    kind: HotReloadKind::Modified,
                    at,
                },
            );
        }

        let settled = watcher.take_settled(at + DEFAULT_DEBOUNCE);
        let names: Vec<&Path> = settled.iter().map(HotReloadEvent::path).collect();
        assert_eq!(
            names,
            vec![Path::new("a.png"), Path::new("b.png"), Path::new("c.png")]
        );
    }

    #[test]
    fn clear_forgets_the_watched_paths_and_the_backend() {
        let mut watcher = FileWatcher::new();
        watcher.files.push("a.png".into());
        watcher.dirs.push("assets".into());
        watcher.clear();
        assert!(!watcher.is_active());
        assert_eq!(watcher.watched_count(), 0);
    }

    #[test]
    fn watching_a_real_file_reports_a_write() {
        use std::io::Write;

        // A real inotify round trip: the tests above cover the coalescing logic, and
        // this one proves the backend is wired to the channel.
        let dir = std::env::temp_dir().join(format!(
            "giochi-hot-reload-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let file = dir.join("probe.txt");
        std::fs::write(&file, b"one").expect("seed file");

        let mut watcher = FileWatcher::with_debounce(Duration::from_millis(20));
        watcher
            .watch_files(std::slice::from_ref(&file))
            .expect("watch");
        assert!(watcher.is_active());

        let mut handle = std::fs::OpenOptions::new()
            .append(true)
            .open(&file)
            .expect("open");
        handle.write_all(b"two").expect("write");
        handle.flush().expect("flush");
        drop(handle);

        // Give the backend a moment, then poll until the debounce closes or 2 s pass.
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut seen = Vec::new();
        while Instant::now() < deadline {
            seen.extend(watcher.poll());
            if !seen.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }

        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            seen.iter().any(|event| event.path() == file),
            "a write to the watched file must be reported, got {seen:?}"
        );
    }

    #[test]
    fn a_relative_directory_is_watched_as_an_absolute_one() {
        use std::io::Write;

        // The regression this exists for: `notify` resolves the path it watches, so the
        // events arrive absolute. Filtering them against the relative name the caller
        // passed meant the watcher started, reported nothing, and looked healthy.
        let dir = PathBuf::from("target/hot-reload-relative-probe");
        std::fs::create_dir_all(&dir).expect("temp dir under the crate");
        let file = dir.join("probe.txt");
        std::fs::write(&file, b"a").expect("seed file");

        let mut watcher = FileWatcher::with_debounce(Duration::from_millis(10));
        watcher
            .watch_dirs(std::slice::from_ref(&dir))
            .expect("watch");

        // The watcher stores what it will compare against: absolute, canonical.
        let watched = watcher.watched_files();
        let _ = watched; // files are only tracked by `watch_files`
        let stored = watcher.dirs.clone();
        assert_eq!(stored.len(), 1);
        assert!(
            stored[0].is_absolute(),
            "the stored directory must be absolute, got {:?}",
            stored[0]
        );

        let mut handle = std::fs::OpenOptions::new()
            .append(true)
            .open(&file)
            .expect("open");
        handle.write_all(b"b").expect("write");
        handle.flush().expect("flush");
        drop(handle);

        let deadline = Instant::now() + Duration::from_secs(2);
        let mut seen = Vec::new();
        while Instant::now() < deadline && seen.is_empty() {
            seen.extend(watcher.poll());
            std::thread::sleep(Duration::from_millis(20));
        }

        let expected = std::fs::canonicalize(&file).expect("canonical");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            seen.iter().any(|event| event.path() == expected),
            "a relative watch must still report an absolute event, got {seen:?}"
        );
    }
}
