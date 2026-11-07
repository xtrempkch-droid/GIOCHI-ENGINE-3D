//! Resource handles as Lua userdata.
//!
//! The engine's contract is opaque `u64` handles with explicit creation and
//! destruction (R2). Lua gets the same handles wrapped in userdata, because a raw
//! number in a script is impossible to tell apart from a typo:
//!
//! ```lua
//! local player = app:texture("assets/player.png")
//! print(player:width(), player:height())
//! player:destroy()          -- deterministic
//! -- or simply let it be collected; `Texture` frees the handle in `Drop`
//! ```
//!
//! `Texture` can free itself because the texture registry is shared
//! (`RenderContext::shared_textures`), so destruction needs no engine borrow and is
//! safe from a garbage-collection finaliser. `Sound` and `Music` cannot: the mixer is
//! owned by the engine, so those handles live until the app destroys them or the
//! process ends. That asymmetry is documented in `docs/LUA_BINDING.md`.

use std::cell::Cell;
use std::rc::Rc;

use giochi_engine::{MusicHandle, SoundHandle, TextureHandle, TextureRegistry};
use mlua::{UserData, UserDataMethods};

/// A texture owned by the script.
///
/// Destroys the handle when collected, so a script cannot leak one by forgetting;
/// `destroy()` makes the lifetime explicit when a game wants to reclaim memory early.
/// Both are idempotent together: whichever happens first clears the handle.
pub(crate) struct Texture {
    // `Cell` so `destroy()` can take `&self`: the registry is shared, and `&mut`
    // would stop a script from destroying a texture it also passes to `gfx:draw`.
    handle: Cell<TextureHandle>,
    registry: Rc<TextureRegistry>,
}

impl Texture {
    pub(crate) fn new(handle: TextureHandle, registry: Rc<TextureRegistry>) -> Self {
        Self {
            handle: Cell::new(handle),
            registry,
        }
    }

    /// The raw handle, for the drawing functions. `0` once destroyed.
    pub(crate) fn handle(&self) -> TextureHandle {
        self.handle.get()
    }

    /// Releases the handle, reporting whether it was still live.
    fn release(&self) -> bool {
        let handle = self.handle.replace(0);
        handle != 0 && self.registry.destroy_texture(handle)
    }
}

impl Drop for Texture {
    fn drop(&mut self) {
        self.release();
    }
}

impl UserData for Texture {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("width", |_, this, ()| {
            Ok(this
                .registry
                .dimensions(this.handle())
                .map(|(width, _)| width)
                .unwrap_or(0))
        });
        methods.add_method("height", |_, this, ()| {
            Ok(this
                .registry
                .dimensions(this.handle())
                .map(|(_, height)| height)
                .unwrap_or(0))
        });
        methods.add_method("alive", |_, this, ()| {
            let handle = this.handle();
            Ok(handle != 0 && this.registry.contains(handle))
        });
        methods.add_method("destroy", |_, this, ()| Ok(this.release()));
        methods.add_meta_method(mlua::MetaMethod::ToString, |_, this, ()| {
            Ok(format!("Texture({})", this.handle()))
        });
    }
}

/// A fully decoded sound effect.
///
/// No `Drop`: destroying a sound needs the mixer, which the engine owns. Use
/// `app:destroy_sound(sound)`, or let the app release everything on shutdown.
pub(crate) struct Sound {
    handle: SoundHandle,
}

impl Sound {
    pub(crate) fn new(handle: SoundHandle) -> Self {
        Self { handle }
    }

    pub(crate) fn handle(&self) -> SoundHandle {
        self.handle
    }
}

impl UserData for Sound {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(mlua::MetaMethod::ToString, |_, this, ()| {
            Ok(format!("Sound({})", this.handle))
        });
    }
}

/// A streamed music track. Same lifetime rules as [`Sound`].
pub(crate) struct Music {
    handle: MusicHandle,
}

impl Music {
    pub(crate) fn new(handle: MusicHandle) -> Self {
        Self { handle }
    }

    pub(crate) fn handle(&self) -> MusicHandle {
        self.handle
    }
}

impl UserData for Music {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(mlua::MetaMethod::ToString, |_, this, ()| {
            Ok(format!("Music({})", this.handle))
        });
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    #[test]
    fn releasing_a_handle_is_idempotent() {
        // The registry needs a render context, so this covers the `Cell` arithmetic
        // that `Texture::release` relies on; the end-to-end path is in
        // tests/lua_smoke.rs.
        let handle = Cell::new(7u64);
        assert_eq!(handle.replace(0), 7);
        assert_eq!(handle.replace(0), 0);
    }
}
