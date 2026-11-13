//! 3D at the C boundary — part II of the ABI, `GIOCHI_ABI_VERSION 2`.
//!
//! Specified in `docs/C_ABI_CONTRACT.md` §8–§12 and explained in `docs/C_ABI.md`.
//!
//! ```c
//! GiochiEngine3D* engine = giochi_engine3d_create(NULL);
//! GiochiMeshHandle cube;
//! GiochiShaderHandle shader;
//! giochi_mesh_create_cube(1.0f, &cube);
//! giochi_shader_builtin(GIOCHI_SHADER_PHONG, &shader);
//!
//! GiochiCamera3D camera = giochi_camera3d_new();
//! giochi_camera3d_look_at(&camera, (GiochiVec3){0.0f, 2.0f, 3.0f, {0}},
//!                         (GiochiVec3){0.0f, 0.0f, 0.0f, {0}});
//!
//! giochi_engine3d_begin_frame(engine);
//! giochi_renderer3d_clear(engine, (GiochiColor3D){0.07f, 0.09f, 0.14f, 1.0f, {0}});
//! GiochiTransform3D model = giochi_transform3d_identity();
//! giochi_renderer3d_draw_mesh(engine, cube, shader, &model, &camera,
//!                             (GiochiColor3D){0.85f, 0.62f, 0.35f, 1.0f, {0}});
//! giochi_engine3d_end_frame(engine);
//!
//! giochi_mesh_destroy(cube);
//! giochi_shader_destroy(shader);
//! giochi_engine3d_destroy(engine);
//! ```
//!
//! # Why 3D is an opaque pointer and 2D is a singleton
//!
//! ADR-039. `RenderContext3D` owns a `glow::Context` and an SDL `GLContext`, neither of
//! which has a C representation, and its size changes between releases. A caller that
//! could see the layout would break on the next minor version. Part I's global
//! functions cannot describe that, so part II hands the caller a pointer and keeps the
//! layout to itself.
//!
//! # One engine per thread
//!
//! `giochi_mesh_*` and `giochi_shader_*` take no engine (§10.5): a mesh is a GL object
//! belonging to the renderer that made it, so those four functions act on the most
//! recently created engine on the current thread. Creating a second one is refused
//! rather than allowed to retarget them, which keeps "most recently created" meaning
//! "the only one".

use core::ffi::c_char;
use std::cell::Cell;

use giochi_engine::error::codes;
use giochi_engine::platform::window::{Window, WindowConfig};
use giochi_engine::platform::Platform;
use giochi_engine_3d::context::{self as context3d, GlesVersion};
use giochi_engine_3d::math3d::{Camera3D, Color3D, Transform3D, Vec3};
use giochi_engine_3d::shader::BuiltinShader;
use giochi_engine_3d::DirectionalLight;
use giochi_engine_3d::{Error3D, RenderContext3D};

use crate::state::{self, guard};

/// Window title used when the caller does not care. There is no title field in
/// `GiochiWindowConfig`; a 3D console app is fullscreen and nobody reads it.
const WINDOW_TITLE: &str = "GIOCHI 3D";

/// Requested size when `giochi_engine3d_create` is given `NULL`.
const DEFAULT_WIDTH: u32 = 1280;
const DEFAULT_HEIGHT: u32 = 720;

/// The 3D engine, as C sees it: an incomplete type.
///
/// Deliberately **not** `#[repr(C)]` — it holds a `Platform`, a `Window` and a
/// `RenderContext3D`, none of which has a C representation, and the layout changes
/// whenever any of them does. `giochi.h` forward-declares it and every function takes or
/// returns a pointer (ADR-039).
pub struct GiochiEngine3D {
    /// Kept alive because the window and the GL context belong to it.
    _platform: Platform,
    /// The drawable. KMSDRM binds the panel's native mode and ignores the requested
    /// size (ADR-035), which is why the size is read back rather than assumed.
    window: Window,
    render: RenderContext3D,
}

thread_local! {
    /// The engine created by `giochi_engine3d_create` on this thread.
    ///
    /// Exists for §10.5: the mesh and shader functions have no engine parameter, so they
    /// need to know which renderer's registry to touch. Set by `create`, cleared by
    /// `destroy`, and a second `create` on the same thread is refused — so a non-null
    /// value here always means exactly one live engine, and the pointer is valid for as
    /// long as the caller has not destroyed it.
    static CURRENT_3D: Cell<*mut GiochiEngine3D> = const { Cell::new(core::ptr::null_mut()) };
}

/// Maps a 3D error onto the numeric ABI code.
///
/// Exhaustive rather than wildcarded: `Error3D` is not `#[non_exhaustive]`, so a new
/// variant must be classified here rather than silently degrading to `INTERNAL`.
fn error_code(err: &Error3D) -> i32 {
    match err {
        Error3D::InvalidHandle(_) => codes::INVALID_HANDLE,
        Error3D::InvalidArgument(_) => codes::INVALID_ARGUMENT,
        Error3D::Platform(_) => codes::PLATFORM,
        // A context that fails to create and a shader that fails to compile are both
        // rendering failures from the caller's point of view; the message distinguishes
        // them and the shader one carries the driver log.
        Error3D::Context(_) | Error3D::GlObject(_) | Error3D::Shader { .. } => codes::RENDER,
        Error3D::Asset(_) => codes::ASSET,
    }
}

/// Stores `err` as the thread's last error and returns its code.
fn fail3d(err: &Error3D) -> i32 {
    state::set_last_error(&err.to_string());
    error_code(err)
}

/// Runs `body` with the current thread's engine.
///
/// `Err` is `NOT_INITIALIZED` when there is none — the same code part I uses for a call
/// before `giochi_engine_init`.
fn with_current<R>(body: impl FnOnce(&mut GiochiEngine3D) -> R) -> Result<R, i32> {
    let pointer = CURRENT_3D.with(Cell::get);
    if pointer.is_null() {
        return Err(codes::NOT_INITIALIZED);
    }

    // SAFETY: the pointer came from `Box::into_raw` in `create` and is only cleared in
    // `destroy`, which runs at most once per engine. The caller owns the pointer and
    // passing a stale one is the same use-after-free any opaque C handle has. No two
    // `&mut` exist at once: no function here calls another that borrows the engine.
    Ok(body(unsafe { &mut *pointer }))
}

/// Checks that `engine` is the live engine for this thread, or reports why not.
///
/// An opaque pointer in C is only as good as its owner's bookkeeping, but "the caller got
/// it right" is not the same as "nothing can be checked": `giochi_engine3d_create` refuses
/// a second engine on a thread, so exactly one pointer is valid at any moment and comparing
/// against it turns a stale-pointer use-after-free into `GIOCHI_ERROR_INVALID_HANDLE`. That
/// is worth one thread-local read per call.
///
/// Returns `Err` with the code to report: `INVALID_ARGUMENT` for NULL, which is a caller
/// mistake in the call itself, and `INVALID_HANDLE` for a pointer that is not the live
/// engine, which is a lifetime mistake.
fn validate_engine(engine: *mut GiochiEngine3D) -> Result<&'static mut GiochiEngine3D, i32> {
    if engine.is_null() {
        return Err(state::invalid_argument("engine must not be NULL"));
    }
    if engine != CURRENT_3D.with(Cell::get) {
        state::set_last_error(
            "engine is not the one this thread created; it is either from another thread \
             or already destroyed",
        );
        return Err(codes::INVALID_HANDLE);
    }

    // SAFETY: `engine` is exactly the pointer `create` stored in this thread's slot and
    // `destroy` has not run (it clears the slot first). No two `&mut` exist at once because
    // no function here calls another that borrows the engine.
    Ok(unsafe { &mut *engine })
}

/// Whether a 3D engine exists on this thread.
///
/// Used by `giochi_engine_init` to refuse the 2D engine with a clear message instead of
/// letting SDL fail with "an `EventPump` instance is already alive".
pub(crate) fn engine_is_live() -> bool {
    !CURRENT_3D.with(Cell::get).is_null()
}

/// Turns a caller-supplied `const char*` into a `String`, or reports why not.
fn string_or_error(pointer: *const c_char, what: &str) -> Option<String> {
    match state::cstr(pointer) {
        Some(text) => Some(text),
        None => {
            state::set_last_error(&format!("{what} must be a non-NULL, valid UTF-8 string"));
            None
        }
    }
}

// ---- 10.1 lifecycle ------------------------------------------------------

/// Creates the 3D engine: SDL, an OpenGL-ES window, and the renderer.
///
/// `config` may be `NULL`, which uses 1280x720 windowed. Returns `NULL` on failure; the
/// reason is in [`giochi_get_last_error`](crate::giochi_get_last_error) because there is
/// no engine to ask.
///
/// Only one engine may exist per thread (§10.5). Creating a second one without an
/// intervening [`giochi_engine3d_destroy`] fails rather than retargeting the mesh and
/// shader functions.
///
/// # C ABI
///
/// ```c
/// GiochiEngine3D* giochi_engine3d_create(const GiochiWindowConfig* config);
/// ```
#[no_mangle]
pub extern "C" fn giochi_engine3d_create(config: *const WindowConfig) -> *mut GiochiEngine3D {
    guard(core::ptr::null_mut(), || {
        if !CURRENT_3D.with(Cell::get).is_null() {
            state::set_last_error(
                "a 3D engine already exists on this thread; \
                 call giochi_engine3d_destroy before creating another",
            );
            return core::ptr::null_mut();
        }
        // The 2D engine owns the process-wide SDL state, including the single `EventPump`.
        // Letting SDL report that would hand the caller "an `EventPump` instance is already
        // alive", which names an internal type they have never used and never says the rule
        // that actually applies (ADR-037: a game picks one renderer).
        if state::is_installed() {
            state::set_last_error(
                "the 2D engine is already initialised in this process; the 2D and 3D \
                 renderers cannot share one (ADR-037) -- call giochi_engine_shutdown first",
            );
            return core::ptr::null_mut();
        }

        let config = if config.is_null() {
            WindowConfig::new(DEFAULT_WIDTH, DEFAULT_HEIGHT)
        } else {
            // SAFETY: non-NULL pointer to a caller-provided `WindowConfig`. It is copied
            // field by field and never retained.
            unsafe { *config }
        };

        let platform = match Platform::new() {
            Ok(platform) => platform,
            Err(err) => {
                state::fail(&err);
                return core::ptr::null_mut();
            }
        };

        // Before the window: the `SDL_GL_*` attributes are process-global and read when
        // the window is created, not when the context is.
        if let Err(err) = context3d::configure_attributes(&platform, GlesVersion::Gles2) {
            state::set_last_error(&err.to_string());
            return core::ptr::null_mut();
        }

        let window = match Window::new_opengl(&platform, config, WINDOW_TITLE) {
            Ok(window) => window,
            Err(err) => {
                state::fail(&err);
                return core::ptr::null_mut();
            }
        };

        let gl = match context3d::create(&window) {
            Ok(gl) => gl,
            Err(err) => {
                state::set_last_error(&err.to_string());
                return core::ptr::null_mut();
            }
        };

        let (width, height) = window.drawable_size();
        if (width, height) != (config.width, config.height) {
            // KMSDRM is fullscreen-only and binds the panel's mode, so the drawable is
            // often not what was asked for. Saying so once beats debugging a scene laid
            // out for a screen that does not exist.
            log::info!(
                "3D: requested {}x{}, backend opened {width}x{height}",
                config.width,
                config.height
            );
        }

        let engine = Box::new(GiochiEngine3D {
            _platform: platform,
            window,
            render: RenderContext3D::new(gl),
        });

        let pointer = Box::into_raw(engine);
        CURRENT_3D.with(|cell| cell.set(pointer));
        pointer
    })
}

/// Destroys the 3D engine, releasing every mesh, shader and the GL context.
///
/// `NULL` is a no-op, but a stale non-NULL pointer is not: once the engine is destroyed a
/// second call with the same pointer is rejected (with a message in
/// [`giochi_get_last_error`](crate::giochi_get_last_error)) instead of freeing the
/// allocation twice. Handles created by the engine become invalid.
///
/// # C ABI
///
/// ```c
/// void giochi_engine3d_destroy(GiochiEngine3D* engine);
/// ```
#[no_mangle]
pub extern "C" fn giochi_engine3d_destroy(engine: *mut GiochiEngine3D) {
    guard((), || {
        if engine.is_null() {
            return;
        }

        // The slot must *equal* the pointer: a NULL slot means the engine was already
        // destroyed (or never created), and reconstructing the `Box` again is a double
        // free. The earlier check only rejected a non-NULL mismatch, so a second call
        // with the stale pointer passed and freed the allocation twice.
        let current = CURRENT_3D.with(Cell::get);
        if current != engine {
            state::set_last_error(
                "giochi_engine3d_destroy was given an engine that is not the live one on \
                 this thread; it is either from another thread or already destroyed",
            );
            return;
        }

        // Clear before dropping: if a destructor panics, the slot must not keep pointing
        // at freed memory.
        CURRENT_3D.with(|cell| cell.set(core::ptr::null_mut()));

        // SAFETY: the pointer came from `Box::into_raw` in `create`, the slot compared
        // equal to it, and clearing the slot above makes a second call fail the check.
        let mut engine = unsafe { Box::from_raw(engine) };
        if !engine.render.release_all() {
            log::warn!("3D engine destroyed with GL objects still referenced elsewhere");
        }
    })
}

/// Applies the GL state, sets the viewport from the real drawable size and clears.
///
/// Every frame starts here. The viewport comes from the window, not from the requested
/// configuration, because on KMSDRM the two differ (ADR-035).
///
/// # C ABI
///
/// ```c
/// int32_t giochi_engine3d_begin_frame(GiochiEngine3D* engine);
/// ```
#[no_mangle]
pub extern "C" fn giochi_engine3d_begin_frame(engine: *mut GiochiEngine3D) -> i32 {
    guard(codes::INTERNAL, || {
        let engine = match validate_engine(engine) {
            Ok(engine) => engine,
            Err(code) => return code,
        };
        engine.render.begin_frame(&engine.window);
        codes::OK
    })
}

/// Presents the frame: swaps the back buffer and blocks until vsync on KMSDRM.
///
/// The block is why frame timing must be measured **before** this call, not after — see
/// `docs/3D.md` §7, where reading the clock afterwards made the balance game report the
/// panel's refresh rate as its own cost.
///
/// # C ABI
///
/// ```c
/// int32_t giochi_engine3d_end_frame(GiochiEngine3D* engine);
/// ```
#[no_mangle]
pub extern "C" fn giochi_engine3d_end_frame(engine: *mut GiochiEngine3D) -> i32 {
    guard(codes::INTERNAL, || {
        let engine = match validate_engine(engine) {
            Ok(engine) => engine,
            Err(code) => return code,
        };
        engine.render.end_frame(&engine.window);
        codes::OK
    })
}

// ---- 10.2 camera and transforms -----------------------------------------

/// Returns a camera at the origin looking down -Z, 60° vertical FOV.
///
/// `aspect` is `0.0`, meaning "ask the drawable": on a console the panel's real size is
/// authoritative (ADR-035).
///
/// # C ABI
///
/// ```c
/// GiochiCamera3D giochi_camera3d_new(void);
/// ```
#[no_mangle]
pub extern "C" fn giochi_camera3d_new() -> Camera3D {
    guard(Camera3D::new(), Camera3D::new)
}

/// Points `camera` from `position` at `target`.
///
/// Returns nothing because there is nothing useful to return: an unusable camera (up
/// parallel to the view direction, or `position == target`) is reported by
/// [`giochi_renderer3d_draw_mesh`], which is the call that needs a view matrix.
///
/// # C ABI
///
/// ```c
/// void giochi_camera3d_look_at(GiochiCamera3D* camera,
///                              GiochiVec3 position, GiochiVec3 target);
/// ```
#[no_mangle]
pub extern "C" fn giochi_camera3d_look_at(camera: *mut Camera3D, position: Vec3, target: Vec3) {
    guard((), || {
        if camera.is_null() {
            state::invalid_argument("camera must not be NULL");
            return;
        }

        // SAFETY: non-NULL pointer to a caller-provided `GiochiCamera3D`. The reserved
        // tail is preserved: a caller that zeroes it gets it back zeroed.
        let camera = unsafe { &mut *camera };
        camera.position = position;
        camera.target = target;
    })
}

/// Returns the identity transform: no translation, no rotation, unit scale.
///
/// A zeroed `GiochiTransform3D` has **zero scale**, which draws nothing. This is the
/// value to start from.
///
/// # C ABI
///
/// ```c
/// GiochiTransform3D giochi_transform3d_identity(void);
/// ```
#[no_mangle]
pub extern "C" fn giochi_transform3d_identity() -> Transform3D {
    guard(Transform3D::IDENTITY, || Transform3D::IDENTITY)
}

/// Returns a transform that only translates.
///
/// # C ABI
///
/// ```c
/// GiochiTransform3D giochi_transform3d_from_translation(GiochiVec3 translation);
/// ```
#[no_mangle]
pub extern "C" fn giochi_transform3d_from_translation(translation: Vec3) -> Transform3D {
    guard(Transform3D::from_translation(translation), || {
        Transform3D::from_translation(translation)
    })
}

// ---- 10.3 meshes and shaders --------------------------------------------

/// Loads a Wavefront OBJ file as a mesh.
///
/// Writes the handle to `out` and returns `GIOCHI_ERROR_OK`, or writes nothing and
/// returns a negative code. The mesh belongs to this thread's engine (§10.5) and must be
/// released with [`giochi_mesh_destroy`].
///
/// # C ABI
///
/// ```c
/// int32_t giochi_mesh_load_obj(const char* path, GiochiMeshHandle* out);
/// ```
#[no_mangle]
pub extern "C" fn giochi_mesh_load_obj(path: *const c_char, out: *mut u64) -> i32 {
    guard(codes::INTERNAL, || {
        if out.is_null() {
            state::invalid_argument("out must not be NULL");
            return codes::INVALID_ARGUMENT;
        }
        let Some(path) = string_or_error(path, "mesh path") else {
            return codes::INVALID_ARGUMENT;
        };

        match with_current(|engine| engine.render.load_obj_mesh(std::path::Path::new(&path))) {
            Err(code) => {
                if code == codes::NOT_INITIALIZED {
                    state::set_last_error(
                        "giochi_mesh_load_obj needs a 3D engine; call giochi_engine3d_create first",
                    );
                }
                code
            }
            Ok(Ok(handle)) => {
                // SAFETY: non-NULL, checked above, and the contract says it points at
                // one `GiochiMeshHandle` the caller owns.
                unsafe { *out = handle };
                codes::OK
            }
            Ok(Err(err)) => fail3d(&err),
        }
    })
}

/// Creates a cube centred on the origin with the given edge length.
///
/// The same mesh `create_cube_mesh` builds on the Rust side, indexed and with outward
/// winding. Exists so a C program can draw something without shipping an OBJ.
///
/// # C ABI
///
/// ```c
/// int32_t giochi_mesh_create_cube(float size, GiochiMeshHandle* out);
/// ```
#[no_mangle]
pub extern "C" fn giochi_mesh_create_cube(size: f32, out: *mut u64) -> i32 {
    guard(codes::INTERNAL, || {
        if out.is_null() {
            state::invalid_argument("out must not be NULL");
            return codes::INVALID_ARGUMENT;
        }

        match with_current(|engine| engine.render.create_cube_mesh(size)) {
            Err(code) => {
                if code == codes::NOT_INITIALIZED {
                    state::set_last_error(
                        "giochi_mesh_create_cube needs a 3D engine; call giochi_engine3d_create first",
                    );
                }
                code
            }
            Ok(Ok(handle)) => {
                // SAFETY: as in `giochi_mesh_load_obj`.
                unsafe { *out = handle };
                codes::OK
            }
            Ok(Err(err)) => fail3d(&err),
        }
    })
}

/// Releases a mesh.
///
/// `0` and unknown handles are `GIOCHI_ERROR_INVALID_HANDLE`, matching part I: a double
/// free is a bug in the caller and must be visible.
///
/// # C ABI
///
/// ```c
/// int32_t giochi_mesh_destroy(GiochiMeshHandle mesh);
/// ```
#[no_mangle]
pub extern "C" fn giochi_mesh_destroy(mesh: u64) -> i32 {
    guard(codes::INTERNAL, || {
        match with_current(|engine| engine.render.destroy_mesh(mesh)) {
            Err(code) => {
                if code == codes::NOT_INITIALIZED {
                    state::set_last_error(
                        "giochi_mesh_destroy needs the 3D engine that created the mesh",
                    );
                }
                code
            }
            Ok(true) => codes::OK,
            Ok(false) => {
                state::set_last_error(&format!("mesh {mesh} does not exist"));
                codes::INVALID_HANDLE
            }
        }
    })
}

/// Compiles a vertex/fragment shader pair read from disk.
///
/// R10: shader source lives in files, so there is no function that takes GLSL as a
/// string (§11). On failure the error message carries the driver's own compile or link
/// log, which is the only thing that makes a shader bug findable.
///
/// # C ABI
///
/// ```c
/// int32_t giochi_shader_load(const char* vert_path, const char* frag_path,
///                            GiochiShaderHandle* out);
/// ```
#[no_mangle]
pub extern "C" fn giochi_shader_load(
    vert_path: *const c_char,
    frag_path: *const c_char,
    out: *mut u64,
) -> i32 {
    guard(codes::INTERNAL, || {
        if out.is_null() {
            state::invalid_argument("out must not be NULL");
            return codes::INVALID_ARGUMENT;
        }
        let Some(vertex) = string_or_error(vert_path, "vertex shader path") else {
            return codes::INVALID_ARGUMENT;
        };
        let Some(fragment) = string_or_error(frag_path, "fragment shader path") else {
            return codes::INVALID_ARGUMENT;
        };

        match with_current(|engine| {
            engine.render.load_shader(
                std::path::Path::new(&vertex),
                std::path::Path::new(&fragment),
            )
        }) {
            Err(code) => {
                if code == codes::NOT_INITIALIZED {
                    state::set_last_error(
                        "giochi_shader_load needs a 3D engine; call giochi_engine3d_create first",
                    );
                }
                code
            }
            Ok(Ok(handle)) => {
                // SAFETY: as in `giochi_mesh_load_obj`.
                unsafe { *out = handle };
                codes::OK
            }
            Ok(Err(err)) => fail3d(&err),
        }
    })
}

/// Compiles one of the shaders the engine ships (§10.6).
///
/// `which` is `GIOCHI_SHADER_FLAT`, `GIOCHI_SHADER_TEXTURED` or `GIOCHI_SHADER_PHONG`;
/// anything else is `GIOCHI_ERROR_INVALID_ARGUMENT` rather than a guess.
///
/// # C ABI
///
/// ```c
/// int32_t giochi_shader_builtin(int32_t which, GiochiShaderHandle* out);
/// ```
#[no_mangle]
pub extern "C" fn giochi_shader_builtin(which: i32, out: *mut u64) -> i32 {
    guard(codes::INTERNAL, || {
        if out.is_null() {
            state::invalid_argument("out must not be NULL");
            return codes::INVALID_ARGUMENT;
        }

        let Some(shader) = builtin_shader(which) else {
            state::set_last_error(&format!(
                "{which} is not a built-in shader; expected GIOCHI_SHADER_FLAT (0), \
                 GIOCHI_SHADER_TEXTURED (1) or GIOCHI_SHADER_PHONG (2)"
            ));
            return codes::INVALID_ARGUMENT;
        };

        match with_current(|engine| engine.render.create_builtin_shader(shader)) {
            Err(code) => {
                if code == codes::NOT_INITIALIZED {
                    state::set_last_error(
                        "giochi_shader_builtin needs a 3D engine; call giochi_engine3d_create first",
                    );
                }
                code
            }
            Ok(Ok(handle)) => {
                // SAFETY: as in `giochi_mesh_load_obj`.
                unsafe { *out = handle };
                codes::OK
            }
            Ok(Err(err)) => fail3d(&err),
        }
    })
}

/// Releases a shader program. Mirrors [`giochi_mesh_destroy`].
///
/// # C ABI
///
/// ```c
/// int32_t giochi_shader_destroy(GiochiShaderHandle shader);
/// ```
#[no_mangle]
pub extern "C" fn giochi_shader_destroy(shader: u64) -> i32 {
    guard(codes::INTERNAL, || {
        match with_current(|engine| engine.render.destroy_shader(shader)) {
            Err(code) => {
                if code == codes::NOT_INITIALIZED {
                    state::set_last_error(
                        "giochi_shader_destroy needs the 3D engine that created the shader",
                    );
                }
                code
            }
            Ok(true) => codes::OK,
            Ok(false) => {
                state::set_last_error(&format!("shader {shader} does not exist"));
                codes::INVALID_HANDLE
            }
        }
    })
}

/// One vertex for [`giochi_mesh_create`].
///
/// The layout is public because a C caller builds the array itself: 40 bytes, alignment
/// 4, eight `float` fields in the order position, normal, texture coordinate, then two
/// reserved words. It is deliberately **not** the crate's `Vertex` (32 bytes, no
/// reserved): that type is internal and may gain a field, and this one may not.
///
/// `uv` is `0.0..=1.0`; a mesh built without UVs puts `(0, 0)` and samples one texel if
/// it is drawn textured.
///
/// A normal of `(0, 0, 0)` (or a non-finite one) means "derive it from the triangles",
/// the same rule the OBJ loader applies to a file without `vn`. Do **not** rely on a zero
/// normal meaning "flat": `normalize(vec3(0.0))` is undefined in GLSL ES 1.00, so the
/// surface would be grey on one driver and black on another. A non-finite position cannot
/// be projected or used to derive a normal, so it is rejected.
///
/// # C ABI
///
/// ```c
/// typedef struct GiochiVertex3D {
///     float    position[3];
///     float    normal[3];
///     float    uv[2];
///     uint32_t _reserved[2];
/// } GiochiVertex3D;   /* size 40, align 4 */
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GiochiVertex3D {
    /// Model-space position.
    pub position: [f32; 3],
    /// Surface normal; normalised on upload. `(0, 0, 0)` means "let the engine leave it
    /// flat", which is the ambient-only shade.
    pub normal: [f32; 3],
    /// Texture coordinate, `0.0..=1.0`.
    pub uv: [f32; 2],
    /// Reserved for ABI-compatible growth. Must be zero.
    pub _reserved: [u32; 2],
}

/// Uploads a mesh from caller-provided vertices and indices.
///
/// This is the C counterpart of `RenderContext3D::create_mesh`: the way to draw geometry
/// that is not a cube and does not come from an OBJ — terrain generated from a heightmap,
/// a procedurally skinned shape, geometry produced by the game itself.
///
/// `indices` are triangle indices into `vertices` (`u32`, GLES 2.0 with
/// `GL_OES_element_index_uint`, which every driver measured here exposes). Every index
/// must be in range; one that is not is rejected rather than read out of bounds on the
/// GPU.
///
/// Works on the most recently created engine on this thread (§10.5), like
/// [`giochi_mesh_load_obj`].
///
/// # C ABI
///
/// ```c
/// int32_t giochi_mesh_create(const GiochiVertex3D* vertices, uint32_t vertex_count,
///                            const uint32_t* indices, uint32_t index_count,
///                            GiochiMeshHandle* out);
/// ```
#[no_mangle]
pub extern "C" fn giochi_mesh_create(
    vertices: *const GiochiVertex3D,
    vertex_count: u32,
    indices: *const u32,
    index_count: u32,
    out: *mut u64,
) -> i32 {
    guard(codes::INTERNAL, || {
        if out.is_null() {
            state::invalid_argument("out must not be NULL");
            return codes::INVALID_ARGUMENT;
        }
        if vertices.is_null() {
            state::invalid_argument("vertices must not be NULL");
            return codes::INVALID_ARGUMENT;
        }
        if indices.is_null() {
            state::invalid_argument("indices must not be NULL");
            return codes::INVALID_ARGUMENT;
        }
        if vertex_count == 0 || index_count == 0 {
            state::invalid_argument("a mesh needs at least one vertex and one index");
            return codes::INVALID_ARGUMENT;
        }
        if index_count % 3 != 0 {
            state::invalid_argument("index_count must describe whole triangles");
            return codes::INVALID_ARGUMENT;
        }

        // SAFETY: non-NULL (checked above) and the C contract says the pointers cover
        // `vertex_count` / `index_count` valid elements.
        let (source_vertices, source_indices) = unsafe {
            (
                std::slice::from_raw_parts(vertices, vertex_count as usize),
                std::slice::from_raw_parts(indices, index_count as usize),
            )
        };

        let mut converted = Vec::with_capacity(source_vertices.len());
        for (index, vertex) in source_vertices.iter().enumerate() {
            if !vertex.position.iter().all(|value| value.is_finite()) {
                state::set_last_error(&format!(
                    "vertex {index} has a non-finite position and cannot be projected"
                ));
                return codes::INVALID_ARGUMENT;
            }
            // A non-finite normal becomes zero, which `create_mesh` replaces with a
            // derived face normal.
            let normal = if vertex.normal.iter().all(|value| value.is_finite()) {
                vertex.normal
            } else {
                [0.0, 0.0, 0.0]
            };
            converted.push(giochi_engine_3d::mesh::Vertex::new(
                vertex.position,
                normal,
                vertex.uv,
            ));
        }

        match with_current(|engine| engine.render.create_mesh(&converted, source_indices)) {
            Err(code) => {
                if code == codes::NOT_INITIALIZED {
                    state::set_last_error(
                        "giochi_mesh_create needs a 3D engine; call giochi_engine3d_create first",
                    );
                }
                code
            }
            Ok(Ok(handle)) => {
                // SAFETY: `out` was checked for NULL above.
                unsafe { *out = handle };
                codes::OK
            }
            Ok(Err(err)) => fail3d(&err),
        }
    })
}

// ---- 10.3a GL textures ---------------------------------------------------

/// Decodes an image file and uploads it as a GL texture for meshes.
///
/// The 2D `giochi_texture_create` returns a `sdl2::render` texture, which raw GLES
/// cannot sample; this is the 3D counterpart. Returns `0` on failure, with the reason
/// in [`giochi_get_last_error`](crate::giochi_get_last_error).
///
/// Works on the most recently created engine on this thread (§10.5), like
/// [`giochi_mesh_load_obj`].
///
/// # C ABI
///
/// ```c
/// GiochiGlTextureHandle giochi_gl_texture_create(const char* path);
/// ```
#[no_mangle]
pub extern "C" fn giochi_gl_texture_create(path: *const c_char) -> u64 {
    guard(0, || {
        let Some(path) = string_or_error(path, "path") else {
            return 0;
        };

        match with_current(|engine| engine.render.create_texture(std::path::Path::new(&path))) {
            Err(code) => {
                if code == codes::NOT_INITIALIZED {
                    state::set_last_error(
                        "giochi_gl_texture_create needs a 3D engine; call giochi_engine3d_create first",
                    );
                }
                0
            }
            Ok(Ok(handle)) => handle,
            Ok(Err(err)) => {
                fail3d(&err);
                0
            }
        }
    })
}

/// Uploads raw RGBA8 pixels as a GL texture.
///
/// `pixels` must point to exactly `width * height * 4` bytes. Useful for a procedurally
/// generated texture, which has no file to load.
///
/// Returns `0` on failure.
///
/// # C ABI
///
/// ```c
/// GiochiGlTextureHandle giochi_gl_texture_create_rgba(const uint8_t* pixels,
///                                                     uint32_t width, uint32_t height);
/// ```
#[no_mangle]
pub extern "C" fn giochi_gl_texture_create_rgba(pixels: *const u8, width: u32, height: u32) -> u64 {
    guard(0, || {
        if pixels.is_null() {
            state::invalid_argument("pixels must not be NULL");
            return 0;
        }
        let Some(len) = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(4))
        else {
            state::invalid_argument("width and height overflow the address space");
            return 0;
        };
        if width == 0 || height == 0 {
            state::invalid_argument("width and height must be non-zero");
            return 0;
        }

        // SAFETY: non-NULL (checked above) and the C contract says it points to
        // `width * height * 4` readable bytes.
        let bytes = unsafe { std::slice::from_raw_parts(pixels, len) };

        match with_current(|engine| engine.render.create_texture_from_rgba(width, height, bytes)) {
            Err(code) => {
                if code == codes::NOT_INITIALIZED {
                    state::set_last_error(
                        "giochi_gl_texture_create_rgba needs a 3D engine; call giochi_engine3d_create first",
                    );
                }
                0
            }
            Ok(Ok(handle)) => handle,
            Ok(Err(err)) => {
                fail3d(&err);
                0
            }
        }
    })
}

/// Releases a GL texture. `0` and unknown handles are errors, like the mesh ones.
///
/// # C ABI
///
/// ```c
/// int32_t giochi_gl_texture_destroy(GiochiGlTextureHandle texture);
/// ```
#[no_mangle]
pub extern "C" fn giochi_gl_texture_destroy(texture: u64) -> i32 {
    guard(codes::INTERNAL, || {
        match with_current(|engine| engine.render.destroy_texture(texture)) {
            Err(code) => {
                if code == codes::NOT_INITIALIZED {
                    state::set_last_error(
                        "giochi_gl_texture_destroy needs the 3D engine that created the texture",
                    );
                }
                code
            }
            Ok(true) => codes::OK,
            Ok(false) => {
                state::set_last_error(&format!("texture {texture} does not exist"));
                codes::INVALID_HANDLE
            }
        }
    })
}

/// Width of a GL texture in pixels, or `0` for an invalid handle.
///
/// # C ABI
///
/// ```c
/// uint32_t giochi_gl_texture_width(GiochiGlTextureHandle texture);
/// ```
#[no_mangle]
pub extern "C" fn giochi_gl_texture_width(texture: u64) -> u32 {
    guard(0, || {
        with_current(|engine| {
            engine
                .render
                .texture_size(texture)
                .map(|(width, _)| width)
                .unwrap_or(0)
        })
        .unwrap_or(0)
    })
}

/// Height of a GL texture in pixels, or `0` for an invalid handle.
///
/// # C ABI
///
/// ```c
/// uint32_t giochi_gl_texture_height(GiochiGlTextureHandle texture);
/// ```
#[no_mangle]
pub extern "C" fn giochi_gl_texture_height(texture: u64) -> u32 {
    guard(0, || {
        with_current(|engine| {
            engine
                .render
                .texture_size(texture)
                .map(|(_, height)| height)
                .unwrap_or(0)
        })
        .unwrap_or(0)
    })
}

/// Maps a `GIOCHI_SHADER_*` constant onto the crate's enum. `None` for anything else.
fn builtin_shader(which: i32) -> Option<BuiltinShader> {
    match which {
        crate::abi_types::GIOCHI_SHADER_FLAT => Some(BuiltinShader::Flat),
        crate::abi_types::GIOCHI_SHADER_TEXTURED => Some(BuiltinShader::Textured),
        crate::abi_types::GIOCHI_SHADER_PHONG => Some(BuiltinShader::Phong),
        _ => None,
    }
}

/// Replaces the light every draw in the next frames is shaded with.
///
/// `GIOCHI_SHADER_PHONG` reads it; the other two built-ins ignore it. The light is engine
/// state and stays in force until it is set again — it is not a per-draw argument, because
/// GLES 2.0's uniform limits make uploading once per change cheaper than once per mesh.
///
/// `light` is sanitised on the way in, exactly as the Rust `RenderContext3D::set_light`
/// does: a zero-length direction falls back to straight down instead of normalising to a
/// NaN the driver renders as black, and `shininess` is floored at 1 because `pow(x, 0)`
/// paints a disc rather than a highlight. A caller cannot produce a black frame by leaving
/// a field at zero.
///
/// # Errors
///
/// `GIOCHI_ERROR_INVALID_ARGUMENT` if `light` is NULL; `GIOCHI_ERROR_INVALID_HANDLE` if
/// `engine` is not the live engine on the calling thread.
///
/// # C ABI
///
/// ```c
/// int32_t giochi_renderer3d_set_light(GiochiEngine3D* engine,
///                                     const GiochiDirectionalLight* light);
/// ```
#[no_mangle]
pub extern "C" fn giochi_renderer3d_set_light(
    engine: *mut GiochiEngine3D,
    light: *const DirectionalLight,
) -> i32 {
    guard(codes::INTERNAL, || {
        let engine = match validate_engine(engine) {
            Ok(engine) => engine,
            Err(code) => return code,
        };
        if light.is_null() {
            state::invalid_argument(
                "light must not be NULL; build one with the fields of GiochiDirectionalLight",
            );
            return codes::INVALID_ARGUMENT;
        }

        // SAFETY: non-NULL pointer to a caller-provided `#[repr(C)]` value; copied, never
        // retained. `set_light` sanitises what it stores.
        engine.render.set_light(unsafe { *light });
        codes::OK
    })
}

// ---- 10.4 drawing -------------------------------------------------------

/// Sets the clear colour and clears colour and depth immediately.
///
/// Clearing here as well as in [`giochi_engine3d_begin_frame`] makes the call
/// order-independent: `begin_frame` then `clear` re-clears with the colour just given,
/// and `clear` then `begin_frame` leaves the colour in place for `begin_frame` to use.
///
/// # C ABI
///
/// ```c
/// int32_t giochi_renderer3d_clear(GiochiEngine3D* engine, GiochiColor3D color);
/// ```
#[no_mangle]
pub extern "C" fn giochi_renderer3d_clear(engine: *mut GiochiEngine3D, color: Color3D) -> i32 {
    guard(codes::INTERNAL, || {
        let engine = match validate_engine(engine) {
            Ok(engine) => engine,
            Err(code) => return code,
        };
        engine.render.set_clear_color(color);
        let state = engine.render.state();
        state.clear(engine.render.glow());
        codes::OK
    })
}

/// Draws `mesh` with `shader` at `model`, seen through `camera`, in `color`.
///
/// `model` and `camera` must both be non-NULL: a NULL model is not silently the identity
/// (§10.7), because a caller that forgot it would get a scene piled at the origin with no
/// error.
///
/// Returns `GIOCHI_ERROR_INVALID_HANDLE` when either handle is unknown, and
/// `GIOCHI_ERROR_INVALID_ARGUMENT` when the camera cannot produce a view matrix
/// (`position == target`, or `up` parallel to the view direction).
///
/// # C ABI
///
/// ```c
/// int32_t giochi_renderer3d_draw_mesh(GiochiEngine3D* engine,
///                                     GiochiMeshHandle mesh,
///                                     GiochiShaderHandle shader,
///                                     const GiochiTransform3D* model,
///                                     const GiochiCamera3D* camera,
///                                     GiochiColor3D color);
/// ```
#[no_mangle]
pub extern "C" fn giochi_renderer3d_draw_mesh(
    engine: *mut GiochiEngine3D,
    mesh: u64,
    shader: u64,
    model: *const Transform3D,
    camera: *const Camera3D,
    color: Color3D,
) -> i32 {
    guard(codes::INTERNAL, || {
        let engine = match validate_engine(engine) {
            Ok(engine) => engine,
            Err(code) => return code,
        };
        if model.is_null() {
            state::invalid_argument(
                "model must not be NULL; use giochi_transform3d_identity() for no transform",
            );
            return codes::INVALID_ARGUMENT;
        }
        if camera.is_null() {
            state::invalid_argument("camera must not be NULL; use giochi_camera3d_new()");
            return codes::INVALID_ARGUMENT;
        }

        // SAFETY: the engine was validated above and the two structs are non-NULL pointers
        // to caller-provided `#[repr(C)]` values, read and never retained.
        let (model, camera) = unsafe { (&*model, &*camera) };

        match engine
            .render
            .draw_mesh_colored(mesh, shader, model, camera, color)
        {
            // `false` means a handle was unknown. `Error3D` has no variant for that on
            // the draw path, so it is reported here rather than invented upstream.
            Ok(false) => {
                state::set_last_error(&format!(
                    "mesh {mesh} or shader {shader} does not exist in this engine"
                ));
                codes::INVALID_HANDLE
            }
            Ok(true) => codes::OK,
            // A model matrix that is not finite, or a camera with no view matrix, arrives as
            // `InvalidArgument` and maps to the code the caller expects.
            Err(err) => fail3d(&err),
        }
    })
}

/// Draws `mesh` sampling `texture`, with `tint` multiplied over the sample.
///
/// `shader` must declare `uniform sampler2D u_texture`; `GIOCHI_SHADER_TEXTURED` does.
/// `tint` is multiplied into the sampled colour, so white shows the image unchanged.
///
/// The texture is bound to unit 0 for the duration of the draw.
///
/// # C ABI
///
/// ```c
/// int32_t giochi_renderer3d_draw_mesh_textured(GiochiEngine3D* engine,
///                                              GiochiMeshHandle mesh,
///                                              GiochiShaderHandle shader,
///                                              GiochiGlTextureHandle texture,
///                                              const GiochiTransform3D* model,
///                                              const GiochiCamera3D* camera,
///                                              GiochiColor3D tint);
/// ```
#[no_mangle]
pub extern "C" fn giochi_renderer3d_draw_mesh_textured(
    engine: *mut GiochiEngine3D,
    mesh: u64,
    shader: u64,
    texture: u64,
    model: *const Transform3D,
    camera: *const Camera3D,
    tint: Color3D,
) -> i32 {
    guard(codes::INTERNAL, || {
        let engine = match validate_engine(engine) {
            Ok(engine) => engine,
            Err(code) => return code,
        };
        if model.is_null() {
            state::invalid_argument(
                "model must not be NULL; use giochi_transform3d_identity() for no transform",
            );
            return codes::INVALID_ARGUMENT;
        }
        if camera.is_null() {
            state::invalid_argument("camera must not be NULL; use giochi_camera3d_new()");
            return codes::INVALID_ARGUMENT;
        }

        // SAFETY: the engine was validated above and the two structs are non-NULL
        // pointers to caller-provided `#[repr(C)]` values, read and never retained.
        let (model, camera) = unsafe { (&*model, &*camera) };

        match engine
            .render
            .draw_mesh_textured(mesh, shader, texture, model, camera, tint)
        {
            Ok(false) => {
                state::set_last_error(&format!(
                    "mesh {mesh}, shader {shader} or texture {texture} does not exist in this engine"
                ));
                codes::INVALID_HANDLE
            }
            Ok(true) => codes::OK,
            Err(err) => fail3d(&err),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_shader_constant_maps_to_the_matching_variant() {
        // Pinned because the numbers are in the header: renumbering them would silently
        // give a C caller a different shader.
        assert_eq!(builtin_shader(0), Some(BuiltinShader::Flat));
        assert_eq!(builtin_shader(1), Some(BuiltinShader::Textured));
        assert_eq!(builtin_shader(2), Some(BuiltinShader::Phong));
        assert_eq!(builtin_shader(-1), None);
        assert_eq!(builtin_shader(3), None);
        assert_eq!(builtin_shader(i32::MAX), None);
    }

    #[test]
    fn the_transform_helpers_are_usable_from_c() {
        // Rust's `Default` is the identity, so a Rust caller cannot get this wrong. A **C**
        // caller writing `GiochiTransform3D t = {0};` gets scale (0,0,0) and draws nothing,
        // which is the trap `giochi_transform3d_identity` exists to avoid -- and the reason
        // the helper is worth a function rather than a comment in the header.
        let c_style_zeroed = Transform3D {
            position: Vec3::ZERO,
            rotation: giochi_engine_3d::math3d::Quat::IDENTITY,
            scale: Vec3::ZERO,
            _reserved: [0; 4],
        };
        assert_eq!(c_style_zeroed.scale.x, 0.0, "the trap this function avoids");
        assert_eq!(
            Transform3D::default().scale.x,
            1.0,
            "Rust's default is safe"
        );

        let identity = giochi_transform3d_identity();
        assert_eq!(identity.scale.x, 1.0);
        assert_eq!(identity.scale.y, 1.0);
        assert_eq!(identity.scale.z, 1.0);
        assert_eq!(identity.position.x, 0.0);
        assert_eq!(identity._reserved, [0; 4]);
    }

    #[test]
    fn from_translation_keeps_unit_scale() {
        let model = giochi_transform3d_from_translation(Vec3::new(1.0, 2.0, 3.0));

        assert_eq!(model.position.x, 1.0);
        assert_eq!(model.position.y, 2.0);
        assert_eq!(model.position.z, 3.0);
        assert_eq!(model.scale.x, 1.0, "translation must not zero the scale");
    }

    #[test]
    fn a_new_camera_lets_the_renderer_fill_the_aspect() {
        // 0.0 means "ask the drawable". A camera that guessed 16:9 would letterbox a
        // console's 4:3 panel instead of using it.
        let camera = giochi_camera3d_new();
        assert_eq!(camera.aspect, 0.0);
        assert!(camera.position.is_finite());
    }

    #[test]
    fn null_pointers_are_rejected_rather_than_dereferenced() {
        assert_eq!(
            giochi_mesh_load_obj(core::ptr::null(), core::ptr::null_mut()),
            codes::INVALID_ARGUMENT
        );
        assert_eq!(
            giochi_mesh_create_cube(1.0, core::ptr::null_mut()),
            codes::INVALID_ARGUMENT
        );
        assert_eq!(
            giochi_shader_builtin(0, core::ptr::null_mut()),
            codes::INVALID_ARGUMENT
        );
        assert_eq!(
            giochi_renderer3d_clear(core::ptr::null_mut(), Color3D::WHITE),
            codes::INVALID_ARGUMENT
        );
        assert_eq!(
            giochi_engine3d_begin_frame(core::ptr::null_mut()),
            codes::INVALID_ARGUMENT
        );
        assert_eq!(
            giochi_engine3d_end_frame(core::ptr::null_mut()),
            codes::INVALID_ARGUMENT
        );

        let light = DirectionalLight::default();
        assert_eq!(
            giochi_renderer3d_set_light(core::ptr::null_mut(), &light),
            codes::INVALID_ARGUMENT
        );
    }

    #[test]
    fn destroying_null_is_a_no_op() {
        // Idempotent teardown matters: a C caller that bails out of three branches should
        // not have to remember whether it created the engine.
        giochi_engine3d_destroy(core::ptr::null_mut());
        assert_eq!(state::last_error_text(), "");
    }

    #[test]
    fn the_mesh_functions_report_not_initialised_without_an_engine() {
        // No engine has been created in this test binary, so every one of them must say so
        // instead of dereferencing a null slot.
        let mut handle = 0u64;
        assert_eq!(
            giochi_mesh_create_cube(1.0, &mut handle),
            codes::NOT_INITIALIZED
        );
        assert_eq!(handle, 0, "a failed call must not write the out parameter");

        assert_eq!(giochi_mesh_destroy(1), codes::NOT_INITIALIZED);
        assert_eq!(
            giochi_shader_builtin(0, &mut handle),
            codes::NOT_INITIALIZED
        );
        assert_eq!(giochi_shader_destroy(1), codes::NOT_INITIALIZED);
    }

    #[test]
    fn an_unknown_builtin_is_rejected_before_touching_the_engine() {
        let mut handle = 0u64;
        assert_eq!(
            giochi_shader_builtin(99, &mut handle),
            codes::INVALID_ARGUMENT
        );
        assert!(
            state::last_error_text().contains("not a built-in shader"),
            "the message must say what was expected"
        );
        assert_eq!(handle, 0);
    }

    #[test]
    fn a_pointer_that_is_not_the_live_engine_is_refused() {
        // No engine exists in this test, so every pointer is "not the live engine". Before
        // `validate_engine` existed the code checked only for NULL and then dereferenced
        // whatever it was given, with a SAFETY comment claiming a check that was not there.
        // An opaque pointer in C is only as good as its owner, but the one engine per thread
        // rule makes this comparison possible, and it turns a use-after-free into a code.
        let dangling = core::ptr::NonNull::<GiochiEngine3D>::dangling().as_ptr();

        assert_eq!(giochi_engine3d_begin_frame(dangling), codes::INVALID_HANDLE);
        assert_eq!(giochi_engine3d_end_frame(dangling), codes::INVALID_HANDLE);
        assert_eq!(
            giochi_renderer3d_clear(dangling, Color3D::WHITE),
            codes::INVALID_HANDLE
        );
        assert_eq!(
            giochi_renderer3d_set_light(dangling, &DirectionalLight::default()),
            codes::INVALID_HANDLE
        );
        assert!(
            state::last_error_text().contains("already destroyed")
                || state::last_error_text().contains("another thread"),
            "the message must say why: {}",
            state::last_error_text()
        );

        // A NULL pointer is a mistake in the call, not in its lifetime, so it keeps the
        // distinct code part I uses for a bad argument.
        assert_eq!(
            giochi_engine3d_begin_frame(core::ptr::null_mut()),
            codes::INVALID_ARGUMENT
        );
    }
}
