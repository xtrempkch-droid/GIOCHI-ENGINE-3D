//! # giochi-engine-lua
//!
//! Lua 5.4 binding for GIOCHI-ENGINE — the project's **reference binding**
//! (`docs/DECISIONS.md`, ADR-004) and the proof that the C-shaped core is consumable
//! from a scripting language without touching the engine.
//!
//! ```lua
//! local giochi = require("giochi")
//!
//! local app = giochi.app { title = "my game", width = 960, height = 540 }
//! local player = app:texture("assets/player.png")
//!
//! local x, y = 100, 100
//!
//! app:run {
//!     update = function(time, input, audio)
//!         if input:key_down(giochi.keys.d) then x = x + 200 * time.delta end
//!         if input:key_down(giochi.keys.a) then x = x - 200 * time.delta end
//!     end,
//!     render = function(gfx)
//!         gfx:draw(player, { x, y, 64, 64 })
//!     end,
//! }
//! ```
//!
//! # The three rules of the binding
//!
//! 1. **A callback only sees what it needs.** `update` receives a clock, the input
//!    snapshot and the mixer; `render` receives the renderer. Nothing else is
//!    reachable from inside a frame, so a script cannot draw from `update` or read
//!    input from `render` by accident — and the binding needs no `unsafe`
//!    (`#![forbid(unsafe_code)]`), because `mlua`'s scoped userdata invalidates the
//!    capabilities when the callback returns.
//! 2. **Handles are still handles.** `Texture`, `Sound` and `Music` wrap the engine's
//!    `u64` ids. `Texture` frees its handle when collected *and* on `destroy()`.
//! 3. **Errors end the run.** A Lua error inside a callback stops the loop and is
//!    re-raised by `app:run`, so a broken script never turns into a silent window.
//!
//! The full reference — every function, every field, and the reasoning — is
//! `docs/LUA_BINDING.md`.
//!
//! # Running a script
//!
//! ```bash
//! cargo run -p giochi-demo -- lua giochi-engine-lua/examples/mini_game.lua
//! ```
#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod app;
mod caps;
mod handles;
mod keys;
mod values;

use std::path::Path;

use giochi_engine::GiochiError;
use mlua::{Lua, Table};

/// Semantic version of the binding, as declared in `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The name a script uses to reach the module: `require("giochi")`.
pub const MODULE_NAME: &str = "giochi";

/// Creates a Lua VM with every safe standard library loaded.
///
/// The `debug` library is excluded on purpose: `mlua` refuses to load it in safe
/// mode, and the engine never needs it.
pub fn new_vm() -> Result<Lua, String> {
    let lua = Lua::new_with(mlua::StdLib::ALL_SAFE, mlua::LuaOptions::default())
        .map_err(|error| error.to_string())?;
    register(&lua).map_err(|error| error.to_string())?;
    Ok(lua)
}

/// Installs the `giochi` module, registers it for `require` and returns it.
///
/// Also exposed as `<lua> giochi` for a script that would rather not `require` it.
pub fn register(lua: &Lua) -> mlua::Result<Table> {
    let module = lua.create_table()?;

    module.set("version", VERSION)?;
    module.set("module_version", giochi_engine::VERSION)?;
    module.set("abi_version", giochi_engine::ABI_VERSION)?;

    let keys = lua.create_table()?;
    for (name, code) in keys::keys() {
        keys.set(name, code)?;
    }
    module.set("keys", keys)?;

    let mouse = lua.create_table()?;
    for (name, code) in keys::mouse_buttons() {
        mouse.set(name, code)?;
    }
    module.set("mouse", mouse)?;

    let axes = lua.create_table()?;
    for (name, code) in keys::axes() {
        axes.set(name, code)?;
    }
    module.set("axes", axes)?;

    let buttons = lua.create_table()?;
    for (name, code) in keys::buttons() {
        buttons.set(name, code)?;
    }
    module.set("buttons", buttons)?;

    module.set("app", lua.create_function(create_app)?)?;
    module.set(
        "quit",
        lua.create_function(|_, ()| {
            app::request_quit();
            Ok(())
        })?,
    )?;

    // `require("giochi")` must find it, and `giochi.app` must work without `require`.
    let package: Table = lua.globals().get("package")?;
    let loaded: Table = package.get("loaded")?;
    loaded.set(MODULE_NAME, module.clone())?;
    lua.globals().set(MODULE_NAME, module.clone())?;

    Ok(module)
}

/// `giochi.app { title =, width =, height =, fps =, vsync = }` — creates the engine.
fn create_app(lua: &Lua, options: Option<Table>) -> mlua::Result<mlua::AnyUserData> {
    if app::app_is_alive() {
        return Err(mlua::Error::RuntimeError(
            "an app already exists in this process: the engine is a singleton, so drop the \
             previous one (set it to nil and call collectgarbage()) before creating another"
                .into(),
        ));
    }

    let config = match options {
        Some(table) => app::config_from_table(&table)?,
        None => giochi_engine::EngineConfig::default(),
    };

    let engine = giochi_engine::Engine::new(config).map_err(to_lua_error)?;
    lua.create_userdata(app::App::new(engine))
}

/// Runs a `.lua` file with `args` exposed as the global `arg` table.
///
/// `arg[0]` is the script path and `arg[1..]` the arguments, which is the convention
/// every Lua program expects.
pub fn run_file(lua: &Lua, path: impl AsRef<Path>, args: &[String]) -> mlua::Result<()> {
    let path = path.as_ref();
    let display = path.display().to_string();

    let arg = lua.create_table()?;
    arg.set(0, display.clone())?;
    for (index, value) in args.iter().enumerate() {
        arg.set(index + 1, value.clone())?;
    }
    lua.globals().set("arg", arg)?;

    let source = std::fs::read_to_string(path)
        .map_err(|error| mlua::Error::RuntimeError(format!("cannot read '{display}': {error}")))?;

    lua.load(&source).set_name(&display).exec()
}

/// Converts an engine error into a Lua error, keeping the numeric code visible.
pub(crate) fn to_lua_error(error: GiochiError) -> mlua::Error {
    let code = match &error {
        GiochiError::InvalidHandle(_) => giochi_engine::codes::INVALID_HANDLE,
        GiochiError::InvalidArgument(_) => giochi_engine::codes::INVALID_ARGUMENT,
        GiochiError::NotInitialized => giochi_engine::codes::NOT_INITIALIZED,
        GiochiError::Platform(_) => giochi_engine::codes::PLATFORM,
        GiochiError::Render(_) => giochi_engine::codes::RENDER,
        GiochiError::Asset(_) => giochi_engine::codes::ASSET,
        GiochiError::Audio(_) => giochi_engine::codes::AUDIO,
        GiochiError::Scene(_) => giochi_engine::codes::SCENE,
        GiochiError::Unsupported(_) => giochi_engine::codes::UNSUPPORTED,
        _ => giochi_engine::codes::INTERNAL,
    };
    mlua::Error::RuntimeError(format!("{error} (giochi error {code})"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendored_lua_starts() {
        let lua = new_vm().expect("vendored Lua 5.4 must start");
        let result: i32 = lua
            .load("return 1 + 1")
            .eval()
            .expect("Lua must evaluate arithmetic");
        assert_eq!(result, 2);
    }

    #[test]
    fn the_module_is_reachable_both_ways() {
        let lua = new_vm().unwrap();
        let required: Table = lua.load("return require('giochi')").eval().unwrap();
        let global: Table = lua.load("return giochi").eval().unwrap();
        assert_eq!(
            required.get::<String>("version").unwrap(),
            global.get::<String>("version").unwrap()
        );
        assert_eq!(required.get::<String>("version").unwrap(), VERSION);
    }

    #[test]
    fn key_tables_are_exposed_to_lua() {
        let lua = new_vm().unwrap();
        let space: u32 = lua
            .load("return giochi.keys.space")
            .eval()
            .expect("keys.space must exist");
        assert_eq!(space, giochi_engine::input::keyboard::KEY_SPACE);

        let left_mouse: u32 = lua.load("return giochi.mouse.left").eval().unwrap();
        assert_eq!(left_mouse, giochi_engine::input::mouse::MOUSE_LEFT);

        let axis: u32 = lua.load("return giochi.axes.left_x").eval().unwrap();
        assert_eq!(axis, giochi_engine::input::gamepad::AXIS_LEFT_X);

        let button: u32 = lua.load("return giochi.buttons.south").eval().unwrap();
        assert_eq!(button, giochi_engine::input::gamepad::BUTTON_SOUTH);
    }

    #[test]
    fn an_unknown_key_is_nil_not_an_error() {
        let lua = new_vm().unwrap();
        let missing: Option<u32> = lua.load("return giochi.keys.nonexistent").eval().unwrap();
        assert_eq!(missing, None);
    }

    #[test]
    fn quit_is_a_plain_function() {
        let lua = new_vm().unwrap();
        lua.load("giochi.quit()").exec().unwrap();
    }

    #[test]
    fn engine_errors_become_lua_errors_with_their_code() {
        let error = to_lua_error(GiochiError::InvalidArgument("nope"));
        let text = error.to_string();
        assert!(text.contains("nope"), "{text}");
        assert!(text.contains("-2"), "{text}");
    }
}
