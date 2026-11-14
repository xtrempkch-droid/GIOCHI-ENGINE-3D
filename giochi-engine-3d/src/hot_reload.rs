//! Hot reload of shaders and meshes for the 3D renderer.
//!
//! The 2D engine owns the [`FileWatcher`](giochi_engine::hot_reload::FileWatcher) — a
//! watcher is a watcher, and `notify` does not care which crate asked. What belongs
//! *here* is the part that needs a GL context: compiling a new program, parsing a new
//! OBJ, and swapping it under a handle that the scene already holds.
//!
//! # Why the reload keeps the handle
//!
//! A scene stores `ShaderHandle`/`MeshHandle` in its own fields, a batch may cache them,
//! and the C ABI hands them to callers as `uint64_t`. Issuing a new handle on reload
//! would mean every holder has to learn about it, which is exactly the coupling this
//! feature exists to avoid. [`HandleRegistry::replace`](crate::render3d) swaps the
//! resource under the same number instead.
//!
//! # What is preserved, and what is not
//!
//! * **Uniforms**: a program managed by [`ShaderReloader`] records the values written
//!   through its setters (`set_mat4`, `set_color`, …) and replays them into the
//!   recompiled program. Per-draw uniforms need no replay — the next draw writes them
//!   again — but a constant set once at load time would otherwise silently revert to
//!   zero.
//! * **GL state** (depth test, blending, culling) belongs to [`crate::context::GlState`],
//!   not to the program, and is untouched.
//! * **The mesh's buffers** are rebuilt, so a reload is also a re-upload. That is the
//!   cost of a reload and the reason it is debounced.
//!
//! # A shader that does not compile
//!
//! The previous program stays bound and the driver's full log is reported at `error!`
//! level. A hot reload that turned a typo into a blank screen would be worse than no hot
//! reload at all.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::render3d::{MeshHandle, RenderContext3D, ShaderHandle};

/// Counters for the devtools console and the overlay.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ShaderReloadStats {
    /// Shaders recompiled successfully.
    pub shaders_reloaded: u64,
    /// Shaders the driver refused; the previous program stayed in place.
    pub shader_failures: u64,
    /// Meshes reloaded successfully.
    pub meshes_reloaded: u64,
    /// Meshes whose file could not be parsed or read.
    pub mesh_failures: u64,
    /// Reserved for future ABI-compatible fields. Must be zero.
    pub _reserved: [u32; 4],
}

/// The source files a shader handle was built from.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ShaderFiles {
    vertex: PathBuf,
    fragment: PathBuf,
}

/// The file a mesh handle was built from.
#[derive(Debug, Clone, PartialEq, Eq)]
struct MeshFile {
    path: PathBuf,
}

/// Tracks which handles came from which files, and recompiles them on demand.
///
/// ```no_run
/// use std::path::{Path, PathBuf};
/// use giochi_engine_3d::hot_reload::ShaderReloader;
/// # use giochi_engine_3d::RenderContext3D;
/// # fn example(render: &mut RenderContext3D, gl: &glow::Context) -> Result<(), Box<dyn std::error::Error>> {
/// let mut reloader = ShaderReloader::new();
///
/// let handle = render.load_shader(
///     Path::new("shaders/flat.vert"),
///     Path::new("shaders/flat.frag"),
/// )?;
/// reloader.track_shader(handle, Path::new("shaders/flat.vert"), Path::new("shaders/flat.frag"));
/// // Shaders loaded from a file already record their uniforms; nothing else to do.
/// assert_eq!(render.shader_captures_uniforms(handle), Some(true));
///
/// // When the watcher reports `shaders/flat.frag`:
/// reloader.reload_shader(render, handle);
/// # Ok(())
/// # }
/// ```
///
/// # C ABI
///
/// Not exposed until phase 24, where `giochi_hot_reload_enable` reaches the same code
/// through the engine's watcher.
#[repr(C)]
pub struct ShaderReloader {
    shaders: HashMap<ShaderHandle, ShaderFiles>,
    meshes: HashMap<MeshHandle, MeshFile>,
    stats: ShaderReloadStats,
    _reserved: [u32; 4],
}

impl std::fmt::Debug for ShaderReloader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShaderReloader")
            .field("shaders", &self.shaders.len())
            .field("meshes", &self.meshes.len())
            .field("stats", &self.stats)
            .finish_non_exhaustive()
    }
}

impl Default for ShaderReloader {
    fn default() -> Self {
        Self::new()
    }
}

impl ShaderReloader {
    /// Creates an empty tracker.
    pub fn new() -> Self {
        Self {
            shaders: HashMap::new(),
            meshes: HashMap::new(),
            stats: ShaderReloadStats::default(),
            _reserved: [0; 4],
        }
    }

    /// Records the files `handle` was compiled from.
    ///
    /// Tracking is opt-in: a built-in shader has no files and needs no entry, and a
    /// program nobody edits costs nothing. Returns `false` for handle `0`.
    pub fn track_shader(&mut self, handle: ShaderHandle, vertex: &Path, fragment: &Path) -> bool {
        if handle == 0 {
            return false;
        }
        self.shaders.insert(
            handle,
            ShaderFiles {
                vertex: vertex.to_path_buf(),
                fragment: fragment.to_path_buf(),
            },
        );
        true
    }

    /// Records the OBJ file `handle` was loaded from.
    pub fn track_mesh(&mut self, handle: MeshHandle, path: &Path) -> bool {
        if handle == 0 {
            return false;
        }
        self.meshes.insert(
            handle,
            MeshFile {
                path: path.to_path_buf(),
            },
        );
        true
    }

    /// Stops tracking `handle` — call it before destroying the shader, so a later
    /// reload does not try to recompile a handle that is gone.
    pub fn forget_shader(&mut self, handle: ShaderHandle) -> bool {
        self.shaders.remove(&handle).is_some()
    }

    /// Stops tracking a mesh.
    pub fn forget_mesh(&mut self, handle: MeshHandle) -> bool {
        self.meshes.remove(&handle).is_some()
    }

    /// How many shaders are tracked.
    pub fn tracked_shaders(&self) -> usize {
        self.shaders.len()
    }

    /// How many meshes are tracked.
    pub fn tracked_meshes(&self) -> usize {
        self.meshes.len()
    }

    /// The counters since construction.
    pub fn stats(&self) -> ShaderReloadStats {
        self.stats
    }

    /// Recompiles one tracked shader. Returns whether the new program is in place.
    ///
    /// On failure the previous program stays bound and the driver's log is reported at
    /// `error!` level. An untracked handle is `false`, not a panic: the caller may be
    /// iterating events that mention a file it no longer uses.
    pub fn reload_shader(&mut self, render: &mut RenderContext3D, handle: ShaderHandle) -> bool {
        let Some(files) = self.shaders.get(&handle).cloned() else {
            log::debug!("shader {handle} is not tracked for reload");
            return false;
        };

        match render.reload_shader_sources(handle, &files.vertex, &files.fragment) {
            Ok(kind) => {
                self.stats.shaders_reloaded = self.stats.shaders_reloaded.saturating_add(1);
                log::info!(
                    "shader {handle} recompiled from '{}' + '{}' ({kind:?})",
                    files.vertex.display(),
                    files.fragment.display()
                );
                true
            }
            Err(err) => {
                self.stats.shader_failures = self.stats.shader_failures.saturating_add(1);
                log::error!(
                    "shader {handle} was not reloaded, keeping the previous program: {err}"
                );
                false
            }
        }
    }

    /// Reloads every tracked shader whose vertex **or** fragment file is `path`.
    ///
    /// Returns how many reloaded. `0` is the normal answer for an edited file that no
    /// live shader uses.
    pub fn reload_shaders_for(&mut self, render: &mut RenderContext3D, path: &Path) -> usize {
        let handles: Vec<ShaderHandle> = self
            .shaders
            .iter()
            .filter(|(_, files)| files.vertex == path || files.fragment == path)
            .map(|(handle, _)| *handle)
            .collect();

        let mut reloaded = 0usize;
        for handle in handles {
            if self.reload_shader(render, handle) {
                reloaded += 1;
            }
        }
        reloaded
    }

    /// Reloads every tracked shader. Returns how many succeeded.
    pub fn reload_all_shaders(&mut self, render: &mut RenderContext3D) -> usize {
        let mut handles: Vec<ShaderHandle> = self.shaders.keys().copied().collect();
        // Deterministic order: the log reads like the handle table.
        handles.sort_unstable();
        let mut reloaded = 0usize;
        for handle in handles {
            if self.reload_shader(render, handle) {
                reloaded += 1;
            }
        }
        reloaded
    }

    /// Reloads every tracked mesh whose file is `path`. Returns how many succeeded.
    pub fn reload_meshes_for(&mut self, render: &mut RenderContext3D, path: &Path) -> usize {
        let handles: Vec<MeshHandle> = self
            .meshes
            .iter()
            .filter(|(_, file)| file.path == path)
            .map(|(handle, _)| *handle)
            .collect();

        let mut reloaded = 0usize;
        for handle in handles {
            match render.reload_mesh(handle) {
                Ok(()) => {
                    reloaded += 1;
                    self.stats.meshes_reloaded = self.stats.meshes_reloaded.saturating_add(1);
                    log::info!("mesh {handle} reloaded from '{}'", path.display());
                }
                Err(err) => {
                    self.stats.mesh_failures = self.stats.mesh_failures.saturating_add(1);
                    log::error!("mesh {handle} was not reloaded, keeping the previous one: {err}");
                }
            }
        }
        reloaded
    }

    /// Applies one settled file change, choosing between shader and mesh by extension.
    ///
    /// This is the entry point the engine calls from its hot-reload dispatch, and the one
    /// a game that owns its own watcher should call. Returns how many resources were
    /// reloaded.
    pub fn apply_path(&mut self, render: &mut RenderContext3D, path: &Path) -> usize {
        match path.extension().and_then(|ext| ext.to_str()) {
            Some("vert" | "frag") => self.reload_shaders_for(render, path),
            Some("obj") => self.reload_meshes_for(render, path),
            _ => 0,
        }
    }

    /// Forgets every tracked resource. Used on teardown.
    pub fn clear(&mut self) {
        self.shaders.clear();
        self.meshes.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_reloader_tracks_nothing() {
        let reloader = ShaderReloader::new();
        assert_eq!(reloader.tracked_shaders(), 0);
        assert_eq!(reloader.tracked_meshes(), 0);
        assert_eq!(reloader.stats(), ShaderReloadStats::default());
    }

    #[test]
    fn handle_zero_is_never_tracked() {
        let mut reloader = ShaderReloader::new();
        assert!(!reloader.track_shader(0, Path::new("a.vert"), Path::new("a.frag")));
        assert!(!reloader.track_mesh(0, Path::new("a.obj")));
        assert_eq!(reloader.tracked_shaders(), 0);
        assert_eq!(reloader.tracked_meshes(), 0);
    }

    #[test]
    fn tracking_and_forgetting_are_symmetric() {
        let mut reloader = ShaderReloader::new();
        assert!(reloader.track_shader(7, Path::new("a.vert"), Path::new("a.frag")));
        assert!(reloader.track_mesh(9, Path::new("a.obj")));
        assert_eq!(reloader.tracked_shaders(), 1);
        assert_eq!(reloader.tracked_meshes(), 1);

        assert!(reloader.forget_shader(7));
        assert!(reloader.forget_mesh(9));
        assert!(!reloader.forget_shader(7), "forgetting twice reports it");
        assert_eq!(reloader.tracked_shaders(), 0);
        assert_eq!(reloader.tracked_meshes(), 0);
    }

    #[test]
    fn re_tracking_replaces_the_files() {
        let mut reloader = ShaderReloader::new();
        reloader.track_shader(3, Path::new("old.vert"), Path::new("old.frag"));
        reloader.track_shader(3, Path::new("new.vert"), Path::new("new.frag"));
        assert_eq!(reloader.tracked_shaders(), 1);
        assert_eq!(
            reloader.shaders.get(&3),
            Some(&ShaderFiles {
                vertex: PathBuf::from("new.vert"),
                fragment: PathBuf::from("new.frag"),
            })
        );
    }

    #[test]
    fn clear_drops_every_tracked_resource() {
        let mut reloader = ShaderReloader::new();
        reloader.track_shader(1, Path::new("a.vert"), Path::new("a.frag"));
        reloader.track_mesh(2, Path::new("a.obj"));
        reloader.clear();
        assert_eq!(reloader.tracked_shaders(), 0);
        assert_eq!(reloader.tracked_meshes(), 0);
    }

    #[test]
    fn the_stats_are_stable_for_the_c_abi() {
        let stats = ShaderReloadStats::default();
        assert_eq!(stats._reserved, [0; 4]);
    }
}
