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
