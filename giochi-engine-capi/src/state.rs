//! Process-wide state: the engine singleton, the thread-local error slot, the
//! panic guard, and the "current subsystem" slots used while a scene callback runs.
//!
//! # Why a singleton
//!
//! ABI v1 is singleton-based (ADR-012): the C side never sees an opaque context
//! pointer, it calls `giochi_*` functions that act on the one engine instance.
//! The instance lives in a `thread_local!` cell, which is also what makes the
//! single-threaded contract (ADR-010) enforceable: a call from another thread finds
//! no engine and gets `GIOCHI_ERROR_NOT_INITIALIZED` instead of racing.
//!
//! # Why the raw pointers
//!
//! A scene callback runs *inside* the engine's loop, so the engine cell is already
//! borrowed and a nested borrow would panic. The three slots below are set by the
//! scene adapter from the references the engine lends to `Scene::update` and
//! `Scene::render`, and cleared immediately after the callback returns. They are
//! therefore only ever dereferenced while the value they point at is alive and
//! exclusively ours — the same reborrow-to-raw-pointer pattern the standard library
//! uses, not a shared global.

use core::ffi::{c_char, CStr};
use std::cell::{Cell, RefCell};
use std::ffi::CString;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::Rc;

use giochi_engine::error::codes;
use giochi_engine::{AudioSystem, Engine, GiochiError, Input, RenderContext, TextureRegistry};

thread_local! {
    /// The single engine instance. `None` until `giochi_engine_init`.
    static ENGINE: RefCell<Option<Engine>> = const { RefCell::new(None) };

    /// Shared handle to the engine's texture registry.
    ///
    /// `TextureRegistry` methods all take `&self`, so this clone is usable from
    /// anywhere — including inside a callback while the engine is borrowed.
    static TEXTURES: RefCell<Option<Rc<TextureRegistry>>> = const { RefCell::new(None) };

    /// Message reported by `giochi_get_last_error`, NUL-terminated.
    static LAST_ERROR: RefCell<CString> = RefCell::new(CString::default());

    /// Renderer lent to the callback that is running right now, if any.
    static CURRENT_RENDER: Cell<*mut RenderContext> = const { Cell::new(core::ptr::null_mut()) };

    /// Input snapshot lent to the running `update` callback, if any.
    static CURRENT_INPUT: Cell<*const Input> = const { Cell::new(core::ptr::null()) };

    /// Mixer lent to the running callback, if any.
    static CURRENT_AUDIO: Cell<*mut AudioSystem> = const { Cell::new(core::ptr::null_mut()) };

    /// `true` while a scene callback is on the stack, i.e. while re-entering the
    /// engine would deadlock or corrupt the scene stack.
    static IN_CALLBACK: Cell<bool> = const { Cell::new(false) };

    /// `true` between the start and the end of a `giochi_engine_run*`/`step` call **on
    /// this thread**.
    ///
    /// Thread-local, like every other flag here. The engine itself is thread-local
    /// (ADR-012), so "a run is in progress" is a fact about one thread; a global made a
    /// second thread's `giochi_engine_run_scene` fail with "already running on this
    /// thread", which was both false and unfixable from the caller's side. It also made
    /// the capi's own tests race against each other, which is how this was found: one test
    /// entered a run while a parallel one asserted an unrelated error code.
    static RUN_IN_PROGRESS: Cell<bool> = const { Cell::new(false) };

    /// Set by `giochi_engine_request_quit`, consumed by the running scene adapter.
    ///
    /// Thread-local for the same reason as [`RUN_IN_PROGRESS`]: two threads may each own
    /// their own engine (there is one engine *per thread*, not per process), and a
    /// process-global flag let a quit requested on one thread stop the other thread's
    /// loop. It also made `giochi_engine_is_running` answer from another thread's state,
    /// because inside a callback it reads this flag rather than the borrowed engine.
    static QUIT_REQUESTED: Cell<bool> = const { Cell::new(false) };
}

// ---------------------------------------------------------------------------
// Last error
// ---------------------------------------------------------------------------

/// Stores `message` as the thread's last error, making it NUL-safe first.
pub(crate) fn set_last_error(message: &str) {
    let sanitised = message.replace('\0', " ");
    LAST_ERROR.with(|slot| {
        *slot.borrow_mut() = CString::new(sanitised).unwrap_or_default();
    });
}

/// Clears the thread's last error. Called on entry to every `giochi_*` function so
/// that a successful call always leaves `giochi_get_last_error()` empty.
fn clear_last_error() {
    LAST_ERROR.with(|slot| {
        let mut slot = slot.borrow_mut();
        if !slot.as_bytes().is_empty() {
            *slot = CString::default();
        }
    });
}

/// Pointer to the thread's last error message; never NULL.
pub(crate) fn last_error_pointer() -> *const c_char {
    LAST_ERROR.with(|slot| slot.borrow().as_ptr())
}

/// The thread's last error message as a `String`, for tests that assert on its wording.
#[cfg(test)]
pub(crate) fn last_error_text() -> String {
    LAST_ERROR.with(|slot| slot.borrow().to_string_lossy().into_owned())
}

/// Records `err` and returns its ABI code.
pub(crate) fn fail(err: &GiochiError) -> i32 {
    set_last_error(&err.to_string());
    error_code(err)
}

/// Maps an engine error onto the numeric ABI code.
///
/// `GiochiError` is `#[non_exhaustive]`, so the wildcard arm is reachable when a
/// later phase adds a variant: an unknown error must degrade to `INTERNAL`, never
/// to success.
pub(crate) fn error_code(err: &GiochiError) -> i32 {
    match err {
        GiochiError::InvalidHandle(_) => codes::INVALID_HANDLE,
        GiochiError::InvalidArgument(_) => codes::INVALID_ARGUMENT,
        GiochiError::NotInitialized => codes::NOT_INITIALIZED,
        GiochiError::Platform(_) => codes::PLATFORM,
        GiochiError::Render(_) => codes::RENDER,
        GiochiError::Asset(_) => codes::ASSET,
        GiochiError::Audio(_) => codes::AUDIO,
        GiochiError::Scene(_) => codes::SCENE,
        GiochiError::Unsupported(_) => codes::UNSUPPORTED,
        _ => codes::INTERNAL,
    }
}

/// Records an argument error and returns its code.
pub(crate) fn invalid_argument(message: &str) -> i32 {
    set_last_error(message);
    codes::INVALID_ARGUMENT
}

/// Records "this subsystem is not available here" and returns `GIOCHI_ERROR_SCENE`.
pub(crate) fn unavailable(message: &str) -> i32 {
    set_last_error(message);
    codes::SCENE
}

fn not_initialized() -> i32 {
    set_last_error("the engine is not initialised; call giochi_engine_init() first");
    codes::NOT_INITIALIZED
}

// ---------------------------------------------------------------------------
// Panic guard
// ---------------------------------------------------------------------------

/// Runs `body`, turning a panic into `fallback` plus `GIOCHI_ERROR_INTERNAL`.
///
/// No Rust panic may cross the boundary: an `extern "C"` function that unwinds is
/// undefined behaviour (and aborts since Rust 1.81). Every exported function is
/// wrapped in this, which also gives the "clear the error on entry" behaviour for
/// free.
pub(crate) fn guard<R>(fallback: R, body: impl FnOnce() -> R) -> R {
    clear_last_error();
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(value) => value,
        Err(payload) => {
            set_last_error(&format!(
                "caught a panic at the C boundary: {}",
                panic_message(&payload)
            ));
            fallback
        }
    }
}

fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_owned()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "non-string panic payload".to_owned()
    }
}

/// Runs `body`, catching a panic and returning the payload instead of unwinding.
///
/// Like [`guard`], but without deciding the fallback: the caller still has cleanup to do
/// (emptying the scene stack, delivering exits) *before* the error is recorded, because
/// that cleanup runs user callbacks and a user callback clears the error on entry. The
/// payload is returned so the message can be written afterwards.
pub(crate) fn catch_panic<R>(body: impl FnOnce() -> R) -> Result<R, Box<dyn std::any::Any + Send>> {
    catch_unwind(AssertUnwindSafe(body))
}

/// The human-readable text of a caught panic payload, for `catch_panic` callers.
pub(crate) fn describe_panic(payload: &Box<dyn std::any::Any + Send>) -> String {
    panic_message(payload)
}

// ---------------------------------------------------------------------------
// Engine access
// ---------------------------------------------------------------------------

/// Runs `body` with a shared reference to the engine.
pub(crate) fn with_engine<R>(hint: &str, body: impl FnOnce(&Engine) -> R) -> Result<R, i32> {
    ENGINE.with(|cell| match cell.try_borrow() {
        Ok(guard) => match guard.as_ref() {
            Some(engine) => Ok(body(engine)),
            None => Err(not_initialized()),
        },
        Err(_) => Err(unavailable(hint)),
    })
}

/// Runs `body` with a mutable reference to the engine.
pub(crate) fn with_engine_mut<R>(
    hint: &str,
    body: impl FnOnce(&mut Engine) -> R,
) -> Result<R, i32> {
    ENGINE.with(|cell| match cell.try_borrow_mut() {
        Ok(mut guard) => match guard.as_mut() {
            Some(engine) => Ok(body(engine)),
            None => Err(not_initialized()),
        },
        Err(_) => Err(unavailable(hint)),
    })
}

/// Stores the engine and the shared texture registry.
pub(crate) fn install(engine: Engine) {
    TEXTURES.with(|slot| *slot.borrow_mut() = Some(engine.render().shared_textures()));
    ENGINE.with(|slot| *slot.borrow_mut() = Some(engine));
}

/// Drops the engine and the shared registry.
pub(crate) fn uninstall() {
    ENGINE.with(|slot| *slot.borrow_mut() = None);
    TEXTURES.with(|slot| *slot.borrow_mut() = None);
}

/// Returns whether an engine is currently installed.
pub(crate) fn is_installed() -> bool {
    ENGINE.with(|cell| {
        cell.try_borrow()
            .map(|guard| guard.is_some())
            .unwrap_or(true)
    })
}

// ---------------------------------------------------------------------------
// Subsystem access
// ---------------------------------------------------------------------------

/// Runs `body` with the texture registry.
///
/// Uses the shared `Rc`, so this works before, during and after a run: texture
/// handles can be created and destroyed from any callback.
pub(crate) fn with_textures<R>(body: impl FnOnce(&TextureRegistry) -> R) -> Result<R, i32> {
    TEXTURES.with(|cell| match cell.borrow().as_ref() {
        Some(registry) => Ok(body(registry)),
        None => Err(not_initialized()),
    })
}

/// Runs `body` with the renderer.
///
/// Inside a `render` callback the renderer is the one the engine lent to the scene;
/// otherwise it is the engine's own, reached through a normal borrow.
///
/// # Safety
///
/// The pointer is only read while a callback is running, and the scene adapter
/// clears it before returning.
pub(crate) fn with_render<R>(
    hint: &str,
    body: impl FnOnce(&mut RenderContext) -> R,
) -> Result<R, i32> {
    let current = CURRENT_RENDER.with(Cell::get);
    if !current.is_null() {
        // SAFETY: `current` was derived from the `&mut RenderContext` the engine
        // handed to the running scene, and is cleared as soon as the callback
        // returns. It is not used by the engine in between.
        return Ok(body(unsafe { &mut *current }));
    }
    with_engine_mut(hint, |engine| body(engine.render_mut()))
}

/// Runs `body` with the input snapshot.
pub(crate) fn with_input<R>(hint: &str, body: impl FnOnce(&Input) -> R) -> Result<R, i32> {
    let current = CURRENT_INPUT.with(Cell::get);
    if !current.is_null() {
        // SAFETY: shared reference derived from `&Input` lent to the running
        // callback; `&Input` aliases freely.
        return Ok(body(unsafe { &*current }));
    }
    with_engine(hint, |engine| body(engine.input()))
}

/// Runs `body` with the mixer.
pub(crate) fn with_audio<R>(
    hint: &str,
    body: impl FnOnce(&mut AudioSystem) -> R,
) -> Result<R, i32> {
    let current = CURRENT_AUDIO.with(Cell::get);
    if !current.is_null() {
        // SAFETY: derived from the `&mut AudioSystem` lent to the running callback,
        // and cleared when it returns.
        return Ok(body(unsafe { &mut *current }));
    }
    with_engine_mut(hint, |engine| body(engine.audio_mut()))
}

/// Marks a callback as running and installs the subsystems it may use.
pub(crate) fn enter_callback(
    render: *mut RenderContext,
    input: *const Input,
    audio: *mut AudioSystem,
) {
    CURRENT_RENDER.with(|slot| slot.set(render));
    CURRENT_INPUT.with(|slot| slot.set(input));
    CURRENT_AUDIO.with(|slot| slot.set(audio));
    IN_CALLBACK.with(|slot| slot.set(true));
}

/// Removes the subsystem slots and clears the callback flag.
pub(crate) fn leave_callback() {
    IN_CALLBACK.with(|slot| slot.set(false));
    CURRENT_RENDER.with(|slot| slot.set(core::ptr::null_mut()));
    CURRENT_INPUT.with(|slot| slot.set(core::ptr::null()));
    CURRENT_AUDIO.with(|slot| slot.set(core::ptr::null_mut()));
}

/// Returns whether a scene callback is currently executing.
pub(crate) fn in_callback() -> bool {
    IN_CALLBACK.with(Cell::get)
}

// ---------------------------------------------------------------------------
// Quit flag and run state
// ---------------------------------------------------------------------------

/// Records a quit request, to be consumed by the running scene.
pub(crate) fn request_quit() {
    QUIT_REQUESTED.with(|flag| flag.set(true));
}

/// Returns and clears the pending quit request.
pub(crate) fn take_quit_request() -> bool {
    QUIT_REQUESTED.with(|flag| flag.replace(false))
}

/// Returns whether a quit has been requested but not consumed yet.
pub(crate) fn quit_requested() -> bool {
    QUIT_REQUESTED.with(Cell::get)
}

/// Discards a pending quit request.
pub(crate) fn clear_quit_request() {
    QUIT_REQUESTED.with(|flag| flag.set(false));
}

/// Returns whether a `run`/`step` call is in progress on this thread.
pub(crate) fn run_in_progress() -> bool {
    RUN_IN_PROGRESS.with(Cell::get)
}

/// Runs `body` with this thread marked as "a run is in progress", clearing the mark
/// afterwards **even if `body` panics**.
///
/// A plain set/reset pair around a call that can unwind leaves the flag stuck at `true`
/// forever, and then every later `run`/`step` on this thread is refused as re-entrant. The
/// panic still reaches `guard`'s `catch_unwind` at the boundary, so the caller sees
/// `GIOCHI_ERROR_INTERNAL` and the engine is unusable from then on with no explanation.
pub(crate) fn with_run_in_progress<R>(body: impl FnOnce() -> R) -> R {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            RUN_IN_PROGRESS.with(|cell| cell.set(false));
        }
    }

    RUN_IN_PROGRESS.with(|cell| cell.set(true));
    let _reset = Reset;
    body()
}

/// Clears the in-progress mark unconditionally, for the paths that only tear down.
pub(crate) fn clear_run_in_progress() {
    RUN_IN_PROGRESS.with(|cell| cell.set(false));
}

// ---------------------------------------------------------------------------
// C string helpers
// ---------------------------------------------------------------------------

/// Borrows a NUL-terminated UTF-8 string from a C pointer.
///
/// Returns `None` for a NULL pointer, invalid UTF-8, or a string whose interior
/// bytes are unusable; the caller is expected to report
/// `GIOCHI_ERROR_INVALID_ARGUMENT`.
pub(crate) fn cstr(pointer: *const c_char) -> Option<String> {
    if pointer.is_null() {
        return None;
    }
    // SAFETY: the contract states the caller passes a NUL-terminated string that
    // stays valid for the duration of the call.
    let text = unsafe { CStr::from_ptr(pointer) };
    text.to_str().ok().map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn a_run_on_another_thread_does_not_mark_this_one() {
        // The mark used to be a process-global `AtomicBool` while the engine is
        // thread-local (ADR-012), so a run on thread A made thread B's
        // `giochi_engine_run_scene` fail with "already running on this thread" — false, and
        // nothing the caller could act on. It also made this crate's own tests race: one
        // test entered a run while a parallel one asserted an unrelated error code, which
        // is how it was found on CI.
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();

        let worker = std::thread::spawn(move || {
            with_run_in_progress(|| {
                started_tx.send(()).expect("signal");
                release_rx.recv().expect("wait for release");
            });
        });

        started_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("the worker should have entered the run");

        assert!(
            !run_in_progress(),
            "another thread's run must not mark this thread"
        );

        release_tx.send(()).expect("release");
        worker.join().expect("worker");

        // And this thread can still take the mark itself afterwards.
        assert!(!run_in_progress());
        with_run_in_progress(|| assert!(run_in_progress()));
        assert!(!run_in_progress());
    }

    #[test]
    fn a_panic_inside_a_run_still_clears_the_mark() {
        // Without the `Drop` guard, a panic in a scene callback unwound past the reset and
        // left the flag at `true` forever: `guard`'s `catch_unwind` reported
        // `GIOCHI_ERROR_INTERNAL` to the caller, and every later run on that thread was
        // then refused as re-entrant, with no way to recover.
        let hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));

        let outcome = std::panic::catch_unwind(|| {
            with_run_in_progress(|| panic!("boom"));
        });

        std::panic::set_hook(hook);

        assert!(outcome.is_err(), "the panic should have propagated");
        assert!(
            !run_in_progress(),
            "the mark must be cleared even when the body unwinds"
        );
    }

    #[test]
    fn a_quit_requested_on_another_thread_does_not_stop_this_one() {
        // The engine is thread-local, so two threads may each own one. `QUIT_REQUESTED`
        // was a process-global `AtomicBool`, which meant a quit on thread A stopped
        // thread B's loop and made B's `giochi_engine_is_running` report 0 from inside a
        // callback — the same defect `RUN_IN_PROGRESS` had, fixed the same way.
        let (started_tx, started_rx) = mpsc::channel();

        let worker = std::thread::spawn(move || {
            request_quit();
            started_tx.send(()).expect("signal");
        });

        started_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("the worker should have set its own flag");
        worker.join().expect("worker");

        assert!(
            !quit_requested(),
            "another thread's quit must not stop this thread's loop"
        );
        assert!(
            !take_quit_request(),
            "and there must be nothing for this thread to consume"
        );

        // The flag still works per thread.
        request_quit();
        assert!(quit_requested());
        assert!(take_quit_request(), "the request is consumable once");
        assert!(!quit_requested());
    }
}
