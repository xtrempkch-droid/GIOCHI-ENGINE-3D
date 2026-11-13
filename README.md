# GIOCHI-ENGINE

[![CI](https://github.com/xtrempkch-droid/GIOCHI-ENGINE-3D/actions/workflows/ci.yml/badge.svg)](https://github.com/xtrempkch-droid/GIOCHI-ENGINE-3D/actions/workflows/ci.yml)
[![Cross compile](https://github.com/xtrempkch-droid/GIOCHI-ENGINE-3D/actions/workflows/cross-compile.yml/badge.svg)](https://github.com/xtrempkch-droid/GIOCHI-ENGINE-3D/actions/workflows/cross-compile.yml)
[![Release](https://github.com/xtrempkch-droid/GIOCHI-ENGINE-3D/actions/workflows/release.yml/badge.svg)](https://github.com/xtrempkch-droid/GIOCHI-ENGINE-3D/actions/workflows/release.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

A game engine in Rust on top of SDL2, designed from day one to be consumed from any
language through a C ABI. **2D** through `sdl2::render`, and **3D** through OpenGL ES 2.0
over EGL in a separate crate. Primary target: `aarch64-unknown-linux-gnu` (Linux consoles
with KMSDRM); development on `x86_64-unknown-linux-gnu`.

> **Status: 0.4.0.** Phases 0–18, plus the networking layer and ABI v4. The workspace,
> the SDL2 platform layer, the fixed-timestep loop, the 2D renderer with opaque texture
> handles, keyboard/mouse/gamepad input, the asset cache, the audio mixer, the scene
> stack, the `Engine` facade, the **C ABI** (`libgiochi.so` / `libgiochi.a` + generated
> `giochi.h`, parts I–IV, version 4), the **Lua binding** (`giochi` module + a complete
> reference game), the **3D crate** — GLES/EGL context, arbitrary OBJ or generated
> geometry, textures, Blinn-Phong lighting, `Camera3D` with a controller, instancing,
> static batching and upscaling — the **networking crate** (`giochi-net`:
> host-authoritative UDP, interest management, optional `laminar` transport and mDNS
> discovery) and the **profiler** (frame statistics, `F1` overlay), with CI, release
> packaging and the guides. Validated on a Rockchip PX30 (Mali-G31) through 0.3.1, and
> 0.4.0 — networking, textures and arbitrary OBJ models — on an **R35S (RK3326 /
> Cortex-A35, Mali-G31 Panfrost, KMSDRM)**. See [PROGRESS.md](PROGRESS.md).


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
| [`giochi-engine-3d`](giochi-engine-3d/) | lib | 3D: GLES 2.0 over EGL, meshes, **textures**, shaders, lighting, camera |
| [`giochi-net`](giochi-net/) | lib | Networking: host-authoritative UDP, interest management, optional `laminar`/`mdns` |
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

The 3D crate has its own:

```bash
cargo run -p giochi-engine-3d --example cube_demo      # one lit, textured cube
cargo run -p giochi-engine-3d --example model_viewer   # any .obj, framed automatically
cargo run -p giochi-engine-3d --example mini_fps       # F2 instancing, F3 render scale
cargo run -p giochi-engine-3d --example ball_balance    # moving camera + physics-lite
```

They need a GL driver; `SDL_VIDEODRIVER=offscreen` runs them headless.

### Profiling

Phase 15 adds an in-game overlay and a 60-frame frame-time window behind the
`profiling` feature. `F1` toggles the panel; without the feature everything compiles to
a no-op.

```bash
cargo run -p giochi-demo --example profiler_demo --features profiling
cargo run --release -p giochi-demo --example profiler_demo --features profiling -- \
    --sprites 1000 --frames 600
```

The demo's `--immediate` arm draws through the pre-phase-16 path and `--centred-camera`
reproduces the phase-15 scene, so a before/after table comes from one binary;
`--textures N`, `--use-atlas`, `--sprite-size N` and `--batch-max N` select the
scenario. `docs/PERF_LOG.md` records the measured numbers and the profiler's overhead
(≤1.1 % on x86_64, 1.94 % on the PX30 at 1000 sprites). See ADR-042 for why the overlay
is a bitmap font on `sdl2::render` rather than egui/puffin.

### Batching

Phase 16 makes `draw_texture` queue into a `SpriteBatch`: consecutive sprites that
share a texture become one draw call, without reordering (SDL's 2D renderer has no
depth buffer, so reordering would change what overlapping sprites look like).
`present` flushes automatically, the public API is unchanged, and `TextureAtlas` packs
several images into one texture for the cases where the order alternates.

```rust
let atlas = renderer.load_atlas(&["assets/player.png", "assets/coin.png"])?;
renderer.draw_texture(atlas.texture(), dst, atlas.region("coin").pixel_rect())?;
// one draw call on flush
```

Measured: 1000 sprites of one texture go from 1000 draw calls to 1, and `render2d`
drops 3.08x on the accelerated backend (0.403 → 0.131 ms). On the PX30 a scene of ten
interleaved textures runs at 10.8 → 58.6 FPS at 5000 sprites once the images share an
atlas. On SDL's `software` renderer batching costs 3 % at 1000 sprites — that backend
rasterises triangles instead of using its fast blit — so a host there can set a batch
of 1 or call `draw_texture_immediate`. Details in `docs/PERF_LOG.md` and ADR-043.

### Textures on 3D objects

A 3D object is textured in three steps: upload the image, draw with a shader that samples
it, and pass the texture to the draw call. The vertex format has carried a `uv`
attribute since the first mesh, and every primitive (`cube`, `plane`, `sphere`) and
every OBJ loaded by the crate supplies one — so there is no extra vertex work.

```rust
use std::path::Path;
use giochi_engine_3d::math3d::{Camera3D, Color3D, Transform3D, Vec3};
use giochi_engine_3d::shader::BuiltinShader;

// 1. Upload. PNG, JPEG and BMP all work; the format is sniffed from the file.
let texture = render.create_texture(Path::new("assets/checker.png"))?;
let (width, height) = render.texture_size(texture).expect("live texture");

// 2. A shader that samples `u_texture`. `Textured` is the built-in one.
let shader = render.create_builtin_shader(BuiltinShader::Textured)?;
let cube = render.create_cube_mesh(1.0)?;

// 3. Draw. The last argument is a tint multiplied over the sample; WHITE is "as-is".
let camera = Camera3D::new().with_aspect(16.0 / 9.0);
render.begin_frame(&window);
render.clear(&window, Color3D::SKY);
render.draw_mesh_textured(
    cube,
    shader,
    texture,
    &Transform3D::IDENTITY.with_position(Vec3::new(0.0, 0.0, -4.0)),
    &camera,
    Color3D::WHITE,
)?;
render.end_frame(&window);

// 4. Explicit teardown, like every other handle.
render.destroy_texture(texture);
```

Procedural textures skip the file entirely:

```rust
// 2x2 checkerboard: red and blue, RGBA8, exactly width * height * 4 bytes.
let pixels: [u8; 16] = [
    255, 0, 0, 255,   0, 0, 255, 255,
    0, 0, 255, 255, 255, 0, 0, 255,
];
let texture = render.create_texture_from_rgba(2, 2, &pixels)?;
```

Things worth knowing:

* **A 3D texture is not a 2D texture.** `giochi_engine::TextureRegistry` (and
  `giochi_texture_create` in C) returns an `sdl2::render` texture for the 2D canvas;
  raw GLES cannot sample it. In 3D use `RenderContext3D::create_texture`, or
  `giochi_gl_texture_create` / `_create_rgba` from C.
* **Power-of-two vs not.** GLES 2.0 only allows mipmaps and `GL_REPEAT` on
  power-of-two sizes. The upload picks `REPEAT` + trilinear filtering for those and
  `CLAMP_TO_EDGE` + bilinear for the rest, so an odd-sized sprite atlas does not render
  black.
* **`uv` is per vertex**, in `0.0..=1.0`. A `Vertex::flat` has `(0, 0)` and samples a
  single texel; use `Vertex::new(position, normal, uv)` when building geometry by hand.
* **Tint, not lighting.** The textured shader multiplies the sample by one Lambert
  term and by `u_tint`; use `GIOCHI_SHADER_PHONG` when you need specular highlights.
* `render.texture_count()` is part of the leak check `release_all` performs.

Runnable example (the cube uses any image you point it at):

```bash
cargo run -p giochi-engine-3d --example cube_demo -- --texture assets/checker.png
SDL_VIDEODRIVER=offscreen cargo run -p giochi-engine-3d --example cube_demo -- \
    --texture assets/checker.png --frames 3 --quadro /tmp/cube.bmp
```

From C, the same three steps after `giochi_engine3d_create`:

```c
GiochiGlTextureHandle tex = giochi_gl_texture_create("assets/checker.png");
GiochiShaderHandle textured;
giochi_shader_builtin(GIOCHI_SHADER_TEXTURED, &textured);

GiochiColor3D white = { .r = 1, .g = 1, .b = 1, .a = 1, ._reserved = {0} };
giochi_renderer3d_draw_mesh_textured(engine, cube, textured, tex,
                                      &model, &camera, white);

giochi_gl_texture_destroy(tex);
```

### Your own 3D models (a dragon, a character, a level)

Cubes and spheres are **convenience primitives** for examples and tests — they are not
what the engine is limited to. Geometry comes from either of two places:

| Source | API | Use |
|---|---|---|
| A Wavefront `.obj` file | `render.load_obj_mesh(path)` / `giochi_mesh_load_obj` | anything exported from Blender, Maya, ZBrush, Tinkercad… |
| Raw vertices and indices | `render.create_mesh(&vertices, &indices)` / `giochi_mesh_create` | terrain, procedural geometry, a mesh generated at runtime |

```bash
# Any .obj you have. The camera frames it automatically from its bounding sphere.
cargo run -p giochi-engine-3d --example model_viewer -- dragon.obj
cargo run -p giochi-engine-3d --example model_viewer -- dragon.obj --texture dragon.png
SDL_VIDEODRIVER=offscreen cargo run -p giochi-engine-3d --example model_viewer -- \
    dragon.obj --frames 3 --quadro dragon.bmp
```

In code, the whole path is three lines:

```rust
let mesh    = render.load_obj_mesh(Path::new("assets/dragon.obj"))?;
let shader  = render.create_builtin_shader(BuiltinShader::Phong)?;
render.draw_mesh_colored(mesh, shader, &model, &camera, Color3D::new(0.8, 0.7, 0.6, 1.0))?;
```

**Exporting the OBJ** (Blender): `File → Export → Wavefront (.obj)`. Enable
*Triangulate* or let the loader do it (it triangulates on load), and enable *Write
Normals*. UVs are only needed if you will texture it — the loader keeps them.

What the loader accepts and does:

* **Triangles and quads.** Quads are triangulated on load.
* **Several objects/groups** in one file (`o`/`g`) are merged into **one mesh**, which is
  what lets one handle draw a whole character.
* **Missing normals** are derived per face from the triangles; a `vn` that is `NaN` is
  treated as missing rather than shading the model black.
* **Missing UVs** become `(0, 0)`; a textured draw then samples one texel.
* **A non-finite position** (a corrupt file) is refused with the file name and the vertex
  index, instead of drawing nothing.
* **One texture per file.** Materials from `.mtl` are not applied: split the model into
  files if different parts need different images.
* **No skeletal animation.** The engine uploads static geometry. An animated character is
  either several meshes (one per pose/part) or geometry you transform yourself.

Model size matters on the console: indices are `u32`, so a mesh may exceed 65 535
vertices, but each vertex is 32 bytes — a 100 k-vertex dragon is ~3.2 MB of VBO plus
indices. `render.mesh_vertex_count(handle)` and `mesh_index_count` report what was
uploaded.

### 3D: instancing, static batching and upscaling

Three optimisations from phase 17, all additive:

```rust
// Many copies of one mesh, one draw call.
let matrices: Vec<Mat4> = crates.iter().map(Transform3D::matrix).collect();
let calls = render.draw_instanced(
    crate_mesh, instanced_shader, &matrices, &camera, Color3D::new(0.9, 0.7, 0.4, 1.0),
)?;                       // 1 with the driver's extension, matrices.len() without

// Scenery that never moves, assembled once.
let batch = StaticBatch::build(&mut render, StaticBatchBuilder::new()
    .add_mesh(&vertices, &indices, &Transform3D::from_translation(Vec3::new(3.0, 0.0, 0.0)).matrix())?
    .add_mesh(&vertices, &indices, &Transform3D::IDENTITY.matrix())?)?;
render.draw_mesh_colored(batch.mesh(), shader, &Transform3D::IDENTITY, &camera, colour)?;

// Fewer pixels when the frame is fill-bound.
render.set_render_scale(Some(0.75));
```

Instancing is detected at runtime across GLES 3.0 core, `GL_ANGLE_instanced_arrays` and
`GL_EXT_instanced_arrays`. Neither the PX30 nor the x86_64 dev machine exposes ANGLE —
both have `GL_EXT_instanced_arrays` — which is why the detection covers all three rather
than trusting the name in the plan.

`mini_fps` demonstrates all of it: **F2** toggles instancing, **F3** cycles the render
scale, and the static scenery is one batch per material.

Measured: 100 instanced cubes take the frame from 103 draw calls to 4 and from 4391.7 to
25776.0 FPS on x86_64 (**5.87x**); 12.19x in the criterion benchmark. Upscaling at 0.75
gives **+61 %** on the PX30 (fill-bound) and **−30 %** on a discrete GPU (submission-bound),
so measure before turning it on. See `docs/PERF_LOG.md` and ADR-044.

### Networking

`giochi-net` is a host-authoritative LAN layer: the host simulates, clients send input
and receive state. It is synchronous (no `tokio`), never blocks the fixed timestep, and
has **zero dependencies** unless you opt in to `laminar` (reliable/ordered packets) or
`mdns` (host discovery).

```rust
use giochi_net::{Cliente, Host, Mensagem};

// Host: port 0 asks the OS for an ephemeral one.
let mut host = Host::novo(None)?;
println!("listening on {}", host.endereco()?.port());

// Client: "ip:porta".
let mut cliente = Cliente::entrar("192.168.1.10:47721")?;

// Each frame: pump both ends, then read what arrived.
for (jogador, mensagem) in host.processar(64) {
    if let Mensagem::Input { mover_x, .. } = mensagem {
        /* simulate player `jogador` */
    }
}
for mensagem in cliente.processar(64) {
    match mensagem {
        Mensagem::Inimigos { inimigos, .. } => { /* AOI-filtered snapshot */ }
        Mensagem::Global { dados, .. } => { /* wave, tower, boss: opaque bytes */ }
        _ => {}
    }
}
```

Interest management is per client: `host.definir_area(jogador, raio)` sets how much of
the world that player sees, and `Host::tick_snapshot` sends each client only the enemies
inside its area, at `SNAPSHOT_HZ` (15 Hz) regardless of the 60 Hz call rate. The report's
arithmetic is in [docs/REDE.md](docs/REDE.md): ~150 enemies × 4 clients go from ~45 KB/s
to ~4 KB/s.

| Feature | What it adds | When to enable |
|---|---|---|
| *(default)* | UDP transport, protocol, AOI, host/client | always usable; zero dependencies |
| `laminar` | reliable-ordered, reliable-sequenced and unreliable channels | shop purchases, global state, anything that cannot be lost |
| `mdns` | `Anunciante` / `Descoberta` for finding hosts on the LAN | handhelds, where typing an IP is not an option |

```bash
cargo test -p giochi-net                    # 39 tests, zero dependencies
cargo test -p giochi-net --all-features     # + laminar and mdns
cargo run -p giochi-net --example loopback_smoke   # host + client end to end
```

`loopback_smoke` is the same path a console runs: a real host, a real client, handshake,
input, AOI filtering and opaque global state. It exists as an example because the libtest
harness does not start on the R35S (it links a `GLIBC_2.39` weak symbol and the device has
2.38) — plain binaries link only what the device offers.

The C ABI exposes the whole loop as `giochi_net_*` (ABI v4): `host_create`,
`client_join`, `pump`, `send_input`, `poll_input`, `host_snapshot`, `broadcast`,
`poll_snapshot`, `poll_global`, `set_area`, `set_camera_pos`. Both poll functions return
`GIOCHI_NET_AGUARDE` (`1`) when there is nothing new, which is the normal case at 60 Hz.
Full guide: [docs/C_ABI.md](docs/C_ABI.md) §11; design and measurements:
[docs/REDE.md](docs/REDE.md).

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

`gates.sh` also builds and tests `giochi-net` with every optional feature
(`--all-features`), because the workspace build alone never compiles `laminar` or `mdns`;
and `scripts/check-aarch64.sh` cross-compiles both, since they pull in system-interface
crates the default transport does not.

## Design rules (short version)

1. The public surface is C-shaped from the start.
2. Resources are `u64` handles with explicit create/destroy.
3. Every public struct is `#[repr(C)]` and ends with `_reserved`.
4. `thiserror` inside, `i32` codes plus `giochi_get_last_error()` outside.
5. **`giochi-engine` stays 2D.** No `render` backend abstraction, no meshes, no shaders,
   no pipeline in the 2D crate. 3D is a separate crate on EGL + GLES
   ([`giochi-engine-3d`](giochi-engine-3d/)) and the two never share a canvas (ADR-037).

Decisions and their rationale: [docs/DECISIONS.md](docs/DECISIONS.md). How the
rules are actually enforced by the crate layout, and why the modules depend on each
other the way they do: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Documentation

| File | Contents |
|---|---|
| [docs/API_GUIDE.md](docs/API_GUIDE.md) | Task-oriented guide: scenes, rendering, input, audio, assets, errors |
| [docs/LUA_BINDING.md](docs/LUA_BINDING.md) | The `giochi` Lua module, capabilities, handles, the reference game |
| [docs/C_ABI.md](docs/C_ABI.md) | Consuming the engine from C, Python, Go, Zig, C# (parts I–IV) |
| [docs/REDE.md](docs/REDE.md) | Networking: protocol, interest management, measurements, what is missing |
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
