//! Phase 30's acceptance, scripted: the editor boots, lays out and paints frames.
//!
//! One test per binary on purpose — SDL allows a single `EventPump` per process, which
//! is the same constraint `engine_smoke.rs` documents. The dummy video driver keeps the
//! check headless; `--frames` is what makes a loop that runs "until the user closes the
//! window" terminate.
use giochi_editor::{run, EditorOptions};

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
}
