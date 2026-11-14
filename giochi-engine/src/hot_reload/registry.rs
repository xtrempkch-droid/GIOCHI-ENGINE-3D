//! Which extensions reload, and in what order.
//!
//! A frame that has to reload a shader, a texture and a script does not care about the
//! order — until it does. A shader that samples a texture must be compiled against the
//! texture that exists *now*, and a script that names both wants the assets ready before
//! it runs. The order below is the plan's, and it is a small, explicit table rather than
//! a callback list so it can be inspected, tested and printed by the devtools console
//! (phase 20).

use std::collections::BTreeMap;
use std::path::Path;

use crate::hot_reload::HotReloadEvent;

/// Shaders are reloaded first: a compile error must not be masked by a failed texture.
pub const PRIORITY_SHADER: u8 = 30;
/// Meshes sit between shaders and textures: a mesh's layout is what a shader is
/// compiled against, and it costs a file parse rather than an upload.
pub const PRIORITY_MESH: u8 = 25;
/// Textures after the geometry that samples them.
pub const PRIORITY_TEXTURE: u8 = 20;
/// Scripts last: a reloaded script starts from a clean VM, so everything it can touch
/// must already be current.
pub const PRIORITY_SCRIPT: u8 = 10;

/// Maps a file extension to a reload priority.
///
/// ```no_run
/// use giochi_engine::hot_reload::{AssetReloadRegistry, HotReloadEvent, HotReloadKind};
///
/// let mut registry = AssetReloadRegistry::with_defaults();
/// registry.register("toml", 5);
///
/// let events = vec![
///     HotReloadEvent::new("game.lua".into(), HotReloadKind::Modified),
///     HotReloadEvent::new("quad.frag".into(), HotReloadKind::Modified),
/// ];
/// let ordered = registry.order(events);
/// assert_eq!(ordered[0].extension(), Some("frag"));
/// ```
///
/// # C ABI
///
/// Not exposed: the table is engine configuration, not game state.
#[repr(C)]
pub struct AssetReloadRegistry {
    /// Sorted so iteration and `extensions()` are deterministic.
    rules: BTreeMap<String, u8>,
    _reserved: [u32; 4],
}

impl std::fmt::Debug for AssetReloadRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AssetReloadRegistry")
            .field("rules", &self.rules)
            .finish_non_exhaustive()
    }
}

impl Default for AssetReloadRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl AssetReloadRegistry {
    /// An empty table: nothing is reloaded, and [`Self::order`] preserves input order
    /// for every event whose extension is unknown.
    pub fn new() -> Self {
        Self {
            rules: BTreeMap::new(),
            _reserved: [0; 4],
        }
    }

    /// The table the engine starts with: shaders, meshes, textures, scripts.
    ///
    /// `.vert`/`.frag` share [`PRIORITY_SHADER`], `.obj` is a mesh, `.png`/`.bmp`/
    /// `.jpg`/`.jpeg` are textures and `.lua` is a script. Anything else is not watched
    /// until a caller registers it.
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        for extension in ["vert", "frag"] {
            registry.register(extension, PRIORITY_SHADER);
        }
        registry.register("obj", PRIORITY_MESH);
        for extension in ["png", "bmp", "jpg", "jpeg"] {
            registry.register(extension, PRIORITY_TEXTURE);
        }
        registry.register("lua", PRIORITY_SCRIPT);
        registry
    }

    /// Adds or replaces the priority for `extension`.
    ///
    /// The leading dot is optional and the name is case-insensitive, because
    /// `Sprite.PNG` on a case-preserving filesystem is still a PNG. An empty extension
    /// is ignored and returns `false`.
    pub fn register(&mut self, extension: &str, priority: u8) -> bool {
        let extension = normalise(extension);
        if extension.is_empty() {
            return false;
        }
        self.rules.insert(extension, priority);
        true
    }

    /// Removes `extension` from the table. Returns whether it was there.
    pub fn unregister(&mut self, extension: &str) -> bool {
        self.rules.remove(&normalise(extension)).is_some()
    }

    /// Whether files with this extension are reloaded at all.
    pub fn handles(&self, path: &Path) -> bool {
        self.priority_for(path).is_some()
    }

    /// The priority for `path`, or `None` when its extension is not registered.
    pub fn priority_for(&self, path: &Path) -> Option<u8> {
        let extension = path.extension()?.to_str()?;
        self.rules.get(&normalise(extension)).copied()
    }

    /// The registered extensions, in alphanumeric order.
    pub fn extensions(&self) -> Vec<String> {
        self.rules.keys().cloned().collect()
    }

    /// How many extensions are registered.
    pub fn len(&self) -> usize {
        self.rules.len()
    }

    /// Whether nothing is registered.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Sorts `events` so the highest-priority extension comes first.
    ///
    /// The sort is **stable**, so events of equal priority keep the order the watcher
    /// produced (which is path order). Events whose extension is not registered are
    /// dropped: the watcher reports every file under a watched directory, and a `.swp`
    /// file next to a shader is not a reload.
    pub fn order(&self, events: Vec<HotReloadEvent>) -> Vec<HotReloadEvent> {
        let mut kept: Vec<(u8, HotReloadEvent)> = events
            .into_iter()
            .filter_map(|event| self.priority_for(event.path()).map(|p| (p, event)))
            .collect();
        // Descending priority, stable within a priority.
        kept.sort_by_key(|(priority, _)| std::cmp::Reverse(*priority));
        kept.into_iter().map(|(_, event)| event).collect()
    }
}

/// Lower-cased extension without the leading dot.
fn normalise(extension: &str) -> String {
    extension
        .trim()
        .trim_start_matches('.')
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::hot_reload::HotReloadKind;

    fn event(path: &str) -> HotReloadEvent {
        HotReloadEvent::new(PathBuf::from(path), HotReloadKind::Modified)
    }

    #[test]
    fn an_empty_registry_orders_nothing() {
        let registry = AssetReloadRegistry::new();
        assert!(registry.is_empty());
        assert!(registry.order(vec![event("a.png")]).is_empty());
    }

    #[test]
    fn the_defaults_cover_the_four_kinds() {
        let registry = AssetReloadRegistry::with_defaults();
        assert!(registry.handles(Path::new("shaders/quad.frag")));
        assert!(registry.handles(Path::new("models/teapot.obj")));
        assert!(registry.handles(Path::new("assets/player.PNG")));
        assert!(registry.handles(Path::new("game.lua")));
        assert!(!registry.handles(Path::new("README.md")));
        assert!(!registry.handles(Path::new("no-extension")));
    }

    #[test]
    fn the_leading_dot_and_case_do_not_matter() {
        let mut registry = AssetReloadRegistry::new();
        assert!(registry.register(".PNG", 1));
        assert!(registry.handles(Path::new("a.png")));
        assert_eq!(registry.priority_for(Path::new("a.Png")), Some(1));
    }

    #[test]
    fn an_empty_extension_is_refused() {
        let mut registry = AssetReloadRegistry::new();
        assert!(!registry.register("  ", 1));
        assert!(!registry.register(".", 1));
        assert!(registry.is_empty());
    }

    #[test]
    fn shaders_come_before_textures_and_scripts() {
        let registry = AssetReloadRegistry::with_defaults();
        let ordered = registry.order(vec![
            event("game.lua"),
            event("assets/player.png"),
            event("shaders/quad.frag"),
        ]);
        let names: Vec<&str> = ordered
            .iter()
            .map(|e| e.file_name().unwrap_or_default())
            .collect();
        assert_eq!(names, vec!["quad.frag", "player.png", "game.lua"]);
    }

    #[test]
    fn unregistered_files_are_dropped_rather_than_reordered() {
        let registry = AssetReloadRegistry::with_defaults();
        let ordered = registry.order(vec![event("a.png"), event(".a.png.swp")]);
        assert_eq!(ordered.len(), 1);
        assert_eq!(ordered[0].file_name(), Some("a.png"));
    }

    #[test]
    fn equal_priorities_keep_their_input_order() {
        let registry = AssetReloadRegistry::with_defaults();
        let ordered = registry.order(vec![event("z.png"), event("a.png"), event("m.png")]);
        let names: Vec<&str> = ordered
            .iter()
            .map(|e| e.file_name().unwrap_or(""))
            .collect();
        assert_eq!(names, vec!["z.png", "a.png", "m.png"]);
    }

    #[test]
    fn unregister_removes_the_rule() {
        let mut registry = AssetReloadRegistry::with_defaults();
        let before = registry.len();
        assert!(registry.unregister(".png"));
        assert_eq!(registry.len(), before - 1);
        assert!(!registry.handles(Path::new("a.png")));
        assert!(!registry.unregister("png"), "removing twice must report it");
    }

    #[test]
    fn extensions_are_listed_in_order() {
        let registry = AssetReloadRegistry::with_defaults();
        let extensions = registry.extensions();
        let mut sorted = extensions.clone();
        sorted.sort();
        assert_eq!(extensions, sorted);
        assert_eq!(extensions.len(), registry.len());
    }
}
