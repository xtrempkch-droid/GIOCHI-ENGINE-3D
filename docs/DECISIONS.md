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
