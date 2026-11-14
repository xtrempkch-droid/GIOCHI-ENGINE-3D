//! Hot reload of a Lua script: the file changes, the generation moves.
//!
//! The scene stops the loop when the generation it started with no longer matches
//! (`LuaScene::transition`), and the host restarts the VM. These tests prove the two ends
//! of that contract — the watcher notices, and the flag survives exactly one read — so a
//! regression cannot leave the host in a restart loop or in a run that never reloads.

use std::path::PathBuf;
use std::time::{Duration, Instant};

fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "giochi-lua-reload-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

#[test]
fn a_saved_script_restarts_and_a_deleted_one_does_not() {
    // One test, not two: the watcher and the generation are process-wide, and cargo runs
    // the tests of one binary in parallel threads — two tests here would observe each
    // other's scripts.
    let dir = temp_dir();

    // ---- a saved script asks for a restart --------------------------------
    let script = dir.join("game.lua");
    std::fs::write(&script, b"-- version 1\n").expect("write script");

    giochi_engine_lua::watch_script(&script).expect("the watch must install");
    let watched = giochi_engine_lua::watched_script_paths();
    assert_eq!(watched.len(), 1, "one file is watched");
    assert!(
        watched[0].is_absolute(),
        "the watcher reports canonical paths, got {:?}",
        watched[0]
    );

    let before = giochi_engine_lua::script_generation();
    let _ = giochi_engine_lua::take_reload_request();

    std::fs::write(&script, b"-- version 2\n").expect("edit script");

    let deadline = Instant::now() + Duration::from_secs(3);
    let mut after = before;
    while Instant::now() < deadline && after == before {
        after = giochi_engine_lua::poll_script_changes();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_ne!(
        after, before,
        "a saved script must advance the generation within three seconds"
    );

    // The scene is what asks for a restart, once it sees the generation move.
    giochi_engine_lua::request_script_reload();
    assert!(
        giochi_engine_lua::take_reload_request(),
        "the host must learn why the loop stopped"
    );
    assert!(
        !giochi_engine_lua::take_reload_request(),
        "the request must not survive a second read, or the host would loop forever"
    );

    // ---- a deleted script cannot be restarted ----------------------------
    let before_delete = giochi_engine_lua::script_generation();
    std::fs::remove_file(&script).expect("remove script");

    // Long enough for the backend to report the deletion and for the debounce to close.
    let deadline = Instant::now() + Duration::from_millis(600);
    while Instant::now() < deadline {
        let _ = giochi_engine_lua::poll_script_changes();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        giochi_engine_lua::script_generation(),
        before_delete,
        "a deleted script cannot be restarted, so it must not ask for one"
    );

    // ---- and the watch can be dropped ------------------------------------
    giochi_engine_lua::unwatch_script();
    assert!(giochi_engine_lua::watched_script_paths().is_empty());

    let _ = std::fs::remove_dir_all(&dir);
}
