//! The `App` userdata and the `Scene` implementation that calls back into Lua.
//!
//! ```lua
//! local app = giochi.app { title = "mini_game", width = 960, height = 540 }
//! local player = app:texture("assets/player.png")
//! app:run {
//!     update = function(time, input, audio) ... end,
//!     render = function(gfx) ... end,
//! }
//! ```
//!
//! # Why the app owns the engine
//!
//! `App` holds the [`Engine`] by value, so the setup methods (`texture`, `sound`,
//! `set_volume`, …) borrow it exactly like Rust code would, and a script that calls
//! them *during* a run gets a clear "the app is busy" error from `mlua`'s borrow
//! check rather than a panic.
//!
//! The per-frame work does not go through the app at all: [`LuaScene`] receives the
//! renderer, the input snapshot and the mixer from the engine and hands them to the
//! Lua callbacks as scoped capabilities (see [`crate::caps`]). That is ADR-028's rule
//! — a callback may use exactly what the engine lends it — expressed without `unsafe`.

use std::cell::Cell;
use std::rc::Rc;

use giochi_engine::{
    AudioSystem, Engine, EngineConfig, Input, RenderContext, Scene, SceneTransition, Time,
};
use mlua::{Function, Lua, Table, UserData, UserDataMethods};

use crate::caps::{AudioRef, Gfx, InputRef};
use crate::handles::{Music, Sound, Texture};

thread_local! {
    /// Set by `giochi.quit()`, read by the scene's `transition`.
    ///
    /// A thread-local flag instead of a method on `App` on purpose: calling `app:quit()`
    /// from inside a callback would need the engine that `Engine::run` is already
    /// holding.
    static QUIT_REQUESTED: Cell<bool> = const { Cell::new(false) };
}

/// Records a quit request from Lua.
pub(crate) fn request_quit() {
    QUIT_REQUESTED.with(|flag| flag.set(true));
}

fn take_quit_request() -> bool {
    QUIT_REQUESTED.with(|flag| flag.replace(false))
}

fn clear_quit_request() {
    QUIT_REQUESTED.with(|flag| flag.set(false));
}

thread_local! {
    /// Weak marker of the live `App`, so a second `giochi.app` can be refused with a
    /// clear message instead of SDL's "an `EventPump` instance is already alive".
    static APP_ALIVE: std::cell::RefCell<std::rc::Weak<()>> =
        const { std::cell::RefCell::new(std::rc::Weak::new()) };
}

/// Returns `true` when an `App` is still reachable from Lua.
pub(crate) fn app_is_alive() -> bool {
    APP_ALIVE.with(|slot| slot.borrow().upgrade().is_some())
}

/// A running engine, as Lua sees it.
pub(crate) struct App {
    engine: Engine,
    textures: Rc<giochi_engine::TextureRegistry>,
    /// Dropped with the app; `app_is_alive` watches it through a `Weak`.
    _alive: Rc<()>,
}

impl App {
    pub(crate) fn new(engine: Engine) -> Self {
        let textures = engine.render().shared_textures();
        let alive = Rc::new(());
        APP_ALIVE.with(|slot| *slot.borrow_mut() = Rc::downgrade(&alive));
        Self {
            engine,
            textures,
            _alive: alive,
        }
    }
}

impl UserData for App {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        // ---- setup, outside the loop -------------------------------------------
        methods.add_method_mut("texture", |_, this, path: String| {
            let handle = this
                .engine
                .render_mut()
                .load_texture(&path)
                .map_err(crate::to_lua_error)?;
            Ok(Texture::new(handle, Rc::clone(&this.textures)))
        });

        methods.add_method_mut("sound", |_, this, path: String| {
            this.engine
                .audio_mut()
                .load_sound(&path)
                .map(Sound::new)
                .map_err(crate::to_lua_error)
        });

        methods.add_method_mut("music", |_, this, path: String| {
            this.engine
                .audio_mut()
                .load_music(&path)
                .map(Music::new)
                .map_err(crate::to_lua_error)
        });

        methods.add_method_mut("destroy_sound", |_, this, sound: mlua::AnyUserData| {
            let sound = sound.borrow::<Sound>()?;
            Ok(this.engine.audio_mut().destroy_sound(sound.handle()))
        });

        methods.add_method_mut("destroy_music", |_, this, music: mlua::AnyUserData| {
            let music = music.borrow::<Music>()?;
            Ok(this.engine.audio_mut().destroy_music(music.handle()))
        });

        methods.add_method_mut("set_volume", |_, this, volume: i32| {
            this.engine.audio_mut().set_volume(volume);
            Ok(())
        });

        methods.add_method_mut(
            "set_clear_color",
            |_, this, color: crate::values::LuaColor| {
                this.engine.set_clear_color(color.0);
                Ok(())
            },
        );

        // ---- inspection, outside the loop ---------------------------------------
        methods.add_method("size", |_, this, ()| Ok(this.engine.render().size()));
        methods.add_method("video_driver", |_, this, ()| {
            Ok(this.engine.platform().video_driver_name().to_owned())
        });
        methods.add_method("audio_available", |_, this, ()| {
            Ok(this.engine.audio().is_available())
        });
        methods.add_method("volume", |_, this, ()| Ok(this.engine.audio().volume()));
        methods.add_method("frames", |_, this, ()| Ok(this.engine.run_stats().frames));
        methods.add_method("steps", |_, this, ()| Ok(this.engine.run_stats().steps));
        methods.add_method("elapsed", |_, this, ()| Ok(this.engine.run_stats().elapsed));
        methods.add_method("fps", |_, this, ()| Ok(this.engine.run_stats().fps()));
        methods.add_method("texture_count", |_, this, ()| {
            Ok(this.engine.render().texture_count())
        });
        methods.add_method("sound_count", |_, this, ()| {
            Ok(this.engine.audio().sound_count())
        });
        methods.add_method("timestep", |_, this, ()| Ok(this.engine.fixed_timestep()));

        // ---- the loop -----------------------------------------------------------
        methods.add_method_mut("run", |lua, this, options: Table| {
            let update: Option<Function> = options.get("update")?;
            let render: Option<Function> = options.get("render")?;
            let frames: Option<u64> = options.get("frames")?;

            if update.is_none() && render.is_none() {
                return Err(mlua::Error::RuntimeError(
                    "app:run needs at least one of `update` or `render`".into(),
                ));
            }

            let scene = LuaScene::new(lua.clone(), update, render);
            match frames {
                Some(limit) if limit > 0 => this.engine.set_frame_limit(limit),
                _ => this.engine.clear_frame_limit(),
            }

            clear_quit_request();
            let result = this.engine.run(Box::new(scene));

            // A script error is reported after the loop stops, so the window is not
            // left on screen behind an error the host never sees.
            if let Some(error) = take_scene_error() {
                return Err(error);
            }
            result.map_err(crate::to_lua_error)
        });

        methods.add_method_mut(
            "step",
            |lua, this, (frame_time, options): (f32, Option<Table>)| {
                let update: Option<Function> = match options.as_ref() {
                    Some(table) => table.get("update")?,
                    None => None,
                };
                let render: Option<Function> = match options.as_ref() {
                    Some(table) => table.get("render")?,
                    None => None,
                };
                let scene = LuaScene::new(lua.clone(), update, render);
                clear_quit_request();
                this.engine.push_scene(Box::new(scene));
                this.engine.step(frame_time).map_err(crate::to_lua_error)?;
                this.engine.scenes_mut().clear();
                clear_quit_request();
                match take_scene_error() {
                    Some(error) => Err(error),
                    None => Ok(()),
                }
            },
        );

        methods.add_meta_method(mlua::MetaMethod::ToString, |_, this, ()| {
            let (width, height) = this.engine.render().size();
            Ok(format!(
                "App({width}x{height} via {})",
                this.engine.platform().video_driver_name()
            ))
        });
    }
}

thread_local! {
    /// First callback error, carried out of the scene to `app:run`.
    static SCENE_ERROR: std::cell::RefCell<Option<mlua::Error>> = const { std::cell::RefCell::new(None) };
}

fn record_scene_error(error: mlua::Error) {
    SCENE_ERROR.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            *slot = Some(error);
        }
    });
}

fn take_scene_error() -> Option<mlua::Error> {
    SCENE_ERROR.with(|slot| slot.borrow_mut().take())
}

/// The engine scene that forwards to the script's callbacks.
pub(crate) struct LuaScene {
    lua: Lua,
    update: Option<Function>,
    render: Option<Function>,
    /// Set when a callback raised, so the loop stops instead of spinning on a script
    /// that is now broken.
    failed: bool,
    /// The script generation this scene was built for.
    ///
    /// A `.lua` file cannot be patched in place: the running body is the program. When
    /// the generation moves, the loop stops so the host can build a new VM and run the
    /// edited file (see `crate::reload`).
    generation: u32,
}

impl LuaScene {
    pub(crate) fn new(lua: Lua, update: Option<Function>, render: Option<Function>) -> Self {
        Self {
            lua,
            update,
            render,
            failed: false,
            generation: crate::reload::generation(),
        }
    }

    fn call_update(&self, time: Time, input: &Input, audio: &mut AudioSystem) -> mlua::Result<()> {
        let Some(update) = self.update.as_ref() else {
            return Ok(());
        };

        let clock = self.lua.create_table()?;
        clock.set("delta", time.delta)?;
        clock.set("elapsed", time.elapsed)?;
        clock.set("frame", time.frame_count)?;

        self.lua.scope(|scope| {
            let input = scope.create_userdata(InputRef { input })?;
            let audio = scope.create_userdata(AudioRef { audio })?;
            update.call::<()>((clock.clone(), input, audio))
        })
    }

    fn call_render(&self, renderer: &mut RenderContext) -> mlua::Result<()> {
        let Some(render) = self.render.as_ref() else {
            return Ok(());
        };

        self.lua.scope(|scope| {
            let gfx = scope.create_userdata(Gfx { render: renderer })?;
            render.call::<()>(gfx)
        })
    }
}

impl Scene for LuaScene {
    fn update(&mut self, time: Time, input: &Input, audio: &mut AudioSystem) {
        if let Err(error) = self.call_update(time, input, audio) {
            self.failed = true;
            record_scene_error(error);
        }
    }

    fn render(&mut self, renderer: &mut RenderContext) {
        if let Err(error) = self.call_render(renderer) {
            self.failed = true;
            record_scene_error(error);
        }
    }

    fn transition(&mut self) -> SceneTransition {
        // A broken callback must stop the loop: continuing would raise the same error
        // every frame and never return to the host.
        //
        // `giochi.quit()` also stops it; `app:run` clears the flag before the next run.
        if self.failed || take_quit_request() {
            return SceneTransition::Quit;
        }

        // Draining the script watcher here is what makes a reload take effect on the
        // next frame instead of only after the window closes. `??` is not used because
        // the generation only ever moves forward.
        let generation = crate::reload::poll_changes();
        if generation != self.generation {
            crate::reload::request_reload();
            log::info!("script changed; stopping the loop to reload");
            return SceneTransition::Quit;
        }

        SceneTransition::Continue
    }
}

/// Builds the [`EngineConfig`] from a Lua options table.
pub(crate) fn config_from_table(table: &Table) -> mlua::Result<EngineConfig> {
    let width: u32 = table.get::<Option<u32>>("width")?.unwrap_or(960);
    let height: u32 = table.get::<Option<u32>>("height")?.unwrap_or(540);
    let target_fps: u32 = table.get::<Option<u32>>("fps")?.unwrap_or(60);
    let vsync: Option<bool> = table.get("vsync")?;
    let title: Option<String> = table.get("title")?;

    if width == 0 || height == 0 {
        return Err(mlua::Error::RuntimeError(
            "window width and height must be non-zero".into(),
        ));
    }

    let mut config = EngineConfig::new(width, height, title.as_deref().unwrap_or("giochi"));
    config.target_fps = target_fps;
    config.vsync = u32::from(vsync.unwrap_or(true));
    Ok(config)
}
