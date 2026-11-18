//! Where the game's files live at runtime.
//!
//! Two layouts have to work and they disagree about what "next to the binary" means:
//!
//! * `cargo run -p giochi-racer` from the repository root sees `giochi-racer/assets/`;
//! * the phase-46 package puts the binary in `bin/` and the assets in `assets/`, so
//!   `assets/tracks/...` resolves relative to the executable and *not* to the shell's
//!   working directory — a game started from a desktop launcher inherits `/`.
//!
//! The order is deliberately executable-first, then working directory, then the
//! repository checkout relative to `CARGO_MANIFEST_DIR` (which is how `cargo test` finds
//! the committed scene even though the test binary runs from `target/`).

use std::path::{Path, PathBuf};

/// Candidate roots, in the order they are tried.
pub fn search_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(directory) = exe.parent() {
            roots.push(directory.to_path_buf());
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        roots.push(cwd);
    }
    roots.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    roots
}

/// Resolves `relative` against the first root that has it, or returns it unchanged.
///
/// Returning the unresolved path rather than `None` is deliberate: the caller's error
/// message then names the path the game *looked for*, which is the only useful thing to
/// print when an asset is missing.
pub fn resolve(relative: impl AsRef<Path>) -> PathBuf {
    let relative = relative.as_ref();
    if relative.is_absolute() {
        return relative.to_path_buf();
    }
    for root in search_roots() {
        let candidate = root.join(relative);
        if candidate.exists() {
            return candidate;
        }
    }
    relative.to_path_buf()
}

/// The directory the game treats as its asset root.
///
/// `assets/` next to the executable when it exists (the package), otherwise the
/// repository's `giochi-racer/assets` (a checkout).
///
/// A candidate only counts when it has a `tracks/` directory. The repository root also
/// has an `assets/` — the engine demo's sprites — so "the first directory called assets"
/// finds the wrong one and a track that names an imported model then looks for it in the
/// engine's fixtures.
pub fn asset_root() -> PathBuf {
    for root in search_roots() {
        for candidate in [
            root.join("assets"),
            root.join("giochi-racer").join("assets"),
        ] {
            if candidate.join("tracks").is_dir() {
                return candidate;
            }
        }
    }
    PathBuf::from("assets")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_absolute_path_is_returned_untouched() {
        let path = PathBuf::from("/nowhere/at/all.scene");
        assert_eq!(resolve(&path), path);
    }

    #[test]
    fn a_path_that_exists_somewhere_is_found() {
        // `Cargo.toml` is in the crate directory, which is one of the search roots
        // because the test binary has no assets next to it in `target/`.
        let resolved = resolve("Cargo.toml");
        assert!(resolved.exists(), "{resolved:?} should exist");
        assert!(resolved.ends_with("Cargo.toml"));
    }

    #[test]
    fn a_missing_path_comes_back_unchanged_so_the_error_can_name_it() {
        let missing = "definitely/not/here.scene";
        assert_eq!(resolve(missing), PathBuf::from(missing));
    }

    #[test]
    fn the_asset_root_is_the_games_not_the_engines() {
        // The workspace root has an `assets/` too (the demo's sprites); the game's is the
        // one with `tracks/` in it.
        let root = asset_root();
        assert!(root.is_dir(), "{root:?} should be the assets directory");
        assert!(root.join("tracks").is_dir(), "{root:?} has no tracks");
        assert!(
            root.ends_with("giochi-racer/assets") || root.join("giochi-racer").exists(),
            "{root:?} is the engine's asset folder"
        );
    }

    #[test]
    fn the_search_roots_are_ordered_executable_cwd_checkout() {
        let roots = search_roots();
        assert_eq!(roots.len(), 3);
        assert!(roots[2].ends_with("giochi-racer"));
    }
}
