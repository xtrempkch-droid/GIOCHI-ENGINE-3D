//! The project + asset-browser flow, driven through `EditorState` — no window, no SDL.
//!
//! The example `editor_project_demo` does the same against a real folder and is what CI
//! greps; this test is the one that runs in `cargo test` with no side effects, and it pins
//! the two things most likely to regress: that saving a scene registers it in
//! `project.toml`, and that a missing scene in the project file is a warning rather than a
//! refusal to open.

use std::path::{Path, PathBuf};

use giochi_editor::editor::{EditorState, StatusKind};
use giochi_editor::panels::asset_browser::AssetBrowserState;
use giochi_editor::project::{AssetKind, Project};

fn temp(name: &str) -> PathBuf {
    let path =
        std::env::temp_dir().join(format!("giochi-project-flow-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    path
}

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("dir");
    }
    std::fs::write(path, contents).expect("write");
}

#[test]
fn a_project_created_imported_and_saved_reopens_complete() {
    let root = temp("full");
    let parent = root.parent().expect("parent");
    let name = root
        .file_name()
        .expect("name")
        .to_string_lossy()
        .into_owned();

    let mut state = EditorState::new();
    state.create_project(parent, &name).expect("created");
    assert_eq!(
        state.project.as_ref().map(|p| p.name.as_str()),
        Some(name.as_str())
    );

    // Import a texture (`assets/`) and a scene asset.
    let texture_source = root.join("source.png");
    write(&texture_source, "png bytes");
    let imported = state.import_asset(&texture_source).expect("imported");
    assert!(imported.starts_with(root.join("assets").join("textures")));
    assert_eq!(state.assets.entries.len(), 1, "the listing was refreshed");
    assert_eq!(state.assets.selected.as_deref(), Some(imported.as_path()));

    // Build a scene and save it into the project.
    state.new_scene();
    let scene = root.join("scenes").join("level.scene");
    state.save_scene_as(&scene).expect("saved");
    assert!(!state.dirty);

    // The project file now lists it, relative to the root.
    let reopened = Project::open(&root).expect("reopened");
    assert_eq!(reopened.scenes, vec!["scenes/level.scene".to_owned()]);
    assert_eq!(reopened.name, name);

    // And the editor can open the project and the scene again from scratch.
    let mut other = EditorState::new();
    other.open_project(&root).expect("opened project");
    other.open_scene(&scene).expect("opened scene");
    let scene_data = other.scene.as_ref().expect("scene");
    assert_eq!(scene_data.nodes.len(), 3);
    assert_eq!(other.status.kind, StatusKind::Info);
    assert_eq!(other.assets.entries.len(), 1);

    // The browser's filter narrows the same listing.
    let mut browser = AssetBrowserState::new();
    browser.refresh(other.project.as_ref());
    assert_eq!(browser.entries.len(), 1);
    browser.set_filter(other.project.as_ref(), AssetKind::Model);
    assert!(browser.entries.is_empty());
    browser.set_filter(other.project.as_ref(), AssetKind::Texture);
    assert_eq!(browser.entries.len(), 1);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_project_listing_a_missing_scene_still_opens_with_a_warning() {
    let root = temp("missing-scene");
    Project::create(&root, "missing").expect("created");
    write(
        &root.join("project.toml"),
        "name = \"missing\"\nscenes = [\"scenes/gone.scene\"]\n",
    );

    let mut state = EditorState::new();
    state.open_project(&root).expect("opens anyway");
    assert_eq!(state.status.kind, StatusKind::Warning);
    assert!(
        state.status.text.contains("gone.scene"),
        "{}",
        state.status.text
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn importing_without_a_project_says_so() {
    let mut state = EditorState::new();
    let source = temp("orphan-source").join("file.png");
    write(&source, "png");
    let error = state.import_asset(&source).expect_err("no project");
    assert!(error.contains("open a project"), "{error}");
    let _ = std::fs::remove_dir_all(source.parent().expect("parent"));
}

#[test]
fn opening_a_project_that_does_not_exist_keeps_the_previous_one() {
    let root = temp("keep-old");
    Project::create(&root, "keep").expect("created");

    let mut state = EditorState::new();
    state.open_project(&root).expect("opened");
    let error = state
        .open_project("/nonexistent/project/directory")
        .expect_err("missing");
    assert!(error.contains("project.toml"), "{error}");
    assert!(state.project.is_some(), "the previous project survives");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_scene_dialogs_start_in_the_projects_scenes_folder() {
    let root = temp("dialogs");
    Project::create(&root, "dialogs").expect("created");

    let mut with_project = EditorState::new();
    with_project.open_project(&root).expect("opened");
    assert_eq!(with_project.scenes_directory(), root.join("scenes"));

    let without = EditorState::new();
    assert_eq!(
        without.scenes_directory(),
        std::env::current_dir().expect("cwd")
    );
    let _ = std::fs::remove_dir_all(&root);
}
