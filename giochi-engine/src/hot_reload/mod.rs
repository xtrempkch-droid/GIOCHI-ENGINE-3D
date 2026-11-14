//! Hot reload: edit a file, see the change in the running game.
//!
//! # What this module is
//!
//! Three separable pieces, each usable on its own:
//!
//! | Type | Job |
//! |---|---|
//! | [`FileWatcher`] | turns filesystem notifications into a per-frame `poll()` on the main thread |
//! | [`AssetReloadRegistry`] | decides which extensions reload, and in what order |
//! | [`HotReloader`] | the two together: watch, debounce, filter, order |
//!
//! The engine wires all three into its frame loop when [`Engine::enable_hot_reload`]
//! is called and the `hot_reload` feature is on. Everything else in the engine is
//! untouched by the feature: the module does not exist without it, so the release
//! binary is bit-for-bit the one from phase 18.
//!
//! [`Engine::enable_hot_reload`]: crate::Engine::enable_hot_reload
//!
//! # Why the reload happens on the main thread
//!
//! `notify` calls back from its own thread. SDL's canvas, the mixer and the texture
//! registry are neither `Send` nor `Sync` — the engine is single-threaded by design
//! (see the crate docs). So the watch thread does the **only** thing that is safe
//! from another thread: it pushes the path into a channel. [`HotReloader::poll`]
//! drains that channel inside the frame, where `&mut RenderContext` is available.
//!
//! A design that called `render.reload_texture()` from the watch callback would need
//! `unsafe impl Send` on half the engine to compile, and would still be a data race.
//!
//! # Turning it off
//!
//! Three independent switches, in order of precedence:
//!
//! 1. [`HotReloadConfig::enabled`] — what the program asked for;
//! 2. `GIOCHI_HOT_RELOAD=0` (or `false`/`off`/`no`) — what the machine says;
//! 3. `cfg!(debug_assertions)` — the default when neither says anything.
//!
//! The default matters: a debug build reloads, a release build does not, and a
//! release build *with* the feature compiled in still does not unless it is asked to.
//!
//! # What reloads
//!
//! Textures (`.png`, `.bmp`, `.jpg`), shaders and meshes in `giochi-engine-3d`, and
//! Lua scripts through `giochi-engine-lua`. What does **not** reload is listed in
//! `docs/HOT_RELOAD.md` — a texture whose size changed, a mesh whose vertex count
//! changed, and anything the game copied out of an asset at load time.
//!
//! # C ABI
//!
//! Phase 19 exposes nothing to C (R3). `giochi_hot_reload_enable`/`disable` arrive in
//! phase 24 with ABI v4, and they drive the same [`HotReloader`] the Rust API uses.

pub mod registry;
pub mod watcher;

use std::path::{Path, PathBuf};

use crate::error::GiochiError;

pub use registry::AssetReloadRegistry;
pub use watcher::{FileWatcher, DEFAULT_DEBOUNCE};

/// Environment variable that overrides the hot-reload default.
///
/// `GIOCHI_HOT_RELOAD=0`, `false`, `off` or `no` disables it; `1`, `true`, `on` or
/// `yes` enables it. Anything else — including an empty value — keeps the default.
pub const HOT_RELOAD_ENV: &str = "GIOCHI_HOT_RELOAD";

/// What happened to a file.
///
/// A rename over the original is reported as [`HotReloadKind::Modified`], because that
/// is what an editor's "save" does and what the game should react to.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum HotReloadKind {
    /// The file did not exist and now does.
    #[default]
    Created = 0,
    /// The file's contents or metadata changed, including a rename over it.
    Modified = 1,
    /// The file is gone. A texture reload for a deleted file fails and keeps the old one.
    Deleted = 2,
}

/// One settled change to one watched file.
///
/// Created by [`FileWatcher::poll`]; a game that calls [`HotReloader::poll`] gets the
/// same value, already filtered and ordered by the registry.
#[repr(C)]
#[derive(Clone, PartialEq, Eq)]
pub struct HotReloadEvent {
    path: PathBuf,
    kind: HotReloadKind,
    _reserved: [u32; 4],
}

impl HotReloadEvent {
    /// Creates an event. Public because a game replaying or testing a reload needs to
    /// build one without a filesystem.
    pub fn new(path: PathBuf, kind: HotReloadKind) -> Self {
        Self {
            path,
            kind,
            _reserved: [0; 4],
        }
    }

    /// The file that changed, as it was watched (relative paths stay relative).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// What happened to it.
    pub fn kind(&self) -> HotReloadKind {
        self.kind
    }

    /// The lower-cased extension without the dot, if the path has one.
    pub fn extension(&self) -> Option<&str> {
        self.path.extension().and_then(|ext| ext.to_str())
    }

    /// The final path component, if it is valid UTF-8.
    pub fn file_name(&self) -> Option<&str> {
        self.path.file_name().and_then(|name| name.to_str())
    }

    /// Whether the file was removed.
    pub fn is_deleted(&self) -> bool {
        self.kind == HotReloadKind::Deleted
    }

    /// `true` for every reserved word. The engine checks this on receive so a caller
    /// that zeroed the tail cannot be surprised later; see `docs/C_ABI_CONTRACT.md`.
    pub fn reserved_is_zero(&self) -> bool {
        self._reserved == [0; 4]
    }
}

impl std::fmt::Debug for HotReloadEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HotReloadEvent")
            .field("path", &self.path)
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

/// Configuration for [`HotReloader`].
///
/// # C ABI
///
/// ```c
/// typedef struct GiochiHotReloadConfig {
///     uint32_t enabled;          /* 0 = off, non-zero = on */
///     uint32_t debounce_ms;      /* 0 keeps the default (150) */
///     uint32_t _reserved[4];
/// } GiochiHotReloadConfig;
/// ```
///
/// Size: 24 bytes, alignment: 4.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HotReloadConfig {
    /// Whether to watch anything at all. Defaults to [`Self::default_enabled`].
    pub enabled: bool,
    /// Debounce window in milliseconds; `0` means [`DEFAULT_DEBOUNCE`].
    pub debounce_ms: u32,
    /// Reserved for future ABI-compatible fields. Must be zero.
    pub _reserved: [u32; 4],
}

impl HotReloadConfig {
    /// What a build with no configuration does: on in debug, off in release.
    pub const fn default_enabled() -> bool {
        cfg!(debug_assertions)
    }

    /// A configuration with the given switch and the default debounce.
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            debounce_ms: 0,
            _reserved: [0; 4],
        }
    }

    /// A configuration that follows [`Self::default_enabled`].
    pub fn from_default() -> Self {
        Self::new(Self::default_enabled())
    }

    /// A configuration read from [`HOT_RELOAD_ENV`], falling back to the default.
    ///
    /// A value that is neither an affirmative nor a negative word is ignored with a
    /// warning: `GIOCHI_HOT_RELOAD=maybe` must not silently turn a debug build's hot
    /// reload off.
    pub fn from_env() -> Self {
        match std::env::var(HOT_RELOAD_ENV) {
            Ok(value) => match parse_switch(&value) {
                Some(enabled) => Self::new(enabled),
                None => {
                    log::warn!(
                        "{HOT_RELOAD_ENV}={value:?} is neither on nor off; \
                         keeping the default ({})",
                        Self::default_enabled()
                    );
                    Self::from_default()
                }
            },
            Err(_) => Self::from_default(),
        }
    }

    /// The debounce window this configuration asks for.
    pub fn debounce(&self) -> std::time::Duration {
        if self.debounce_ms == 0 {
            DEFAULT_DEBOUNCE
        } else {
            std::time::Duration::from_millis(u64::from(self.debounce_ms))
        }
    }
}

impl Default for HotReloadConfig {
    fn default() -> Self {
        Self::from_default()
    }
}

/// Counters for the overlay and the devtools console (phase 20).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HotReloadStats {
    /// Events reported by [`HotReloader::poll`] so far.
    pub events: u64,
    /// Events the engine applied successfully.
    pub applied: u64,
    /// Events the engine tried and could not apply (parse error, missing file, …).
    pub failed: u64,
    /// Events dropped because no extension rule matched them.
    pub ignored: u64,
    /// Reserved for future ABI-compatible fields. Must be zero.
    pub _reserved: [u32; 4],
}

/// Watches files, and reports the settled changes that are worth reloading.
///
/// The engine owns one when hot reload is enabled; a host that drives a
/// [`RenderContext`](crate::render::RenderContext) itself can own one too:
///
/// ```no_run
/// use std::path::PathBuf;
/// use giochi_engine::hot_reload::{HotReloadConfig, HotReloader};
///
/// # fn example() -> Result<(), giochi_engine::GiochiError> {
/// let mut reloader = HotReloader::new(HotReloadConfig::new(true));
/// reloader.watch_dirs(&[PathBuf::from("assets")])?;
///
/// // Once per frame, before drawing:
/// for event in reloader.poll() {
///     println!("reload {}", event.path().display());
/// }
/// # Ok(())
/// # }
/// ```
///
/// # C ABI
///
/// Not exposed until phase 24 (`giochi_hot_reload_enable`/`_disable`/`_poll`).
#[repr(C)]
pub struct HotReloader {
    watcher: FileWatcher,
    registry: AssetReloadRegistry,
    stats: HotReloadStats,
    enabled: bool,
    _reserved: [u32; 4],
}

impl std::fmt::Debug for HotReloader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HotReloader")
            .field("enabled", &self.enabled)
            .field("watched", &self.watcher.watched_count())
            .field("stats", &self.stats)
            .finish_non_exhaustive()
    }
}

impl HotReloader {
    /// Creates a reloader. Nothing is watched until [`Self::watch_files`] or
    /// [`Self::watch_dirs`] is called.
    pub fn new(config: HotReloadConfig) -> Self {
        Self {
            watcher: FileWatcher::with_debounce(config.debounce()),
            registry: AssetReloadRegistry::with_defaults(),
            stats: HotReloadStats::default(),
            // A disabled reloader keeps the object but never installs a backend, so a
            // release build with the feature compiled in costs one `bool` check per
            // frame and no `inotify` descriptors.
            enabled: config.enabled,
            _reserved: [0; 4],
        }
    }

    /// Whether polling actually reports anything.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Enables or disables reporting without recreating the object.
    ///
    /// Disabling stops the backend and forgets the watched paths; enabling again needs
    /// a new [`Self::watch_files`]/[`Self::watch_dirs`] call, because "which files does
    /// this game care about" is not something the reloader can guess.
    pub fn set_enabled(&mut self, enabled: bool) {
        if self.enabled == enabled {
            return;
        }
        self.enabled = enabled;
        if !enabled {
            self.watcher.clear();
        }
        log::info!(
            "hot reload {}",
            if enabled { "enabled" } else { "disabled" }
        );
    }

    /// The extension table, for inspection and for registering a game's own formats.
    pub fn registry(&self) -> &AssetReloadRegistry {
        &self.registry
    }

    /// The extension table, mutably.
    pub fn registry_mut(&mut self) -> &mut AssetReloadRegistry {
        &mut self.registry
    }

    /// Counters since construction.
    pub fn stats(&self) -> HotReloadStats {
        self.stats
    }

    /// Records that the engine applied `count` events.
    pub fn note_applied(&mut self, count: u64) {
        self.stats.applied = self.stats.applied.saturating_add(count);
    }

    /// Records that `count` events could not be applied.
    pub fn note_failed(&mut self, count: u64) {
        self.stats.failed = self.stats.failed.saturating_add(count);
    }

    /// Watches the named files. Returns how many were newly added.
    ///
    /// # Errors
    ///
    /// [`GiochiError::Asset`] when the backend cannot be started. A disabled reloader
    /// accepts the call and watches nothing, so enabling later does not need the caller
    /// to branch on a flag.
    pub fn watch_files(&mut self, paths: &[PathBuf]) -> Result<usize, GiochiError> {
        if !self.enabled {
            return Ok(0);
        }
        let added = self.watcher.watch_files(paths)?;
        if added > 0 {
            log::info!("hot reload: watching {added} file(s)");
        }
        Ok(added)
    }

    /// Watches the named directories recursively. Returns how many were newly added.
    ///
    /// # Errors
    ///
    /// [`GiochiError::Asset`] when the backend cannot be started or a directory does
    /// not exist.
    pub fn watch_dirs(&mut self, paths: &[PathBuf]) -> Result<usize, GiochiError> {
        if !self.enabled {
            return Ok(0);
        }
        let added = self.watcher.watch_dirs(paths)?;
        if added > 0 {
            log::info!("hot reload: watching {added} directory(ies)");
        }
        Ok(added)
    }

    /// Number of watched paths (files plus directories).
    pub fn watched_count(&self) -> usize {
        self.watcher.watched_count()
    }

    /// Whether a watch backend is actually running.
    pub fn is_watching(&self) -> bool {
        self.watcher.is_active()
    }

    /// The changes that settled since the last call, filtered and ordered.
    ///
    /// Returns nothing when disabled, which is the whole cost of having the feature
    /// compiled in and switched off.
    pub fn poll(&mut self) -> Vec<HotReloadEvent> {
        if !self.enabled {
            return Vec::new();
        }

        let raw = self.watcher.poll();
        let before = raw.len();
        let ordered = self.registry.order(raw);
        let ignored = before.saturating_sub(ordered.len()) as u64;
        self.stats.ignored = self.stats.ignored.saturating_add(ignored);
        self.stats.events = self.stats.events.saturating_add(ordered.len() as u64);

        if !ordered.is_empty() {
            log::debug!(
                "hot reload: {} settled change(s), {} ignored",
                ordered.len(),
                ignored
            );
        }
        ordered
    }

    /// Stops watching everything and forgets the paths.
    pub fn clear(&mut self) {
        self.watcher.clear();
    }
}

/// Parses an environment switch. `None` means "not a recognisable value".
fn parse_switch(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "on" | "yes" => Some(true),
        "0" | "false" | "off" | "no" => Some(false),
        _ => None,
    }
}

/// Process-wide counter, bumped every time a reloadable script changes.
///
/// A Lua state cannot be patched in place: the script is the program, not a resource it
/// manipulates. The honest behaviour is to restart the VM with the edited script, and a
/// VM restart is something the *host* must do — it owns the state. A counter is the
/// smallest thing that crosses that boundary: the engine bumps it when a `.lua` file
/// settles (it already has the watcher), and `giochi-engine-lua` quits the loop when the
/// number it started with no longer matches, so the host can reload.
///
/// The engine has no Lua dependency, so this lives here rather than in the binding.
/// Without the `hot_reload` feature the epoch is constant zero and nothing restarts.
static SCRIPT_EPOCH: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// The current script generation. `0` until a script reload happens.
pub fn script_epoch() -> u32 {
    SCRIPT_EPOCH.load(std::sync::atomic::Ordering::Relaxed)
}

/// Advances the script generation and returns the new value.
///
/// Called by the engine when a watched `.lua` file settles; exposed so a host that
/// reloads scripts its own way (an embedded interpreter, a compiled game script) can
/// use the same signal.
pub fn bump_script_epoch() -> u32 {
    SCRIPT_EPOCH
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        .wrapping_add(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_accessors_report_the_path_parts() {
        let event = HotReloadEvent::new(
            PathBuf::from("assets/ui/player.PNG"),
            HotReloadKind::Created,
        );
        assert_eq!(event.path(), Path::new("assets/ui/player.PNG"));
        assert_eq!(event.extension(), Some("PNG"));
        assert_eq!(event.file_name(), Some("player.PNG"));
        assert!(!event.is_deleted());
        assert!(event.reserved_is_zero());
        assert_eq!(event.kind(), HotReloadKind::Created);
    }

    #[test]
    fn a_path_without_an_extension_has_none() {
        let event = HotReloadEvent::new(PathBuf::from("Makefile"), HotReloadKind::Modified);
        assert_eq!(event.extension(), None);
        assert_eq!(event.file_name(), Some("Makefile"));
    }

    #[test]
    fn the_kinds_are_stable_for_the_c_abi() {
        assert_eq!(HotReloadKind::Created as i32, 0);
        assert_eq!(HotReloadKind::Modified as i32, 1);
        assert_eq!(HotReloadKind::Deleted as i32, 2);
        assert_eq!(HotReloadKind::default(), HotReloadKind::Created);
    }

    #[test]
    fn a_config_with_zero_debounce_uses_the_default() {
        let config = HotReloadConfig::new(true);
        assert_eq!(config.debounce(), DEFAULT_DEBOUNCE);
        assert_eq!(config._reserved, [0; 4]);

        let mut explicit = HotReloadConfig::new(true);
        explicit.debounce_ms = 40;
        assert_eq!(explicit.debounce(), std::time::Duration::from_millis(40));
    }

    #[test]
    fn a_disabled_reloader_watches_and_polls_nothing() {
        let mut reloader = HotReloader::new(HotReloadConfig::new(false));
        assert!(!reloader.is_enabled());
        assert_eq!(
            reloader
                .watch_dirs(&[PathBuf::from("assets")])
                .expect("disabled"),
            0
        );
        assert!(!reloader.is_watching());
        assert!(reloader.poll().is_empty());
        assert_eq!(reloader.stats(), HotReloadStats::default());
    }

    #[test]
    fn enabling_and_disabling_is_observable() {
        let mut reloader = HotReloader::new(HotReloadConfig::new(true));
        assert!(reloader.is_enabled());
        reloader.set_enabled(false);
        assert!(!reloader.is_enabled());
        assert!(!reloader.is_watching());
        reloader.set_enabled(true);
        assert!(reloader.is_enabled());
    }

    #[test]
    fn the_stats_accumulate_without_overflowing() {
        let mut reloader = HotReloader::new(HotReloadConfig::new(true));
        reloader.note_applied(u64::MAX);
        reloader.note_applied(1);
        assert_eq!(reloader.stats().applied, u64::MAX, "saturating");
        reloader.note_failed(2);
        assert_eq!(reloader.stats().failed, 2);
    }

    #[test]
    fn the_switch_words_are_the_only_accepted_values() {
        for on in ["1", "true", "ON", " yes ", "True"] {
            assert_eq!(parse_switch(on), Some(true), "{on:?}");
        }
        for off in ["0", "false", "Off", "no", "NO"] {
            assert_eq!(parse_switch(off), Some(false), "{off:?}");
        }
        for neither in ["", "maybe", "2", "enabled"] {
            assert_eq!(parse_switch(neither), None, "{neither:?}");
        }
    }

    #[test]
    fn polling_a_disabled_reloader_does_not_touch_the_counters() {
        let mut reloader = HotReloader::new(HotReloadConfig::new(false));
        let _ = reloader.poll();
        assert_eq!(reloader.stats().ignored, 0);
        assert_eq!(reloader.stats().events, 0);
    }
}
