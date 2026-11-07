# Architecture decisions — GIOCHI-ENGINE

Decisions are append-only. A decision is superseded, never silently rewritten:
add a new ADR that states what it replaces.

| ADR | Title | Status |
|---|---|---|
| 001 | Four-crate workspace | Accepted |
| 002 | Resources are opaque `u64` handles | Accepted |
| 003 | `thiserror` inside, `i32` + message outside | Accepted |
| 004 | Lua is the reference binding (not Python) | Accepted |
| 005 | 2D-only scope | Accepted |
| 006 | Every public struct is `#[repr(C)]` with `_reserved` | Accepted |
| 007 | Window title as a fixed `[u8; 64]` buffer | Accepted |
| 008 | Texture registry owns the SDL texture creator | Accepted |
| 009 | `KeyCode` is the SDL keycode as `u32` | Accepted |
| 010 | The engine is single-threaded | Accepted |
| 011 | `src/error.rs` added to the module tree | Accepted |
| 012 | ABI v1 is singleton-based (no context pointers) | Accepted |
| 013 | Wrapper structs keep a `_reserved` tail | Accepted |
| 014 | CMake 4 policy workaround for bundled SDL2 | Accepted |
| 015 | KMSDRM is auto-selected, with a loud desktop fallback | Accepted |
| 016 | `Platform` owns an `Rc`'d SDL context | Accepted |
| 017 | `sdl2` is built with `unsafe_textures` | Accepted |
| 018 | `RenderContext` owns the window and the texture registry | Accepted |
| 019 | Input snapshots per frame, with injectable state | Accepted |
| 020 | Gamepad axes and buttons are queried live from SDL | Accepted |
| 021 | Missing audio device degrades to silent mode | Accepted |
| 022 | The resource cache is keyed by the exact path string | Accepted |
| 023 | `Chunk`/`Music` are stored behind `Rc` in the handle registry | Accepted |
| 024 | The engine lends its subsystems to scenes; scenes never own them | Accepted |
| 025 | Scene transitions are requests applied after the frame | Accepted |
| 026 | `Engine::run_current` is the single loop; `--frames` lives in the engine | Accepted |
| 027 | The ARM64 gate is `cargo check`; linking is a separate, verified path | Accepted |
| 028 | A callback may use exactly what the engine lends it | Accepted |
| 029 | `Engine::step` shares the loop body with `run_current` | Accepted |
| 030 | Lua gets capabilities, not a global engine | Accepted |
| 031 | `Texture` freed by `Drop`, `Sound`/`Music` by the app | Accepted |
| 032 | The C-shaped rules are checked by a test, not by review | Accepted |
| 033 | The MSRV is verified, and it is not the one the prompt suggested | Accepted |
| 034 | Optional image decoders are probed, never assumed | Accepted |
| 035 | The drawable size is authoritative, not the configured size | Accepted |
| 036 | Assets ship as PNG plus a BMP twin, resolved at load time | Accepted |

---

## ADR-001 — Four-crate workspace

**Context.** The engine must be consumable from Rust *and* from any C-capable
language, and it must ship a reference scripting binding plus a demo.

**Decision.** Four crates:

| Crate | Kind | Responsibility |
|---|---|---|
| `giochi-engine` | lib | Idiomatic Rust API, C-shaped data. `#![forbid(unsafe_code)]`. |
| `giochi-engine-capi` | `rlib` + `cdylib` + `staticlib` | `#[no_mangle] extern "C"` surface, panics caught, `giochi.h`. |
| `giochi-engine-lua` | lib | Reference binding (phase 8). |
| `giochi-demo` | bin | Bootstrap, examples and the `lua` subcommand. |

**Why.** The `unsafe`, FFI and `mlua` dependencies stay out of the core crate, so
gameplay code cannot accidentally depend on them. The demo binary is the only
place that knows how to assemble a runnable app.

**Consequences.** The core cannot re-export anything unsafe. Every ABI addition is
a two-crate edit (engine + capi), which is intentional friction.

---

## ADR-002 — Resources are opaque `u64` handles

**Context.** `Rc<Texture>`/`Arc<Texture>` cannot cross a language boundary, and
ownership would become ambiguous.

**Decision.** Users only ever see `u64` (`TextureHandle`, `SoundHandle`,
`MusicHandle`, `SceneHandle`), with `0` meaning null. The `Rc<T>` lives inside a
crate-private `HandleRegistry<T>` (`RefCell<HashMap<u64, Rc<T>>>` + atomic
counter). Handles are never reused.

**Consequences.**
* Lifetime is explicit: create/destroy pairs, no RAII at the boundary.
* A stale handle can never alias a new resource.
* Destroying an unknown handle is a reported error.
* The engine is therefore not stateless: no resource can outlive the engine.

---

## ADR-003 — `thiserror` inside, `i32` + message outside

**Context.** Rust callers want typed errors with `?`; C callers cannot see Rust
enums or `Box<dyn Error>`.

**Decision.** Rust API returns `GiochiResult<T> = Result<T, GiochiError>`, where
`GiochiError` is a `thiserror` enum. The C ABI returns `int32_t` codes from
`giochi_engine::error::codes` plus a thread-local message retrieved with
`giochi_get_last_error()`.

**Consequences.** Every error variant must map to a code (`GiochiError::code()`),
so the code table is the single place where the two worlds meet. `GiochiError`
never crosses FFI.

---

## ADR-004 — Lua is the reference binding (not Python)

**Decision.** `giochi-engine-lua` (mLua 0.10, vendored Lua 5.4) is the reference
binding. Python is used only as a *consumer* smoke test (`ctypes`) in phase 7.

**Why Lua.**
* Embeds as ~250 KB of C that cross-compiles cleanly to `aarch64` (vendored
  feature builds on ARM64 consoles).
* No GIL, no global runtime, no per-target CPython packaging.
* `mlua` gives safe wrappers and a mature `UserData` story for the engine handles.

**Why not Python as the binding.** CPython would have to be cross-compiled and
shipped for the console target, adds a large runtime and a GIL, and would drag
build-system requirements into the engine workspace. As a *test* it is ideal: it
proves the C ABI works from a completely different language with no Rust
involvement.

**Consequences.** Any C ABI change must be exercised from Lua in phase 8; the
Python test covers only version + basic lifecycle.

---

## ADR-005 — 2D-only scope

**Decision.** No 3D in v2. No render-backend abstraction, no mesh/vertex
buffer/shader/pipeline concepts, and no preparation for them.

**Why.** The 2D renderer is SDL2's `render` API, which is simple, well-tested on
KMSDRM and sufficient for the target games. 3D would require EGL + GLES, a
different resource model and a different pipeline design; designing for it now
would slow 2D and freeze the wrong abstractions.

**Consequences.** A future `giochi-engine-3d` crate (EGL + GLES) is the only
sanctioned path. It must not reuse `sdl2::render`. Requests for 3D are answered
with this ADR and the current phase continues.

---

## ADR-006 — Every public struct is `#[repr(C)]` with `_reserved`

**Decision.** Every `pub struct` is `#[repr(C)]`, has only primitive/public-safe
fields or private internals, and ends with `_reserved: [u32; N]`, `N >= 4`.
Public functions take no explicit lifetime and no generic parameters.

**Why.** It makes the C projection mechanical and guarantees that adding fields
cannot silently break layout for existing consumers.

**Consequences.** Data the user sees is plain values (`Vec2`, `Rect`, `Color`,
`Sprite`, `Camera`, `Time`, `EngineConfig`). Rich behaviour lives in methods, not
in fields. `tests/abi_layout.rs` asserts the resulting sizes.

---

## ADR-007 — Window title as a fixed `[u8; 64]` buffer

**Context.** `EngineConfig` must be C-shaped; `String` is forbidden.

**Options.** `*const c_char` (needs `unsafe`, lifetime rules, NULL handling) or a
fixed inline buffer.

**Decision.** `title: [u8; 64]`, NUL-terminated UTF-8, truncated on a character
boundary. Rust callers use `EngineConfig::new(width, height, title)`; C callers
write into `config.title`.

**Consequences.** Titles longer than 63 bytes are truncated. A C caller that
writes invalid UTF-8 gets `title_str() == ""` instead of undefined behaviour.

---

## ADR-008 — Texture registry owns the SDL texture creator

**Context.** The prompt sketches `create_texture(creator, path)`. A creator
parameter would force a generic (`TextureCreator<T>`) or a lifetime-bearing
public signature, both forbidden by ADR-006.

**Decision.** `TextureRegistry` stores the creator internally; the public API is
`TextureRegistry::create_texture(&self, path: &str) -> GiochiResult<TextureHandle>`.

**Consequences.** The registry must be constructed after (and tied to) the render
context. That is already the engine's lifecycle: renderer first, then textures.

---

## ADR-009 — `KeyCode` is the SDL keycode as `u32`

**Context.** A Rust `enum` for keys would need a C layout contract and would have
to mirror SDL's ~200 keycodes.

**Decision.** `pub type KeyCode = u32`, holding the SDL keycode value (`SDLK_*`).
The engine's constants (`KEY_W`, `KEY_ESCAPE`, ...) are provided for the common
keys; any other SDL keycode can be passed numerically.

**Consequences.** No enum-layout negotiation with other languages, no exhaustive
matching, and new keys need no engine release. The trade-off is that invalid
keycodes are simply "never down" instead of a type error.

---

## ADR-010 — The engine is single-threaded

**Decision.** All engine objects live on one thread. Registries use `Rc` and
`RefCell`; public objects are intentionally neither `Send` nor `Sync`.

**Why.** SDL2 rendering, windowing and mixer are single-threaded on KMSDRM, and
the target consoles have no use for engine-level threading. `Arc<Mutex<...>>`
would add atomics and lock noise to every draw call for zero benefit.

**Consequences.** A consumer that wants threads must marshal work onto the engine
thread; the C ABI documents this. `Handle` values themselves are plain `u64` and
may be stored anywhere.

---

## ADR-011 — `src/error.rs` added to the module tree

**Context.** The project structure lists one file per subsystem but no home for
the error type, while golden rule 3 requires `thiserror`-based errors.

**Decision.** Add `giochi-engine/src/error.rs`, re-exported as
`giochi_engine::GiochiError`, `GiochiResult` and `codes`.

**Why.** Errors are a cross-cutting concern: putting them under `core` would make
`math` and `render` depend on `core`, violating the layering rules.

**Consequences.** One extra file; no module dependency violations.

---

## ADR-012 — ABI v1 is singleton-based (no context pointers)

**Context.** The prompt's illustrative header passes `GiochiRenderContext*` to
`sprite_draw`. Exposing a context pointer means exposing an engine-internal object
and adding a handle type the contract would have to define.

**Decision.** ABI v1 has one engine instance per process. Rendering, audio and
input are reached through free `giochi_render_*`, `giochi_audio_*` and
`giochi_input_*` functions; textures, sounds, music and scenes are handles.

**Why.** Fewer moving parts for C consumers, no opaque-pointer lifetime rules, no
possibility of drawing with a stale context. The Rust facade keeps the explicit
`RenderContext` object, which is where the type safety is useful.

**Consequences.** Multiple simultaneous windows/contexts are not expressible in
ABI v1. That is acceptable: the target console has one screen. If ever needed,
new `giochi_context_*` functions can be added without breaking existing code.

---

## ADR-013 — Wrapper structs keep a `_reserved` tail

**Context.** ADR-006 says *every* public struct carries reserved space, but some
public structs are wrappers with private internals (`Engine`, `Window`,
`RenderContext`, `Input`, `AudioSystem`, `SceneManager`, registries).

**Decision.** Apply `#[repr(C)]` and a private `_reserved: [u32; 4]` tail to those
wrappers as well. They are nevertheless never passed by value across FFI: the C
side sees handles and free functions.

**Why.** It keeps the rule absolute ("every public struct"), so a reviewer never
has to decide case by case, and it leaves room for future state without changing
the type's layout.

**Consequences.** A few unused words per object. No behavioural cost.

---

## ADR-014 — CMake 4 policy workaround for bundled SDL2

**Context.** `sdl2-sys 0.38` builds its vendored SDL2 with a CMake project that
declares `cmake_minimum_required(VERSION 3.0.x)`. CMake 4.x refuses to configure
such projects.

**Decision.** `.cargo/config.toml` sets
`CMAKE_POLICY_VERSION_MINIMUM = "3.5"` for every cargo invocation
(`force = false`, so a user value wins).

**Why.** It is the documented CMake 4 migration path, it changes no SDL2 source,
and it keeps `cargo build` a one-command operation on modern distros.

**Consequences.** If SDL2 is ever built outside cargo (e.g. a distro package), the
variable is harmless. Revisit when `sdl2-sys` bumps its CMake minimum.

---

## ADR-015 — KMSDRM is auto-selected, with a loud desktop fallback

**Context.** The production target boots straight into the engine and must use
SDL2's `kmsdrm` backend, but developers work on desktops where a compositor holds
DRM master and KMSDRM cannot start. Unconditionally defaulting to KMSDRM makes
`cargo run` fail on every development machine; never defaulting to it means the
console build quietly runs on the wrong driver.

**Decision.** Negotiation happens once, in `Platform::new`:

1. `SDL_VIDEODRIVER` already set → use it, never touch it
   (`VideoDriverChoice::Explicit`).
2. Not set → select `kmsdrm` (`VideoDriverChoice::AutoKmsdrm`).
3. Auto-selected `kmsdrm` fails to initialise → log a warning, unset the variable
   and retry with SDL's default desktop driver
   (`VideoDriverChoice::AutoFallback`).
4. An *explicit* driver that fails is never retried. The error names the driver,
   SDL's message, the DRM devices present and the relevant hints, and points at
   `SDL_VIDEODRIVER`.

**Why the fallback is not "silent".** It only ever happens when the engine, not
the user, chose the driver, it is logged at `warn`, and the resolved choice is
observable through `Platform::video_driver_choice()`. A console build that must
fail hard can check that value or set `SDL_VIDEODRIVER` explicitly.

**Consequences.** `cargo run -p giochi-demo` works on a desktop and still prefers
KMSDRM on a console. Consumers that switch on the choice can tell the two apart.

---

## ADR-016 — `Platform` owns an `Rc`'d SDL context

**Context.** SDL2 requires `SDL_Quit` to run after every window is destroyed, and
the `sdl2` crate ties subsystem lifetimes to the `Sdl` handle. A `Window` that only
borrowed the platform would force a lifetime parameter — forbidden on public
functions and structs.

**Decision.** `Platform` owns `Rc<SdlContext>` (the `Sdl`, video, timer and event
subsystems). `Window` holds a clone of that `Rc`, so a window may outlive the
`Platform` handle that created it. The field is private; no `Rc` is observable from
the public API.

**Why not a lifetime parameter.** `Window<'a>` would violate ADR-006 and would
infect every user signature. Why not `Arc`: the engine is single-threaded
(ADR-010), so atomics would be pure overhead.

**Consequences.** Field declaration order matters in `Window`: the SDL window drops
before the `Rc` releases the video subsystem. That ordering is documented on the
struct and is the reason the field is named `_context` and declared last.

---

## ADR-017 — `sdl2` is built with `unsafe_textures`

**Context.** Without that feature, `sdl2::render::Texture<'r>` borrows the canvas
and `TextureCreator::create_texture`/`create_texture_from_surface` do not exist at
all — only `Canvas::create_texture`, which returns a texture tied to the canvas
borrow. Storing textures in a `HashMap<u64, Rc<Texture>>` (ADR-002) is then
impossible without either a lifetime parameter on `TextureRegistry` (forbidden by
ADR-006) or `unsafe` raw-pointer juggling (forbidden in the core crate).

**Decision.** Enable the `unsafe_textures` cargo feature of `sdl2`. It changes
`Texture` to an owned value with a `Drop` impl that calls `SDL_DestroyTexture`, so
a texture can live in the handle registry independently of any borrow.

**Why this is sound in this engine.** The feature's documented hazard is using a
texture with a *different* canvas than its creator. The engine has exactly one
canvas for its whole lifetime (`RenderContext`) and one registry bound to that
canvas's creator (ADR-018), which is the same guarantee the feature asks for.

**Consequences.** One extra cargo feature, recorded in the workspace manifest with
a comment. If the engine ever needs multiple canvases, `TextureRegistry` must track
which renderer a texture belongs to.

---

## ADR-018 — `RenderContext` owns the window and the texture registry

**Context.** SDL binds a renderer to a window: `CanvasBuilder` consumes the
`sdl2::video::Window`. A `RenderContext` that only borrowed a `Window` would need a
lifetime parameter on a public struct and on `RenderContext::new`, both forbidden.

**Decision.** `Window::into_render_context()` consumes the `Window` (it splits into
the SDL window plus the shared SDL context), and `RenderContext` owns the canvas,
the `TextureRegistry` and the current `Camera`. Window-level operations the engine
still needs (`size`, `drawable_size`, `set_title`, `fit_camera_to_window`) are
delegated through the canvas.

**Consequences.**
* The lifecycle is explicit: create the `Window`, then hand it to the renderer.
* There is exactly one canvas and one registry per engine, which is also what makes
  ADR-017 sound and what the singleton C ABI (ADR-012) assumes.
* `RenderContext::textures()` borrows `&self`, so a draw loop must use the
  delegating methods (`create_texture`, `destroy_texture`, `texture_size`) instead
  of holding the registry reference across `clear`/`draw_*`/`present`. This is
  documented on `textures()`.

---

## ADR-019 — Input snapshots per frame, with injectable state

**Context.** Scenes need one consistent input view per frame, and the same input
code must be testable without a window and drivable from the C ABI and from
scripted replays. Two designs compete: query SDL directly from gameplay code, or
snapshot into an engine-owned state.

**Decision.** [`Input`] owns a snapshot. `Input::update(&[Event])` runs once per
frame, before the scene's `update`, and:

1. **clears** the one-frame data (pressed/released edges, mouse delta, wheel);
2. **applies** every event in order;
3. opens the first available controller if none is open.

Held state (keys down, buttons down, cursor position) survives the frame. Every
state machine is a pure method on its type (`KeyboardState::press/release`,
`MouseState::press/release/add_wheel`, `set_position`, `add_delta`), so tests and
replays drive the identical code path with no SDL involvement.

**Why not query SDL on demand.** It would fetch a different value on every call
inside one frame (SDL updates between calls), make edge detection impossible
without hidden globals, and force every query in the C ABI to touch SDL.

**Consequences.** One unavoidable extra step per frame (`input.update(&events)`),
documented in every example. Consumers that inject input use
`Input::without_gamepad()` plus `keyboard_mut()`/`mouse_mut()`.

---

## ADR-020 — Gamepad axes and buttons are queried live from SDL

**Context.** SDL can either deliver `ControllerAxisMotion`/`ControllerButtonDown`
events that the engine accumulates, or report the current device state on demand
(`GameController::axis`/`button`).

**Decision.** Query live. `GamepadState` holds the open `GameController` and
delegates `axis`/`is_down` to SDL, which already applies the dead zones and
curves from the controller mapping. The engine only handles
`ControllerDeviceAdded`/`ControllerDeviceRemoved`, because those change *which*
device is open.

**Why.** The live query is the authoritative current state, needs no accumulation
books, and cannot desynchronise if an event is dropped. It also keeps
`GamepadState` tiny: no axis array, no button set.

**Consequences.** Gamepad state cannot be injected the way keyboard and mouse can
(there is no virtual controller). Tests therefore cover id mapping and
normalisation directly and assert "disconnected ⇒ neutral" rather than faking a
device; a real controller is needed to exercise the live path.

---

## ADR-021 — Missing audio device degrades to silent mode

**Context.** `SDL_mixer` needs an open audio device to load and play anything.
CI containers, headless builders and consoles that have not started their sound
driver all lack one. Two options: fail `AudioSystem::new`, or start anyway.

**Decision.** `AudioSystem::new` only fails if the platform layer itself cannot be
asked for the subsystem. When the device cannot be opened, it logs a warning and
returns a usable system in **silent mode**:

* `AudioSystem::is_available()` returns `false`;
* `load_sound`, `load_music`, `play_sound`, `play_music` return
  `GiochiError::Audio` with a message saying
  the engine is in silent mode;
* volume, pause/resume/stop and every query remain safe no-ops.

**Why.** A game must not refuse to start because a container has no sound card, and
tests must be able to build an `AudioSystem` deterministically. A caller that
requires audio can check `is_available()` once at start-up and decide for itself.

**Consequences.** Every audio call site must be prepared for an `Err` even in code
that "obviously" has sound. That is the same discipline the C ABI already imposes
(`i32` return codes). `AudioSystem::silent()` exists so tests can assert the silent
behaviour without depending on the host.

---

## ADR-022 — The resource cache is keyed by the exact path string

**Context.** `"assets/player.png"`, `"./assets/player.png"` and an absolute path all
name the same file. Keying the cache by a canonicalised path would merge them.

**Decision.** Key by the string the caller passed. No canonicalisation, no
symlink resolution, no case folding.

**Why.** Canonicalising costs a filesystem round-trip on every `load_texture` — the
exact cost the cache exists to avoid — and still does not merge `"Player.png"` with
`"player.png"` on a case-insensitive filesystem. Predictable behaviour ("the same
argument returns the same handle") is worth more than implicit merging.

**Consequences.** A project that spells a path two ways uploads two textures. The
documented rule is simple enough to follow, and `ResourceManager::is_texture_cached`
lets a caller verify what the engine actually holds.

**Stale handles.** A texture destroyed behind the cache's back would leave the
manager handing out a dead handle. `ResourceManager` therefore validates every
cached handle against the registry on lookup and reloads when it is gone.

---

## ADR-023 — `Chunk`/`Music` are stored behind `Rc` in the handle registry

**Context.** `sdl2::mixer::Chunk` and `Music` own their `Mix_Chunk`/`Mix_Music` and
free them on `Drop`. `Music<'a>` carries a lifetime, but `Music::from_file` returns
`Music<'static>`, so it fits a `HandleRegistry`.

**Decision.** Both live in crate-private `HandleRegistry<Chunk>` and
`HandleRegistry<Music<'static>>`, exposed only as `SoundHandle`/`MusicHandle`. The
registries keep the SDL objects alive independently of any borrow, exactly like
textures (ADR-002).

**Why not a plain `HashMap<u64, Chunk>`.** `HandleRegistry` already guarantees the
two properties the ABI needs: `0` is never issued and handles are never reused, so a
stale sound handle can never play a new sound.

**Consequences.** Destroying a music handle also halts playback, because the mixer
must never keep a pointer to a freed `Mix_Music`. That is enforced in
`AudioSystem::destroy_music`.

---

## ADR-024 — The engine lends its subsystems to scenes; scenes never own them

**Context.** A scene needs the renderer, the input snapshot and the audio system.
Three designs exist: the scene owns them, the scene receives a context struct, or
the engine passes each one as a method parameter.

**Decision.** Every `Scene` callback receives exactly what it needs, borrowed from
the engine:

```text
update(&mut self, Time, &Input, &mut AudioSystem)
render(&mut self, &mut RenderContext)
```

Nothing is stored in the scene. The `Engine` remains the single owner.

**Why.**
* A `SceneContext` struct would have to bundle `&mut` references with lifetimes,
  which is exactly what R1 forbids on anything user-visible and what makes the C ABI
  singleton design necessary anyway (ADR-012).
* Letting the scene own the renderer would make the scene unmovable and the stack
  unflexible (`SceneManager` stores `Box<dyn Scene>`).
* Passing parameters keeps every scene a plain struct with no engine plumbing.

**Consequences.** `update` has four parameters. A scene that wants to play a sound
must do it in `update` (or store the handle and play it there); `render` cannot
reach audio, which is the right split: rendering is read-only for game state.

**Audio was added in phase 5.** Phase 0–4 declared `update(&mut self, Time, &Input)`.
The `mini_game` example immediately showed the gap: a scene could load a sound
handle but had no legal way to play it. Widening the signature before the C ABI and
the Lua binding exist is a one-crate edit; doing it later would break both.

---

## ADR-025 — Scene transitions are requests applied after the frame

**Context.** A scene must be able to end the game, pop itself (return to a menu) or
replace itself. Doing that from inside `render` would free the scene while it is
still drawing.

**Decision.** `Scene::transition()` is polled **after** `render` + `present`, and the
engine applies the answer:

| `SceneTransition` | Engine action |
|---|---|
| `Continue` | nothing |
| `Quit` | sets the loop's quit flag and leaves `is_running()` false |
| `Pop` | pops the top scene (its `on_exit` runs) |
| `Replace` / `Push` | logged as unusable from a transition value |

**Why `Replace`/`Push` cannot be applied.** A transition is a plain `enum`
(`#[repr(C)]`, no payload), so it cannot carry a `Box<dyn Scene>`. A scene that wants
to switch does it explicitly through `Engine::push_scene`/`replace_scene`, which take
the new scene. The transition enum stays ABI-friendly (a `u32` in C) and the awkward
"enum with a boxed payload" disappears.

**Runaway guard.** More than 10 000 transitions in one run returns
`GiochiError::Scene` instead of spinning forever. That only happens if a scene
returns a transition every single frame *and* the engine kept applying it, which is
a scene bug the engine reports rather than hangs on.

**Consequences.** A scene cannot free itself mid-frame. `Quit` reaches a coherent
state: after `run_current` returns, `is_running()` is `false` — a bug the phase-5
integration test caught in the first implementation, where the loop broke out but
left the flag set.

---

## ADR-026 — `Engine::run_current` is the single loop; `--frames` lives in the engine

**Context.** Every example and the demo need "run N frames and exit" so they can be
verified from a script. Implementing that per binary duplicates the loop and lets
each one drift.

**Decision.** `Engine` owns the loop and the frame limit:

* `Engine::run_current()` runs the scene stack; `Engine::run(scene)` replaces the
  stack first;
* `Engine::set_frame_limit(n)` / `clear_frame_limit()` control the automatic stop;
* `Engine::run_stats()` reports frames, steps, elapsed time and derived rates;
* `Engine::reset_quit()` re-arms the loop after a quit, for hosts that run one scene,
  return to their own UI, then run another.

**Why in the engine and not in the demo.** The frame limit is the mechanism every
smoke test depends on, so it must be in the library — otherwise `giochi-demo`,
`mini_game`, `audio_test` and `input_test` would each reimplement it and only the
tested one would be correct.

**Consequences.** `run_current` clears the limit only when the caller says so; the
limit persists across `reset_quit`, which is what a test wants. `giochi-demo --verify`
uses it to prove the full pipeline (texture loaded, sound loaded, audio device state)
in one frame.

## ADR-027 — The ARM64 gate is `cargo check`; linking is a separate, verified path

**Context.** Every phase gate runs
`cargo check --workspace --target aarch64-unknown-linux-gnu`, as the master prompt
specifies. While adding the release workflow in phase 6, a real cross build was
attempted for the first time, and it failed for two reasons that `cargo check`
cannot see:

1. `.cargo/config.toml` had no
   `[target.aarch64-unknown-linux-gnu] linker`, so Cargo used the host `cc` for the
   ARM64 target and the link died with
   `rust-lld: error: --fix-cortex-a53-843419 is only supported on AArch64`.
2. The `image`, `mixer` and `ttf` features of the `sdl2` crate link the *system*
   `SDL2_image`/`SDL2_mixer`/`SDL2_ttf`. The `bundled` feature builds core SDL2
   only, so nothing provided those three libraries for aarch64, and the link died
   with `cannot find -lSDL2_mixer`.

Neither is detectable by `cargo check`, which never links. The gate was therefore
green while the primary production target could not be built at all.

**Decision.**

* Fix the linker, permanently, in `.cargo/config.toml`. Host builds ignore it.
* Keep `cargo check` as the per-phase gate: it is fast, and the phase protocol asks
  for exactly that command.
* Add `scripts/build-aarch64.sh`, which performs a real cross **build** and asserts
  the resulting binaries are AArch64 ELF by inspecting them with `file`. It
  resolves the satellite libraries from `CROSS_SYSROOT` or from the Debian/Ubuntu
  multiarch directory, and fails with instructions when they are absent.
* Run it in the `cross-compile` workflow on every relevant push and in the release
  workflow for both targets, so the gap cannot reopen.

**Why not make the link part of the mandatory gate.** It needs ARM64 SDL2 packages
installed on the developer machine, which is a much heavier prerequisite than a
Rust target and a GCC cross compiler. The gate must stay runnable everywhere; the
link check runs where ARM64 libraries are guaranteed to exist.

**Consequences.** `docs/CROSS_COMPILE.md` documents both scripts and the verified
multiarch recipe. `README.md` no longer claims `scripts/check-aarch64.sh --release`
performs a link — it never did, and `--release` was simply forwarded to
`cargo check`.

## ADR-028 — A callback may use exactly what the engine lends it

**Context.** The C ABI is singleton-based (ADR-012): `giochi_render_draw_rect` has no
context parameter, so it must find the renderer somewhere. Inside a scene callback
the engine is already holding `&mut RenderContext`, so a nested lookup through the
singleton would either panic (`RefCell`) or be undefined behaviour (a second `&mut`
into an already-borrowed value).

The naive fix — a thread-local `*mut RenderContext` set for the whole run — is UB
under Rust's aliasing rules: the pointer outlives the `&mut` it was derived from and
is used after `clear`/`present` invalidated it.

**Decision.**

1. The **texture registry** became `Rc<TextureRegistry>` inside `RenderContext`
   (`RenderContext::shared_textures`). Every registry operation takes `&self`, so
   `giochi_texture_*` works from any context with no borrow at all.
2. `update` and `render` install the subsystems they were *given as parameters* into
   thread-local slots, and clear them when the callback returns. A pointer is only
   ever dereferenced while the reference it came from is alive and not used.
3. `on_enter`/`on_exit` are delivered by the C layer **outside** the engine borrow
   (`giochi_engine_run_scene` calls them around `Engine::run`; the deferred exit is
   delivered after the stack is emptied), so they can use everything.
4. Anything not lent reports `GIOCHI_ERROR_SCENE` with a message naming the callback
   the call belongs in.

The resulting matrix (also in `docs/C_ABI_CONTRACT.md` §3.8):

| Callback | Renderer | Input | Mixer | Textures |
|---|---|---|---|---|
| `on_enter` / `on_exit` | yes | — | yes | yes |
| `update` | — | yes | yes | yes |
| `render` | yes | — | — | yes |

**Consequences.** A scene can create textures anywhere but can only draw in
`render`, which the contract already required. `on_exit` fires once per
`giochi_engine_run_scene` call rather than once per stack residency, so a C host has
a simple, symmetric contract instead of the full Rust stack semantics. The engine's
`Scene` trait is unchanged: no existing scene had to be edited.

## ADR-029 — `Engine::step` shares the loop body with `run_current`

**Context.** The ABI publishes `giochi_engine_step(float frame_time)`: one frame,
caller-supplied frame time, no sleeping. `Engine::run_current` takes its frame time
from an internal clock and sleeps, and `Engine::set_frame_limit(1)` would abuse the
statistics and still ignore the parameter.

**Decision.** Extract the frame body into `Engine::run_frame(frame_time)`, returning
`FrameOutcome::{Continue, Transitioned, Stop}`. `run_current` keeps the loop, the
sleep and the transition guard; `Engine::step` runs one frame and accumulates
statistics without resetting them.

**Consequences.** There is exactly one implementation of the mandated frame order, so
`run` and `step` cannot drift. A host driving `step` in its own loop gets cumulative
`frames`/`steps`/`elapsed`, which is what an editor's profiler wants. The refactor
was verified by the existing `engine_smoke` test and by all five example smoke runs.

## ADR-030 — Lua gets capabilities, not a global engine

**Context.** The Lua callbacks run inside the engine's frame. `Scene::update` receives
`&Input` and `&mut AudioSystem`, `Scene::render` receives `&mut RenderContext`. A Lua
closure created once at start-up cannot capture those references, and the C ABI's
answer (thread-local raw pointers) would cost `#![forbid(unsafe_code)]`.

**Decision.** Each callback is invoked inside `Lua::scope`, with the subsystems it is
allowed to touch wrapped in short-lived userdata and passed as arguments:

```lua
update = function(time, input, audio) ... end,
render = function(gfx) ... end,
```

`mlua` invalidates scoped userdata when the scope drops, so a capability stored in a
global raises a Lua error instead of dereferencing a dead borrow. The texture registry
is the exception: it is `Rc`-shared, so `Texture` userdata can be created and freed
anywhere, including from the garbage collector.

**Why not a command buffer.** Deferring draw calls would keep the API safe and free of
`unsafe`, but it also defers every error to the end of the frame, adds an allocation
per draw and makes the API second-order. Immediate mode with a scope costs one small
allocation per callback and keeps errors where they happen.

**Why not `unsafe` `*mut` like the C ABI.** The C ABI has no choice: a C caller has no
lifetime to check. Lua does — `Lua::scope` is exactly that check. Using `unsafe` here
would be opting out of a guarantee that is available.

**Consequences.** The binding is not a thin wrapper: `update` and `render` have a
different shape from `Scene`. That is deliberate and documented in
`docs/LUA_BINDING.md`. The app owns the engine and everything used outside the loop,
which is why `app:` methods raise "the app is busy" if called mid-frame, and why
`giochi.quit()` is a module function rather than a method.

## ADR-031 — `Texture` freed by `Drop`, `Sound`/`Music` by the app

**Context.** R2 requires explicit lifetimes and no RAII *at the C boundary*. Lua has a
garbage collector, and a script that forgets `destroy` would leak for the life of the
process.

**Decision.** `Texture` frees its handle from `Drop` **and** from `:destroy()`, both
idempotent. `Sound` and `Music` are freed only by `app:destroy_sound`/`destroy_music`
or at process exit.

**Why the asymmetry.** `Drop` needs to reach the registry. Textures have one, shared
and safely reachable from anywhere (`RenderContext::shared_textures`). The mixer is
owned by the engine, so a `Drop` for `Sound` would need the engine — which does not
exist at collection time and would be borrowed during a frame.

**Why `Drop` does not violate R2.** R2 governs the *C boundary*, where a caller has no
destructor semantics to rely on. In Lua, finalisation is guaranteed by the language, so
using it is the idiomatic expression of the same rule: every handle has exactly one
owner and is released exactly once. The explicit `:destroy()` still exists for
determinism, and `docs/LUA_BINDING.md` §6 states both.

**Consequences.** `mini_game.lua` ends with `app:texture_count() == 0`; the reference
game and both integration tests assert it. Sounds and music are process-scoped, which
matches how a scripted game uses them.

## ADR-032 — The C-shaped rules are checked by a test, not by review

**Context.** R1 and R3 are invariants about *every* public item: `#[repr(C)]` on every
user-visible type, `_reserved: [u32; N ≥ 4]` as the last field of every `#[repr(C)]`
struct, no `Rc`/`Arc`/`Box`/`String`/`Vec`/`HashMap` in a public field, no generic or
explicitly-borrowing public function. A reviewer checks the code in front of them; a new
module, a new field or a new `pub` helper can break the rule silently, and the failure
only appears when a C consumer tries to use the type — months later, in another language.

**Decision.** `giochi-engine/tests/c_shape.rs` reads the crate's own source and fails the
gate on any violation. It runs as part of `cargo test`, so it needs no extra tooling and
no CI step of its own. The scan is textual rather than `syn`-based, because the codebase
is `rustfmt`-formatted and a readable 200-line test beats an extra dependency.

**What the test deliberately allows.**

* **Private `_reserved`.** R3 asks for the space, not for the visibility. A private
  padding field occupies the same bytes and is strictly better: a C caller cannot
  corrupt padding it was told to ignore.
* **`#[repr(C)]` enums without a tail.** R3 names structs. A C enum is the size of its
  discriminant, a reserved tail would need a variant, and the ABI maps these to plain
  `int`s anyway (`docs/C_ABI_CONTRACT.md` §3).
* **`GiochiError`.** R4 requires a `thiserror` enum with `String` payloads that never
  crosses the boundary. It sits in an allow-list that also demands the cited rule, and a
  second test asserts every entry names a real type and a real rule, so the list cannot
  rot into a way of silencing failures.

**Why the test checks itself.** `the_scanner_finds_the_crate_types` fails if the scan
stops finding the types it is meant to check, and the allow-list test fails if an entry
goes stale. Without that, a refactor that moved every file would turn the whole suite
green and prove nothing.

**Consequences.** The audit that produced this ADR found three real violations, fixed in
the same change: `FrameClock` had neither `#[repr(C)]` nor `_reserved`, and
`SceneManager` had `_reserved` but no `#[repr(C)]`. Both are marked `#[repr(C)]` now,
with `FrameClock` gaining its reserved tail. Verified by breaking the rule on purpose
(removing `#[repr(C)]` from `Sprite`, shrinking `RunStats::_reserved` to `[u32; 2]`):
each failure names the file, the line and the rule.

## ADR-033 — The MSRV is verified, and it is not the one the prompt suggested

**Context.** The workspace declared `rust-version = "1.75"`. Nothing checked it. A
declared MSRV is a promise to whoever is on an older toolchain: cargo will refuse to build
with a clear message instead of failing somewhere inside a dependency.

Probing it (`cargo +1.75.0 build --workspace`, then 1.79, 1.82) shows the promise was
already false:

* `mlua 0.10.5` requires rustc **1.79**;
* `size_of` as a bare prelude function needs **1.80** (used by `tests/abi_layout.rs`);
* 1.79 fails, **1.82 builds the whole workspace**.

The prompt's own dependency list (`mlua = "0.10"`) makes 1.75 unreachable, so the
specification contradicts itself here; the declaration is what has to change.

**Decision.** `rust-version = "1.82"`, with a comment saying how it was measured and that
bumping a dependency requires probing again. The README's prerequisite row names the same
number.

**Why not keep 1.75 and pin `mlua` lower.** Downgrading the reference binding to satisfy a
number nobody uses would trade a real capability for a cosmetic claim. The honest fix is to
publish the floor the project actually has.

**Consequences.** Anyone on 1.75–1.81 now gets cargo's clear "requires rustc 1.82" message
instead of a confusing failure inside `mlua`. The floor is a measured value, not a guess:
1.79 was tried and failed, 1.82 was tried and passed.

## ADR-034 — Optional image decoders are probed, never assumed

**Context.** `ensure_image_subsystem` asked SDL_image for `PNG | JPG`. `IMG_Init` is
all-or-nothing: it fails if *any* requested flag cannot be satisfied. On a console image
`libSDL2_image` can be built with no external codec at all — it links only `libSDL2`,
`libm` and `libc`, and the PNG and JPEG entry points are compiled as "not supported"
stubs. On such a target the engine's initialisation failed, file-based texture loading
was declared dead, and the log blamed SDL_image for what was really one missing codec —
while BMP, TGA, PNM and QOI kept working, because those decoders are built in and need
no flag.

The failure is invisible on a desktop, where the distribution's `libSDL2_image` always
has both codecs, so nothing in the desktop test suite could have caught it.

**Decision.** Open the subsystem with `InitFlag::empty()`, which fails only if
SDL_image itself is unusable, then probe `PNG` and `JPG` separately and warn once per
missing decoder. Formats that need no flag need no second step.

**Why "ask for everything and accept the failure" is not enough.** A flag set is a
*request*, and a single failure cannot distinguish "no SDL_image" from "no PNG" from
"no JPG". Probing one flag at a time is the only way to learn what a given build has.

**Consequences.** A codec-less target loads BMP assets and logs exactly which formats it
cannot decode, instead of refusing to start. BMP is the safe deployment format for such
images ([KMSDRM.md](KMSDRM.md) §8.4), and `--verify` reports the live texture count so a
mis-shipped PNG shows up on the first run rather than as an invisible sprite.

## ADR-035 — The drawable size is authoritative, not the configured size

**Context.** `EngineConfig::width`/`height` were logged as if they described the window.
On a desktop they do. On KMSDRM they do not: the backend is fullscreen-only, binds the
panel's native mode and silently ignores the requested size. Asking for 1280x720 on a
640x480 panel produces a 640x480 drawable with no error, no warning and no scaling. A
scene laid out for 1280x720 is cropped, and the startup log agrees with the configuration
rather than with the screen — which is the worst possible combination for debugging.

**Decision.** The engine reads the real drawable from the renderer once the window
exists, reports that in the startup line, and logs an extra
`requested WxH, backend opened WxH` line when the two differ.
`giochi_render_size` and `RenderContext::size()` keep returning the real drawable — they
always did — so the API and the log now tell the same story.

**What is deliberately not done.** No scaling is attempted and the requested size is not
silently rewritten into the config. Games read `RenderContext::size()` and lay out
against it. Auto-scaling would hide the mismatch behind a transform and make the same
game code behave differently on KMSDRM and on a desktop.

**Consequences.** The mismatch becomes a logged fact on the first frame instead of a
mystery on the panel. Two checks moved with it: the C smoke test no longer asserts the
requested render size on kmsdrm, and `--quadro` screenshots are documented as panel-sized
([KMSDRM.md](KMSDRM.md) §8.3).

---

## ADR-036 — Assets ship as PNG plus a BMP twin, resolved at load time

**Context.** ADR-034 makes the engine tolerate a codec-less `libSDL2_image`, but
tolerating it does not make the repository runnable there. `assets/` held only
`player.png` and `checker.png`, so on the R35S — whose `libSDL2_image` links only
`libSDL2`, `libm` and `libc` — a plain checkout could not run its own demo:
`load_texture("assets/player.png")` failed with "Unsupported image format". The
hardware run only worked because a hand-converted BMP had been deployed **renamed to
`.png`**, which is a lie the format auto-detection happened to accept.

**Decision.** Every asset is committed twice, `<name>.png` and `<name>.bmp`, with
identical pixels. The shipped consumers try the PNG first and fall back to the BMP
when loading fails: the demo, the Lua reference game, and both ABI smoke tests
(`c_smoke.c`, `python_smoke.py`). `scripts/png-to-bmp.py` regenerates the twins from
the PNGs — `--check` compares bytes and `scripts/gates.sh` runs it — and
`giochi-engine/tests/asset_formats.rs` decodes both through SDL and compares sizes.

The Rust examples under `giochi-engine/examples/` still name the PNG directly: they
are host-side teaching code, and the two reference games already show the pattern.

**What is deliberately not done.** No capability query was added to the public API.
`Renderer::image_formats()` would need a C-shaped return type, `_reserved` padding and
a documented lifetime for a bitmask that saves one failed `IMG_Load` per asset. No
auto-conversion happens at runtime either: converting on the device would mean
shipping a decoder to the platform that lacks one.

**Why not BMP only.** BMP is the safe deployment format, but it is 22× larger here
(4.2 KB against 194 bytes for a 32x32 sprite) and PNG is what every other consumer of
`assets/` expects. Keeping both costs 20 KB of repository and buys "works on both".

**Consequences.** Adding an asset means running `scripts/png-to-bmp.py assets/new.png`
and committing both files; the test fails loudly if that step is skipped. The 32-bit
`BI_BITFIELDS` + `BITMAPV4HEADER` form is used so the alpha channel survives — SDL
decodes it with no external library.
