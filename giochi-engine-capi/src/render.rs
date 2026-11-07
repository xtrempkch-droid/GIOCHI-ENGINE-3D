//! Textures, drawing and the camera.
//!
//! Two access paths, chosen automatically:
//!
//! * the **texture registry** is shared (`Rc<TextureRegistry>`), so `giochi_texture_*`
//!   works from any callback and from outside the loop;
//! * **drawing** needs `&mut RenderContext`, which is only borrowable inside
//!   `render` (where the engine lends it) or outside a run. Calling a draw function
//!   from `update` reports `GIOCHI_ERROR_SCENE` instead of panicking.

use core::ffi::c_char;

use giochi_engine::error::codes;
use giochi_engine::math::{Color, Rect};
use giochi_engine::render::{Camera, Sprite};

use crate::abi_types::GIOCHI_ERROR_INVALID_ARGUMENT;
use crate::state::{self, guard};

const RENDER_HINT: &str = "drawing is only available inside a render callback (or outside a run); \
     simulation belongs in update";

/// Creates a texture by loading an image file (PNG, JPEG, BMP, …).
///
/// Goes through the resource cache: loading the same path twice returns the same
/// handle and uploads once.
///
/// Returns `0` on failure; query [`giochi_get_last_error`](crate::giochi_get_last_error).
///
/// # C ABI
///
/// ```c
/// GiochiTexture giochi_texture_create(const char* path);
/// ```
#[no_mangle]
pub extern "C" fn giochi_texture_create(path: *const c_char) -> giochi_engine::TextureHandle {
    guard(0, || {
        let Some(path) = string_or_error(path, "texture path") else {
            return 0;
        };
        match state::with_render(RENDER_HINT, |render| render.load_texture(&path)) {
            Ok(Ok(handle)) => handle,
            Ok(Err(err)) => {
                state::fail(&err);
                0
            }
            Err(_) => 0,
        }
    })
}

/// Creates a texture from an encoded image already in memory.
///
/// The format is sniffed from the bytes. Use this on targets without a filesystem,
/// or to ship assets inside the binary.
///
/// # C ABI
///
/// ```c
/// GiochiTexture giochi_texture_create_memory(const uint8_t* data, size_t len);
/// ```
#[no_mangle]
pub extern "C" fn giochi_texture_create_memory(
    data: *const u8,
    len: usize,
) -> giochi_engine::TextureHandle {
    guard(0, || {
        if data.is_null() || len == 0 {
            state::invalid_argument("image buffer must not be NULL or empty");
            return 0;
        }
        // SAFETY: the contract requires `data` to point at `len` readable bytes for
        // the duration of the call.
        let bytes = unsafe { core::slice::from_raw_parts(data, len) };
        match state::with_textures(|textures| textures.create_texture_from_bytes(bytes)) {
            Ok(Ok(handle)) => handle,
            Ok(Err(err)) => {
                state::fail(&err);
                0
            }
            Err(_) => 0,
        }
    })
}

/// Releases a texture.
///
/// Returns `GIOCHI_ERROR_INVALID_HANDLE` for `0` or an already-destroyed handle: a
/// double destroy is always reported.
///
/// # C ABI
///
/// ```c
/// int32_t giochi_texture_destroy(GiochiTexture texture);
/// ```
#[no_mangle]
pub extern "C" fn giochi_texture_destroy(texture: giochi_engine::TextureHandle) -> i32 {
    guard(codes::INTERNAL, || {
        match state::with_textures(|textures| textures.destroy_texture(texture)) {
            Ok(true) => codes::OK,
            Ok(false) => {
                state::set_last_error(&format!(
                    "texture handle {texture} is not live (0 or already destroyed)"
                ));
                codes::INVALID_HANDLE
            }
            Err(code) => code,
        }
    })
}

/// Returns the texture width in pixels, or `0` for an invalid handle.
///
/// # C ABI
///
/// ```c
/// uint32_t giochi_texture_width(GiochiTexture texture);
/// ```
#[no_mangle]
pub extern "C" fn giochi_texture_width(texture: giochi_engine::TextureHandle) -> u32 {
    guard(0, || {
        state::with_textures(|textures| textures.dimensions(texture))
            .ok()
            .flatten()
            .map(|(width, _)| width)
            .unwrap_or(0)
    })
}

/// Returns the texture height in pixels, or `0` for an invalid handle.
///
/// # C ABI
///
/// ```c
/// uint32_t giochi_texture_height(GiochiTexture texture);
/// ```
#[no_mangle]
pub extern "C" fn giochi_texture_height(texture: giochi_engine::TextureHandle) -> u32 {
    guard(0, || {
        state::with_textures(|textures| textures.dimensions(texture))
            .ok()
            .flatten()
            .map(|(_, height)| height)
            .unwrap_or(0)
    })
}

/// Fills the frame with `color`.
///
/// The engine does this itself before every `render` callback, so a scene only needs
/// it to override the clear colour.
///
/// # C ABI
///
/// ```c
/// int32_t giochi_render_clear(GiochiColor color);
/// ```
#[no_mangle]
pub extern "C" fn giochi_render_clear(color: Color) -> i32 {
    guard(codes::INTERNAL, || {
        match state::with_render(RENDER_HINT, |render| render.clear(color)) {
            Ok(()) => codes::OK,
            Err(code) => code,
        }
    })
}

/// Draws a filled rectangle in **screen** space.
///
/// # C ABI
///
/// ```c
/// int32_t giochi_render_draw_rect(const GiochiRect* rect, GiochiColor color);
/// ```
#[no_mangle]
pub extern "C" fn giochi_render_draw_rect(rect: *const Rect, color: Color) -> i32 {
    guard(codes::INTERNAL, || {
        let Some(rect) = copy_or_error(rect, "rect") else {
            return GIOCHI_ERROR_INVALID_ARGUMENT;
        };
        match state::with_render(RENDER_HINT, |render| render.draw_rect(rect, color)) {
            Ok(()) => codes::OK,
            Err(code) => code,
        }
    })
}

/// Draws a texture in **screen** space.
///
/// A zero-sized `src` means "the whole texture". An unknown handle is reported and
/// nothing is drawn; a destination that rounds to zero pixels is a successful
/// no-op.
///
/// # C ABI
///
/// ```c
/// int32_t giochi_render_draw_texture(GiochiTexture texture,
///                                    const GiochiRect* dst,
///                                    const GiochiRect* src);
/// ```
#[no_mangle]
pub extern "C" fn giochi_render_draw_texture(
    texture: giochi_engine::TextureHandle,
    dst: *const Rect,
    src: *const Rect,
) -> i32 {
    guard(codes::INTERNAL, || {
        let Some(dst) = copy_or_error(dst, "dst") else {
            return GIOCHI_ERROR_INVALID_ARGUMENT;
        };
        let src = if src.is_null() {
            Rect::ZERO
        } else {
            // SAFETY: non-NULL pointer to a caller-provided struct; copied.
            unsafe { *src }
        };
        match state::with_render(RENDER_HINT, |render| render.draw_texture(texture, dst, src)) {
            Ok(Ok(())) => codes::OK,
            Ok(Err(err)) => state::fail(&err),
            Err(code) => code,
        }
    })
}

/// Draws a sprite, treating its rectangle as **world** space.
///
/// The current camera is applied. Never produces NaN geometry: non-finite fields
/// are sanitised by the engine.
///
/// # C ABI
///
/// ```c
/// int32_t giochi_sprite_draw(const GiochiSprite* sprite);
/// ```
#[no_mangle]
pub extern "C" fn giochi_sprite_draw(sprite: *const Sprite) -> i32 {
    guard(codes::INTERNAL, || {
        let Some(sprite) = copy_or_error(sprite, "sprite") else {
            return GIOCHI_ERROR_INVALID_ARGUMENT;
        };
        match state::with_render(RENDER_HINT, |render| render.draw_sprite(&sprite)) {
            Ok(Ok(())) => codes::OK,
            Ok(Err(err)) => state::fail(&err),
            Err(code) => code,
        }
    })
}

/// Writes the render target size, in physical pixels, into the out-parameters.
///
/// Either out-parameter may be NULL. Both are written only on success.
///
/// # C ABI
///
/// ```c
/// int32_t giochi_render_size(uint32_t* out_width, uint32_t* out_height);
/// ```
#[no_mangle]
pub extern "C" fn giochi_render_size(out_width: *mut u32, out_height: *mut u32) -> i32 {
    guard(codes::INTERNAL, || {
        match state::with_render(RENDER_HINT, |render| render.size()) {
            Ok((width, height)) => {
                // SAFETY: the contract requires each non-NULL out-parameter to be a
                // writable `uint32_t`.
                unsafe {
                    if !out_width.is_null() {
                        *out_width = width;
                    }
                    if !out_height.is_null() {
                        *out_height = height;
                    }
                }
                codes::OK
            }
            Err(code) => code,
        }
    })
}

/// Presents the frame (page flip / buffer swap).
///
/// Presenting cannot fail in SDL2, so this returns nothing.
///
/// # C ABI
///
/// ```c
/// void giochi_render_present(void);
/// ```
#[no_mangle]
pub extern "C" fn giochi_render_present() {
    guard((), || {
        let _ = state::with_render(RENDER_HINT, |render| render.present());
    })
}

/// Sets the camera that `giochi_sprite_draw` uses.
///
/// A `zoom` that is not finite or not positive is treated as `1.0`.
///
/// # C ABI
///
/// ```c
/// void giochi_camera_set(const GiochiCamera* camera);
/// ```
#[no_mangle]
pub extern "C" fn giochi_camera_set(camera: *const Camera) {
    guard((), || {
        if camera.is_null() {
            state::invalid_argument("camera must not be NULL");
            return;
        }
        // SAFETY: non-NULL pointer to a caller-provided struct; copied.
        let camera = unsafe { *camera };
        let _ = state::with_render(RENDER_HINT, |render| render.set_camera(camera));
    })
}

/// Writes the current camera into `out_camera`.
///
/// When none was set, this is the default: viewport = the whole window, zoom = 1,
/// position = the world origin.
///
/// # C ABI
///
/// ```c
/// void giochi_camera_get(GiochiCamera* out_camera);
/// ```
#[no_mangle]
pub extern "C" fn giochi_camera_get(out_camera: *mut Camera) {
    guard((), || {
        if out_camera.is_null() {
            state::invalid_argument("out_camera must not be NULL");
            return;
        }
        if let Ok(camera) = state::with_render(RENDER_HINT, |render| render.camera()) {
            // SAFETY: non-NULL, caller-owned writable struct.
            unsafe { *out_camera = camera };
        }
    })
}

/// Copies `*pointer`, reporting a NULL argument.
fn copy_or_error<T: Copy>(pointer: *const T, name: &str) -> Option<T> {
    if pointer.is_null() {
        state::set_last_error(&format!("{name} must not be NULL"));
        return None;
    }
    // SAFETY: non-NULL pointer to a caller-provided value; copied immediately.
    Some(unsafe { *pointer })
}

/// Reads a NUL-terminated string, reporting a NULL/invalid argument.
fn string_or_error(pointer: *const c_char, name: &str) -> Option<String> {
    match state::cstr(pointer) {
        Some(text) => Some(text),
        None => {
            state::set_last_error(&format!("{name} must be a non-NULL, valid UTF-8 string"));
            None
        }
    }
}
