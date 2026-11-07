# GIOCHI-ENGINE

A **2D** game engine in Rust on top of SDL2, designed from day one to be consumed
from any language through a C ABI. Primary target: `aarch64-unknown-linux-gnu`
(Linux consoles with KMSDRM); development on `x86_64-unknown-linux-gnu`.

> **Status: phase 1 (platform + game loop).** The workspace, the C ABI contract,
> every value type, the SDL2 platform layer, the window wrapper and the
> fixed-timestep loop exist. Rendering lands in phase 2 — see
> [PROGRESS.md](PROGRESS.md).

## Why "C-shaped" Rust?

Every type a user can touch has a stable C layout, and every resource is an opaque
`u64` handle:

```rust
pub type TextureHandle = u64; // 0 = invalid

#[repr(C)]
pub struct Sprite {
    pub texture: TextureHandle,
    pub x: f32, pub y: f32, pub w: f32, pub h: f32,
    pub _reserved: [u32; 8],
}
```

No `Rc`, `Arc`, `String`, `Vec`, generic or lifetime ever appears in a public
struct field or a public function signature. That is what makes the future C, C++,
Lua, Python, Go, Zig or C# binding a wrapper instead of a rewrite. The full
projection is in [docs/C_ABI_CONTRACT.md](docs/C_ABI_CONTRACT.md) — that document
is authoritative.

## Workspace

| Crate | Kind | Purpose |
|---|---|---|
| [`giochi-engine`](giochi-engine/) | lib | Idiomatic Rust API, C-shaped data, no `unsafe` |
| [`giochi-engine-capi`](giochi-engine-capi/) | `cdylib` + `staticlib` | `extern "C"` surface and generated `giochi.h` |
| [`giochi-engine-lua`](giochi-engine-lua/) | lib | Reference binding (Lua 5.4 via mLua) |
| [`giochi-demo`](giochi-demo/) | bin | Bootstrap binary, examples, `lua` subcommand |

## Build

Prerequisites: Rust (stable), a C toolchain, CMake and pkg-config. SDL2 is built
from source, so no system SDL2 is required.

```bash
cargo build --workspace
cargo run -p giochi-demo      # opens a 1280x720 window; ESC or close quits
```

The demo accepts `--frames N` to run a fixed number of frames and exit, which is
handy for smoke tests:

```bash
cargo run -p giochi-demo -- --frames 60
```

### Video driver

On a Linux console the engine selects SDL2's `kmsdrm` driver automatically, but
only when `SDL_VIDEODRIVER` is unset — an explicit value is never overwritten. If
the auto-selected KMSDRM backend cannot start (a desktop compositor holds DRM
master, `/dev/dri` is unavailable, …) the engine logs a warning and falls back to
the desktop driver. Pin the driver to make that failure loud instead:

```bash
SDL_VIDEODRIVER=kmsdrm ./target/debug/giochi-demo   # production console
SDL_VIDEODRIVER=x11    ./target/debug/giochi-demo   # development desktop
```

All quality gates:

```bash
scripts/gates.sh             # fmt, clippy, build, test, doc, aarch64 check
SKIP_ARM64=1 scripts/gates.sh
```

ARM64 details: [docs/CROSS_COMPILE.md](docs/CROSS_COMPILE.md).

## Design rules (short version)

1. The public surface is C-shaped from the start.
2. Resources are `u64` handles with explicit create/destroy.
3. Every public struct is `#[repr(C)]` and ends with `_reserved`.
4. `thiserror` inside, `i32` codes plus `giochi_get_last_error()` outside.
5. **2D only.** No `render` backend abstraction, meshes, shaders or pipelines. 3D,
   if it ever happens, is a separate `giochi-engine-3d` crate on EGL + GLES.

Decisions and their rationale: [docs/DECISIONS.md](docs/DECISIONS.md).

## Documentation

| File | Contents |
|---|---|
| [docs/C_ABI_CONTRACT.md](docs/C_ABI_CONTRACT.md) | Authoritative C ABI: types, sizes, error codes, functions |
| [docs/DECISIONS.md](docs/DECISIONS.md) | Architecture decision records |
| [docs/CROSS_COMPILE.md](docs/CROSS_COMPILE.md) | ARM64 cross toolchain |
| [PROGRESS.md](PROGRESS.md) | Phase status and validated gates |
| [CHANGELOG.md](CHANGELOG.md) | Release history |
| [CONTRIBUTING.md](CONTRIBUTING.md) | Workflow, gates, coding rules |

## License

MIT — see [LICENSE](LICENSE).
