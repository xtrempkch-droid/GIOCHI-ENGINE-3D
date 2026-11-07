//! C scenes: a callback table adapted to the engine's [`Scene`] trait.
//!
//! A `Scene` in Rust is a trait object owned by the engine. At the C boundary a
//! scene is a handle plus a copied [`GiochiSceneCallbacks`] table, so the adapter
//! has to live in two places at once:
//!
//! * the **registry** keeps the table alive and lets the handle be used again;
//! * an **adapter** (`CScene`) is boxed into the engine's stack for the duration of
//!   one `giochi_engine_run_scene` call.
//!
//! Only `update` and `render` are invoked from inside the adapter, because those
//! are the two callbacks the engine borrows subsystems for. `on_enter`/`on_exit`
//! are invoked by [`crate::engine`] *outside* the engine borrow, so a scene can
//! create and destroy textures and sounds there (see `docs/C_ABI.md`).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use giochi_engine::error::codes;
use giochi_engine::{AudioSystem, Input, RenderContext, Scene, SceneTransition, Time};

use crate::abi_types::GiochiSceneCallbacks;
use crate::state::{self, guard};

/// One registered C scene.
pub(crate) struct SceneEntry {
    /// Copy of the caller's callback table.
    pub(crate) callbacks: GiochiSceneCallbacks,
    /// Number of live adapters for this scene on the engine's stack.
    on_stack: u32,
    /// `true` between `giochi_engine_run_scene` entry and exit.
    running: bool,
    /// `on_exit` has to be delivered, but the engine was busy at the time.
    exit_pending: bool,
}

/// Entries plus the handle allocator; handles are never reused.
pub(crate) struct SceneRegistry {
    entries: HashMap<u64, Rc<RefCell<SceneEntry>>>,
    next: u64,
}

impl Default for SceneRegistry {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            next: 1,
        }
    }
}

impl SceneRegistry {
    fn insert(&mut self, callbacks: GiochiSceneCallbacks) -> u64 {
        let handle = self.next;
        self.next += 1;
        self.entries.insert(
            handle,
            Rc::new(RefCell::new(SceneEntry {
                callbacks,
                on_stack: 0,
                running: false,
                exit_pending: false,
            })),
        );
        handle
    }

    fn get(&self, handle: u64) -> Option<Rc<RefCell<SceneEntry>>> {
        self.entries.get(&handle).cloned()
    }
}

thread_local! {
    static REGISTRY: RefCell<SceneRegistry> = RefCell::new(SceneRegistry::default());
}

/// Clears every callback slot, even if the user callback panics.
struct CallbackScope;

impl Drop for CallbackScope {
    fn drop(&mut self) {
        state::leave_callback();
    }
}

/// Adapter that lets the engine drive a C callback table.
struct CScene {
    entry: Rc<RefCell<SceneEntry>>,
}

impl Scene for CScene {
    fn on_enter(&mut self) {
        // The user-facing `on_enter` already ran, outside the engine borrow. This
        // only records that the scene is now on the stack.
        let mut entry = self.entry.borrow_mut();
        entry.on_stack = entry.on_stack.saturating_add(1);
        entry.exit_pending = false;
    }

    fn on_exit(&mut self) {
        // Deferred: the engine calls this while it is borrowed, and the user's
        // `on_exit` needs to destroy handles, which needs the engine.
        let mut entry = self.entry.borrow_mut();
        entry.on_stack = entry.on_stack.saturating_sub(1);
        entry.exit_pending = true;
    }

    fn update(&mut self, time: Time, input: &Input, audio: &mut AudioSystem) {
        let callbacks = self.entry.borrow().callbacks;
        let Some(update) = callbacks.update else {
            return;
        };

        state::enter_callback(
            core::ptr::null_mut(),
            input as *const Input,
            audio as *mut AudioSystem,
        );
        let _scope = CallbackScope;
        update(callbacks.user_data, &time as *const Time);
    }

    fn render(&mut self, renderer: &mut RenderContext) {
        let callbacks = self.entry.borrow().callbacks;
        let Some(render) = callbacks.render else {
            return;
        };

        state::enter_callback(
            renderer as *mut RenderContext,
            core::ptr::null(),
            core::ptr::null_mut(),
        );
        let _scope = CallbackScope;
        render(callbacks.user_data);
    }

    fn transition(&mut self) -> SceneTransition {
        if state::take_quit_request() {
            SceneTransition::Quit
        } else {
            SceneTransition::Continue
        }
    }
}

/// Creates a scene from a callback table and returns its handle.
pub(crate) fn create(callbacks: GiochiSceneCallbacks) -> u64 {
    REGISTRY.with(|registry| registry.borrow_mut().insert(callbacks))
}

/// Looks up a scene entry, reporting `GIOCHI_ERROR_INVALID_HANDLE` when unknown.
pub(crate) fn lookup(handle: u64) -> Result<Rc<RefCell<SceneEntry>>, i32> {
    if handle == 0 {
        state::set_last_error("scene handle 0 is never valid");
        return Err(codes::INVALID_HANDLE);
    }
    REGISTRY.with(|registry| match registry.borrow().get(handle) {
        Some(entry) => Ok(entry),
        None => {
            state::set_last_error(&format!("unknown or destroyed scene handle {handle}"));
            Err(codes::INVALID_HANDLE)
        }
    })
}

/// Marks the scene as running (or not) and reports the previous state.
pub(crate) fn set_running(entry: &Rc<RefCell<SceneEntry>>, running: bool) -> bool {
    let mut entry = entry.borrow_mut();
    let previous = entry.running;
    entry.running = running;
    previous
}

/// Returns whether the scene is currently being driven by `giochi_engine_run_scene`.
pub(crate) fn is_running(entry: &Rc<RefCell<SceneEntry>>) -> bool {
    entry.borrow().running
}

/// Builds the adapter the engine will drive for one `run_scene` call.
pub(crate) fn adapter(entry: &Rc<RefCell<SceneEntry>>) -> Box<dyn Scene> {
    Box::new(CScene {
        entry: Rc::clone(entry),
    })
}

/// Removes a handle from the registry.
///
/// Refuses while the scene is running: the adapter would keep the entry alive, but
/// the handle would then refer to nothing and the deferred `on_exit` could never be
/// observed.
pub(crate) fn destroy(handle: u64) -> i32 {
    if handle == 0 {
        state::set_last_error("scene handle 0 is never valid");
        return codes::INVALID_HANDLE;
    }

    let entry = match lookup(handle) {
        Ok(entry) => entry,
        Err(code) => return code,
    };
    if is_running(&entry) {
        state::set_last_error("cannot destroy a scene while giochi_engine_run_scene is running it");
        return codes::SCENE;
    }

    let removed = REGISTRY.with(|registry| registry.borrow_mut().entries.remove(&handle).is_some());
    if removed {
        codes::OK
    } else {
        codes::INVALID_HANDLE
    }
}

/// Delivers every deferred `on_exit`.
///
/// Must be called with no engine borrow held, which is why `crate::engine` calls it
/// after `run`/`step`/`shutdown` rather than from the adapter itself.
pub(crate) fn drain_pending_exits() {
    let pending: Vec<Rc<RefCell<SceneEntry>>> = REGISTRY.with(|registry| {
        let registry = registry.borrow();
        registry
            .entries
            .values()
            .filter(|entry| {
                let entry = entry.borrow();
                entry.exit_pending && entry.on_stack == 0
            })
            .cloned()
            .collect()
    });

    for entry in pending {
        let callbacks = {
            let mut entry = entry.borrow_mut();
            entry.exit_pending = false;
            entry.callbacks
        };
        if let Some(on_exit) = callbacks.on_exit {
            on_exit(callbacks.user_data);
        }
    }
}

/// Invokes the user's `on_enter` for `entry`.
///
/// Called by `crate::engine` before the engine borrow is taken, so the callback can
/// use every subsystem.
pub(crate) fn deliver_on_enter(callbacks: &GiochiSceneCallbacks) {
    if let Some(on_enter) = callbacks.on_enter {
        on_enter(callbacks.user_data);
    }
}

// ---------------------------------------------------------------------------
// Exported functions
// ---------------------------------------------------------------------------

/// Creates a scene from a callback table.
///
/// `callbacks` must be non-NULL and contain `update` and `render`; returns `0` and
/// sets the last error otherwise. The table is copied, so the caller may free it
/// after the call — but `user_data` and the function pointers must stay valid until
/// [`giochi_scene_destroy`].
///
/// # C ABI
///
/// ```c
/// GiochiScene giochi_scene_create(const GiochiSceneCallbacks* callbacks);
/// ```
#[no_mangle]
pub extern "C" fn giochi_scene_create(
    callbacks: *const GiochiSceneCallbacks,
) -> giochi_engine::SceneHandle {
    guard(0, || {
        if callbacks.is_null() {
            state::invalid_argument("callbacks must not be NULL");
            return 0;
        }
        // SAFETY: non-NULL pointer to a caller-provided struct; copied immediately.
        let callbacks = unsafe { *callbacks };
        if !callbacks.is_complete() {
            state::invalid_argument("callbacks must provide both update and render");
            return 0;
        }
        create(callbacks)
    })
}

/// Destroys a scene handle.
///
/// Returns `GIOCHI_ERROR_INVALID_HANDLE` for `0` or an unknown handle, and
/// `GIOCHI_ERROR_SCENE` while `giochi_engine_run_scene` is driving it.
///
/// # C ABI
///
/// ```c
/// int32_t giochi_scene_destroy(GiochiScene scene);
/// ```
#[no_mangle]
pub extern "C" fn giochi_scene_destroy(scene: giochi_engine::SceneHandle) -> i32 {
    guard(codes::INTERNAL, || destroy(scene))
}

/// Returns the number of scenes on the engine's stack, or `0` when the engine is
/// not initialised.
///
/// A diagnostic, like `giochi_get_last_error`: it exists so a host (and the smoke
/// tests) can assert that a `giochi_engine_run_scene` call left nothing behind.
///
/// # C ABI
///
/// ```c
/// uint32_t giochi_engine_scene_depth(void);
/// ```
#[no_mangle]
pub extern "C" fn giochi_engine_scene_depth() -> u32 {
    guard(0, || {
        state::with_engine("the engine is busy", |engine| engine.scenes().depth())
            .map(|depth| depth as u32)
            .unwrap_or(0)
    })
}
