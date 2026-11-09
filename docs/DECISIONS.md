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
| 037 | 3D is a separate crate, not a render backend | Accepted (supersedes ADR-005) |
| 038 | GLES 2.0 by default, 3.0 opt-in | Accepted |
| 039 | 3D uses an opaque engine pointer, unlike the 2D ABI v1 | Accepted |

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
sanctioned path. It must not reuse `sdl2::render`.

**Superseded by ADR-037** for the 3D half: extension v2.1 adds phases 9–14, which build
that crate. The 2D decisions in this ADR stand unchanged — 3D is still a separate crate
with its own pipeline, and `giochi-engine` still does not grow 3D concepts.

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

---

## ADR-037 — 3D is a separate crate, not a render backend

**Context.** The v2 scope was 2D-only (ADR-005), which named the sanctioned path for
3D: `giochi-engine-3d` with EGL + GLES, never `sdl2::render`. Phase 9 arrives through
extension v2.1, which builds exactly that. The question it forces is the one ADR-005
deferred: how do the two renderers relate?

**Decision.** Two crates, two renderers, no shared abstraction.

* `giochi-engine` keeps `sdl2::render` and is not modified except for additive
  window/GL helpers.
* `giochi-engine-3d` drives GLES through EGL via SDL2's window. It depends on
  `giochi-engine` for `Platform`, `Window`, input and audio — not for `render`.
* A game chooses one. There is no `RenderBackend` trait, no runtime switch, no
  "unified" context.

**Why not one abstraction.** A trait over `sdl2::render` and GLES would have to expose
the union of both: texture creation, state, pipeline and batching for GL; canvas
semantics for SDL. Every 2D caller would pay for concepts it never uses, and the
trait would be frozen before a single line of 3D existed — the exact failure ADR-005
was written to avoid. The overlap that actually exists (window, input, audio, handles)
is shared by sharing those modules, not by abstracting the renderer.

**What was added to the 2D crate, and why it is additive.** `Window::new_opengl`,
`Window::sdl_window` and `Platform::video_subsystem` are new `pub` items. None changes
an existing signature, so no 2D caller recompiles differently and `GIOCHI_ABI_VERSION`
does not move. `new_opengl` exists because `SDL_WINDOW_OPENGL` must be set when the
window is built, and the 2D `Window::new` deliberately does not set it: a window with
that flag plus a 2D renderer makes some drivers choose the software path.

**Consequences.** The two crates can be versioned and broken independently. A bug in
one cannot be fixed by the other's test suite, which is why the golden rule of the 3D
extension is "3D must never regress 2D": the 2D gates run unchanged in every 3D phase,
and the 3D crate has its own.

---

## ADR-038 — GLES 2.0 by default, 3.0 opt-in

**Context.** The ARM64 handhelds this project targets (RK3326 class, R35S/X35H) ship
drivers built for a fixed-function pipeline. Asking for GLES 3.0 on them fails, or —
worse — succeeds and then breaks on the first call that relies on a 3.0 feature. On a
desktop the driver answers 3.2 happily, so a bug that only appears on the console is
invisible during development.

**Decision.** Request GLES 2.0 unless `GIOCHI_GL_VERSION=3` is set. Detect the version
the driver actually returned, expose it as `RenderContext3D::gles_version()` and
`supports_gles3()`, and require every 3.0-only path to check the latter.

**Why detection, not trust.** The `GL_VERSION` string is not portable: Mesa reports
`OpenGL ES 3.2 Mesa ...`, some embedded drivers report `OpenGL ES 2.0 build ...`, and a
few report the fixed-function profile `OpenGL ES-CM 1.1`. The parser takes the first
integer it can find, treats `>= 3` as 3.0 and `2` as 2.0, and falls back to the
*requested* version when the string is unparseable — so an unusual driver is never
silently downgraded to a version the caller did not ask for.

**What is deliberately not done.** No extension allowlist and no feature flags are
exposed yet. `OES_vertex_array_object` is the one that matters and phase 11 handles it
inside the mesh code; exposing it now would promise a compatibility surface before the
first mesh draws.

**Consequences.** A shader written for `#version 300 es` will fail to compile on the
default context — deliberately, and with the driver's full log (see R10). Requesting
3.0 on hardware that has it is one environment variable away, and the fallback is
logged rather than silent.

---

## ADR-039 — 3D uses an opaque engine pointer, unlike the 2D ABI v1

**Context.** Part I of the C ABI hides a single engine-owned renderer behind
`giochi_render_*` functions, with no opaque context pointer in the header (ADR-012).
3D cannot follow that shape: `RenderContext3D` holds a `glow::Context` and an SDL
`GLContext`, neither of which has a C representation, and its size legitimately changes
between releases.

**Decision.** `GiochiEngine3D` is an opaque, forward-declared struct in `giochi.h`. The
caller never sees a layout; every function takes or returns a pointer. Creation returns
`NULL` on failure, because a caller cannot read an error code from an engine it does
not have — the same reason `SDL_CreateWindow` returns `NULL`.

**Why the divergence is safe.** Part I is unchanged, so existing consumers keep
working and `GIOCHI_ABI_VERSION` moves only when part II ships (phase 14). The two
shapes coexist because they never describe the same object: `giochi_render_*` is the
global 2D canvas, `giochi_engine3d_*` is an instance the caller owns.

**Consequences.** 3D callers have explicit lifetime management they do not have in 2D
(`giochi_engine3d_create` / `_destroy`). That is the correct trade: a 3D engine owns a
GPU context, and pretending otherwise would hide the expensive object behind global
state.

---

## ADR-040 — Lighting is one directional light on the renderer, Blinn-Phong, with a CPU normal matrix

**Context.** Phase 13 asks for directional Phong (ambient + diffuse + specular). Three
choices are not obvious and each has a cheap-looking alternative that fails later.

1. *Where the light lives.* Per mesh, per shader, per draw call, or per frame?
2. *What "direction" means.* The vector the light travels, or the vector towards it?
3. *How normals reach world space.* `mat3(u_model)` as the phase 11 flat shader already
   did, or an inverse-transpose computed somewhere.

**Decision.**

**One light, on `RenderContext3D`, sanitised on assignment.** `set_light` stores a
clamped `DirectionalLight`; every draw uploads it. Per-mesh lights would need a
registry, a handle and a lifetime for something every scene in this repository sets
once, and the per-draw cost is five uniforms either way. A scene that needs more than
one light bakes the extra ones into its vertex colours, which is what GLES 2.0 games of
this size have always done.

**`direction` points the way the light travels.** The shader negates it once. The
alternative convention is equally common, and that is exactly why it is written down
twice — in the struct docs and in `docs/3D.md` — instead of being inferred from the
shader.

**The normal matrix is `transpose(inverse(mat3(model)))`, computed on the CPU in
`Mat4::normal_matrix()`.** GLSL ES 1.00 has no `inverse()`, so it cannot be done in the
shader; `mat3(u_model)` is what the `flat` shader uses and is only accidentally correct
because nothing in phases 9–12 scaled an object non-uniformly. A singular model matrix
(a collapsing animation passing through a zero scale) yields the identity and a warning,
matching `Mat4::inverse`.

**Blinn-Phong (half vector) rather than classic Phong (`reflect`).** One `normalize`
instead of a `reflect` and a `pow`, and it does not degenerate when the eye sits near
the mirror direction — which is precisely where a player looking at a highlight is.

**Consequences.** `BuiltinShader::Phong` is additive: `flat` and `textured` are byte
identical to phase 11, so the validated 2D and 3D paths cannot regress because a light
was added. `mini_fps` switched to `phong`; `ball_balance` kept `flat` because its CI
colour thresholds are tuned to it and the ball is a sphere seen through a uniform
scale, where the flat shader's `mat3(u_model)` is exact.

**Verified by.** `giochi-engine-3d/tests/lighting.rs`, which reads the framebuffer back
and asserts the shading rather than the call sequence: a sphere is brighter above its
centre than below it under a downward light, that relationship inverts when the light
does, the specular term raises the peak, a red light zeroes the green and blue channels,
and a cube saturates under a downward light while going black under an upward one.

---

## ADR-041 — Four part-II functions act on an implicit "current 3D engine"

**Context.** The contract spells the mesh and shader calls without an engine parameter:

```c
int32_t giochi_mesh_load_obj(const char* path, GiochiMeshHandle* out);
int32_t giochi_mesh_destroy(GiochiMeshHandle mesh);
int32_t giochi_shader_load(const char* vert, const char* frag, GiochiShaderHandle* out);
int32_t giochi_shader_destroy(GiochiShaderHandle shader);
```

But a mesh is a GL object: the vertex/index buffers were created by a particular
`RenderContext3D`, and destroying one needs that context current. So these four functions
have to know *which* engine they act on, and the signature deliberately does not say.

The alternatives were both worse. Changing the signatures breaks the contract that part I
proved is worth keeping stable, and it makes every call four words longer for a parameter
that is almost always the same value. A process-wide registry of (handle → engine) would
be a second source of truth that has to stay in step with the renderer's own registry, and
would silently leak when an engine is destroyed with handles outstanding.

**Decision.** They act on the **most recently created engine on the calling thread**, held
in a thread-local pointer set by `giochi_engine3d_create` and cleared by
`giochi_engine3d_destroy`. Creating a second engine on the same thread is **refused**
(`NULL`, `GIOCHI_ERROR_INVALID_ARGUMENT`) rather than allowed to retarget them.

**Why refusing is the important half.** Without it, "most recently created" would be a
rule a caller could break by accident: create engine A, create engine B, destroy a mesh
made by A, and the destroy would look in B's registry, find nothing, and report
`INVALID_HANDLE` for a handle that is perfectly valid. With one engine per thread the
mapping is a bijection and the implicit parameter is unambiguous. The cost is that a
thread cannot drive two independent 3D scenes; that is the same cost part I already
accepted (ADR-012), and the workaround is the same — one engine, two views.

**Consequences.** A caller that wants several engines uses several threads, one each. That
is a legitimate pattern and the thread-local makes it work without any locking. The five
functions that *do* take an engine pointer (`begin_frame`, `end_frame`, `clear`,
`draw_mesh`, `destroy`) still validate nothing about the pointer's provenance beyond
non-NULL: an opaque handle in C is only ever as good as its owner's bookkeeping.

---

## ADR-042 — Phase 15 profiles with a feature-gated hand-rolled overlay, not egui/puffin

**Context.** The performance extension suggests `puffin` plus `egui`/`egui-sdl2` for
the phase-15 profiler, and explicitly allows a manual SDL overlay if the GUI crates
do not work on ARM64. Two facts of this codebase push that decision earlier than a
portability failure would:

1. The 2D renderer is the SDL `render` API, chosen so the KMSDRM path needs no
   OpenGL (ADR-037). `egui-sdl2` integrates through an OpenGL context; adopting it
   would make the 2D path depend on GL and on EGL/GBM behaviour on the console.
2. The production image is a minimal Buildroot rootfs. A GUI crate tree and a font
   asset are both real costs on that image, and the overlay is a developer aid, not a
   game feature.

`puffin` alone would give scopes, but not the frame-window aggregation, the C-ready
`FrameStats` shape that phase 18c needs, or the overlay. The project already has a
rule for this: measure with the simplest instrument that can produce a number.

**Decision.**

- Profiling is the `profiling` Cargo feature, with **no new dependency**. With the
  feature off, every `Profiler` method is an empty inlined body, the renderers do not
  increment counters, and the overlay returns immediately.
- `Profiler` measures scopes with `std::time::Instant` and keeps the last
  `FRAME_WINDOW` (60) samples in `Mutex<Vec<FrameStats>>`. The mutex is required by
  the extension and is uncontended; it is locked once per frame, never while drawing.
- `FrameStats` stores time as `u64` nanoseconds, not `Duration`. `Duration` has no
  guaranteed C layout; `GiochiFrameStats` in phase 18c is specified with
  `frame_time_ns: u64`. The Rust type therefore uses the ABI-ready representation and
  offers `frame_time()`/`frame_time_ms()` accessors for ergonomics.
- The overlay is drawn with `sdl2::render` from a 5x7 bitmap font authored in
  `src/perf/text.rs`, rasterised into **one persistent** RGBA texture that is updated
  in place. It refreshes at 1 Hz (the FPS number is already a 60-frame average) and
  toggles with `F1`; `Engine` starts it visible when the feature is compiled in. The
  sample and the summary are read only on a refresh, not on every frame. Panels
  narrower than 800 px use the 1x font, which is four times less pixels to upload.
- Audio call time is accumulated by the mixer itself in a `Cell<u64>` (the scene
  receives `&mut AudioSystem` directly, so there is no wrapper to time); the 3D
  renderer counts draws, meshes and vertices under its own forwarding `profiling`
  feature.

**Why the four named scopes and not an arbitrary tree.** The overlay has to answer
"where did the frame go" for the systems this engine actually has: update, 2D render,
3D render and audio. Unknown scope names are still accepted and accumulated in
`other_ns`, so a host is not blocked; a call-tree UI is not worth a dependency for
four rows.

**Consequences.** There is no flamegraph, no per-scope nesting display and no GPU
timer (GLES 2.0 and `sdl2::render` expose none). `gpu_time_hint_ns` stays zero. The
overhead at the 1000-sprite baseline is **≤1.1 %** on x86_64 and, on the PX30,
**0.56 %** of wall time with the panel hidden and **1.94 %** with it visible; that is
inside the extension's 2 % budget. The measurements also found the limit of the
approach: at 20000 sprites the per-draw counter increments cost ~5 % of CPU and the
panel pushes a 39.8 FPS run to 35.7 FPS. That stress point is exactly what phase 16's
batching removes; the budget is specified for a normal frame, and this is recorded in
`docs/PERF_LOG.md` rather than hidden. Because Cargo cannot say "on in debug, off in
release" as a default feature, `profiling` is explicitly opt-in in every build, which
is also what the phase-15 acceptance command uses.

**Verified by.** `perf/frame_stats.rs` and `perf/profiler.rs` unit tests, the
`profiler_smoke` integration test (counts, percentiles, overlay pixels, no texture
leak), the `context_smoke` 3D counter test, and the off/on comparison on x86_64 and on
the PX30 in `docs/PERF_LOG.md`.

## ADR-043 — 2D batching merges consecutive texture runs, and the atlas is a shelf packer

**Context.** Phase 16 has to turn "one `SDL_RenderCopy` per sprite" into "a few
`SDL_RenderGeometry` calls". Two facts constrain the design:

1. SDL's 2D renderer has **no depth buffer**: overlapping draws composite in
   submission order. Any reordering is therefore visible, not just slower or faster.
   A "sort the frame by texture" batcher changes what a game looks like whenever two
   sprites overlap, silently, and no test would catch it because the pixels are
   right for a different scene.
2. `SDL_RenderGeometry` takes one texture per call. There is no multi-texture batch
   in the 2D API — that is what the 3D path's instancing work (phase 17) is for.

**Decision.**

- `RenderContext::draw_texture` queues a quad into a `SpriteBatch`. The batch splits
  the queue into **runs of consecutive quads that share a texture**, and
  `flush_batch` issues one `render_geometry` call per run, in submission order. A
  texture change starts a new run; nothing is ever reordered. A scene that draws its
  sprites grouped (a tilemap, then entities) collapses to one call per group; a scene
  that interleaves ten textures gets ten calls per ten sprites, which is the old cost
  and is documented rather than hidden.
- Indices in each run are **run-relative**. `SDL_RenderGeometry` validates them
  against the vertex slice it is handed, so absolute offsets into the whole batch made
  every run after the first fail to parse. This was a real bug, caught by the
  "later draw must stay on top" test.
- `flush_batch` is called by `present`, by `read_pixels` (which is why it became
  `&mut self`), by `clear`, by every unbatched primitive (`fill_rect`, `draw_line`,
  `draw_point`, `draw_texture_ex`, `draw_texture_immediate`), by `destroy_texture`,
  by `clear_resource_cache`, by `set_batch_config`, and by `Engine::step` at the end
  of its 2D scope. So no public method can be observed with work still queued.
- `DEFAULT_MAX_SPRITES` is **1024**, not "as many as fit". A single `render_geometry`
  call of several thousand quads is *slower* than the same quads in chunks on the
  software renderer: it sets up the vertex array per call and then rasterises each
  triangle with per-vertex interpolation, instead of the fast blit path
  `RenderCopy` uses. Measured at 5000 sprites, `software`: 1024 quads/call 28.5 ms,
  4096 quads/call 44.9 ms, 8192 44.3 ms. On `opengl`/`opengles2` the limit makes no
  measurable difference (5000 sprites: 1.215 ms at 4096, 1.213 ms at 1024).
- `draw_texture_immediate` keeps the old `RenderCopy` path. `draw_texture_ex`
  (rotation, mirroring) also stays unbatched: SDL rotates around the centre of the
  destination rectangle, and reproducing that in four CPU-computed corners is not
  worth it for the handful of rotated elements a 2D UI has.
- `TextureAtlas` is a **shelf packer with 1 px padding**, sorted by height, rounded up
  to power-of-two dimensions, capped at `ATLAS_MAX_SIZE` (4096, the smallest maximum
  texture size the targets guarantee — GLES 2.0 only requires 2048). Regions carry
  both the normalised UV rectangle and the pixel rectangle, so a caller can either
  draw with `src` in pixels or hand the UVs to a custom shader. The atlas owns one
  texture in the normal registry; it is destroyed like any other. `from_images` takes
  paths, `from_rgba_images` takes buffers, and both share the same packer, padding
  rules and duplicate-name check.
- **The public API does not change (R12).** `giochi_render_draw_texture` still queues
  a quad; C consumers, Lua bindings and existing games get the speed-up without
  recompiling. `SpriteBatch`/`SpriteBatchConfig`/`TextureAtlas` are additive, and
  their fields are `#[repr(C)]` with a reserved tail like every other public struct.

**Why not an atlas-only design ("every sprite must be in an atlas").** Most games
already load PNGs by path and would have to be rewritten. Batching by run costs one
comparison per quad and helps those games immediately; the atlas is then an *extra*
win for content that wants one upload.

**Consequences.** Memory per batch is `max_sprites * (4 vertices + 6 indices)`;
1024 sprites is 4096 vertices and 6144 indices, about 160 KiB of vertex data, reused
across frames. A texture destroyed while its sprites are still queued silently drops
those sprites (`destroy_texture` flushes first, so this only happens if the handle
died some other way) and logs at debug level. There is no multi-texture batch, no
sorting, and no z-order API change. `SpriteBatch` is `#[repr(C)]` but not part of the
C ABI, so its layout is not frozen.

**Measured outcome.** See `docs/PERF_LOG.md` "After Phase 16". The short version:

- Draw calls: N → 1 for a one-texture scene at 1000 sprites on every backend.
- `opengl`, phase-15 scene: `render2d` 0.403 → 0.131 ms (**3.08x**), whole frame
  2390.8 → 6765.5 FPS (**2.83x**). Whole-field scene: **4.49x** FPS.
- Criterion bench: 737 → 191 µs at 1000 sprites (**3.86x**).
- PX30 (`opengles2`): `render2d` at 20000 sprites **28.495 → 22.853 ms (−19.8 %)**;
  ten interleaved textures vs an atlas at 5000 sprites **10.8 → 58.6 FPS (5.4x)**.
- `software` renderer, whole-field scene: **−3.1 %** at 1000 sprites. Batching is not
  free there, because that backend's geometry path is a triangle rasteriser and its
  `RenderCopy` is a fast blit. It is inside the 10 % threshold, the cause is named, and
  a host on that backend can set a batch of 1 or use `draw_texture_immediate`.

A run of one quad is drawn with `RenderCopy`, not `render_geometry`: a single sprite has
nothing to merge, and the copy is cheaper. A run of two or more is geometry.

**Verified by.** 17 unit tests in `render/batch.rs` (run boundaries, run-relative
indices, single-quad runs keeping their rectangles, source clipping shrinking the
destination, capacity, reserved tail), 10 in `render/atlas.rs` (shelf packing,
non-overlap and padding, power-of-two sizing, in-memory validation), the
`render_batching` integration test (1000 sprites → 1 call, pixels identical to the
immediate path, submission order preserved, culling, auto-flush at the limit, file and
in-memory atlas → 1 call, no texture leak) and the criterion benchmark
`benches/sprites_2d.rs`.

## ADR-044 — Instancing is detected at runtime for three providers, and upscaling is bilinear

**Context.** Phase 17 asks for instanced rendering, static mesh batching and optional
resolution scaling. Three facts decided the design, and two of them contradict what the
plan expected.

1. **The extension the plan named is not the one the hardware has.** The extension
   suggested `GL_ANGLE_instanced_arrays` and warned that "not every ARM64 driver exposes
   it". Measured, *neither* target exposes it: the PX30 (Mali-G31/Mesa 24.0.0 Panfrost)
   and the x86_64 development machine (RX 580/Mesa 26.0.8 radeonsi) both report
   `GL_EXT_instanced_arrays` + `GL_EXT_draw_instanced` and no ANGLE, and no
   `GL_OES_draw_instanced` either. Detection is not a nicety here — a detector written to
   the plan would have fallen back on 100 % of the available hardware, and the phase would
   have "shipped" at 1.0x with every test passing.
2. **GLES 2.0 guarantees only eight vertex attributes.** A `mat4` per instance consumes
   four slots and the mesh uses three, which is seven — one spare. The plan warned this
   could overflow and suggested packing a quaternion plus a position (7 floats) and
   rebuilding the matrix in the shader. At eight slots it fits, and reconstructing a
   matrix per vertex would cost more than the slot it saved.
3. **FSR is out of reach, not merely optional.** FSR 1.0 is a compute-shader pair and
   GLES 2.0 has no compute; FSR 2.x needs motion vectors and temporal accumulation. The
   plan already said this; the ADR records *what was done instead*, because "FSR-like"
   is a claim a reader will test against the code.

**Decision.**

- **Three providers, detected once at context creation.** `InstancingSupport` is
  `Core` (GLES 3.0, through glow), `Angle` or `Ext` (both loaded with
  `SDL_GL_GetProcAddress`), or `None`. `GlContext` queries `GL_EXTENSIONS` eagerly —
  the string is only valid while the context is current — and loads the entry points
  while the SDL window is still in scope. Checking `supports_gles3()` first means a
  GLES 3.0 context never needs a symbol lookup.
- **A fourth state is deliberate: advertised extension, null entry point.** The detection
  logs a warning and moves to the next provider rather than trusting the string. A driver
  bug there would otherwise turn into a call through a null pointer.
- **The model matrix arrives as four `vec4` attributes (3..6), column-major.** They are
  bound by name before linking, like the mesh attributes, so the numbering is one table
  rather than two. `Program::is_instanced` is answered by `glGetAttribLocation` **after
  linking**, not by reading the source: a shader that declares an attribute and never
  reads it is optimised away, and the question is "will this program accept it".
- **The instanced path binds no VAO.** `glVertexAttribDivisor` is vertex-array state;
  setting it on a mesh's VAO would leave the divisor on for that mesh's next ordinary
  draw, where attributes 3..6 would read from a buffer that is not bound. Seven
  `vertexAttribPointer` calls per instanced draw is nothing beside the N draws it
  replaces, and the code resets the divisors and disables the slots afterwards because
  `bind_vertex_array(None)` does not.
- **`draw_instanced` returns the number of backend draw calls it issued** — 1 with a
  provider, `instances.len()` without, 0 for an empty batch. The fallback is not an
  error; it draws the same picture through the ordinary path. Returning the count is
  what lets a caller (and a test) tell the two apart instead of inferring it from the
  frame rate.
- **`StaticBatch` bakes transforms and lives in the ordinary mesh registry.** The result
  is drawn with `draw_mesh_colored`, so nothing downstream learns that a batch exists,
  and it is destroyed, counted and released like any other mesh. Normals go through
  `Mat4::normal_matrix` and are renormalised on the CPU, because the inverse-transpose
  changes their length under a non-uniform scale.
- **Upscaling is a bilinear full-screen quad, not a shader effect.** `GL_LINEAR` on the
  colour attachment does the filtering in the texture unit; the pass is four lines of
  GLSL and one `draw_arrays`. The renderer reports the **low-resolution** size from
  `drawable_size` while a scale is set, so the camera's aspect ratio follows
  automatically — the same "the real surface is not what you asked for" rule KMSDRM
  forced in phase 9, reused rather than special-cased. `read_pixels` runs the upscale
  first, so a capture is the picture the player sees.

**Why not sort the scene into batches automatically.** Deciding what is static is a
question about the game's future, not about the geometry: an obstacle that will be
knocked over next frame is indistinguishable from a wall today. Baking the wrong thing
does not slow the frame down, it freezes the object in place. The API therefore makes
the caller say it, and the demo shows the split: 48 immobile obstacles and crates in two
batches, 100 cubes that exist to be instanced left out.

**Consequences.**

- No instancing on a driver with only `GL_EXT_draw_instanced`: it supplies an instance
  count but no per-instance attributes, which is the entire mechanism. Such a driver gets
  the loop, at the loop's speed, and `draw_instanced` says so in its return value.
- A batch cannot animate, cannot change shader, and is expensive to rebuild. That is the
  documented contract, not a limitation to work around later.
- The batch has one colour. A scene with two materials becomes two batches — still a
  large win, and the honest description of what "50 meshes → 1 draw call" means.
- `Mat4` is 80 bytes on the wire and the driver wants 64, so the instance buffer is
  repacked every draw instead of cast. A stride mismatch would read every instance from a
  wrong offset and draw garbage rather than failing, so the repack is asserted by a test
  that checks the translation column lands at `data[12..15]`.
- Upscaling only helps a **fill-bound** frame. Measured: +61.3 % at 0.75 on the PX30
  (5000 cubes, 30.0 → 48.4 FPS) and **−29.6 %** on the x86_64 discrete GPU at 100 cubes,
  where the frame is submission-bound and the extra pass is pure cost. A host that turns
  it on unconditionally is choosing to be slower on a fast machine.

**Verified by.** 6 pixel-level tests in `tests/instancing.rs` (instanced frame identical
to the loop within 2 % of pixels, the translation column landing where column-major puts
it, empty and unknown handles, the reported support matching the extension string), 3 in
`tests/static_batch.rs` (batch identical to 5 separate draws, 50 → 1 mesh, empty batch
rejected), 5 in `tests/upscale.rs` (visible frame at 1.0/0.75/0.5/0.25, window-sized
output, no GL error, target released), 11 unit tests in `mesh/static_batch.rs`, 5 in
`render3d/instanced.rs`, 5 in `render3d/upscale.rs`, 5 in `context/instancing.rs`, and
`benches/meshes_3d.rs`.
