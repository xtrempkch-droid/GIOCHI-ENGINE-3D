//! Engine lifecycle at the C boundary.
//!
//! ABI v1 exposes one engine instance per thread (ADR-012). Every function here
//! either drives that instance or asks it whether it is still running; none of them
//! hands out a pointer to it.

use core::ffi::c_char;
use std::cell::RefCell;
use std::ffi::CString;

use giochi_engine::error::codes;
use giochi_engine::{Engine, EngineConfig, GiochiError};

use crate::abi_types::GiochiEngineConfig;
use crate::scene;
use crate::state::{self, guard};

/// Frame time assumed by `giochi_engine_step` when the caller passes something
/// unusable.
const DEFAULT_STEP: f32 = 1.0 / 60.0;

thread_local! {
    /// Storage for the string returned by `giochi_engine_video_driver`.
    static DRIVER_NAME: RefCell<CString> = RefCell::new(CString::default());
}

/// Initialises the engine, creating the window, renderer, mixer and input state.
///
/// `config` may be NULL, in which case the defaults are used. Calling it twice
/// without an intervening [`giochi_engine_shutdown`] is an error: there is one
/// engine per process (ADR-012).
///
/// # C ABI
///
/// ```c
/// int32_t giochi_engine_init(const GiochiEngineConfig* config);
/// ```
#[no_mangle]
pub extern "C" fn giochi_engine_init(config: *const GiochiEngineConfig) -> i32 {
    guard(codes::INTERNAL, || {
        if state::in_callback() {
            return state::unavailable("giochi_engine_init cannot be called from a scene callback");
        }
        if state::is_installed() {
            return state::invalid_argument("the engine is already initialised");
        }

        let engine_config = if config.is_null() {
            EngineConfig::default()
        } else {
            // SAFETY: non-NULL pointer to a caller-provided struct; read field by
            // field and never retained.
            unsafe { (*config).to_engine() }
        };

        match Engine::new(engine_config) {
            Ok(engine) => {
                state::install(engine);
                codes::OK
            }
            Err(err) => state::fail(&err),
        }
    })
}

/// Releases the engine and everything it owns.
///
/// Idempotent. Scenes still on the stack receive their `on_exit` first, so they can
/// destroy their handles while the renderer and the mixer still exist.
///
/// # C ABI
///
/// ```c
/// void giochi_engine_shutdown(void);
/// ```
#[no_mangle]
pub extern "C" fn giochi_engine_shutdown() {
    guard((), || {
        if state::in_callback() {
            state::unavailable("giochi_engine_shutdown cannot be called from a scene callback");
            return;
        }

        // Empty the stack while the engine is alive: `on_exit` is queued, not run.
        let _ = state::with_engine_mut("the engine is busy", |engine| engine.scenes_mut().clear());
        // Deliver those exits with no engine borrow held.
        scene::drain_pending_exits();

        state::uninstall();
        state::set_run_in_progress(false);
        state::clear_quit_request();
    })
}

/// Runs the current scene stack until it quits, the window closes, or `Escape` is
/// pressed. Blocks.
///
/// With an empty stack the loop still runs and draws the clear colour, which is what
/// a loading screen does.
///
/// # C ABI
///
/// ```c
/// int32_t giochi_engine_run(void);
/// ```
#[no_mangle]
pub extern "C" fn giochi_engine_run() -> i32 {
    guard(codes::INTERNAL, || run_loop(|engine| engine.run_current()))
}

/// Runs `scene` until it quits.
///
/// The scene receives `on_enter` before its first frame and `on_exit` after its last
/// one, exactly once per call. When the call returns the engine's scene stack is
/// empty again, so the same handle can be run a second time.
///
/// # C ABI
///
/// ```c
/// int32_t giochi_engine_run_scene(GiochiScene scene);
/// ```
#[no_mangle]
pub extern "C" fn giochi_engine_run_scene(scene_handle: giochi_engine::SceneHandle) -> i32 {
    guard(codes::INTERNAL, || {
        if state::in_callback() {
            return state::unavailable(
                "giochi_engine_run_scene cannot be called from a scene callback",
            );
        }
        if state::run_in_progress() {
            return state::unavailable("giochi_engine_run_scene is already running on this thread");
        }

        let entry = match scene::lookup(scene_handle) {
            Ok(entry) => entry,
            Err(code) => return code,
        };
        if scene::set_running(&entry, true) {
            return state::unavailable("this scene is already running on this thread");
        }

        // `on_enter` runs here, outside any engine borrow, so the callback can use
        // the renderer, the mixer and the texture registry.
        let callbacks = entry.borrow().callbacks;
        scene::deliver_on_enter(&callbacks);

        let code = run_loop(|engine| engine.run(scene::adapter(&entry)));

        scene::set_running(&entry, false);
        code
    })
}

/// Advances the engine by exactly one frame.
///
/// `frame_time` is the wall-clock time, in seconds, the simulation should believe
/// elapsed; a non-finite or non-positive value is replaced by `1/60`. Nothing is
/// slept, so the caller owns the pacing.
///
/// # C ABI
///
/// ```c
/// int32_t giochi_engine_step(float frame_time);
/// ```
#[no_mangle]
pub extern "C" fn giochi_engine_step(frame_time: f32) -> i32 {
    guard(codes::INTERNAL, || {
        if state::in_callback() || state::run_in_progress() {
            return state::unavailable("giochi_engine_step cannot be called while the loop runs");
        }

        let frame_time = if frame_time.is_finite() && frame_time > 0.0 {
            frame_time
        } else {
            DEFAULT_STEP
        };

        state::set_run_in_progress(true);
        let result = state::with_engine_mut(
            "giochi_engine_step cannot be called from a scene callback",
            |engine| {
                // Stepping means "advance one frame": a scene that quit must not
                // silently keep the engine stopped.
                if !engine.is_running() {
                    engine.reset_quit();
                }
                engine.step(frame_time)
            },
        );
        state::set_run_in_progress(false);

        let code = match result {
            Ok(Ok(())) => codes::OK,
            Ok(Err(err)) => state::fail(&err),
            Err(code) => code,
        };
        scene::drain_pending_exits();
        code
    })
}

/// Asks the running loop to stop after the current frame.
///
/// Safe to call from inside a callback, which is the point: inside a callback it
/// only sets a flag, and the running scene reports `Quit` at the end of the frame.
///
/// # C ABI
///
/// ```c
/// void giochi_engine_request_quit(void);
/// ```
#[no_mangle]
pub extern "C" fn giochi_engine_request_quit() {
    guard((), || {
        state::request_quit();
        if !state::run_in_progress() && !state::in_callback() {
            // Outside a run the flag would be discarded by the next `reset_quit`, so
            // apply it to the engine too and leave `is_running` coherent.
            let _ = state::with_engine_mut("the engine is busy", |engine| engine.request_quit());
        }
    })
}

/// Returns `1` while the loop is running and `0` otherwise.
///
/// # C ABI
///
/// ```c
/// int32_t giochi_engine_is_running(void);
/// ```
#[no_mangle]
pub extern "C" fn giochi_engine_is_running() -> i32 {
    guard(0, || {
        if state::in_callback() {
            // Inside a callback the engine cell is borrowed; answer from the mirror.
            let running = state::run_in_progress() && !state::quit_requested();
            return i32::from(running);
        }
        match state::with_engine("the engine is busy", |engine| engine.is_running()) {
            Ok(running) => i32::from(running),
            Err(_) => 0,
        }
    })
}

/// Re-arms the loop after a quit, so a later `run` executes again.
///
/// # C ABI
///
/// ```c
/// int32_t giochi_engine_reset_quit(void);
/// ```
#[no_mangle]
pub extern "C" fn giochi_engine_reset_quit() -> i32 {
    guard(codes::INTERNAL, || {
        if state::in_callback() {
            return state::unavailable(
                "giochi_engine_reset_quit cannot be called from a scene callback",
            );
        }
        state::clear_quit_request();
        match state::with_engine_mut("the engine is busy", |engine| engine.reset_quit()) {
            Ok(()) => codes::OK,
            Err(code) => code,
        }
    })
}

/// Wall-clock duration of the last run, in seconds.
///
/// # C ABI
///
/// ```c
/// float giochi_engine_elapsed(void);
/// ```
#[no_mangle]
pub extern "C" fn giochi_engine_elapsed() -> f32 {
    guard(0.0, || {
        state::with_engine("the engine is busy", |engine| engine.run_stats().elapsed).unwrap_or(0.0)
    })
}

/// Frames rendered by the last run, plus every `giochi_engine_step`.
///
/// # C ABI
///
/// ```c
/// uint64_t giochi_engine_frames(void);
/// ```
#[no_mangle]
pub extern "C" fn giochi_engine_frames() -> u64 {
    guard(0, || {
        state::with_engine("the engine is busy", |engine| engine.run_stats().frames).unwrap_or(0)
    })
}

/// Fixed simulation steps executed by the last run, plus every `giochi_engine_step`.
///
/// # C ABI
///
/// ```c
/// uint64_t giochi_engine_steps(void);
/// ```
#[no_mangle]
pub extern "C" fn giochi_engine_steps() -> u64 {
    guard(0, || {
        state::with_engine("the engine is busy", |engine| engine.run_stats().steps).unwrap_or(0)
    })
}

/// Fixed timestep in seconds.
///
/// # C ABI
///
/// ```c
/// float giochi_engine_timestep(void);
/// ```
#[no_mangle]
pub extern "C" fn giochi_engine_timestep() -> f32 {
    guard(0.0, || {
        state::with_engine("the engine is busy", |engine| engine.fixed_timestep()).unwrap_or(0.0)
    })
}

/// NUL-terminated name of the video driver actually in use (`"KMSDRM"`,
/// `"x11"`, `"dummy"`, …).
///
/// SDL2 spells its console backend `KMSDRM` in capitals; compare
/// case-insensitively if you branch on it.
///
/// Owned by the engine, valid until the next call on the same thread, never NULL.
///
/// # C ABI
///
/// ```c
/// const char* giochi_engine_video_driver(void);
/// ```
#[no_mangle]
pub extern "C" fn giochi_engine_video_driver() -> *const c_char {
    guard(core::ptr::null(), || {
        let name = state::with_engine("the engine is busy", |engine| {
            engine.platform().video_driver_name().to_owned()
        })
        .unwrap_or_default();
        DRIVER_NAME.with(|slot| {
            *slot.borrow_mut() = CString::new(name).unwrap_or_default();
            slot.borrow().as_ptr()
        })
    })
}

/// Runs `start` inside an instrumented loop window.
///
/// Arms the loop, runs it, then empties the stack and delivers the deferred
/// `on_exit` callbacks — with the engine borrow released in between, so a callback
/// that destroys handles still works.
fn run_loop(start: impl FnOnce(&mut Engine) -> Result<(), GiochiError>) -> i32 {
    state::set_run_in_progress(true);
    state::clear_quit_request();

    let result = state::with_engine_mut(
        "the engine is busy; a run cannot start from inside a scene callback",
        |engine| {
            // Every run starts armed, so a previous quit does not silently skip it.
            engine.reset_quit();
            start(engine)
        },
    );

    state::set_run_in_progress(false);
    state::clear_quit_request();

    // The stack is emptied while the engine is still alive so a scene's `on_exit`
    // can destroy its handles, then delivered with no borrow held.
    let _ = state::with_engine_mut("the engine is busy", |engine| engine.scenes_mut().clear());
    scene::drain_pending_exits();

    match result {
        Ok(Ok(())) => codes::OK,
        Ok(Err(err)) => state::fail(&err),
        Err(code) => code,
    }
}
