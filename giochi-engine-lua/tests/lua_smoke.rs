//! Headless integration test for the Lua binding.
//!
//! Everything goes through the Lua side: `giochi.app`, textures, the frame
//! callbacks, `giochi.quit()`, error propagation and teardown. A mistake in the
//! userdata wiring fails here rather than in a game.
//!
//! One test per binary: SDL allows a single `EventPump` per process.

use giochi_engine_lua::{new_vm, run_file};

/// Runs `source` with the global `ASSETS` (the assets directory, with a trailing
/// slash) defined, and returns the global `result` the script left behind.
fn eval(source: &str) -> Result<String, String> {
    let lua = new_vm()?;
    let assets = format!("{}/../assets/", env!("CARGO_MANIFEST_DIR"));
    lua.globals()
        .set("ASSETS", assets)
        .map_err(|e| e.to_string())?;
    lua.load(source)
        .set_name("test")
        .exec()
        .map_err(|error| error.to_string())?;
    let result: Option<String> = lua.globals().get("result").map_err(|e| e.to_string())?;
    Ok(result.unwrap_or_default())
}

#[test]
fn lua_binding_end_to_end() {
    std::env::set_var("SDL_VIDEODRIVER", "dummy");

    // ---- the app, the loop and the callbacks ---------------------------------
    let report = eval(
        r##"
        local giochi = require("giochi")

        local app = giochi.app { title = "lua smoke", width = 320, height = 240, vsync = false }
        local width, height = app:size()
        assert(width == 320 and height == 240)

        local player = app:texture(ASSETS .. "player.png")
        assert(player:width() == 32 and player:height() == 32)
        assert(tostring(player):match("^Texture%(%d+%)$"))

        local updates, renders, world_renders = 0, 0, 0
        local saw_input, saw_audio, saw_gfx = false, false, false

        app:run {
            frames = 6,
            update = function(time, input, audio)
                updates = updates + 1
                saw_input = saw_input or (type(input.key_down) == "function")
                saw_audio = saw_audio or (type(audio.set_volume) == "function")
                assert(type(time.delta) == "number" and time.delta > 0)
                assert(type(time.elapsed) == "number")
                assert(input:key_down(424242) == false)   -- unmapped: simply not down
                input:mouse()
                input:wheel()
                input:gamepad_connected()
            end,
            render = function(gfx)
                renders = renders + 1
                world_renders = world_renders + 1
                saw_gfx = true
                gfx:clear { 12, 14, 22 }
                gfx:clear "#0c0e16"
                gfx:rect({ 10, 10, 40, 20 }, { 230, 70, 70 })
                gfx:rect { x = 60, y = 10, w = 20, h = 20, color = "#28a05a" }
                gfx:rect { x = 90, y = 10, w = 20, h = 20, r = 40, g = 160, b = 90 }
                gfx:draw(player, { x = 5, y = 5, w = 32, h = 32 })
                gfx:sprite(player, { 100, 100, 16, 16 })
                gfx:camera { x = 0, y = 0, zoom = 2 }
                assert(gfx:camera().zoom == 2)
                assert(type(gfx:screen { x = 0, y = 0 }.x) == "number")
                local w, h = gfx:size()
                assert(w == 320 and h == 240)
            end,
        }

        local frames, steps = app:frames(), app:steps()
        assert(steps > 0)

        -- A texture dies on destroy() and reports the second attempt.
        assert(player:destroy() == true)
        assert(player:destroy() == false)
        assert(player:alive() == false)
        assert(player:width() == 0)

        -- ...and on collection, which is what makes forgetting harmless.
        do
            local collected = app:texture(ASSETS .. "checker.png")
            assert(collected:width() == 64)
        end
        collectgarbage("collect")
        collectgarbage("collect")
        local live = app:texture_count()

        result = string.format("%d/%d/%d/%d/%s/%s/%s/%d", frames, updates, renders,
                               world_renders, tostring(saw_input), tostring(saw_audio),
                               tostring(saw_gfx), live)
        "##,
    )
    .expect("the script must run");

    let parts: Vec<&str> = report.split('/').collect();
    assert_eq!(parts.len(), 8, "unexpected report: {report}");
    assert_eq!(
        parts[0], "6",
        "the frame limit must stop the loop after 6 frames"
    );
    assert!(parts[1].parse::<u32>().unwrap() > 0, "update must have run");
    assert!(parts[2].parse::<u32>().unwrap() > 0, "render must have run");
    assert_eq!(parts[4], "true", "update must receive the input capability");
    assert_eq!(parts[5], "true", "update must receive the audio capability");
    assert_eq!(parts[6], "true", "render must receive the renderer");
    assert_eq!(
        parts[7], "0",
        "no texture may survive: destroyed explicitly or collected"
    );

    // ---- a script error stops the run and is re-raised ------------------------
    let error = eval(
        r##"
        local giochi = require("giochi")
        local app = giochi.app { width = 160, height = 120, vsync = false }
        app:run {
            update = function() error("boom from lua") end,
            render = function(gfx) gfx:clear { 0, 0, 0 } end,
        }
        "##,
    )
    .expect_err("a Lua error inside update must abort the run");
    assert!(
        error.contains("boom from lua"),
        "the error must reach the host: {error}"
    );

    // ---- giochi.quit() ends the run early -----------------------------------
    let quit = eval(
        r##"
        local giochi = require("giochi")
        local app = giochi.app { width = 160, height = 120, vsync = false }
        local count = 0
        app:run {
            frames = 500,
            update = function() count = count + 1; if count >= 3 then giochi.quit() end end,
            render = function() end,
        }
        -- The number of frames is not fixed: the first frame may run zero fixed
        -- steps, so `count` can reach 3 on the fourth frame.
        result = tostring(app:frames())
        "##,
    )
    .expect("quit must stop the loop");
    let frames: u32 = quit.parse().expect("frames must be a number");
    assert!(
        (3..20).contains(&frames),
        "quit() must end the run long before the 500-frame limit, got {frames}"
    );

    // ---- a render-only scene is legal ---------------------------------------
    let frames = eval(
        r##"
        local giochi = require("giochi")
        local app = giochi.app { width = 160, height = 120, vsync = false }
        app:run { frames = 3, render = function(gfx) gfx:clear { 0, 0, 0 } end }
        result = tostring(app:frames())
        "##,
    )
    .expect("render-only scenes are legal");
    assert_eq!(frames, "3");

    // ---- app:step drives single frames --------------------------------------
    let stepped = eval(
        r##"
        local giochi = require("giochi")
        local app = giochi.app { width = 160, height = 120, vsync = false }
        local before = app:frames()
        app:step(1 / 60, { update = function() end })
        app:step(1 / 60, {})
        result = tostring(app:frames() - before)
        "##,
    )
    .expect("step must run one frame");
    assert_eq!(stepped, "2");

    // ---- setup errors are reported, not swallowed ---------------------------
    let error = eval(
        r##"
        local giochi = require("giochi")
        local app = giochi.app { width = 160, height = 120, vsync = false }
        app:texture(ASSETS .. "does-not-exist.png")
        "##,
    )
    .expect_err("a missing asset must raise");
    assert!(error.contains("does-not-exist"), "{error}");

    let error = eval(
        r##"
        local giochi = require("giochi")
        giochi.app { width = 0, height = 0 }
        "##,
    )
    .expect_err("a zero-sized window must raise");
    assert!(error.contains("non-zero"), "{error}");

    // ---- a real script file, with `arg` wired up ---------------------------
    let script = std::env::temp_dir().join("giochi_lua_binding_test.lua");
    std::fs::write(
        &script,
        r##"
        local giochi = require("giochi")
        assert(arg[0]:match("giochi_lua_binding_test%.lua$"))
        assert(arg[1] == "--auto")
        assert(arg[2] == "7")
        local app = giochi.app { width = 128, height = 96, vsync = false }
        app:run { frames = tonumber(arg[2]), render = function() end }
        print("frames=" .. app:frames())
        "##,
    )
    .unwrap();

    let lua = new_vm().expect("vm");
    let args = vec!["--auto".to_owned(), "7".to_owned()];
    run_file(&lua, &script, &args).expect("the file must run");
    let _ = std::fs::remove_file(&script);
}
