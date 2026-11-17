//! Phase 38's acceptance: what the editor remembers between sessions.
//!
//! Everything here is filesystem-only — no engine, no window — so each `#[test]` runs on its
//! own thread without fighting SDL's singleton.
//!
//! The two questions worth answering are "does the recent list survive a restart" and "after
//! a crash, is there a file to come back to, and does the editor offer it only when it is
//! newer than the scene".

use std::path::PathBuf;
use std::time::Duration;

use giochi_editor::editor::node_edit::NewNodeKind;
use giochi_editor::editor::EditorState;
use giochi_editor::settings::{EditorSettings, RecentKind, SETTINGS_FILE};

fn folder(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "giochi-session-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&path).expect("temp folder");
    path
}

fn state_in(dir: &std::path::Path) -> EditorState {
    let mut state = EditorState::new();
    state.settings_path = dir.join(SETTINGS_FILE);
    state.autosave_dir = dir.join("config");
    state.load_settings();
    state
}

#[test]
fn the_recent_list_survives_a_restart() {
    let dir = folder("recent");
    let scene = dir.join("track.scene");

    let mut first = state_in(&dir);
    first.new_scene();
    first.save_scene_as(&scene).expect("saved");
    assert_eq!(
        first
            .recent(RecentKind::Scene)
            .first()
            .map(|e| e.path.clone()),
        Some(scene.clone())
    );
    drop(first);

    // A new session, same config folder: the list is read back from disk.
    let second = state_in(&dir);
    let entries = second.recent(RecentKind::Scene);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].path, scene);

    // And it can be opened from there.
    let mut third = state_in(&dir);
    third
        .open_scene(&entries[0].path)
        .expect("opened from recent");
    assert_eq!(third.scene_path.as_deref(), Some(scene.as_path()));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn clearing_the_recent_list_is_written_to_disk() {
    let dir = folder("clear");
    let scene = dir.join("a.scene");
    let mut state = state_in(&dir);
    state.new_scene();
    state.save_scene_as(&scene).expect("saved");
    state.clear_recent();
    assert!(state.recent(RecentKind::Scene).is_empty());

    let reloaded = EditorSettings::load(dir.join(SETTINGS_FILE));
    assert!(reloaded.recent.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_crash_leaves_an_autosave_that_the_next_session_offers() {
    let dir = folder("crash");
    let scene = dir.join("work.scene");

    // Session one: save, then keep editing without saving again.
    let mut first = state_in(&dir);
    first.new_scene();
    first.save_scene_as(&scene).expect("saved");
    first.clear_selection();
    first.create_node(NewNodeKind::Sphere);
    let expected = first.scene.as_ref().expect("scene").nodes.len();
    assert!(first.dirty);

    first.autosave.reset(Some(Duration::from_secs(1)));
    let start = std::time::Instant::now();
    assert!(
        !first.tick_autosave(start),
        "the timer starts on the first tick"
    );
    assert!(first.tick_autosave(start + Duration::from_secs(2)));
    let autosave = first.autosave.last_path().expect("autosaved").to_path_buf();
    assert_eq!(autosave, scene.with_file_name("work.scene.autosave"));
    drop(first);

    // Session two: the autosave is newer than the scene, so it is offered, and recovering it
    // brings back the sphere the crash would have lost.
    let mut second = state_in(&dir);
    second.open_scene(&scene).expect("opened");
    assert_eq!(
        second.scene.as_ref().expect("scene").nodes.len(),
        expected - 1,
        "the saved file does not have the sphere"
    );
    assert_eq!(second.find_autosave(), Some(autosave.clone()));
    second.recover_autosave();
    assert_eq!(second.scene.as_ref().expect("scene").nodes.len(), expected);
    assert!(second.dirty, "recovered work is not saved yet");

    // Saving after a recovery writes the scene, and the autosave stops being newer.
    second.save_scene().expect("saved");
    assert!(
        second.find_autosave().is_none(),
        "the scene is now the newer file"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_scene_saved_after_the_autosave_does_not_offer_stale_work() {
    let dir = folder("stale");
    let scene = dir.join("stale.scene");
    let mut state = state_in(&dir);
    state.new_scene();
    state.save_scene_as(&scene).expect("saved");
    state.clear_selection();
    state.create_node(NewNodeKind::Cube);
    state.autosave.reset(Some(Duration::from_secs(1)));
    let start = std::time::Instant::now();
    let _ = state.tick_autosave(start);
    assert!(state.tick_autosave(start + Duration::from_secs(2)));
    assert!(state.find_autosave().is_some());

    // A deliberate save afterwards makes the autosave the older file.
    std::thread::sleep(Duration::from_millis(20));
    state.save_scene().expect("saved");
    assert!(
        state.find_autosave().is_none(),
        "an autosave older than the scene describes work already saved"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn autosave_is_off_when_the_interval_is_zero() {
    let dir = folder("off");
    let scene = dir.join("off.scene");
    let mut state = state_in(&dir);
    state.new_scene();
    state.save_scene_as(&scene).expect("saved");
    state.clear_selection();
    state.create_node(NewNodeKind::Light);
    state.settings.autosave_seconds = 0;
    state.autosave.reset(state.settings.autosave_interval());
    let start = std::time::Instant::now();
    assert!(!state.tick_autosave(start));
    assert!(!state.tick_autosave(start + Duration::from_secs(3600)));
    assert!(state.autosave.last_path().is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_project_lands_in_the_recent_list_as_a_project() {
    let dir = folder("project");
    let mut state = state_in(&dir);
    state
        .create_project(dir.join("workspace"), "racer")
        .expect("created");
    let projects = state.recent(RecentKind::Project);
    assert_eq!(projects.len(), 1);
    assert!(projects[0].path.ends_with("racer"));
    assert!(projects[0].path.join("project.toml").exists());
    let _ = std::fs::remove_dir_all(&dir);
}
