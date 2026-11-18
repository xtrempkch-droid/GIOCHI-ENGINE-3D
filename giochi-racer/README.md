# GIOCHI RACER

An arcade racer for the GIOCHI-OS handheld, built with [GIOCHI-ENGINE](../README.md) and
the [visual editor](../docs/EDITOR_USER_GUIDE.md). Low-poly, 60 fps, one hand on the
triggers.

This is **phase 41** of the master plan: the foundation. A drivable car on a real circuit,
with a chase camera, a clock and a HUD. Opponents (43), menus and persistence (44), dynamic
resolution (45) and the ARM64 package (46) come next.

```text
cargo run -p giochi-racer --release
```

---

## What works today

- One circuit, `assets/tracks/track_easy.scene`: a 200 m oval, 10 m wide, four checkpoints,
  built in the editor's scene format.
- An arcade car with a Rapier body: 120 km/h top speed, brakes, reverse, a handbrake that
  slides, and a steering authority that shrinks with speed.
- A camera that trails the car along its heading and pulls back with speed.
- Lap counting: the four checkpoints in order, then the finish line. A cut course
  invalidates the lap, and the next crossing of the line starts a clean one.
- A HUD: lap, lap time, race time, best lap, speed and gear.
- Gamepad first, keyboard as the fallback.

## Controls

| Action | Gamepad | Keyboard |
|---|---|---|
| Accelerate | RT | `W` / `↑` |
| Brake, then reverse | LT | `S` / `↓` |
| Steer | Left stick | `A` `D` / `←` `→` |
| Handbrake | `B` | `Space` |
| Respawn on the start line | — | `R` |
| Quit | Start | `Esc` / `Q` |

A connected controller is used when it is producing input; the keyboard works at any time.
There is nothing to configure: a controller that is plugged in but idle (a pad on a desk
next to a dev machine) does not lock the keyboard out.

There is no pause menu yet — `Esc` and Start quit. Phase 44 replaces that with the pause
overlay the plan asks for.

## Command line

```text
--track PATH     race on this .scene (default: assets/tracks/track_easy.scene)
--laps N         laps in the race (default: 3)
--size WxH       window size (default: 1280x720)
--frames N       stop after N frames, for a headless check
--auto           drive with a scripted input instead of the keyboard or a gamepad
--quadro PATH    write the last frame to PATH as a BMP
--version, -h    version, help
```

The headless pair is what the gates use:

```bash
SDL_VIDEODRIVER=offscreen cargo run -p giochi-racer -- \
    --frames 600 --auto --quadro /tmp/race.bmp
```

## How it is put together

| Path | What it owns |
|---|---|
| `src/main.rs` | the window, the loop, the command line |
| `src/game.rs` | one race: the frame, in order |
| `src/track/` | an editor `.scene` as a circuit: meshes, colliders, gates, lap counting |
| `src/vehicle/` | the car, its input, its camera, its HUD |
| `src/utils/` | the procedural car and the transform composition the engine lacks |
| `src/assets.rs` | where the files are, in a checkout and in an installed package |

Two decisions are worth knowing before reading the code:

- **The car's handling is not Rapier's.** A rigid-body car needs suspension rays and tyre
  models; this is a low-poly arcade racer, so the driving is a small deterministic model
  (`ArcadeModel`) and Rapier is used for what it is good at — keeping the car out of walls
  and on the floor. The body's rotations are locked: the car cannot flip, at any speed, on
  any kerb. The model is unit-tested; the collider is tested by driving the real track.
- **The car's collider is frictionless.** The model owns every metre per second the car
  gains or loses. A collider with friction adds a second braking force proportional to the
  car's weight, which is invisible under full throttle and fatal under reverse.

The engine and the editor are **not modified** by this crate (master plan, R2). Where a
piece of the engine did not fit, the game has its own: the chase camera (the engine's
`FollowCamera` trails along world `+Z`, not along a heading) and the HUD layout.

## Tested on the console

Measured on 2026-11-18 on the **R35S** (Rockchip RK3326, Cortex-A35, Mali-G31 with
Panfrost, Buildroot 2025.02, glibc 2.41, 640×480 DSI panel, 964 MB) over SSH, with the
game on the real display through KMSDRM:

| | |
|---|---|
| GL context | `GLES 2 on Mali-G31 (Panfrost) (Mesa)`, 128 extensions, instancing `ext` |
| 600 frames | 10.12 s — **59.3 fps**, vblank-locked |
| 1800 frames (30 s) | 30.13 s — **59.7 fps**, clean exit |
| CPU per frame | **5.9 ms** of a 16.7 ms budget — about 65 % headroom for phase 45 |
| Gamepad | `r36s_Gamepad` detected; left untouched, the car stays on the line at 0 km/h |
| Capture | 640×480, verified: sky 35 %, road, red car, HUD text in both corners |

KMSDRM ignores `--size` and uses the panel's own mode, which is why every run is 640×480.

Run it yourself:

```bash
giochi-racer/scripts/device-run.sh --binary path/to/giochi-racer --frames 600
```

The script releases the console (the device's launcher holds DRM master), runs the game,
pulls the frame back, and **restores the screen** from a trap *on the device*, so a dropped
SSH connection cannot leave it black.

> **Building for this device is not `scripts/build-aarch64.sh` alone.** This host's glibc is
> newer than the console's, and the linker records the host's symbol versions: the binary
> then dies with `version 'GLIBC_2.43' not found`. Build inside a container at the device's
> glibc (Debian 13, glibc 2.41); the one-command recipe is in
> [docs/CROSS_COMPILE.md §The host's glibc is the ceiling](../docs/CROSS_COMPILE.md).

## Tracks

Tracks are `.scene` files, and the naming convention that turns one into a circuit is in
[`docs/TRACK_SPECS.md`](docs/TRACK_SPECS.md): a node named `Start`, nodes named
`Checkpoint 1`…`N`, primitives are solid, imported models are scenery.

## Tests

```bash
cargo test -p giochi-racer                 # unit + integration, no window needed
SDL_VIDEODRIVER=offscreen cargo test -p giochi-racer --test hud_pixels
```

| Test binary | What it proves |
|---|---|
| `src/**` unit tests | the handling model, the lap rules, the loader, the camera, the HUD formatting |
| `tests/track_specs.rs` | the shipped track matches the written specification |
| `tests/physics_track.rs` | the real car on the real track: drives, cannot tunnel, stays upright, brakes, respawns, completes a lap |
| `tests/hud_pixels.rs` | text actually reaches the framebuffer, and the four corners lay out where they should |

The last one exists because a HUD that draws nothing looks exactly like a HUD that works
from the outside.

## What is deliberately missing

Physics and gameplay are the game's; the editor's `PlayPlan` shows the scene as it is.
Waypoints and opponents (phase 43), menus and high scores (44), dynamic resolution (45),
and the ARM64 package with its install script (46) are the phases that follow. Anything
that is *not* on that list goes to [`../IDEAS.md`](../IDEAS.md), not into the code.
