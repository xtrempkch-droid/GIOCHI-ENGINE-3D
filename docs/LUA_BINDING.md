# The Lua binding

`giochi-engine-lua` is the project's **reference binding** (ADR-004). It exists to
prove that the engine can be driven from a scripting language without touching a line
of the core, and to be the smallest honest example of what "C-shaped from the start"
buys you.

```lua
local giochi = require("giochi")

local app = giochi.app { title = "my game", width = 960, height = 540 }
local player = app:texture("assets/player.png")

local x, y = 100, 100

app:run {
    update = function(time, input, audio)
        if input:key_down(giochi.keys.d) then x = x + 200 * time.delta end
        if input:key_down(giochi.keys.a) then x = x - 200 * time.delta end
        if input:key_pressed(giochi.keys.space) then audio:play(beep) end
    end,
    render = function(gfx)
        gfx:clear { 12, 14, 22 }
        gfx:draw(player, { x, y, 64, 64 })
    end,
}
```

Run it with the demo binary:

```bash
cargo run -p giochi-demo -- lua giochi-engine-lua/examples/mini_game.lua
cargo run -p giochi-demo -- lua giochi-engine-lua/examples/mini_game.lua --auto --frames 600
```

Any arguments after the script path reach it as `arg[1..]`, with `arg[0]` set to the
script path — the usual Lua convention.

---

## 1. The shape of the binding

| Rust | Lua |
|---|---|
| `Engine` | `giochi.app { ... }` → an `App` userdata |
| `Scene` | the `update` / `render` functions passed to `app:run` |
| `TextureHandle` | a `Texture` userdata (`:width()`, `:height()`, `:destroy()`) |
| `SoundHandle` / `MusicHandle` | `Sound` / `Music` userdata |
| `&Input`, `&mut AudioSystem`, `&mut RenderContext` | the `input`, `audio` and `gfx` capabilities |
| `GiochiError` | a raised Lua error carrying the message and the numeric code |

Nothing in the engine was changed for this binding except two additive helpers
(`RenderContext::shared_textures`, `TextureRegistry::create_texture_from_bytes`), both
delivered in phase 7 for the C ABI.

---

## 2. A callback only sees what it needs

This is the central rule, and the one that keeps the binding free of `unsafe`
(`#![forbid(unsafe_code)]`). A callback is handed capabilities as arguments, and they
stop working the moment it returns:

```lua
app:run {
    update = function(time, input, audio) ... end,   -- clock, input, mixer
    render = function(gfx) ... end,                  -- renderer
}
```

| Capability | Available in | Why |
|---|---|---|
| `input` | `update` | Input drives simulation (ADR-019). |
| `audio` | `update` | A sound played while drawing would be one frame late. |
| `gfx` | `render` | The engine clears before and presents after; drawing anywhere else is a bug. |
| `time` | `update` | `time.delta` is the fixed timestep, not the frame time. |

That is ADR-028 — the C ABI's rule — expressed with `mlua`'s scoped userdata instead
of raw pointers. Storing a capability in a global does not corrupt memory; the next
call on it raises a Lua error because `mlua` invalidated it when the callback returned.

Everything else — loading textures and sounds, reading statistics — belongs to the
app, outside the loop:

```lua
local app = giochi.app { ... }
local player = app:texture("assets/player.png")   -- setup, before app:run
```

Calling `app:texture(...)` from inside a callback raises "the app is busy": the
engine is running and the app is borrowed.

---

## 3. `giochi` module

| Member | Type | Meaning |
|---|---|---|
| `giochi.version` | string | Binding version (`0.2.2`). |
| `giochi.module_version` | string | Engine version. |
| `giochi.abi_version` | integer | C ABI version (`1`). |
| `giochi.app(config?)` | function | Creates the engine and returns an `App`. |
| `giochi.quit()` | function | Asks the running loop to stop after the frame. |
| `giochi.keys` | table | `a`…`z`, `0`…`9`, `space`, `escape`, `enter`, `return`, `tab`, `backspace`, `left`, `right`, `up`, `down`. |
| `giochi.mouse` | table | `left`, `middle`, `right`, `x1`, `x2`. |
| `giochi.axes` | table | `left_x`, `left_y`, `right_x`, `right_y`, `trigger_left`, `trigger_right`. |
| `giochi.buttons` | table | `south`, `east`, `west`, `north`, `back`, `guide`, `start`, `left_stick`, `right_stick`, `left_shoulder`, `right_shoulder`, `dpad_up`, `dpad_down`, `dpad_left`, `dpad_right`. |

The numeric values come from the engine's own constants, so they cannot drift. An
unknown name is `nil`, never an error — the same rule the queries follow.

### `giochi.app(config)`

```lua
local app = giochi.app {
    title = "my game",   -- default "giochi"; truncated on a character boundary at 64 bytes
    width = 960,         -- default 960
    height = 540,        -- default 540
    fps = 60,            -- fixed simulation rate; 0 keeps the engine default
    vsync = true,        -- default true
}
```

Raises on failure (a zero size, no video device). **There is one app per process** —
the engine is a singleton (ADR-012). A second `giochi.app` while the first is still
reachable raises a message saying so; set the old one to `nil` and
`collectgarbage()` first.

---

## 4. `App`

Setup and inspection only. Every method except `run`/`step` needs the engine idle.

| Method | Returns | Notes |
|---|---|---|
| `app:texture(path)` | `Texture` | Goes through the resource cache: the same path twice returns a texture for one upload. |
| `app:sound(path)` | `Sound` | Fully decoded effect (WAV, OGG, FLAC). |
| `app:music(path)` | `Music` | Streamed track. |
| `app:destroy_sound(sound)` | boolean | Reports whether the handle was live. |
| `app:destroy_music(music)` | boolean | Stops playback first. |
| `app:set_volume(0..128)` | — | Clamped, never an error. |
| `app:set_clear_color(color)` | — | Overrides the frame's background. |
| `app:size()` | `width, height` | Physical pixels. |
| `app:video_driver()` | string | `"KMSDRM"`, `"x11"`, `"dummy"`, … — what SDL actually opened. SDL2 spells the console backend in capitals. |
| `app:audio_available()` | boolean | `false` means silent mode (ADR-021). |
| `app:volume()` | integer | Current master volume. |
| `app:frames()` / `app:steps()` | integer | Statistics of the last run. |
| `app:elapsed()` / `app:fps()` | number | Seconds, frames per second. |
| `app:timestep()` | number | The fixed timestep, in seconds. |
| `app:texture_count()` / `app:sound_count()` | integer | Live handles — the leak check. |
| `app:run(options)` | — | Runs until quit, game over or the frame limit. |
| `app:step(dt, options?)` | — | Advances exactly one frame; the caller owns pacing. |

### `app:run{ update =, render =, frames = }`

* At least one of `update`/`render` is required. A render-only scene is legal.
* `frames = N` stops after N display frames. That is what makes a headless test
  possible without a keyboard.
* Returns when the script calls `giochi.quit()`, when `frames` is reached, or when a
  callback raises.

`update` receives `(time, input, audio)`, `render` receives `(gfx)`. `time` is a table:

```
time.delta    seconds of the fixed step (constant)
time.elapsed  simulated seconds since the run started
time.frame    frames completed
```

`update` may run zero, one or several times per frame; never assume a 1:1 ratio.

### Errors end the run

A Lua error inside a callback stops the loop and is re-raised by `app:run`, so a
broken script never leaves a window on screen with the host unaware:

```lua
local ok, err = pcall(function() app:run { ... } end)
if not ok then print("game crashed: " .. err) end
```

Errors raised while the engine is idle (`app:texture("missing.png")`) propagate
immediately, as does a failed `giochi.app`.

---

## 5. Capabilities

### `input`

| Method | Returns |
|---|---|
| `input:key_down(key)` / `key_pressed` / `key_released` | boolean |
| `input:mouse()` | `x, y` |
| `input:mouse_down(button)` | boolean |
| `input:wheel()` | number, positive = up |
| `input:gamepad_connected()` | boolean |
| `input:axis(axis)` | number, `[-1, 1]`; triggers rest at `-1` |
| `input:button_down(button)` | boolean |

`pressed`/`released` are true for one frame. A key pressed *and* released between two
frames reports both edges, so a tap is never lost. An unmapped key is simply `false`.

### `audio`

| Method | Notes |
|---|---|
| `audio:play(sound [, loops])` | `loops` = extra repetitions: `0` once, `-1` forever. |
| `audio:set_volume(0..128)` / `audio:volume()` | Clamped. |
| `audio:available()` | `false` in silent mode. |
| `audio:music_play(music [, loops])` | Default `-1` (loop). |
| `audio:music_pause()` / `music_resume()` / `music_stop()` / `music_playing()` | — |

In silent mode `play` raises `GIOCHI_ERROR_AUDIO`. Wrap it in `pcall` if the game must
survive a machine without a sound card:

```lua
pcall(function() audio:play(beep) end)
```

### `gfx`

| Method | Notes |
|---|---|
| `gfx:size()` | `width, height` |
| `gfx:clear(color)` | The engine already clears each frame; use it to change the colour. |
| `gfx:rect(rect, color)` or `gfx:rect{ x, y, w, h, color = ... }` | Screen space. |
| `gfx:draw(texture, dst [, src])` | `src` defaults to the whole texture. |
| `gfx:sprite(texture, rect)` | **World** space; the camera is applied. |
| `gfx:camera{ x =, y =, zoom = }` / `gfx:camera()` | Sets or returns the camera. |
| `gfx:screen{ x =, y = }` | World → screen point. |
| `gfx:texture(path)` | Creates a texture from inside `render` (see below). |

A colour is a table `{ r, g, b [, a] }`, a positional `{ 230, 70, 70 }`, or a hex
string `"#e64646"` / `"#fff"` / `"#11223380"`. A rectangle is `{ x, y, w, h }` or
`{ x =, y =, w =, h = }`.

The digits of a hex colour must be ASCII hexadecimal (`0-9`, `a-f`, `A-F`); anything else
is a Lua error naming the three accepted lengths. That check is not cosmetic: the digits
are sliced in pairs, and a 6-byte string of non-ASCII characters would be split through the
middle of one, which is a panic rather than an error. A script must never be able to take
the host down by passing a strange string.

Inside `render` the app is busy, so `gfx:texture(path)` is the way to load lazily. It
does **not** use the cache — every `Texture` owns exactly one handle — so keep the
result in a variable rather than loading the same file every frame.

---

## 6. Handles and lifetime

| Type | Lifetime | Method |
|---|---|---|
| `Texture` | Freed by `:destroy()` **or** when collected | `:width()`, `:height()`, `:alive()`, `:destroy()`, `tostring` |
| `Sound` | Freed by `app:destroy_sound`, or at process exit | `tostring` |
| `Music` | Freed by `app:destroy_music`, or at process exit | `tostring` |

`Texture` can free itself because the texture registry is shared
(`RenderContext::shared_textures`), so destruction needs no engine borrow and is safe
from a garbage-collection finaliser. `Sound` and `Music` cannot — the mixer belongs to
the engine — so they live until the app destroys them. `:destroy()` is idempotent:
the second call returns `false`, exactly like the C ABI's
`GIOCHI_ERROR_INVALID_HANDLE`.

`mini_game.lua` asserts `app:texture_count() == 0` at the end, which is what keeps the
leak rule honest.

---

## 7. Design notes

**Why userdata instead of plain integers?** A `u64` in a script is indistinguishable
from a typo, and `sprite:draw(3, ...)` would be a silent bug. Userdata gives method
syntax, `tostring` for debugging, and a place for the GC hook.

**Why is there no `Scene` class?** A scene in Rust owns state and receives borrowed
subsystems. In Lua the closure already owns the state and the parameters are already
borrowed, so a class would add a layer without adding a capability.

**Why `#![forbid(unsafe_code)]`?** Because it is achievable. The C ABI needs `unsafe`
for raw pointers; Lua does not, because `mlua` provides scoped userdata. If the
binding ever needs `unsafe`, that is a design smell to investigate rather than a
formality to accept.

**Why is `giochi.quit()` a module function and not `app:quit()`?** Inside a callback
the app is borrowed by the running loop; a method would need the engine that the loop
already holds.

---

## 8. Testing

```bash
cargo test -p giochi-engine-lua                       # unit + integration
cargo run -p giochi-demo -- lua .../mini_game.lua --auto --frames 600
```

| Test | Covers |
|---|---|
| `src/**` unit tests | Colour/rect conversions, key tables, module registration, error mapping. |
| `tests/lua_smoke.rs` | The whole surface: capabilities, textures, `destroy`, collection, quit, error propagation, `app:step`, `arg`. |
| `tests/mini_game_script.rs` | The reference game: it scores, it ends on game over, and it leaks no texture. |

Every integration test binary holds exactly one test, because SDL allows one
`EventPump` per process.

---

## 9. Reference game

[`giochi-engine-lua/examples/mini_game.lua`](../giochi-engine-lua/examples/mini_game.lua)
is a complete space shooter in ~250 lines: movement, shooting, drones that spawn
faster as the score rises, collisions, lives, game over, and a beep synthesised into a
temporary WAV at start-up (no binary asset in the repository).

```bash
# play it
cargo run -p giochi-demo -- lua giochi-engine-lua/examples/mini_game.lua

# test it (no keyboard needed)
cargo run -p giochi-demo -- lua giochi-engine-lua/examples/mini_game.lua --auto --frames 600
```

Test-only flags, documented in the file header: `--lives N`, `--ram` (steer into the
drones) and `--spawn SECS` / `--speed PX` (fixed spawn interval and drone speed). They
exist so the collision and game-over paths run in a second instead of relying on a
lucky random spawn. The script returns a summary table
(`score`, `lives`, `over`, `frames`, `steps`, `textures`) that `tests/mini_game_script.rs`
asserts on.
