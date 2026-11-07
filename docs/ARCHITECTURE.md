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
giochi-engine-lua     lib       reference binding (Lua 5.4 via mLua)
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
```

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
| `SceneHandle` | phase 7 (`giochi_scene_create`) | phase 7 |

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
| C-shaped public types | Every public struct is `#[repr(C)]`; `tests/abi_layout.rs` asserts byte sizes and alignments. |
| No `Rc`/`String`/`Vec` in public fields | Internals are private; the `Missing_docs`/clippy gates plus review catch leaks. `EngineConfig.title` is a `[u8; 64]` buffer (ADR-007). |
| Handles, not pointers | `handle_registry` is the only owner map and is `pub(crate)` (ADR-002). |
| Errors as codes at the boundary | `GiochiError` (thiserror) inside, `i32` + `giochi_get_last_error()` outside (ADR-003). |
| Core has no `unsafe` | `#![forbid(unsafe_code)]` on `giochi-engine`; only the capi crate may use it, each block with a `// SAFETY:` comment. |
| 2D only | No backend/pipeline/mesh abstractions exist to be tempted by (ADR-005). |

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
| End-to-end with real SDL objects | `tests/*_smoke.rs` with `SDL_VIDEODRIVER=dummy` | yes, headless |
| Rendering correctness | `tests/render_smoke.rs` reads pixels back and asserts colours | yes, headless |
| Playable behaviour | `tests/engine_smoke.rs`, plus `mini_game --auto` | yes, headless |

Each integration test binary contains a **single** test: SDL allows one `EventPump`
per process, and cargo runs the tests inside one binary concurrently.

---

## 8. What comes next

| Phase | Scope |
|---|---|
| 6 ✅ | CI, release packaging, the remaining guides (this document among them) |
| 7 | `giochi-engine-capi`: the `extern "C"` surface, `cbindgen`, C/Python smoke tests |
| 8 | `giochi-engine-lua`: the `giochi` module and `mini_game.lua` |

Anything 3D is out of scope for v2; it would be a separate `giochi-engine-3d` crate
on EGL + GLES, never a mode of this renderer (ADR-005).
