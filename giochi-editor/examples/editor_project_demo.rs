//! `editor_project_demo` — the phase-34 acceptance, end to end, without a window.
//!
//! Creates a project, imports a real texture and a real scene, saves a scene through the
//! editor's own state (which registers it in `project.toml`), reloads everything, and
//! prints what it found. The CI step greps for `project=ok`.
//!
//! ```text
//! cargo run -p giochi-editor --example editor_project_demo            # temp folder
//! cargo run -p giochi-editor --example editor_project_demo -- /tmp/p  # explicit root
//! ```

use std::path::PathBuf;
use std::process::ExitCode;

use giochi_editor::editor::{EditorState, StatusKind};
use giochi_editor::project::{AssetKind, Project};
use giochi_engine_3d::scene::Scene;

fn main() -> ExitCode {
    // An explicit root is kept, so the same folder can be opened by the editor afterwards
    // (`--project <root>`); the default temp folder is removed.
    let explicit = std::env::args().nth(1).map(PathBuf::from);
    let keep = explicit.is_some();
    let root = explicit.unwrap_or_else(|| {
        std::env::temp_dir().join(format!("giochi-editor-demo-{}", std::process::id()))
    });
    let _ = std::fs::remove_dir_all(&root);

    match run(&root, keep) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("erro: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(root: &std::path::Path, keep: bool) -> Result<(), String> {
    // 1. Create the project through the editor, as `File > New Project…` does.
    let mut state = EditorState::new();
    state.create_project(
        root.parent().unwrap_or(root),
        root.file_name()
            .unwrap_or_default()
            .to_str()
            .unwrap_or("demo"),
    )?;
    let project_root = state
        .project
        .as_ref()
        .ok_or("the editor did not record the project")?
        .root
        .clone();
    println!(
        "project={} root={}",
        state.project.as_ref().unwrap().name,
        project_root.display()
    );

    // 2. Import two real files: a texture and a scene.
    let texture_source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join("icons")
        .join("mesh_24.png");
    let texture = state.import_asset(&texture_source)?;
    println!("imported_texture={}", texture.display());

    state.new_scene();
    let scene_path = project_root.join("scenes").join("demo.scene");
    state.save_scene_as(&scene_path)?;
    println!("saved_scene={}", scene_path.display());

    let bundle = project_root.join("scenes").join("bundle.scene");
    Scene::default_scene()
        .save_to_path(&bundle)
        .map_err(|error| error.to_string())?;
    state.import_asset(&bundle)?;

    // 3. `save_scene_as` must have registered the scene in `project.toml`.
    let reopened = Project::open(&project_root)?;
    let registered = reopened
        .scenes
        .iter()
        .any(|scene| scene == "scenes/demo.scene");
    if !registered {
        return Err(format!(
            "scenes/demo.scene is not in project.toml: {:?}",
            reopened.scenes
        ));
    }
    println!("registered_scenes={:?}", reopened.scenes);

    // 4. The browser's listing, through the same API the panel uses.
    let mut assets = giochi_editor::panels::asset_browser::AssetBrowserState::new();
    assets.refresh(Some(&reopened));
    println!(
        "assets={} textures={} scenes={} models={}",
        assets.entries.len(),
        assets
            .entries
            .iter()
            .filter(|entry| entry.kind == AssetKind::Texture)
            .count(),
        assets
            .entries
            .iter()
            .filter(|entry| entry.kind == AssetKind::Scene)
            .count(),
        assets
            .entries
            .iter()
            .filter(|entry| entry.kind == AssetKind::Model)
            .count()
    );

    // 5. Reopen the scene and check the editor sees it, with no error status.
    let mut reopened_state = EditorState::new();
    reopened_state.open_project(&project_root)?;
    reopened_state.open_scene(&scene_path)?;
    let scene = reopened_state.scene.as_ref().ok_or("no scene after open")?;
    println!(
        "reopened_scene={} objects={} status={:?}",
        scene.name,
        scene.nodes.len(),
        reopened_state.status.kind
    );
    if reopened_state.status.kind == StatusKind::Error {
        return Err(reopened_state.status.text.clone());
    }
    if scene.nodes.len() != 3 {
        return Err(format!("expected 3 objects, found {}", scene.nodes.len()));
    }
    // Two imports (one texture, one scene). `scenes/demo.scene` lives in `scenes/`, which
    // is not the asset root: the project separates "what a scene references" from "the
    // scenes themselves".
    if assets.entries.len() != 2 {
        return Err(format!("expected 2 assets, found {}", assets.entries.len()));
    }

    println!("project=ok");
    if keep {
        println!("kept={}", project_root.display());
    } else {
        let _ = std::fs::remove_dir_all(&project_root);
    }
    Ok(())
}
