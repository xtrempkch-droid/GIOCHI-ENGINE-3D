//! The per-frame capabilities: what a Lua callback is allowed to touch.
//!
//! These are created fresh for the duration of one callback and passed as arguments,
//! so a script cannot hold on to the renderer, the input snapshot or the mixer:
//!
//! ```lua
//! app:run {
//!   update = function(time, input, audio) ... end,
//!   render = function(gfx) ... end,
//! }
//! ```
//!
//! The design mirrors ADR-028 for the C ABI — *a callback may use exactly what the
//! engine lends it* — but without a single `unsafe` block: each capability borrows
//! the reference the engine passed to [`Scene::update`]/[`Scene::render`], and
//! `mlua`'s scope mechanism invalidates the userdata when the callback returns.
//! Storing one in a global is possible, but calling it afterwards raises a Lua error
//! instead of touching freed memory.
//!
//! [`Scene::update`]: giochi_engine::Scene::update
//! [`Scene::render`]: giochi_engine::Scene::render

use giochi_engine::render::Sprite;
use giochi_engine::{AudioSystem, Camera, Input, Rect, RenderContext};
use mlua::FromLua;
use mlua::{UserData, UserDataMethods};

use crate::handles::Texture;
use crate::values::{LuaColor, LuaRect};

/// The renderer, available inside `render`.
///
/// The engine clears the screen before the callback and presents after it, so a
/// script only issues draw calls.
pub(crate) struct Gfx<'a> {
    pub(crate) render: &'a mut RenderContext,
}

impl UserData for Gfx<'_> {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("size", |_, this, ()| Ok(this.render.size()));

        methods.add_method_mut("clear", |_, this, color: LuaColor| {
            this.render.clear(color.0);
            Ok(())
        });

        // `gfx:rect { x, y, w, h, color = "#f00" }` or `gfx:rect { x, y, w, h }, color`.
        methods.add_method_mut("rect", |lua, this, args: mlua::MultiValue| {
            let (rect, color) = rect_and_color(lua, args)?;
            this.render.draw_rect(rect, color);
            Ok(())
        });

        methods.add_method_mut(
            "draw",
            |_, this, (texture, dst, src): (mlua::AnyUserData, LuaRect, Option<LuaRect>)| {
                let texture = texture.borrow::<Texture>()?;
                let src = src.map_or(Rect::ZERO, |rect| rect.0);
                this.render
                    .draw_texture(texture.handle(), dst.0, src)
                    .map_err(crate::to_lua_error)
            },
        );

        methods.add_method_mut(
            "sprite",
            |_, this, (texture, rect): (mlua::AnyUserData, LuaRect)| {
                let texture = texture.borrow::<Texture>()?;
                let sprite = Sprite::new(texture.handle(), rect.0.x, rect.0.y, rect.0.w, rect.0.h);
                this.render
                    .draw_sprite(&sprite)
                    .map_err(crate::to_lua_error)
            },
        );

        // `gfx:camera{ x =, y =, zoom = }` sets; `gfx:camera()` returns a table.
        methods.add_method_mut(
            "camera",
            |lua, this, camera: Option<LuaCamera>| match camera {
                Some(camera) => {
                    this.render.set_camera(camera.0);
                    Ok(mlua::Value::Nil)
                }
                None => {
                    let camera = this.render.camera();
                    let table = lua.create_table()?;
                    table.set("x", camera.x)?;
                    table.set("y", camera.y)?;
                    table.set("zoom", camera.zoom)?;
                    Ok(mlua::Value::Table(table))
                }
            },
        );

        methods.add_method("screen", |lua, this, world: LuaPoint| {
            let screen = this.render.world_to_screen(world.0);
            let table = lua.create_table()?;
            table.set("x", screen.x)?;
            table.set("y", screen.y)?;
            Ok(table)
        });

        // Creating a texture here is legal because the registry is shared; caching is
        // deliberately not used, so every `Texture` owns exactly one handle.
        methods.add_method("texture", |_, this, path: String| {
            let handle = this
                .render
                .textures()
                .create_texture(&path)
                .map_err(crate::to_lua_error)?;
            Ok(Texture::new(handle, this.render.shared_textures()))
        });
    }
}

/// Reads a rectangle and a colour from either one table or two arguments.
///
/// The one-table form is what a script writes first (`gfx:rect { x, y, w, h }`), so it
/// must not silently draw nothing: a missing colour is an error naming both spellings.
fn rect_and_color(
    lua: &mlua::Lua,
    args: mlua::MultiValue,
) -> mlua::Result<(Rect, giochi_engine::Color)> {
    let mut values = args.into_iter();
    let rect_value = values.next().ok_or_else(|| {
        mlua::Error::RuntimeError("gfx:rect needs a rectangle, e.g. { x, y, w, h }".into())
    })?;

    let rect = LuaRect::from_lua(rect_value.clone(), lua)?.0;

    if let Some(color) = values.next() {
        if !matches!(color, mlua::Value::Nil) {
            return Ok((rect, LuaColor::from_lua(color, lua)?.0));
        }
    }

    // Fall back to a `color` field inside the rectangle table.
    if let mlua::Value::Table(table) = rect_value {
        if let Some(color) = table.get::<Option<mlua::Value>>("color")? {
            return Ok((rect, LuaColor::from_lua(color, lua)?.0));
        }
        // Or the r/g/b fields inline, so `{ x =, y =, w =, h =, r =, g =, b = }` works.
        if table.contains_key("r")? || table.contains_key("g")? || table.contains_key("b")? {
            return Ok((rect, color_from_components(&table)?));
        }
    }

    Err(mlua::Error::RuntimeError(
        "gfx:rect needs a colour: pass it as a second argument or add `color = ...`".into(),
    ))
}

/// Builds a colour from the `r`/`g`/`b`/`a` fields of `table`.
fn color_from_components(table: &mlua::Table) -> mlua::Result<giochi_engine::Color> {
    let component = |name: &str, default: u8| -> mlua::Result<u8> {
        match table.get::<Option<f64>>(name)? {
            Some(value) if value.is_finite() => Ok(value.round().clamp(0.0, 255.0) as u8),
            Some(_) => Err(mlua::Error::RuntimeError(format!(
                "colour component '{name}' must be finite"
            ))),
            None => Ok(default),
        }
    };
    Ok(giochi_engine::Color {
        r: component("r", 255)?,
        g: component("g", 255)?,
        b: component("b", 255)?,
        a: component("a", 255)?,
        _reserved: [0; 4],
    })
}

/// A camera position.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct LuaCamera(pub(crate) Camera);

/// A world-space point.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct LuaPoint(pub(crate) giochi_engine::Vec2);

impl mlua::FromLua for LuaCamera {
    fn from_lua(value: mlua::Value, _lua: &mlua::Lua) -> mlua::Result<Self> {
        let table = match value {
            mlua::Value::Table(table) => table,
            other => {
                return Err(mlua::Error::FromLuaConversionError {
                    from: other.type_name(),
                    to: "GiochiCamera".to_owned(),
                    message: Some("expected a table { x =, y =, zoom = }".to_owned()),
                })
            }
        };
        let x: f32 = table.get::<Option<f32>>("x")?.unwrap_or(0.0);
        let y: f32 = table.get::<Option<f32>>("y")?.unwrap_or(0.0);
        let zoom: f32 = table.get::<Option<f32>>("zoom")?.unwrap_or(1.0);

        // The viewport is the whole window: a script never sees the render size
        // outside `render`, so it cannot fill this in itself.
        let viewport = Rect::new(0.0, 0.0, 1.0, 1.0);
        let mut camera = Camera::new(viewport);
        camera.x = x;
        camera.y = y;
        camera.zoom = zoom;
        Ok(LuaCamera(camera))
    }
}

impl mlua::FromLua for LuaPoint {
    fn from_lua(value: mlua::Value, _lua: &mlua::Lua) -> mlua::Result<Self> {
        let table = match value {
            mlua::Value::Table(table) => table,
            other => {
                return Err(mlua::Error::FromLuaConversionError {
                    from: other.type_name(),
                    to: "GiochiVec2".to_owned(),
                    message: Some("expected a table { x =, y = }".to_owned()),
                })
            }
        };
        let x: f32 = table.get::<Option<f32>>("x")?.unwrap_or(0.0);
        let y: f32 = table.get::<Option<f32>>("y")?.unwrap_or(0.0);
        Ok(LuaPoint(giochi_engine::Vec2::new(x, y)))
    }
}

/// The input snapshot, available inside `update`.
pub(crate) struct InputRef<'a> {
    pub(crate) input: &'a Input,
}

impl UserData for InputRef<'_> {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("key_down", |_, this, key: u32| Ok(this.input.key_down(key)));
        methods.add_method("key_pressed", |_, this, key: u32| {
            Ok(this.input.key_pressed(key))
        });
        methods.add_method("key_released", |_, this, key: u32| {
            Ok(this.input.key_released(key))
        });
        methods.add_method("mouse", |_, this, ()| {
            let (x, y) = this.input.mouse().position();
            Ok((x, y))
        });
        methods.add_method("mouse_down", |_, this, button: u32| {
            Ok(this.input.mouse_down(button))
        });
        methods.add_method("wheel", |_, this, ()| Ok(this.input.mouse().wheel_delta()));
        methods.add_method("gamepad_connected", |_, this, ()| {
            Ok(this.input.gamepad().is_connected())
        });
        methods.add_method("axis", |_, this, axis: u32| {
            Ok(this.input.gamepad().axis(axis))
        });
        methods.add_method("button_down", |_, this, button: u32| {
            Ok(this.input.gamepad().is_down(button))
        });
    }
}

/// The mixer, available inside `update` and in the app's setup calls.
pub(crate) struct AudioRef<'a> {
    pub(crate) audio: &'a mut AudioSystem,
}

impl UserData for AudioRef<'_> {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("available", |_, this, ()| Ok(this.audio.is_available()));

        methods.add_method_mut(
            "play",
            |_, this, (sound, loops): (mlua::AnyUserData, Option<i32>)| {
                let sound = sound.borrow::<crate::handles::Sound>()?;
                this.audio
                    .play_sound(sound.handle(), loops.unwrap_or(0))
                    .map_err(crate::to_lua_error)
            },
        );

        methods.add_method_mut("set_volume", |_, this, volume: i32| {
            this.audio.set_volume(volume);
            Ok(())
        });
        methods.add_method("volume", |_, this, ()| Ok(this.audio.volume()));

        methods.add_method_mut(
            "music_play",
            |_, this, (music, loops): (mlua::AnyUserData, Option<i32>)| {
                let music = music.borrow::<crate::handles::Music>()?;
                this.audio
                    .play_music(music.handle(), loops.unwrap_or(-1))
                    .map_err(crate::to_lua_error)
            },
        );
        methods.add_method_mut("music_pause", |_, this, ()| {
            this.audio.pause_music();
            Ok(())
        });
        methods.add_method_mut("music_resume", |_, this, ()| {
            this.audio.resume_music();
            Ok(())
        });
        methods.add_method_mut("music_stop", |_, this, ()| {
            this.audio.stop_music();
            Ok(())
        });
        methods.add_method("music_playing", |_, this, ()| {
            Ok(this.audio.is_music_playing())
        });
    }
}
