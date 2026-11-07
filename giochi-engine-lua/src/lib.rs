//! # giochi-engine-lua
//!
//! Lua 5.4 binding for GIOCHI-ENGINE — the project's **reference binding** (see
//! `docs/DECISIONS.md`, ADR-004).
//!
//! Phase 0 provides the smoke helper that proves the vendored Lua runtime links
//! and starts. The `giochi` module, its API and the `mini_game.lua` example are
//! delivered in phase 8, documented in `docs/LUA_BINDING.md`.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// Creates a Lua VM with every safe standard library loaded.
///
/// This is the phase-0 link check for the vendored Lua 5.4 runtime. Errors are
/// returned as plain strings so the helper stays usable before the engine error
/// mapping of phase 8 exists.
///
/// The `debug` library is excluded on purpose: `mlua` refuses to load it in safe
/// mode, and the engine never needs it.
pub fn new_vm() -> Result<mlua::Lua, String> {
    mlua::Lua::new_with(mlua::StdLib::ALL_SAFE, mlua::LuaOptions::default())
        .map_err(|error| error.to_string())
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
}
