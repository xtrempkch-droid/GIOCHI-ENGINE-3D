# API guide

A tour of the Rust API in the order you will meet it. Everything here is
`giochi_engine::*` unless stated otherwise. For the C projection of the same
surface see [C_ABI_CONTRACT.md](C_ABI_CONTRACT.md); for the reasoning behind the
boundaries see [ARCHITECTURE.md](ARCHITECTURE.md).

---

## 1. The five-minute version

```rust
use giochi_engine::{
    AudioSystem, Engine, EngineConfig, Input, RenderContext, Scene, Time,
};

struct MyGame {
    texture: u64,
}

impl Scene for MyGame {
    fn on_enter(&mut self) { /* load assets */ }

    fn update(&mut self, time: Time, input: &Input, audio: &mut AudioSystem) {
        // `time.delta` is the fixed timestep: deterministic simulation
    }

    fn render(&mut self, renderer: &mut RenderContext) {
        // the engine cleared the screen; it presents after this returns
    }

    fn on_exit(&mut self) { /* destroy your handles */ }
}

fn main() -> Result<(), giochi_engine::GiochiError> {
    let mut engine = Engine::new(EngineConfig::new(960, 540, "my game"))?;
    engine.run(Box::new(MyGame { texture: 0 }))?;
    Ok(())
}
```

`Engine::new` creates the window, the renderer, the input state and the audio
system. `Engine::run` runs until the user closes the window, presses `Escape`, or
the scene returns `SceneTransition::Quit`.

---

## 2. Configuration

```rust
use giochi_engine::EngineConfig;
```

| Member | Purpose |
|---|---|
| `EngineConfig::new(width, height, title)` | 60 Hz target, vsync on |
| `with_title(title)` / `set_title(title)` | replaces the title |
| `title_str()` | reads it back; `""` if the buffer is not valid UTF-8 |
| `is_vsync_enabled()` | `vsync != 0` |
| `window_config()` | the matching `WindowConfig` |
| `target_fps` | 1–999 becomes the fixed timestep; anything else keeps 60 Hz |

`width` or `height` of `0` is rejected with `GIOCHI_ERROR_INVALID_ARGUMENT` before
SDL is touched.

The title lives in a fixed `[u8; 64]` buffer, so it is truncated on a UTF-8
character boundary. That is what lets `GiochiEngineConfig` be a plain C struct
(ADR-007).

---

## 3. Scenes

```rust
use giochi_engine::Scene;
```

| Callback | Called | Default |
|---|---|---|
| `on_enter` | once, when pushed or replacing | no-op |
| `on_exit` | once, when popped, replaced, cleared, or on engine drop | no-op |
| `update(time, input, audio)` | once per **fixed step** | required |
| `render(renderer)` | once per **display frame** | required |
| `transition()` | once per frame, after `present` | `Continue` |

`update` may run zero, one or several times between two `render` calls. Never drive
simulation from `render`, and never read `pressed`/`released` edges outside the frame
in which they were produced.

### Transitions

```rust
use giochi_engine::SceneTransition;
```

| Value | Effect |
|---|---|
| `Continue` | keep running |
| `Quit` | stop the loop (`is_running()` becomes `false`) |
| `Pop` | pop this scene, returning to the one below |
| `Replace`, `Push` | cannot be applied from a transition value; use `Engine::replace_scene`/`push_scene` |

The enum carries no payload, which is what keeps it a plain `u32` in C. A scene that
wants to switch state uses the engine methods instead (ADR-025).

### The stack

```rust
engine.push_scene(Box::new(PauseMenu::new()));   // overlay: on_enter now
engine.replace_scene(Box::new(Level2::new()));   // exit old, enter new
engine.pop_scene();                               // back to the scene below
engine.scenes().depth();
```

Only the top scene receives callbacks. The scene below an overlay keeps its state
and is **not** re-entered when the overlay pops.

`EmptyScene` is a no-op scene that counts its callbacks; use it as a placeholder or
in tests.

---

## 4. Running and inspecting a loop

| Method | Purpose |
|---|---|
| `run(scene)` | replace the stack with `scene` and run |
| `run_current()` | run whatever is on the stack (an empty stack is legal and draws the clear colour) |
| `set_frame_limit(n)` / `clear_frame_limit()` | stop after `n` frames — this is what every `--frames` flag uses |
| `reset_quit()` | re-arm the loop after a quit, keeping the timestep |
| `request_quit()` / `is_running()` | cooperative shutdown |
| `run_stats()` | `RunStats { frames, steps, elapsed }` with `fps()`, `step_rate()`, `frame_time_ms()` |
| `frame_stats()` | the last measured frame (`profiling` feature; zeroed otherwise) |
| `frame_time_summary()` | min/max/avg/p50/p99 over the last 60 frames |
| `is_overlay_visible()` / `set_overlay_visible()` / `toggle_overlay()` | the debug panel; `F1` toggles it |
| `time()` | the simulation clock |
| `fixed_timestep()` | the step in seconds |
| `set_clear_color(color)` | background drawn before each frame |

```rust
engine.set_frame_limit(120);
engine.run_current()?;
let stats = engine.run_stats();
println!("{} frames, {} steps, {:.1} fps", stats.frames, stats.steps, stats.fps());
```

### Profiling (feature `profiling`)

`frame_stats()` and `frame_time_summary()` are populated only when the crate is built
with `--features profiling`; without it the methods still exist and return zeroed
values, and the renderer pays nothing. The same feature enables the overlay (`F1`).

```bash
cargo run -p giochi-demo --example profiler_demo --features profiling
cargo run --release -p giochi-demo --example profiler_demo --features profiling -- \
    --sprites 1000 --frames 600
```

```rust
let stats = engine.frame_stats();
let summary = engine.frame_time_summary();
println!(
    "{:.1} fps, frame {:.2} ms, p99 {:.2} ms, {} draw calls",
    summary.fps, stats.frame_time_ms(), summary.p99_ms(), stats.draw_calls
);
```

The baseline and the method behind it live in [`PERF_LOG.md`](PERF_LOG.md); the design
choice (bitmap overlay instead of egui/puffin) is ADR-042.

---

## 5. Rendering

```rust
use giochi_engine::{Camera, Color, Rect, Sprite, Vec2};
```

### Screen-space primitives

```rust
renderer.clear(Color::rgb(16, 18, 28));                       // if you clear yourself
renderer.draw_rect(Rect::new(10.0, 10.0, 100.0, 40.0), Color::rgb(230, 70, 70));
renderer.draw_texture(handle, Rect::new(20.0, 80.0, 64.0, 64.0), Rect::ZERO)?;
renderer.present();
```

`Rect::ZERO` as `src` means "the whole texture". A destination whose width or height
rounds to zero is a no-op, not an error. Coordinates are physical pixels.

### Textures

```rust
let handle = renderer.load_texture("assets/player.png")?;     // cached: 1 upload per path
let other  = renderer.create_texture("assets/coin.png")?;     // uncached primitive
let (w, h) = renderer.texture_size(handle).unwrap_or((0, 0));

renderer.destroy_texture(other);
```

* `load_texture` goes through `ResourceManager`; the same path returns the same
  handle and does not upload twice (ADR-022).
* `create_texture` always uploads.
* `create_texture_from_rgba(w, h, &pixels)` builds a texture from raw RGBA8 — handy
  for generated graphics and for tests.
* `destroy_texture` returns `false` for `0` or an already-destroyed handle.

### Sprites and the camera

```rust
let mut camera = Camera::new(Rect::new(0.0, 0.0, width, height));
camera.x = player.x;
camera.y = player.y;
camera.zoom = 1.25;
renderer.set_camera(camera);

let sprite = Sprite::new(handle, 100.0, 120.0, 64.0, 64.0);
renderer.draw_sprite(&sprite)?;
```

`draw_sprite` treats the sprite rectangle as **world** space and applies the camera;
`draw_rect`/`draw_texture` stay in screen space. The transform is
`screen = viewport.center + (world - camera.position) * zoom`; a zoom that is not
finite and positive degrades to `1.0` (ADR-018).

---

## 6. Input

```rust
use giochi_engine::input::keyboard::{KEY_A, KEY_D, KEY_SPACE};
use giochi_engine::input::mouse::MOUSE_RIGHT;
use giochi_engine::input::gamepad::{AXIS_LEFT_X, BUTTON_SOUTH};
```

Inside a scene, `input` is already updated for the frame:

```rust
if input.key_down(KEY_A) { /* held */ }
if input.key_pressed(KEY_SPACE) { /* went down this frame */ }
if input.key_released(KEY_SPACE) { /* went up this frame */ }

let (x, y) = input.mouse().position();
let drag = input.mouse().delta();
let zoom_step = input.mouse().wheel_delta();
if input.mouse_down(MOUSE_RIGHT) { /* right button held */ }

let stick = input.gamepad().axis(AXIS_LEFT_X);
if input.gamepad().is_down(BUTTON_SOUTH) { /* A / Cross */ }
```

Edge semantics (ADR-019):

| Rule | Behaviour |
|---|---|
| `pressed`/`released` | true for exactly one frame |
| fast tap | press **and** release inside one frame reports both edges |
| key auto-repeat | keeps the key `down`, does not re-raise `pressed` |
| unknown key/button/axis | "not down" / `0.0`, never an error |
| gamepad axes | clamped to `-1.0..=1.0`; triggers rest at `-1.0` |

Driving input yourself (replays, tests, scripted demos):

```rust
let mut input = Input::without_gamepad();
input.begin_frame();
input.keyboard_mut().press(KEY_D);
```

---

## 7. Audio

```rust
use giochi_engine::audio::{AudioSystem, LOOPS_INFINITE, MAX_VOLUME};
```

```rust
let beep = audio.load_sound("assets/beep.wav")?;   // decoded, low latency
audio.play_sound(beep, 0)?;                        // 0 = once, -1 = forever

let track = audio.load_music("assets/theme.ogg")?; // streamed
audio.play_music(track, LOOPS_INFINITE)?;
audio.pause_music();
audio.resume_music();
audio.set_volume(96);                              // clamped to 0..=128

audio.destroy_sound(beep);
audio.destroy_music(track);
```

`loops` is the number of **extra** repetitions. Destroying a music handle stops
playback first, so the mixer never holds a freed stream.

**Silent mode.** With no audio device the engine keeps running:
`audio.is_available()` is `false` and `load`/`play` return
`GIOCHI_ERROR_AUDIO`. Games should ignore that error (ADR-021):

```rust
if let Err(error) = audio.play_sound(beep, 0) {
    log::debug!("sem áudio: {error}");
}
```

---

## 8. Assets

```rust
use giochi_engine::assets::{asset_size, read_asset, MAX_ASSET_SIZE};
```

```rust
let size = asset_size("assets/data.bin")?;      // rejects directories, > 64 MiB
let mut buffer = vec![0u8; size];
let read = read_asset("assets/data.bin", &mut buffer)?;  // never truncates
```

The buffer belongs to the caller, which is what keeps the C ABI allocation-free. A
buffer smaller than the file is an error, not a partial read.

---

## 9. Math

`Vec2`, `Rect` and `Color` are `#[repr(C)]` value types with zeroed `_reserved`
tails.

```rust
let v = Vec2::new(3.0, 4.0);
assert_eq!(v.length(), 5.0);
let moved = v + Vec2::splat(1.0);
let scaled = moved * 2.0;

let r = Rect::new(0.0, 0.0, 100.0, 50.0);
assert!(r.contains(Vec2::new(10.0, 10.0)));
assert!(r.intersection(Rect::new(90.0, 40.0, 100.0, 100.0)).w > 0.0);

let c = Color::rgb(230, 70, 70);
```

`Rect::contains` is half-open (left/top inclusive, right/bottom exclusive), so
adjacent rectangles cannot both claim a shared edge.

---

## 10. Errors

Every fallible Rust call returns `GiochiResult<T> = Result<T, GiochiError>`.

```rust
use giochi_engine::{codes, GiochiError};

match engine_result {
    Err(error) if error.code() == codes::ASSET => { /* missing file */ }
    Err(error) => log::error!("{error}"),
    Ok(value) => { /* ... */ }
}
```

| Variant | Code | Typical cause |
|---|---|---|
| `InvalidHandle` | `-1` | `0` or an already-destroyed handle |
| `InvalidArgument` | `-2` | zero-sized window, empty path, bad timestep |
| `Platform` | `-3` | video driver, window creation |
| `Render` | `-4` | canvas/texture/blend failure |
| `Asset` | `-5` | missing file, unsupported format, too large |
| `Audio` | `-6` | no device, mixer failure |
| `Scene` | `-7` | runaway transitions, scene misuse |
| `NotInitialized` | `-8` | call before the engine exists |
| `Unsupported` | `-9` | feature not available in this build |
| `Internal` | `-99` | unexpected failure |

The numbers are the C ABI's contract; `GiochiError::code()` is the single mapping
point (ADR-003).

---

## 11. Patterns worth copying

**Own resources for exactly one scene.** Load in `on_enter`, destroy in `on_exit`.
The engine's `Drop` cleans up anything left, but relying on that hides leaks.

**Keep per-scene state in the scene.** `SceneManager` stores `Box<dyn Scene>`, so a
scene is just a struct; there is no global singleton to fight.

**Ignore audio errors, respect asset errors.** A missing beep must not stop the game;
a missing sprite sheet will make it unplayable and should be reported.

**Use `--frames` for smoke tests.** `Engine::set_frame_limit` is the same mechanism
`giochi-demo --verify` uses to prove a deployment works in one frame.
