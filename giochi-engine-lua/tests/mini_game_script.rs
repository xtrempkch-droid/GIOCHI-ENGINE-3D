//! Runs the reference Lua game, `examples/mini_game.lua`, end to end.
//!
//! The script returns a summary table instead of only printing, so this test asserts
//! on the real outcome — score, lives, game over, no leaked texture — without parsing
//! stdout.
//!
//! Each scenario gets a **fresh Lua VM**, because the engine is a singleton per
//! process and its SDL event pump lives as long as the `App` userdata does. Dropping
//! the VM releases it.
//!
//! One test per binary: SDL allows a single `EventPump` per process.

use mlua::Table;

/// The values `mini_game.lua` reports.
#[derive(Debug)]
struct Summary {
    frames: u64,
    steps: u64,
    score: u64,
    lives: u64,
    over: bool,
    textures: u64,
}

/// Runs the script in its own VM with `args` and returns its summary.
fn run_game(args: &[&str]) -> Summary {
    let path = format!("{}/examples/mini_game.lua", env!("CARGO_MANIFEST_DIR"));

    let summary = {
        let lua = giochi_engine_lua::new_vm().expect("vm");

        // `arg` follows the Lua convention: arg[0] is the script, arg[1..] the rest.
        let arg = lua.create_table().expect("arg table");
        arg.set(0, path.clone()).unwrap();
        for (index, value) in args.iter().enumerate() {
            arg.set(index + 1, *value).unwrap();
        }
        lua.globals().set("arg", arg).unwrap();

        let source = std::fs::read_to_string(&path).expect("mini_game.lua must exist");
        let table: Table = lua
            .load(&source)
            .set_name(&path)
            .call(())
            .unwrap_or_else(|error| panic!("mini_game.lua must run: {error}"));

        Summary {
            frames: table.get("frames").expect("frames"),
            steps: table.get("steps").expect("steps"),
            score: table.get("score").expect("score"),
            lives: table.get("lives").expect("lives"),
            over: table.get("over").expect("over"),
            textures: table.get("textures").expect("textures"),
        }
    };

    // The VM is gone here, so the engine and its event pump are released before the
    // next scenario creates one.
    summary
}

#[test]
fn the_reference_game_plays_and_ends() {
    std::env::set_var("SDL_VIDEODRIVER", "dummy");

    // ---- the autopilot plays: it must score, and leak nothing -----------------
    let summary = run_game(&["--auto", "--frames", "300"]);

    assert_eq!(summary.frames, 300, "the frame limit must be honoured");
    assert!(summary.steps > 0, "the simulation must have advanced");
    assert!(
        summary.score > 0,
        "the autopilot must shoot and destroy drones: score is {}",
        summary.score
    );
    assert!(
        !summary.over,
        "three lives must survive 300 autopiloted frames"
    );
    assert_eq!(summary.lives, 3);
    assert_eq!(
        summary.textures, 0,
        "every texture must be released before the script returns"
    );

    // ---- ramming reaches the collision and game-over paths -------------------
    // A fast drone and a dense spawn table reach the ship within a second, so the
    // collision and game-over paths do not cost ten seconds of wall clock.
    let summary = run_game(&[
        "--auto", "--ram", "--lives", "1", "--spawn", "0.3", "--speed", "400", "--frames", "300",
    ]);

    assert_eq!(summary.lives, 0, "the ship must have been destroyed");
    assert!(summary.over, "reaching zero lives must end the game");
    assert!(
        summary.frames < 300,
        "game over must stop the loop before the frame limit: {} frames",
        summary.frames
    );
    assert_eq!(summary.textures, 0);

    // ---- no autopilot and no input is survivable ----------------------------
    let summary = run_game(&["--frames", "60"]);

    assert_eq!(summary.score, 0, "without input the ship cannot shoot");
    assert_eq!(summary.lives, 3, "an idle ship is not hit directly");
    assert_eq!(summary.textures, 0);
}
