# GIOCHI-ENGINE — C ABI Contract

**Contract version: `GIOCHI_ABI_VERSION 1`**
**Engine version: `0.1.0`**
**Status: authoritative.** This document is the law of the project. If a Rust type
cannot be expressed in the C form below, the *type* is redesigned — the contract
is never relaxed.

The header `giochi-engine-capi/include/giochi.h` is generated from this contract
by `cbindgen` (phase 7) and must match it exactly.

Implementation progress: phase 0 delivered the value types and the two version
queries; phase 1 delivered the platform layer behind `giochi_engine_init`
(`Platform`, `Window`, `Time`, `GameLoop`); phase 2 delivered the 2D renderer and
the texture registry behind `giochi_texture_*`/`giochi_render_*`. The `extern "C"`
functions themselves land in phase 7.

---

## 1. Scope

The C ABI exposes the whole engine:

* lifecycle of the single engine instance,
* 2D rendering (clear, rect, texture, sprite, camera),
* texture creation/destruction,
* keyboard, mouse and gamepad queries,
* sound effects and music,
* asset reading,
* scenes, as handle + callback table.

Out of scope for ABI v1: 3D, custom shaders, render pipelines, font rendering
(`GiochiFont` is reserved), networking, and anything that requires the consumer
to hold a Rust type.

---

## 2. Ground rules

### 2.1 Handles

```c
typedef uint64_t GiochiTexture;
typedef uint64_t GiochiSound;
typedef uint64_t GiochiMusic;
typedef uint64_t GiochiScene;
typedef uint64_t GiochiFont;   /* reserved in ABI v1, no functions */
```

* `0` is the null/invalid handle. The engine never issues `0`.
* Handles are never reused, so a stale handle can never silently alias a new
  resource.
* Lifetime is explicit: whatever is created must be destroyed by the caller
  (`giochi_texture_create` → `giochi_texture_destroy`, and so on). There is no
  RAII at the boundary.
* Destroying an unknown or already-destroyed handle is a *reported error*, never
  undefined behaviour.

### 2.2 Strings

* All `const char*` parameters are **NUL-terminated UTF-8** and are only read for
  the duration of the call. The engine never retains the pointer.
* Fixed-size string buffers (`GiochiEngineConfig.title`) are truncated on a UTF-8
  character boundary and always NUL-terminated.
* Strings returned by the engine (`giochi_get_last_error`) are owned by the
  engine, valid until the next engine call on the same thread, and must not be
  freed or modified by the caller.

### 2.3 Errors

* Every fallible function returns `int32_t`: `0` on success, a negative code from
  [§4](#4-error-codes) on failure.
* Functions that return a handle signal failure by returning `0`; the reason is
  then available through `giochi_get_last_error()`.
* `giochi_get_last_error()` is **thread-local**. It returns `""` when the last
  call succeeded.
* No Rust `panic` may cross the boundary: the C ABI implementation catches
  unwinding and reports `GIOCHI_ERROR_INTERNAL` (phase 7).

### 2.4 Reserved fields

Every struct that crosses the boundary ends with `uint32_t _reserved[N];` with
`N >= 4`.

* The **caller must zero** `_reserved` before passing the struct to the engine.
* The **engine ignores** its contents.
* New fields may be added for a future ABI revision only when the struct's total
  size and alignment stay unchanged. Otherwise `GIOCHI_ABI_VERSION` is bumped.

### 2.5 Threading

The engine is single-threaded. All calls must be made from the thread that called
`giochi_engine_init`. `giochi_get_last_error()` is the only exception: each thread
has its own slot.

### 2.6 Panics, allocation and unwind

* Memory allocated inside the engine is owned by the engine. The consumer never
  has to free memory returned by a `giochi_*` function.
* Out-parameters are written only on success unless documented otherwise.

---

## 3. Types

All sizes assume a 64-bit target (`aarch64-unknown-linux-gnu`,
`x86_64-unknown-linux-gnu`). They are asserted by
`giochi-engine/tests/abi_layout.rs`; changing one of these numbers breaks the ABI.

### 3.1 `GiochiVec2`

```c
typedef struct GiochiVec2 {
    float    x;
    float    y;
    uint32_t _reserved[4];
} GiochiVec2;
```

| Property | Value |
|---|---|
| Size | 24 |
| Alignment | 4 |
| Rust | `giochi_engine::math::Vec2` |

### 3.2 `GiochiRect`

`x`/`y` is the top-left corner; `w`/`h` are the extents.

```c
typedef struct GiochiRect {
    float    x, y, w, h;
    uint32_t _reserved[4];
} GiochiRect;
```

| Property | Value |
|---|---|
| Size | 32 |
| Alignment | 4 |
| Rust | `giochi_engine::math::Rect` |

### 3.3 `GiochiColor`

```c
typedef struct GiochiColor {
    uint8_t  r, g, b, a;   /* a = 255 is opaque */
    uint32_t _reserved[4];
} GiochiColor;
```

| Property | Value |
|---|---|
| Size | 20 |
| Alignment | 4 |
| Rust | `giochi_engine::math::Color` |

### 3.4 `GiochiSprite`

```c
typedef struct GiochiSprite {
    GiochiTexture texture;
    float         x, y, w, h;   /* destination rect, world space */
    uint32_t      _reserved[8];
} GiochiSprite;
```

| Property | Value |
|---|---|
| Size | 56 |
| Alignment | 8 |
| Rust | `giochi_engine::render::Sprite` |

### 3.5 `GiochiCamera`

The camera maps world space to screen space:

```text
screen = viewport.center + (world - camera.position) * zoom
```

`x`/`y` are the world coordinates placed at the centre of `viewport`. A `zoom` of
`1.0` maps one world unit to one pixel.

```c
typedef struct GiochiCamera {
    float       x, y, zoom;
    GiochiRect  viewport;
    uint32_t    _reserved[4];
} GiochiCamera;
```

| Property | Value |
|---|---|
| Size | 60 |
| Alignment | 4 |
| Rust | `giochi_engine::render::Camera` |

### 3.6 `GiochiTime`

```c
typedef struct GiochiTime {
    float    delta;        /* seconds in the previous frame */
    float    elapsed;      /* seconds since engine start */
    uint64_t frame_count;  /* frames since engine start */
    uint32_t _reserved[4];
} GiochiTime;
```

| Property | Value |
|---|---|
| Size | 32 |
| Alignment | 8 |
| Rust | `giochi_engine::core::time::Time` |

`GiochiTime` is always passed **by pointer to const data**: the engine owns the
value and its lifetime is the duration of the callback.

### 3.7 `GiochiEngineConfig`

```c
typedef struct GiochiEngineConfig {
    uint32_t width;       /* physical pixels */
    uint32_t height;      /* physical pixels */
    uint32_t target_fps;  /* 0 = uncapped */
    uint32_t vsync;       /* 0 = off, non-zero = on */
    char     title[64];   /* NUL-terminated UTF-8 */
    uint32_t _reserved[8];
} GiochiEngineConfig;
```

| Property | Value |
|---|---|
| Size | 112 |
| Alignment | 4 |
| Rust | `giochi_engine::core::engine::EngineConfig` |

### 3.8 `GiochiSceneCallbacks`

Scenes at the C boundary are a callback table plus an opaque `user_data` pointer.
The engine *copies* the table when `giochi_scene_create` is called; `user_data`
must stay valid until `giochi_scene_destroy`.

```c
typedef struct GiochiSceneCallbacks {
    void* user_data;
    void (*on_enter)(void* user_data);                     /* may be NULL */
    void (*on_exit)(void* user_data);                      /* may be NULL */
    void (*update)(void* user_data, const GiochiTime* t);  /* required */
    void (*render)(void* user_data);                       /* required */
    uint32_t _reserved[4];
} GiochiSceneCallbacks;
```

| Property | Value |
|---|---|
| Size (LP64) | 56 |
| Alignment | 8 |

Callbacks must not unwind (in C++ terms: must not throw) and must not call
`giochi_engine_shutdown`. Rendering inside `render` uses the same
`giochi_render_*` functions as the main thread.

---

## 4. Error codes

```c
enum {
    GIOCHI_ERROR_OK              =   0,
    GIOCHI_ERROR_INVALID_HANDLE  =  -1,
    GIOCHI_ERROR_INVALID_ARGUMENT=  -2,
    GIOCHI_ERROR_PLATFORM        =  -3,
    GIOCHI_ERROR_RENDER          =  -4,
    GIOCHI_ERROR_ASSET           =  -5,
    GIOCHI_ERROR_AUDIO           =  -6,
    GIOCHI_ERROR_SCENE           =  -7,
    GIOCHI_ERROR_NOT_INITIALIZED =  -8,
    GIOCHI_ERROR_UNSUPPORTED     =  -9,
    GIOCHI_ERROR_INTERNAL        = -99
};
```

These values are part of the ABI and must not be renumbered. The Rust
counterparts are `giochi_engine::error::codes`.

---

## 5. Functions

Every function below is declared in `giochi.h` in phase 7. "Phase" is the
implementation phase in `PROGRESS.md`, not the ABI version.

### 5.1 Version and diagnostics

```c
/* Always available. */
uint32_t    giochi_abi_version(void);      /* == 1               (phase 0, done) */
uint32_t    giochi_engine_version(void);   /* major<<16|minor<<8|patch (phase 0, done) */
const char* giochi_get_last_error(void);   /* thread-local, never NULL (phase 7) */
```

### 5.2 Engine lifecycle

```c
int32_t giochi_engine_init(const GiochiEngineConfig* config);  /* phase 1 */
void    giochi_engine_shutdown(void);                          /* phase 1 */
int32_t giochi_engine_run(void);         /* blocks until quit     (phase 5) */
int32_t giochi_engine_run_scene(GiochiScene scene); /* phase 5 */
int32_t giochi_engine_step(float frame_time);/* one iteration    (phase 5) */
void    giochi_engine_request_quit(void);                      /* phase 5 */
int32_t giochi_engine_is_running(void);   /* 1/0                  (phase 5) */
```

* `giochi_engine_init` may be called only once per process (until shutdown).
* `giochi_engine_shutdown` is idempotent. It does not free handles: destroy
  textures, sounds, music and scenes first for deterministic behaviour.
* `giochi_engine_run` pumps events, runs fixed-timestep updates and renders until
  `Escape`, the window close button or `giochi_engine_request_quit`.

**Video driver negotiation** (implemented in phase 1, in Rust, as
`Platform::new`):

| Situation | Behaviour |
|---|---|
| `SDL_VIDEODRIVER` set by the caller | Used verbatim. Never replaced, never retried. |
| `SDL_VIDEODRIVER` unset | Engine selects `kmsdrm`. |
| Engine-selected `kmsdrm` cannot start | Warning logged, retried with SDL's default desktop driver. |
| Initialisation fails in every case | `GIOCHI_ERROR_PLATFORM`; the message names the driver, SDL's error, the DRM devices present and the hints below. |

The engine never rewrites `SDL_VIDEODRIVER` when the caller set it, so a
production deployment can pin `kmsdrm` and let the engine fail loudly rather than
fall back. Hints honoured by the KMSDRM backend: `SDL_KMSDRM_REQUIRE_DRM_MASTER`,
`SDL_KMSDRM_DEVICE_INDEX`, `SDL_VIDEO_EGL_DRIVER`, `SDL_VIDEO_GL_DRIVER`.

### 5.3 Textures

```c
GiochiTexture giochi_texture_create(const char* path);                       /* phase 2 */
GiochiTexture giochi_texture_create_memory(const uint8_t* data, size_t len); /* phase 7 */
void          giochi_texture_destroy(GiochiTexture texture);                 /* phase 2 */
uint32_t      giochi_texture_width(GiochiTexture texture);                   /* phase 2 */
uint32_t      giochi_texture_height(GiochiTexture texture);                  /* phase 2 */
```

`/* Returns 0 on failure; query giochi_get_last_error(). */`

`giochi_texture_create` goes through the engine's resource cache (phase 4): loading
the same path twice returns the same handle and charges one upload. Until phase 4
each call uploads its own texture. Width/height return `0` for an invalid handle,
and destroying an unknown or already-destroyed handle returns
`GIOCHI_ERROR_INVALID_HANDLE` — a double destroy is always reported.

In the Rust API the same operations are `TextureRegistry::create_texture`,
`create_texture_from_rgba`, `destroy_texture`, `dimensions` and `len`, or their
`RenderContext` delegations. `create_texture_from_rgba` has no ABI v1 counterpart
because it takes a pixel buffer; `giochi_texture_create_memory` covers it in
phase 7.

### 5.4 Rendering

```c
int32_t giochi_render_clear(GiochiColor color);                                         /* phase 2 */
int32_t giochi_render_draw_rect(const GiochiRect* rect, GiochiColor color);             /* phase 2 */
int32_t giochi_render_draw_texture(GiochiTexture texture,
                                   const GiochiRect* dst,
                                   const GiochiRect* src);                             /* phase 2 */
int32_t giochi_sprite_draw(const GiochiSprite* sprite);                                 /* phase 2 */
int32_t giochi_render_size(uint32_t* out_width, uint32_t* out_height);                  /* phase 2 */
void    giochi_render_present(void);                                                    /* phase 2 */
void    giochi_camera_set(const GiochiCamera* camera);                                  /* phase 2 */
void    giochi_camera_get(GiochiCamera* out_camera);                                    /* phase 2 */
```

* Coordinates are physical pixels of the window. `giochi_sprite_draw` treats the
  sprite rectangle as **world space** and applies the current camera;
  `giochi_render_draw_rect` and `giochi_render_draw_texture` operate in screen
  space.
* `src` equal to a zero rectangle means "the whole texture".
* `giochi_render_present` has no return value: presenting cannot fail in SDL2.
* `giochi_camera_get` writes the default camera (viewport = window, zoom = 1)
  when none was set.
* The camera transform, implemented in phase 2 and shared by Rust and C, is
  `screen = viewport.center + (world - camera.position) * zoom`. A `zoom` that is
  not finite or not positive is treated as `1.0`; non-finite coordinates are
  treated as `0.0`. `giochi_sprite_draw` therefore never produces NaN geometry.
* Float rectangles are rounded to SDL's integer rectangles. A destination whose
  width or height rounds to zero is a no-op that returns success.
* Drawing with an unknown handle returns `GIOCHI_ERROR_INVALID_HANDLE` and draws
  nothing.

### 5.5 Input

```c
int32_t giochi_input_key_down(uint32_t keycode);          /* phase 3 */
int32_t giochi_input_key_pressed(uint32_t keycode);       /* phase 3 */
int32_t giochi_input_key_released(uint32_t keycode);      /* phase 3 */
float   giochi_input_mouse_x(void);                       /* phase 3 */
float   giochi_input_mouse_y(void);                       /* phase 3 */
int32_t giochi_input_mouse_down(uint32_t button);         /* phase 3 */
float   giochi_input_mouse_wheel(void);                   /* phase 3 */
int32_t giochi_gamepad_connected(void);                   /* phase 3 */
float   giochi_gamepad_axis(uint32_t axis);               /* phase 3 */
int32_t giochi_gamepad_button_down(uint32_t button);      /* phase 3 */
```

* `keycode` is the SDL keycode value (`SDLK_*`) as `uint32_t`; mouse buttons use
  `SDL_BUTTON_*` (1 = left, 2 = middle, 3 = right, 4/5 = extra); controller axes
  and buttons use `SDL_CONTROLLER_AXIS_*` / `SDL_CONTROLLER_BUTTON_*`.
* Boolean results are `1`/`0`.
* `pressed`/`released` are true for exactly one frame.
* Queries are valid only after `giochi_engine_init`.

### 5.6 Audio

```c
GiochiSound giochi_sound_load(const char* path);                 /* phase 4 */
void        giochi_sound_destroy(GiochiSound sound);             /* phase 4 */
int32_t     giochi_sound_play(GiochiSound sound, int32_t loops); /* phase 4 */
void        giochi_audio_set_volume(int32_t volume);             /* phase 4, 0..=128 */

GiochiMusic giochi_music_load(const char* path);                 /* phase 4 */
void        giochi_music_destroy(GiochiMusic music);             /* phase 4 */
int32_t     giochi_music_play(GiochiMusic music, int32_t loops); /* phase 4 */
void        giochi_music_pause(void);                            /* phase 4 */
void        giochi_music_resume(void);                           /* phase 4 */
void        giochi_music_stop(void);                             /* phase 4 */
int32_t     giochi_music_is_playing(void);                       /* phase 4 */
```

* `loops` is the number of *extra* repetitions: `0` plays once, `-1` loops
  forever.
* A failing audio device degrades gracefully: load/play return an error but the
  rest of the engine keeps running.

### 5.7 Assets

```c
int32_t giochi_asset_size(const char* path, size_t* out_size);                       /* phase 4 */
int32_t giochi_asset_read(const char* path,
                          uint8_t* buffer,
                          size_t capacity,
                          size_t* out_read);                                         /* phase 4 */
```

Assets larger than `GIOCHI_MAX_ASSET_SIZE` (64 MiB) are rejected with
`GIOCHI_ERROR_ASSET`. `giochi_asset_read` fails instead of truncating when
`capacity` is smaller than the file.

```c
#define GIOCHI_MAX_ASSET_SIZE (64u * 1024u * 1024u)
```

### 5.8 Scenes

```c
GiochiScene giochi_scene_create(const GiochiSceneCallbacks* callbacks);  /* phase 5 */
void        giochi_scene_destroy(GiochiScene scene);                     /* phase 5 */
```

`giochi_scene_create` copies the callback table and returns `0` if `callbacks`,
`update` or `render` is `NULL`.

---

## 6. What the ABI deliberately does not have

| Not exposed | Reason |
|---|---|
| `GiochiRenderContext*` as a first-class object | ABI v1 uses a single engine-owned context behind `giochi_render_*`; no opaque pointer leaks into headers (ADR-012). |
| Rust `String`, `Vec`, `Rc`, `Box`, traits, generics | Cannot be expressed in C; handles replace them. |
| `GiochiSprite` by-value arrays | Callers pass one sprite pointer per call; batching is a phase 2 internal optimisation. |
| Font rendering | TTF text rendering is not part of phases 0–8; `GiochiFont` exists only so the typedef can be added without an ABI break. |
| `GiochiTextureSize` | Texture sizes are returned through two `uint32_t*` out-parameters instead, which keeps the function count and the struct count down. The Rust API's `TextureSize` is a convenience mirror, not an ABI type. |

---

## 7. Compliance checklist for phase 7

1. `giochi.h` is generated by `cbindgen` and diffed against the committed header.
2. Every error path sets the thread-local message.
3. No function unwinds across the boundary (`catch_unwind` at the boundary).
4. `rust_ffi`, `c_smoke` and `python_smoke` tests exercise version, lifecycle,
   texture and render paths.
5. `GIOCHI_ABI_VERSION` is `1` and matches `giochi_abi_version()`.
