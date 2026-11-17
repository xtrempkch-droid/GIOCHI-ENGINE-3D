//! Phase 30's acceptance, scripted: the editor boots, lays out and paints frames.
//!
//! One test per binary on purpose — SDL allows a single `EventPump` per process, which
//! is the same constraint `engine_smoke.rs` documents. The dummy video driver keeps the
//! check headless; `--frames` is what makes a loop that runs "until the user closes the
//! window" terminate.
//!
//! Phase 31 adds the second half: the same loop with a scene open, so the viewport's
//! projection and the window title are exercised where they actually run.
//!
//! **One test function, not two.** SDL's `Sdl` is a process-wide singleton that refuses
//! to initialise from a second thread, and `cargo test` gives every `#[test]` its own
//! thread. Two tests that each open an engine fail with "Cannot initialize `Sdl` from
//! more than one thread" — the same reason `engine_smoke.rs` holds one test.
use giochi_editor::{run, EditorOptions};
use giochi_engine_3d::scene::Scene;

#[test]
fn the_editor_boots_and_renders_frames_without_a_display() {
    std::env::set_var("SDL_VIDEODRIVER", "dummy");

    let stats = run(EditorOptions {
        width: 1280,
        height: 720,
        frames: Some(3),
        ..EditorOptions::default()
    })
    .expect("the editor must open a window, lay out and paint");

    assert_eq!(stats.frames, 3);

    // Same thread, second run: a real file through the same format the editor writes,
    // so this also covers the load path a user takes with `--scene`.
    let path =
        std::env::temp_dir().join(format!("giochi-editor-smoke-{}.scene", std::process::id()));
    Scene::default_scene()
        .save_to_path(&path)
        .expect("scene written");

    let stats = run(EditorOptions {
        width: 1024,
        height: 768,
        frames: Some(3),
        scene: Some(path.clone()),
        ..EditorOptions::default()
    })
    .expect("the editor must open a scene and paint it");

    assert_eq!(stats.frames, 3);
    let _ = std::fs::remove_file(&path);
}
