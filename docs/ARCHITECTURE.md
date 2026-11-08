# Architecture — GIOCHI-ENGINE

A 2D game engine in Rust on SDL2, with a public surface designed to be projected
into C. This page explains how the pieces fit and **why** the boundaries are where
they are. The decisions themselves are recorded in
[DECISIONS.md](DECISIONS.md); the external surface is specified in
[C_ABI_CONTRACT.md](C_ABI_CONTRACT.md).

---

## 1. The shape of the workspace

```text
giochi-engine         lib       idiomatic Rust API, C-shaped data, #![forbid(unsafe_code)]
giochi-engine-capi    rlib +    #[no_mangle] extern "C", panics caught, generated giochi.h
                      cdylib +
                      staticlib
giochi-engine-lua     lib       reference binding (Lua 5.4 via mLua, no unsafe)
giochi-demo           bin       the demo, --frames/--verify, and the `lua` subcommand
```

The core crate never depends on the FFI crate, the FFI crate never depends on Lua,
and the demo is the only place that assembles a runnable application. Adding a
binding therefore never changes the engine.

---

## 2. Layers inside `giochi-engine`

```text
                ┌──────────────────────────────────────────┐
   user code →  │ core::engine::Engine                     │  facade
                └───────┬───────────────┬──────────────────┘
                        │               │
             ┌──────────▼──────┐  ┌─────▼──────────┐
             │ scene::Scene,   │  │ core::game_loop│
             │ SceneManager    │  │ core::time     │
             └───┬─────────┬───┘  └────────────────┘
                 │         │
   ┌─────────────▼──┐  ┌───▼──────────┐  ┌────────────────┐
   │ render::       │  │ input::Input │  │ audio::        │
   │ RenderContext  │  │              │  │ AudioSystem    │
   └───────┬────────┘  └──────┬───────┘  └───────┬────────┘
           │                  │                  │
   ┌───────▼──────────────────▼──────────────────▼────────┐
   │ platform::{Platform, Window}   ← the only sdl2 user  │
   └──────────────────────────────────────────────────────┘

   math::{Vec2, Rect, Color}   depends on nothing
   handle_registry              pub(crate), used by render/audio/scene
   assets::{loader, ResourceManager}
   perf::{Profiler, DebugOverlay}  observes the frame; `profiling` feature
```

`perf` observes the frame instead of owning part of it: `core::engine` opens and closes
the scopes around the scene callbacks, the renderers record their own counters, and the
overlay draws through `RenderContext`. It never imports `sdl2` directly, and without
the `profiling` feature every call is an inlined no-op.

### Dependency rules

| Rule | Reason |
|---|---|
| `core` does not import `sdl2` | `core` owns policy (fixed timestep, frame limit, scene stack). Swapping or mocking SDL must not touch it. `GameLoop` therefore handles `QuitSignal`, a platform type, instead of an SDL event. |
| `render` does not know `scene` | A renderer that knows about scenes grows a game-graph. `Engine` wires them. |
| `math` depends on nothing else | It is the vocabulary every other module shares; a cycle here would be fatal. |
| `handle_registry` is `pub(crate)` | The `HashMap<u64, Rc<T>>` is the implementation of handles, never part of the API. |
| Only `platform` imports `sdl2` | One crate-wide exception is easy to audit. `render` consumes `sdl2` types through `platform`, not by importing the crate. |

---

## 3. Ownership and lifetime

```text
Engine
 ├── Platform        (Rc<SdlContext>: Sdl + video + timer + events + gamecontroller + audio)
 ├── RenderContext   (WindowCanvas + TextureRegistry + ResourceManager + Camera)
 ├── AudioSystem     (HandleRegistry<Chunk> + HandleRegistry<Music> + device)
 ├── Input           (KeyboardState + MouseState + GamepadState + controller subsystem)
 ├── SceneManager    (Vec<Box<dyn Scene>>)
 ├── GameLoop, FrameClock, Time, RunStats
 └── _reserved
```

* Fields drop in declaration order, so the canvas and the mixer are destroyed before
  the SDL context. SDL requires the reverse of construction.
* `Window` holds a clone of the `Rc<SdlContext>`, so it can outlive the `Platform`
  value that created it; the context lives until the last holder is gone (ADR-016).
* `RenderContext` **consumes** the `Window` because SDL binds a renderer to its
  window (ADR-018).
* Scenes never own engine objects: the engine lends `&Input`, `&mut AudioSystem`
  and `&mut RenderContext` per call (ADR-024).

### Handle lifecycle

Every user-held resource is a `u64` where `0` means null:

| Handle | Created by | Destroyed by |
|---|---|---|
| `TextureHandle` | `create_texture`, `create_texture_from_rgba`, `Renderer::load_texture` | `destroy_texture`, `TextureRegistry::clear`, `Engine` drop |
| `SoundHandle` | `AudioSystem::load_sound` | `destroy_sound`, `AudioSystem::clear` |
| `MusicHandle` | `AudioSystem::load_music` | `destroy_music` (also halts playback) |
| `SceneHandle` | `giochi_scene_create` | `giochi_scene_destroy` |

Handles are never reused, so a stale handle can never alias a new resource, and
destroying an unknown handle is a reported error rather than undefined behaviour.

---

## 4. The frame

```text
loop {
    frame_time = clock.tick()                 // real elapsed time
    events     = platform.poll_events()       // one drain per frame
    input.update(&events)                     // clear edges, then apply
    quit?      → game_loop.request_quit()

    steps = game_loop.advance(frame_time)     // 0..=15 at 60 Hz, clamped to 0.25 s
    for _ in 0..steps {
        time.advance(fixed_timestep)          // deterministic delta
        scene.update(time, &input, &mut audio)
    }

    render.clear(clear_color)
    scene.render(&mut render)                 // draw calls only
    render.present()

    match scene.transition() { ... }           // applied AFTER presenting
    sleep(game_loop.sleep_ms(work_time))
}
```

Why this order:

* **Input before simulation.** Edges (`pressed`/`released`) are cleared at the start
  of `input.update`, so the simulation and the render of a frame see the same
  snapshot (ADR-019).
* **Fixed steps, variable render.** Physics stays deterministic at 60 Hz even when
  the display runs at 144 Hz or the process stalls; the accumulator absorbs the
  difference.
* **Clamped catch-up.** A frame longer than 0.25 s (debugger pause, asset load) is
  truncated instead of triggering hundreds of steps — no "spiral of death".
* **Transitions after present.** A scene that quits is freed only once its own frame
  has been drawn (ADR-025).

---

## 5. The C-shaped discipline

The rules in the project prompt (R1–R7) are enforced structurally:

| Rule | How it is enforced |
|---|---|
| C-shaped public types | Every public struct is `#[repr(C)]`; `tests/abi_layout.rs` asserts byte sizes and alignments, `giochi-engine-capi/tests/rust_ffi.rs` does the same for the two boundary-only structs. |
| ...mechanically, not by review | `tests/c_shape.rs` scans the crate source and fails the build on any public type without `#[repr(C)]`, any `#[repr(C)]` struct whose last field is not `_reserved: [u32; N ≥ 4]`, any public field holding `Rc`/`Arc`/`Box`/`String`/`Vec`/`HashMap`, and any public function with generic parameters or an explicit lifetime. |
| The one documented exception | `GiochiError` is a `thiserror` enum with `String` payloads and is deliberately **not** `#[repr(C)]`: R4 requires it, and it never crosses the boundary. `c_shape.rs` carries it in an allow-list that also requires the cited rule. |
| No `Rc`/`String`/`Vec` in public fields | Internals are private; the `missing_docs`/clippy gates plus review catch leaks. `EngineConfig.title` is a `[u8; 64]` buffer (ADR-007). |
| The header matches the Rust | `giochi.h` is generated by `cbindgen` from the crate; `scripts/gen-header.sh --check` fails when the committed copy is stale. |
| Handles, not pointers | `handle_registry` is the only owner map and is `pub(crate)` (ADR-002). |
| Errors as codes at the boundary | `GiochiError` (thiserror) inside, `i32` + `giochi_get_last_error()` outside (ADR-003). |
| Core has no `unsafe` | `#![forbid(unsafe_code)]` on `giochi-engine`; only the capi crate may use it, each block with a `// SAFETY:` comment. |
| 2D only | No backend/pipeline/mesh abstractions exist to be tempted by (ADR-005). |

---

## 5b. Inside `giochi-engine-capi`

```text
lib.rs        crate docs, module wiring, re-exports
abi_types.rs  GiochiEngineConfig, GiochiSceneCallbacks, the error-code constants
state.rs      the engine singleton, last-error slot, panic guard, subsystem slots
version.rs    giochi_abi_version / giochi_engine_version / giochi_get_last_error
engine.rs     init, shutdown, run, run_scene, step, quit, statistics
render.rs     textures, drawing, camera
input.rs      keyboard, mouse, gamepad
audio.rs      sounds, music, volume
assets.rs     asset size and read
scene.rs      the scene registry and the CScene adapter
```

Three mechanisms carry the whole boundary:

* **Singleton.** The engine lives in a `thread_local!` cell. A call from another
  thread, or before `giochi_engine_init`, finds nothing and reports
  `GIOCHI_ERROR_NOT_INITIALIZED` — the single-threaded rule of ADR-010 becomes
  testable rather than aspirational.
* **Panic guard.** Every export runs inside `state::guard`, which also clears the
  error slot on entry. A panic becomes `GIOCHI_ERROR_INTERNAL` with its message.
* **Callback lending.** A callback may use exactly the subsystems the engine lent it
  (ADR-028). The adapter installs them in thread-local slots for the duration of the
  call; `giochi_texture_*` bypasses all of this because the registry is `Rc`-shared.

The header is generated, never written: `cbindgen.toml` maps the engine's Rust names
to the `Giochi*` names of the contract and is regenerated by `scripts/gen-header.sh`.

---

## 5c. Inside `giochi-engine-lua`

```text
lib.rs      crate docs, `giochi` module registration, run_file, error mapping
app.rs      App (owns the Engine), LuaScene (the Scene impl), the quit flag
caps.rs     Gfx<'a>, InputRef<'a>, AudioRef<'a> — the per-frame capabilities
handles.rs  Texture (freed by Drop), Sound, Music
values.rs   Color/Rect conversions from Lua tables and hex strings
keys.rs     the `giochi.keys`/`mouse`/`axes`/`buttons` tables, built from the engine
```

Two mechanisms carry the binding:

* **Callbacks are scoped.** `LuaScene::update`/`render` open `Lua::scope`, wrap the
  references the engine lent them in userdata and pass them as arguments. When the
  callback returns, `mlua` invalidates them, so a script cannot outlive its borrow —
  the ADR-028 rule, without a single `unsafe` (ADR-030).
* **The app owns the engine.** `App` holds the `Engine` by value; setup methods borrow
  it like Rust would, so calling one mid-frame raises "the app is busy" instead of
  panicking. `Engine` is a singleton per process (ADR-012), enforced with a `Weak`
  marker and a message that says so.

The crate is `#![forbid(unsafe_code)]`, like the core.

---

## 6. Failure policy

The engine distinguishes "cannot start" from "degraded but running":

| Failure | Behaviour |
|---|---|
| No video driver | **Error.** Nothing can be drawn. |
| KMSDRM unavailable on an auto-selected driver | Warning + desktop fallback (ADR-015). |
| No audio device | Warning + silent mode; load/play return an error (ADR-021). |
| No game controller | Normal: `gamepad().is_connected()` is `false`. |
| Missing asset | `GiochiError::Asset`, reported at the call site. |
| Overflowing texture/sound handle | Reported as `InvalidHandle`, never silent. |

This is what makes the engine usable in CI, on a bare console, and on a developer
desktop with different drivers.

---

## 7. Testing strategy

| Layer | Where | Needs SDL? |
|---|---|---|
| Pure logic (`Time`, `GameLoop`, `Camera`, `Vec2`/`Rect`, handle registry, input state machines, scene stack, error codes) | unit tests in `src/**` | no |
| ABI layout (sizes, alignment, zeroed `_reserved`) | `tests/abi_layout.rs` | no |
| The C-shaped design rules, over the whole crate source (R1, R3) | `tests/c_shape.rs` | no |
| End-to-end with real SDL objects | `tests/*_smoke.rs` with `SDL_VIDEODRIVER=dummy` | yes, headless |
| Rendering correctness | `tests/render_smoke.rs` reads pixels back and asserts colours | yes, headless |
| Playable behaviour | `tests/engine_smoke.rs`, plus `mini_game --auto` | yes, headless |
| The C ABI, from Rust | `giochi-engine-capi/tests/rust_ffi.rs` | yes, headless |
| The C ABI, from C and Python | `giochi-engine-capi/tests/{c_smoke.c,python_smoke.py}` via `scripts/ffi-smoke.sh` | yes, headless |

Each integration test binary contains a **single** test: SDL allows one `EventPump`
per process, and cargo runs the tests inside one binary concurrently.

---

## 8. What comes next

| Phase | Scope |
|---|---|
| 6 ✅ | CI, release packaging, the remaining guides (this document among them) |
| 7 ✅ | `giochi-engine-capi`: the `extern "C"` surface, `cbindgen`, C/Python smoke tests |
| 8 ✅ | `giochi-engine-lua`: the `giochi` module, `mini_game.lua` and `giochi-demo lua` |

Anything 3D is out of scope for v2; it would be a separate `giochi-engine-3d` crate
on EGL + GLES, never a mode of this renderer (ADR-005).
