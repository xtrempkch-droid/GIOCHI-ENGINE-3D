//! Restarting a script when the file changes.
//!
//! # Why a restart and not a patch
//!
//! A texture is data the program reads; a script **is** the program. There is no
//! supported way to replace the body of a running Lua chunk, and even if there were, the
//! closures the old body handed to `app:run` would still point at the old instructions.
//! The honest behaviour is: detect the change, stop the loop, build a fresh VM, run the
//! edited file. Globals are not carried over — a script that needs state across a reload
//! must keep it in a file, which is what a save system is for (see `docs/HOT_RELOAD.md`).
//!
//! # How the host finds out
//!
//! [`watch_script`] installs a watcher; [`poll_changes`] is called once per frame by the
//! scene's `transition`, and bumps the script generation when a watched `.lua` settles.
//! The scene stops the loop when the generation it started with no longer matches, and
//! [`take_reload_request`] tells the host *why* the loop stopped — a reload must restart
//! it, a quit must not.
//!
//! The watcher is a process-wide singleton because there is one Lua VM and one script at
//! a time. That keeps the whole feature out of `App`'s already C-shaped surface.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Set when the loop stopped because the script changed, cleared by
/// [`take_reload_request`].
static RELOAD_REQUESTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(feature = "hot_reload")]
type Watcher = giochi_engine::hot_reload::FileWatcher;

#[cfg(not(feature = "hot_reload"))]
type Watcher = ();

/// Process-wide watcher, installed by [`watch_script`].
#[cfg(feature = "hot_reload")]
static WATCHER: OnceLock<Mutex<Option<Watcher>>> = OnceLock::new();

fn watcher_slot() -> &'static Mutex<Option<Watcher>> {
    #[cfg(feature = "hot_reload")]
    {
        WATCHER.get_or_init(|| Mutex::new(None))
    }
    #[cfg(not(feature = "hot_reload"))]
    {
        static SLOT: OnceLock<Mutex<Option<()>>> = OnceLock::new();
        SLOT.get_or_init(|| Mutex::new(None))
    }
}

/// Starts watching `script` (and the directory it lives in) for changes.
///
/// Idempotent: calling it again replaces the previous watch, so a host that restarts the
/// script can call it again without leaking watches.
///
/// Without the `hot_reload` feature this is a no-op that returns `Ok(())`: a script must
/// still run, it just will not restart on its own.
///
/// # Errors
///
/// The reason the watch could not be installed, as a string — `mlua` is not involved and
/// this is called before the VM exists, so there is no `mlua::Error` to return yet.
pub fn watch_script(script: &Path) -> Result<(), String> {
    #[cfg(feature = "hot_reload")]
    {
        let mut watcher = giochi_engine::hot_reload::FileWatcher::new();
        let paths = vec![script.to_path_buf()];
        watcher.watch_files(&paths).map_err(|err| err.to_string())?;

        let mut slot = watcher_slot()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *slot = Some(watcher);
        log::info!("watching '{}' for script changes", script.display());
        Ok(())
    }
    #[cfg(not(feature = "hot_reload"))]
    {
        let _ = script;
        Ok(())
    }
}

/// Watches every `.lua` file under `dir`, recursively.
///
/// Useful when a script `require`s its own modules: editing any of them restarts the
/// program, which is the only correct behaviour when the module is part of the program.
///
/// # Errors
///
/// The reason the watch could not be installed.
pub fn watch_dir(dir: &Path) -> Result<(), String> {
    #[cfg(feature = "hot_reload")]
    {
        let mut watcher = giochi_engine::hot_reload::FileWatcher::new();
        let paths = vec![dir.to_path_buf()];
        watcher.watch_dirs(&paths).map_err(|err| err.to_string())?;

        let mut slot = watcher_slot()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *slot = Some(watcher);
        log::info!("watching '{}' for script changes", dir.display());
        Ok(())
    }
    #[cfg(not(feature = "hot_reload"))]
    {
        let _ = dir;
        Ok(())
    }
}

/// Stops watching. Called by the host when it is done with the script.
pub fn unwatch() {
    if let Ok(mut slot) = watcher_slot().lock() {
        *slot = None;
    }
}

/// The current script generation.
///
/// Starts at `0` and advances once per settled change. It is the engine's counter
/// (`giochi_engine::hot_reload::script_epoch`) so that a host which enables the engine's
/// own hot reload sees the same number.
pub fn generation() -> u32 {
    #[cfg(feature = "hot_reload")]
    {
        giochi_engine::hot_reload::script_epoch()
    }
    #[cfg(not(feature = "hot_reload"))]
    {
        0
    }
}

/// Drains the watcher and advances the generation if a script changed.
///
/// Returns the generation after the poll. Called once per frame from the scene; cheap
/// (a channel drain that is usually empty).
pub fn poll_changes() -> u32 {
    #[cfg(feature = "hot_reload")]
    {
        let events = match watcher_slot().lock() {
            Ok(mut slot) => match slot.as_mut() {
                Some(watcher) => watcher.poll(),
                None => return generation(),
            },
            Err(poisoned) => {
                // A panic elsewhere must not silently disable reloading.
                let mut slot = poisoned.into_inner();
                match slot.as_mut() {
                    Some(watcher) => watcher.poll(),
                    None => return generation(),
                }
            }
        };

        for event in events {
            if !event.is_deleted() {
                let generation = giochi_engine::hot_reload::bump_script_epoch();
                log::info!(
                    "script '{}' changed; generation is now {generation}",
                    event.path().display()
                );
            }
        }
    }
    generation()
}

/// Records that the loop is stopping because the script changed.
///
/// Called by the scene; the host reads it after `app:run` returns.
pub fn request_reload() {
    RELOAD_REQUESTED.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// Returns whether the loop stopped to reload the script, clearing the flag.
///
/// A host loops like this:
///
/// ```no_run
/// # fn run_once(_: &str) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }
/// # fn example(script: &str) -> Result<(), Box<dyn std::error::Error>> {
/// loop {
///     run_once(script)?;
///     if !giochi_engine_lua::take_reload_request() {
///         break;
///     }
///     eprintln!("script changed; restarting");
/// }
/// # Ok(())
/// # }
/// ```
pub fn take_reload_request() -> bool {
    RELOAD_REQUESTED.swap(false, std::sync::atomic::Ordering::SeqCst)
}

/// A witness that the watcher was asked for `script`, used by the tests.
///
/// `None` when nothing is being watched.
pub fn watched_paths() -> Vec<PathBuf> {
    #[cfg(feature = "hot_reload")]
    {
        match watcher_slot().lock() {
            Ok(slot) => slot
                .as_ref()
                .map(giochi_engine::hot_reload::FileWatcher::watched_files)
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    }
    #[cfg(not(feature = "hot_reload"))]
    {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_a_watcher_the_generation_is_stable() {
        unwatch();
        let before = generation();
        assert_eq!(poll_changes(), before);
        assert!(watched_paths().is_empty());
    }

    #[test]
    fn a_reload_request_is_taken_once() {
        // The flag is process-wide; clearing first keeps the test order-independent.
        let _ = take_reload_request();
        request_reload();
        assert!(take_reload_request());
        assert!(!take_reload_request(), "the second read must be false");
    }

    #[test]
    fn watching_a_missing_file_reports_the_error() {
        #[cfg(feature = "hot_reload")]
        {
            let error = watch_script(Path::new("/nonexistent/giochi-no-such-dir/game.lua"));
            assert!(error.is_err(), "watching a missing directory must fail");
        }
    }
}
