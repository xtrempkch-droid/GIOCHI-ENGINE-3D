# GIOCHI-ENGINE

[![CI](https://github.com/xtrempkch-droid/GIOCHI-ENGINE-3D/actions/workflows/ci.yml/badge.svg)](https://github.com/xtrempkch-droid/GIOCHI-ENGINE-3D/actions/workflows/ci.yml)
[![Cross compile](https://github.com/xtrempkch-droid/GIOCHI-ENGINE-3D/actions/workflows/cross-compile.yml/badge.svg)](https://github.com/xtrempkch-droid/GIOCHI-ENGINE-3D/actions/workflows/cross-compile.yml)
[![Release](https://github.com/xtrempkch-droid/GIOCHI-ENGINE-3D/actions/workflows/release.yml/badge.svg)](https://github.com/xtrempkch-droid/GIOCHI-ENGINE-3D/actions/workflows/release.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

A **2D** game engine in Rust on top of SDL2, designed from day one to be consumed
from any language through a C ABI. Primary target: `aarch64-unknown-linux-gnu`
(Linux consoles with KMSDRM); development on `x86_64-unknown-linux-gnu`.

> **Status: 0.1.0 — every phase (0 to 8) complete.** The workspace, the SDL2 platform
> layer, the fixed-timestep loop, the 2D renderer with opaque texture handles,
> keyboard/mouse/gamepad input, the asset cache, the audio mixer, the scene stack, the
> `Engine` facade, the **C ABI** (`libgiochi.so` / `libgiochi.a` + generated `giochi.h`)
> and the **Lua binding** (`giochi` module + a complete reference game), with CI,
> release packaging and the guides. See [PROGRESS.md](PROGRESS.md).


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
| [`giochi-engine-capi`](giochi-engine-capi/) | `cdylib` + `staticlib` | `libgiochi` C ABI and generated `giochi.h` |
| [`giochi-engine-lua`](giochi-engine-lua/) | lib | Reference binding — Lua 5.4 via mLua, no `unsafe` |
| [`giochi-demo`](giochi-demo/) | bin | The demo; `--frames`, `--verify`, `lua <script>` |

## Build

### Prerequisites

| Requirement | Notes |
|---|---|
| Rust stable | `rustup toolchain install stable`; the verified minimum is **1.82** (`rust-version` in `Cargo.toml`, driven by `mlua`) |
| A C toolchain | `build-essential` (GCC or Clang) |
| CMake ≥ 3.5 | SDL2 is built from source by the `bundled` feature, so a system SDL2 is **not** needed |
| `pkg-config` | locates optional SDL2 backends |

Optional backends are picked up when their headers are present; missing ones are
simply disabled:

```bash
sudo scripts/install-deps.sh   # the whole list, in one place
```

That script is what CI runs, so a local build and a CI build use the same package set.
Copying it into a job is how the X11 headers and the SDL2 satellites once went missing
from CI while working perfectly on a developer desktop.

On Ubuntu 24.04+ the bundled CMake files are older than the policy minimum CMake 4
accepts. [`cmake/aarch64-linux-toolchain.cmake`](cmake/aarch64-linux-toolchain.cmake)
and [`.cargo/config.toml`](.cargo/config.toml) work around this (ADR-014);
`cargo build` needs no extra flags.

### Native build

```bash
cargo build --workspace
cargo run -p giochi-demo      # opens a 1280x720 window; ESC or close quits
```

The demo accepts `--frames N` to run a fixed number of frames and exit, which is
handy for smoke tests:

```bash
cargo run -p giochi-demo -- --frames 60
```

### Examples

```bash
cargo run -p giochi-engine --example hello_rect      # three rectangles
cargo run -p giochi-engine --example moving_sprite   # bouncing sprite + camera
cargo run -p giochi-engine --example input_test      # WASD, mouse, gamepad
cargo run -p giochi-engine --example audio_test      # Space plays a synthesised beep
cargo run -p giochi-engine --example mini_game       # complete space shooter
```

All examples accept `--frames N` to run for a fixed number of frames, which is how
the smoke tests exercise them without a display.

### Cross build for ARM64

```bash
rustup target add aarch64-unknown-linux-gnu
sudo apt install gcc-aarch64-linux-gnu g++-aarch64-linux-gnu
scripts/check-aarch64.sh            # cargo check: fast, never links
scripts/build-aarch64.sh --release  # full compile and link, asserts AArch64 ELF
```

`scripts/check-aarch64.sh` points `sdl2-sys` at
[`cmake/aarch64-linux-toolchain.cmake`](cmake/aarch64-linux-toolchain.cmake) so the
bundled SDL2 becomes a real cross build instead of a native one that emits x86_64
linker flags. `scripts/build-aarch64.sh` does the same and then actually links,
which additionally needs the ARM64 `libSDL2_image`/`libSDL2_mixer`/`libSDL2_ttf`
(`bundled` builds core SDL2 only). Details, including the verified multiarch
recipe and how to supply a console sysroot:
[docs/CROSS_COMPILE.md](docs/CROSS_COMPILE.md).

### From C (and any language with a C FFI)

```bash
cargo build -p giochi-engine-capi --release
# target/<triple>/release/libgiochi.so, libgiochi.a, and giochi-engine-capi/include/giochi.h
```

```c
#include "giochi.h"

static void update(void *user_data, const GiochiTime *t) {
    (void)user_data; (void)t;
    if (giochi_input_key_pressed(27 /* SDLK_ESCAPE */)) {
        giochi_engine_request_quit();
    }
}

static void render(void *user_data) {
    (void)user_data;
    GiochiRect bar = { .x = 10, .y = 10, .w = 120, .h = 24 };
    GiochiColor green = { .r = 40, .g = 160, .b = 90, .a = 255 };
    giochi_render_draw_rect(&bar, green);
}

int main(void) {
    GiochiEngineConfig cfg = { .width = 960, .height = 540,
                               .target_fps = 60, .vsync = 1, .title = "my game" };
    if (giochi_engine_init(&cfg) != GIOCHI_ERROR_OK) {
        fprintf(stderr, "%s\n", giochi_get_last_error());
        return 1;
    }
    GiochiSceneCallbacks cb = { .update = update, .render = render };
    GiochiScene scene = giochi_scene_create(&cb);
    int32_t code = giochi_engine_run_scene(scene);
    giochi_scene_destroy(scene);
    giochi_engine_shutdown();
    return code == GIOCHI_ERROR_OK ? 0 : 1;
}
```

```bash
cc -std=c11 -I giochi-engine-capi/include my_game.c \
   -L target/x86_64-unknown-linux-gnu/release -lgiochi -o my_game
```

Resources are `uint64_t` handles with explicit create/destroy, `0` is never valid, and
every fallible call returns `0` or a negative code with the reason in
`giochi_get_last_error()`. Full guide: [docs/C_ABI.md](docs/C_ABI.md); the normative
specification is [docs/C_ABI_CONTRACT.md](docs/C_ABI_CONTRACT.md).

```bash
scripts/gen-header.sh --check   # the committed header matches the Rust crate
scripts/ffi-smoke.sh            # builds libgiochi, runs the C and Python consumers
```

### From Lua

```lua
local giochi = require("giochi")

local app = giochi.app { title = "my game", width = 960, height = 540 }
local player = app:texture("assets/player.png")
local x, y = 100, 100

app:run {
    update = function(time, input, audio)
        if input:key_down(giochi.keys.d) then x = x + 200 * time.delta end
        if input:key_down(giochi.keys.a) then x = x - 200 * time.delta end
        if input:key_pressed(giochi.keys.escape) then giochi.quit() end
    end,
    render = function(gfx)
        gfx:clear { 12, 14, 22 }
        gfx:draw(player, { x, y, 64, 64 })
    end,
}
```

```bash
cargo run -p giochi-demo -- lua giochi-engine-lua/examples/mini_game.lua
cargo run -p giochi-demo -- lua giochi-engine-lua/examples/mini_game.lua --auto --frames 600
```

The second form plays itself headlessly, which is how the binding is tested. A callback
receives exactly the capabilities it needs — `update` gets the clock, the input
snapshot and the mixer, `render` gets the renderer — and they stop working when it
returns, so the binding is `#![forbid(unsafe_code)]`. Full reference:
[docs/LUA_BINDING.md](docs/LUA_BINDING.md).

### Minimal render loop

```rust
use giochi_engine::{Color, Platform, Rect, RenderContext, WindowConfig};

let platform = Platform::new()?;
let window = platform.create_window(WindowConfig::new(960, 540), "game")?;
let mut render = RenderContext::new(window)?;

let player = render.create_texture("assets/player.png")?;
render.clear(Color::rgb(12, 14, 22));
render.draw_rect(Rect::new(10.0, 10.0, 100.0, 40.0), Color::rgb(230, 70, 70));
render.draw_texture(player, Rect::new(200.0, 120.0, 64.0, 64.0), Rect::ZERO)?;
render.present();
render.destroy_texture(player);
```

### Input per frame

```rust
use giochi_engine::input::keyboard::{KEY_SPACE, KEY_W};
use giochi_engine::{Input, Platform};

let mut input = Input::new(&platform);

let events = platform.poll_events();
input.update(&events); // clears the one-frame edges, then applies every event

if input.key_down(KEY_W) { /* held */ }
if input.key_pressed(KEY_SPACE) { /* tapped exactly once */ }
let (mouse_x, mouse_y) = input.mouse().position();
```

Order matters: `poll_events` → `input.update` → simulation → `present`. The
`pressed`/`released` edges are valid only until the next `input.update`.

### Assets and audio

```rust
// One upload per path, even across entities.
let sheet = render.load_texture("assets/player.png")?;
assert_eq!(render.load_texture("assets/player.png")?, sheet);

// Sound effects are fully decoded; music is streamed from disk.
let beep = audio.load_sound("assets/beep.wav")?;
audio.play_sound(beep, 0)?;
audio.set_volume(96);

// Explicit teardown: no RAII at the boundary.
audio.destroy_sound(beep);
render.destroy_texture(sheet);
```

`render` is the `&mut RenderContext` from above and `audio` a `&mut AudioSystem`
built with `AudioSystem::new(&platform)?`.

If no audio device is available the engine logs a warning and keeps running in
silent mode: `audio.is_available()` is `false` and load/play return an
`GIOCHI_ERROR_AUDIO` error instead of aborting the game.

### A game, in one file

`Engine` owns the platform, renderer, audio, input and the scene stack; a scene is a
plain struct implementing four callbacks.

```rust
use giochi_engine::{
    AudioSystem, Engine, EngineConfig, Input, RenderContext, Scene, Time,
};

struct Game { /* your state */ }

impl Scene for Game {
    fn on_enter(&mut self) { /* load textures and sounds */ }

    fn update(&mut self, time: Time, input: &Input, audio: &mut AudioSystem) {
        // fixed timestep: `time.delta` is constant, so simulation is deterministic
    }

    fn render(&mut self, renderer: &mut RenderContext) {
        // the engine already cleared the screen and will present after this returns
    }

    fn on_exit(&mut self) { /* destroy every handle you created */ }
}

fn main() -> Result<(), giochi_engine::GiochiError> {
    let mut engine = Engine::new(EngineConfig::new(960, 540, "my game"))?;
    engine.set_frame_limit(600); // optional: stop after 600 frames
    engine.run(Box::new(Game { /* ... */ }))?;
    Ok(())
}
```

Scenes never own engine objects: the engine lends the renderer and the mixer for the
duration of the call (ADR-024). A scene can ask to stop with
`SceneTransition::Quit`, or be swapped with `Engine::push_scene`/`replace_scene`.
`Engine::run_stats()` reports frames, steps and the achieved rate.

### Video driver

On a Linux console the engine selects SDL2's `kmsdrm` driver automatically, but
only when `SDL_VIDEODRIVER` is unset — an explicit value is never overwritten. If
the auto-selected KMSDRM backend cannot start (a desktop compositor holds DRM
master, `/dev/dri` is unavailable, …) the engine logs a warning and falls back to
the desktop driver. Pin the driver to make that failure loud instead:

```bash
SDL_VIDEODRIVER=kmsdrm ./target/debug/giochi-demo   # production console
SDL_VIDEODRIVER=x11    ./target/debug/giochi-demo   # development desktop
SDL_VIDEODRIVER=dummy  ./target/debug/giochi-demo   # headless (tests, CI)
```

The running session usually has to be in the `video` and `render` groups to open
`/dev/dri/card*`. The full procedure, the failure messages and the runtime
libraries the ARM64 target needs: [docs/KMSDRM.md](docs/KMSDRM.md).

All quality gates:

```bash
scripts/gates.sh             # fmt, clippy, build, test, doc, aarch64 check
SKIP_ARM64=1 scripts/gates.sh
```

## Design rules (short version)

1. The public surface is C-shaped from the start.
2. Resources are `u64` handles with explicit create/destroy.
3. Every public struct is `#[repr(C)]` and ends with `_reserved`.
4. `thiserror` inside, `i32` codes plus `giochi_get_last_error()` outside.
5. **2D only.** No `render` backend abstraction, meshes, shaders or pipelines. 3D,
   if it ever happens, is a separate `giochi-engine-3d` crate on EGL + GLES.

Decisions and their rationale: [docs/DECISIONS.md](docs/DECISIONS.md). How the
rules are actually enforced by the crate layout, and why the modules depend on each
other the way they do: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Documentation

| File | Contents |
|---|---|
| [docs/API_GUIDE.md](docs/API_GUIDE.md) | Task-oriented guide: scenes, rendering, input, audio, assets, errors |
| [docs/LUA_BINDING.md](docs/LUA_BINDING.md) | The `giochi` Lua module, capabilities, handles, the reference game |
| [docs/C_ABI.md](docs/C_ABI.md) | Consuming the engine from C, Python, Go, Zig, C# |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Layers, dependency rules, ownership, per-frame order, testing |
| [docs/C_ABI_CONTRACT.md](docs/C_ABI_CONTRACT.md) | Authoritative C ABI: types, sizes, error codes, functions |
| [docs/DECISIONS.md](docs/DECISIONS.md) | Architecture decision records |
| [docs/KMSDRM.md](docs/KMSDRM.md) | Bare-console video: permissions, failures, runtime libs |
| [docs/CROSS_COMPILE.md](docs/CROSS_COMPILE.md) | ARM64 cross toolchain |
| [PROGRESS.md](PROGRESS.md) | Phase status and validated gates |
| [CHANGELOG.md](CHANGELOG.md) | Release history |
| [CONTRIBUTING.md](CONTRIBUTING.md) | Workflow, gates, coding rules |

## License

MIT — see [LICENSE](LICENSE).
