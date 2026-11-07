# GIOCHI-ENGINE — C ABI Contract

**Contract version: `GIOCHI_ABI_VERSION 1` for part I (2D), `2` for part II (3D).**
**Engine version: `0.1.1`**
**Status: authoritative.** This document is the law of the project. If a Rust type
cannot be expressed in the C form below, the *type* is redesigned — the contract
is never relaxed.

Part I (§1–§7) is implemented and exported. Part II (§8–§12) specifies 3D and is
written before the code on purpose; it ships in phase 14 and bumps
`GIOCHI_ABI_VERSION` to 2. Part I signatures never change.

The header `giochi-engine-capi/include/giochi.h` is generated from the Rust crate
by `cbindgen` and must match this document exactly; `scripts/gen-header.sh --check`
enforces that in CI.

**Status: complete.** Every function below is implemented and tested. The
per-phase history is in `PROGRESS.md`; the practical guide to consuming the ABI is
`docs/C_ABI.md`.

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
* Strings returned by the engine (`giochi_get_last_error`,
  `giochi_engine_video_driver`) are owned by the engine, valid until the next engine
  call on the same thread, and must not be freed or modified by the caller.

### 2.3 Errors

* Every fallible function returns `int32_t`: `0` on success, a negative code from
  [§4](#4-error-codes) on failure.
* Functions that return a handle signal failure by returning `0`; the reason is
  then available through `giochi_get_last_error()`.
* **Destructors return `int32_t`.** `giochi_texture_destroy`, `giochi_sound_destroy`,
  `giochi_music_destroy` and `giochi_scene_destroy` report
  `GIOCHI_ERROR_INVALID_HANDLE` for `0`, an unknown handle or a second destroy — a
  double destroy is always visible, never a silent success.
* `giochi_get_last_error()` is **thread-local**. It returns `""` when the last
  call succeeded, because **every function clears the slot on entry**; a stale
  message can never be mistaken for a new failure.
* No Rust `panic` may cross the boundary: every exported function is wrapped in a
  `catch_unwind` guard that reports `GIOCHI_ERROR_INTERNAL` with the panic message.
* Calls made before `giochi_engine_init`, or from a thread other than the one that
  initialised the engine, report `GIOCHI_ERROR_NOT_INITIALIZED` rather than racing.

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

#### Callback contract

| Callback | When | Requirements |
|---|---|---|
| `on_enter` | once, when the scene is pushed or replaces another | may be `NULL`; load resources here |
| `on_exit` | once, when the scene is popped or replaced | may be `NULL`; destroy every handle created in `on_enter` |
| `update` | once per **fixed** simulation step | required; `t->delta` is the fixed timestep, not the display frame time |
| `render` | once per display frame, after the screen was cleared and before it is presented | required; do not present |

`update` may run zero, one or several times per `render`. A scene must not assume a
1:1 ratio, and must not read input in `render` to drive simulation.

#### What a callback may call

A callback runs inside the engine's frame, so it can use exactly what the engine
lends it. Using anything else returns `GIOCHI_ERROR_SCENE` with a message naming the
callback the function belongs in — never a crash, never silently wrong behaviour.

| Callback | `giochi_render_*`, `giochi_camera_*`, `giochi_sprite_draw` | `giochi_input_*`, `giochi_gamepad_*` | `giochi_sound_*`, `giochi_music_*`, `giochi_audio_*` | `giochi_texture_*` | `giochi_asset_*` |
|---|---|---|---|---|---|
| `on_enter` | yes | no | yes | yes | yes |
| `update` | no | yes | yes | yes | yes |
| `render` | yes | no | no | yes | yes |
| `on_exit` | yes | no | yes | yes | yes |
| outside a run | yes | yes | yes | yes | yes |

`on_enter`/`on_exit` are delivered by the library *outside* the frame, which is why
they have the widest access: resource loading and teardown belong there.

The texture registry is shared rather than borrowed (`Rc<TextureRegistry>`, ADR-028),
so `giochi_texture_*` works from every context, including `update`.

Engine control (`giochi_engine_init`, `giochi_engine_run`, `giochi_engine_run_scene`,
`giochi_engine_step`, `giochi_engine_shutdown`) is **not** callable from a callback;
`giochi_engine_request_quit` and `giochi_engine_is_running` are.

The engine calls `on_enter`/`on_exit` exactly once per scene and only while that
scene is on top of the stack. A scene below an overlay keeps its state and receives
no callbacks until the overlay is popped; it is **not** re-entered.

#### Scene lifetime and destroying a running scene

`giochi_scene_create` returns a handle; `giochi_engine_run_scene` blocks until the
scene quits. Each call brackets exactly one `on_enter`/`on_exit` pair and leaves the
engine's scene stack empty, so the same handle can be run again (menu → game → menu).

Destroying the handle of a scene that is currently running is an error
(`GIOCHI_ERROR_SCENE`) — stop the engine first. Destroying a scene that is not
running is always safe, and destroying it twice reports
`GIOCHI_ERROR_INVALID_HANDLE`.

#### Transitions

A `Scene` in the Rust API can ask for a transition (`Continue`/`Quit`/`Pop`); the C
ABI models this with `giochi_engine_request_quit()` and `giochi_engine_is_running()`,
which is what a callback calls when it wants the loop to stop. There is no
transition enum in ABI v1: `Quit` is the only transition a C scene needs, and
push/pop/replace are driven by the host through `giochi_engine_run_scene`.

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

Every function below is declared in `giochi.h`. The trailing comment is the phase
of `PROGRESS.md` that implemented it, kept as history; all of them are implemented.

### 5.1 Version and diagnostics

```c
/* Always available, safe to call before giochi_engine_init. */
uint32_t    giochi_abi_version(void);      /* == GIOCHI_ABI_VERSION (1) */
uint32_t    giochi_engine_version(void);   /* major<<16|minor<<8|patch */
const char* giochi_get_last_error(void);   /* thread-local, never NULL */

/* Diagnostics; return 0 / 0.0 before initialisation instead of failing. */
const char* giochi_engine_video_driver(void);  /* "kmsdrm", "x11", "dummy", … */
uint32_t    giochi_engine_scene_depth(void);   /* scenes on the engine's stack */
```

`giochi_engine_video_driver` returns the driver SDL actually opened, which is how a
deployment proves the KMSDRM path was taken. The string is owned by the library and
valid until the next call on the same thread.

The header also defines the codes from §4 and `GIOCHI_MAX_ASSET_SIZE` (64 MiB) as
`#define`s, generated from `giochi-engine-capi/src/abi_types.rs`.

### 5.2 Engine lifecycle

```c
int32_t giochi_engine_init(const GiochiEngineConfig* config);  /* NULL = defaults */
void    giochi_engine_shutdown(void);                          /* idempotent */
int32_t giochi_engine_run(void);              /* blocks until quit */
int32_t giochi_engine_run_scene(GiochiScene scene);
int32_t giochi_engine_step(float frame_time); /* one iteration, never sleeps */
void    giochi_engine_request_quit(void);     /* safe inside a callback */
int32_t giochi_engine_is_running(void);       /* 1/0 */
int32_t giochi_engine_reset_quit(void);       /* re-arm after a quit */

/* Statistics, accumulated by run and by step. */
uint64_t giochi_engine_frames(void);
uint64_t giochi_engine_steps(void);
float    giochi_engine_elapsed(void);         /* seconds */
float    giochi_engine_timestep(void);        /* fixed step, seconds */
```

* `giochi_engine_init` may be called only once per process (until shutdown).
* `giochi_engine_shutdown` is idempotent. It does not free handles: destroy
  textures, sounds, music and scenes first for deterministic behaviour.
* `giochi_engine_run` pumps events, runs fixed-timestep updates and renders until
  `Escape`, the window close button or `giochi_engine_request_quit`.
* The engine clears the screen before each frame and presents after it; a scene's
  `render` callback only issues draw calls.
* `giochi_engine_run_scene` on an engine with no scene runs the loop and clears the
  screen. That is legal and is what a "loading" screen does.
* `giochi_engine_is_running` returns `0` after a run that ended through
  `giochi_engine_request_quit`, `Escape` or the window close button — the state is
  coherent for a host that checks it before running again.

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
GiochiTexture giochi_texture_create_memory(const uint8_t* data, size_t len); /* decodes PNG/JPEG */
int32_t       giochi_texture_destroy(GiochiTexture texture);                 /* phase 2 */
uint32_t      giochi_texture_width(GiochiTexture texture);                   /* phase 2 */
uint32_t      giochi_texture_height(GiochiTexture texture);                  /* phase 2 */
```

`/* Returns 0 on failure; query giochi_get_last_error(). */`

`giochi_texture_create` goes through the engine's resource cache (phase 4): loading
the same path twice returns the same handle and charges one upload. Until phase 4
each call uploads its own texture. Width/height return `0` for an invalid handle,
and destroying an unknown or already-destroyed handle returns
`GIOCHI_ERROR_INVALID_HANDLE` — a double destroy is always reported.

`giochi_texture_create_memory` decodes an encoded image (PNG, JPEG, BMP, …) whose
format is sniffed from the bytes, so a target without a filesystem can ship its
assets inside the executable. NULL or empty input is
`GIOCHI_ERROR_INVALID_ARGUMENT`.

In the Rust API the same operations are `TextureRegistry::create_texture`,
`create_texture_from_bytes`, `create_texture_from_rgba`, `destroy_texture`,
`dimensions` and `len`, or their `RenderContext` delegations.
`create_texture_from_rgba` has no ABI v1 counterpart because it takes raw pixels
whose dimensions the caller would also have to pass.

Destroying a handle that came from the cache leaves a stale cache entry behind; the
next `giochi_texture_create` for that path detects it and reloads (ADR-022).

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

Input semantics, implemented in phase 3 and shared by Rust and C:

| Rule | Behaviour |
|---|---|
| Frame edges | `pressed`/`released` describe the frame that just ended and clear on the next update. |
| Fast taps | A key pressed **and** released between two frames reports **both** edges in that frame, so a tap is never lost. |
| Key repeat | SDL's auto-repeat (`repeat = 1`) keeps the key `down` but does not re-raise `pressed`. |
| Unknown ids | An unmapped keycode, mouse button, axis or controller button is simply "not down" / `0.0`, never an error. |
| Mouse delta | `giochi_input_mouse_*` returns absolute position; the per-frame delta is Rust-only (`MouseState::delta`). |
| Wheel | Positive values mean scroll up/away from the user. The delta accumulates within a frame and resets on the next one. |
| Gamepad axes | Normalised and clamped to `[-1.0, 1.0]`. Sticks rest at `0.0`, triggers at `-1.0` (SDL's convention). |
| Gamepad selection | The lowest-indexed device SDL maps as a game controller is player one. Device add/remove events are handled; a removal of another device is ignored. |
| No controller subsystem | Keyboard and mouse keep working; `giochi_gamepad_connected` returns `0`. The engine logs a warning at start-up. |

### 5.6 Audio

```c
GiochiSound giochi_sound_load(const char* path);                 /* phase 4 */
int32_t     giochi_sound_destroy(GiochiSound sound);             /* phase 4 */
int32_t     giochi_sound_play(GiochiSound sound, int32_t loops); /* phase 4 */
void        giochi_audio_set_volume(int32_t volume);             /* phase 4, 0..=128 */
int32_t     giochi_audio_volume(void);                           /* phase 4 */

int32_t     giochi_audio_available(void);                        /* phase 4 */

GiochiMusic giochi_music_load(const char* path);                 /* phase 4 */
int32_t     giochi_music_destroy(GiochiMusic music);             /* phase 4 */
int32_t     giochi_music_play(GiochiMusic music, int32_t loops); /* phase 4 */
void        giochi_music_pause(void);                            /* phase 4 */
void        giochi_music_resume(void);                           /* phase 4 */
void        giochi_music_stop(void);                             /* phase 4 */
int32_t     giochi_music_is_playing(void);                       /* phase 4 */
```

* `loops` is the number of *extra* repetitions: `0` plays once, `-1` loops
  forever. Any value below `-1` is treated as `-1`.
* A failing audio device degrades gracefully: `load`/`play` return
  `GIOCHI_ERROR_AUDIO` while the rest of the engine keeps running.
  `giochi_audio_available` lets a consumer check once at start-up instead of
  provoking a failure (it mirrors `AudioSystem::is_available`, phase 4).
* `giochi_audio_set_volume` clamps to `0..=128`; out-of-range values are never an
  error.
* Destroying a music handle stops playback first, so the mixer never holds a freed
  stream.
* Sound effects have 16 mixing channels; playing more than that steals the oldest
  channel (SDL_mixer's behaviour).

### 5.7 Assets

```c
int32_t giochi_asset_size(const char* path, size_t* out_size);                       /* phase 4 */
int32_t giochi_asset_read(const char* path,
                          uint8_t* buffer,
                          size_t capacity,
                          size_t* out_read);                                         /* phase 4 */
```

`giochi_asset_read` writes exactly the file's size into `buffer` and reports it
through `out_read`; the buffer contents past `out_read` are untouched. The engine
never allocates the buffer, so the caller owns the memory and the size policy.

Assets larger than `GIOCHI_MAX_ASSET_SIZE` (64 MiB) are rejected with
`GIOCHI_ERROR_ASSET`. `giochi_asset_read` fails instead of truncating when
`capacity` is smaller than the file.

```c
#define GIOCHI_MAX_ASSET_SIZE (64u * 1024u * 1024u)
```

### 5.8 Scenes

```c
GiochiScene giochi_scene_create(const GiochiSceneCallbacks* callbacks);  /* phase 5 */
int32_t     giochi_scene_destroy(GiochiScene scene);                     /* phase 5 */
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

## 7. Compliance checklist

| Requirement | Where it is enforced |
|---|---|
| `giochi.h` is generated by `cbindgen` and diffed against the committed header | `scripts/gen-header.sh --check`, run by `scripts/ffi-smoke.sh` and CI |
| Every error path sets the thread-local message | `state::fail` / `invalid_argument` / `unavailable`, asserted throughout `tests/rust_ffi.rs` |
| No function unwinds across the boundary | every export is wrapped in `state::guard` (`catch_unwind`) |
| `rust_ffi`, `c_smoke` and `python_smoke` exercise version, lifecycle, texture, render and scene paths | `cargo test -p giochi-engine-capi` and `scripts/ffi-smoke.sh` |
| `GIOCHI_ABI_VERSION` is `1` and matches `giochi_abi_version()` | `tests::abi_version_matches_contract` |
| The C structs have the documented size and alignment | `tests/rust_ffi.rs::abi_type_layouts_match_the_contract` and `giochi-engine/tests/abi_layout.rs` |

---

# Part II — 3D (ABI version 2, phases 9–14)

**Status: specified, not yet exported.** This part is written *before* the code, which
is the rule that made part I work: a type that cannot be expressed here gets redesigned
now, not after six phases of callers depend on it. The functions below land in phase 14;
the types are fixed here so the Rust structs can be written against them.

`GIOCHI_ABI_VERSION` becomes **2** when phase 14 ships. Version 1 consumers keep
working: part I is unchanged, and every 3D symbol is additive.

## 8. How 3D differs from 2D at the boundary

The 2D ABI hides a single engine-owned renderer behind `giochi_render_*` (ADR-012). 3D
cannot use that shape: a game may want a 2D overlay and a 3D scene, and the two
renderers are different subsystems that cannot share a canvas. So 3D gets an **opaque
engine pointer** instead of global functions:

```c
typedef struct GiochiEngine3D GiochiEngine3D;   /* opaque, no layout in the header */
```

Rationale: a `glow::Context` and a `sdl2::video::GLContext` have no C representation at
all, and the 3D renderer's size legitimately changes between releases. An opaque pointer
is the only honest form. This is a deliberate divergence from part I, recorded in
ADR-039.

## 9. Types

### 9.1 `GiochiColor3D`

16 bytes of data plus reserved; every field is a float in `0.0..=1.0`.

```c
typedef struct GiochiColor3D {
    float    r, g, b, a;
    uint32_t _reserved[4];
} GiochiColor3D;                     /* size 32, align 4 */
```

Out-of-range values are clamped by the renderer rather than rejected: a shader
uniform of `2.0` is a caller bug, not a reason to fail a frame.

### 9.2 `GiochiVec3` and `GiochiQuat`

The 3D path cannot use `glam` types across the boundary, so vectors are plain arrays
with a struct wrapper for readability.

```c
typedef struct GiochiVec3 {
    float    x, y, z;
    uint32_t _reserved[4];
} GiochiVec3;                        /* size 28, align 4 */

typedef struct GiochiQuat {
    float    x, y, z, w;
    uint32_t _reserved[4];
} GiochiQuat;                        /* size 32, align 4 */
```

`GiochiQuat` is `(x, y, z, w)` — **not** `(w, x, y, z)`. The order is stated here because
transposing it produces a camera that is subtly wrong rather than visibly broken.

### 9.3 `GiochiTransform3D`

```c
typedef struct GiochiTransform3D {
    GiochiVec3 position;
    GiochiQuat rotation;
    GiochiVec3 scale;
    uint32_t   _reserved[4];
} GiochiTransform3D;                 /* size 104, align 4 */
```

Applied in the order scale → rotation → translation, the same order every scene graph
uses. Documented because getting it wrong looks like a broken pivot.

### 9.4 `GiochiCamera3D`

```c
typedef struct GiochiCamera3D {
    GiochiVec3 position;
    GiochiVec3 target;
    GiochiVec3 up;
    float      fov_y;                /* radians, vertical */
    float      near_plane;
    float      far_plane;
    float      aspect;               /* width / height; 0 = use the drawable */
    uint32_t   _reserved[4];
} GiochiCamera3D;                    /* size 116, align 4 */
```

`aspect == 0.0` means "ask the renderer", which is what a console wants: the panel's
real size is authoritative, the same rule ADR-035 established for 2D.

### 9.5 Handles

3D resources follow the same `u64`, `0`-is-invalid rule as 2D. They live in a **separate**
registry: a `GiochiTexture` and a `GiochiGlTexture` are different resources with
different lifetimes, and letting one be passed where the other is expected is a mistake
worth making impossible.

```c
typedef uint64_t GiochiMeshHandle;      /* 0 invalid */
typedef uint64_t GiochiShaderHandle;    /* 0 invalid */
typedef uint64_t GiochiGlTextureHandle; /* 0 invalid */
```

Ownership: `giochi_mesh_destroy` and `giochi_shader_destroy` release them. Textures are
created through `giochi_gl_texture_create` and released with `giochi_gl_texture_destroy`.
Nothing is freed on engine shutdown — a caller that leaks a mesh leaks it, exactly as in
part I.

## 10. Functions (phase 14)

```c
/* 10.1 Lifecycle — opaque engine pointer */
GiochiEngine3D* giochi_engine3d_create(const GiochiWindowConfig* config);
void            giochi_engine3d_destroy(GiochiEngine3D* engine);
int32_t         giochi_engine3d_begin_frame(GiochiEngine3D* engine);
int32_t         giochi_engine3d_end_frame(GiochiEngine3D* engine);

/* 10.2 Camera — by value in, no allocation */
GiochiCamera3D  giochi_camera3d_new(void);
void            giochi_camera3d_look_at(GiochiCamera3D* camera,
                                        GiochiVec3 position, GiochiVec3 target);

/* 10.3 Meshes and shaders */
int32_t giochi_mesh_load_obj(const char* path, GiochiMeshHandle* out);
int32_t giochi_mesh_destroy(GiochiMeshHandle mesh);
int32_t giochi_shader_load(const char* vert_path, const char* frag_path,
                           GiochiShaderHandle* out);
int32_t giochi_shader_destroy(GiochiShaderHandle shader);

/* 10.4 Drawing */
int32_t giochi_renderer3d_clear(GiochiEngine3D* engine, GiochiColor3D color);
int32_t giochi_renderer3d_draw_mesh(GiochiEngine3D* engine,
                                    GiochiMeshHandle mesh,
                                    GiochiShaderHandle shader,
                                    const GiochiTransform3D* model,
                                    const GiochiCamera3D* camera);
```

Every one returns `int32_t` and reports through `giochi_get_last_error()`, unchanged from
part I. `giochi_engine3d_create` is the single exception: it returns `NULL` on failure,
because a caller cannot use an error code without an engine to read it from — the same
pattern SDL uses for `SDL_CreateWindow`.

## 11. What part II deliberately does not have

| Not exposed | Reason |
|---|---|
| A `GiochiEngine3D` struct layout | Holds `glow::Context`; no C representation exists (ADR-039). |
| `GiochiMat4` as a public type | Matrices are computed by the engine from `GiochiTransform3D` and `GiochiCamera3D`; exposing them invites callers to depend on the multiplication order. |
| GLSL source as a string parameter | Shaders are files (`giochi_shader_load`) or built-ins, per R10. A string parameter would put the whole shader text in the ABI. |
| VAO/OES extension exposure | Whether VAOs are core depends on the GLES version; the renderer abstracts it (fase 11) and callers never see it. |
| A 3D scene callback table | Phase 14 is a draw API, not a loop. A caller drives its own loop, which is what a 3D game does anyway. |

## 12. Compliance checklist for part II

| Requirement | Enforced by |
|---|---|
| Every 3D type is `#[repr(C)]` with `_reserved` | `giochi-engine-3d/tests/abi_layout_3d.rs` (phase 14), mirroring `abi_layout.rs` |
| Handles are `u64` with `0` invalid | `tests/rust_ffi_3d.rs` |
| `GIOCHI_ABI_VERSION == 2` and `giochi_abi_version()` agrees | `python_smoke_3d.py`, `rust_ffi_3d.rs` |
| No `glow`, `glam` or `sdl2` type appears in a public struct field | the same test that checks the 2D crate's rule (ADR-032) |
| 3D additions never change a part I signature | `git diff` review plus `c_smoke.c` continuing to pass untouched |
