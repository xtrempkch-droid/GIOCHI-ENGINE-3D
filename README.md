# GIOCHI-ENGINE

A **2D** game engine in Rust on top of SDL2, designed from day one to be consumed
from any language through a C ABI. Primary target: `aarch64-unknown-linux-gnu`
(Linux consoles with KMSDRM); development on `x86_64-unknown-linux-gnu`.

> **Status: phase 5 (scenes + `Engine`).** The workspace, the C ABI contract, the
> value types, the SDL2 platform layer, the fixed-timestep loop, the 2D renderer with
> opaque texture handles, keyboard/mouse/gamepad input, the asset cache, the audio
> mixer, the scene stack and the `Engine` facade all exist. CI, release packaging and
> the remaining guides land in phase 6 — see [PROGRESS.md](PROGRESS.md).

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
