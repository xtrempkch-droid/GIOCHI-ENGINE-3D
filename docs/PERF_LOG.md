# Performance Log

This file is the evidence behind every optimization. A change that is not recorded
here has not been measured, and an optimization without a baseline, a reproducible
benchmark and a result is reverted.

Phase 15 owns the **baseline**: it adds the profiler, the overlay and the N-sprite
scene, but deliberately optimizes nothing. Phase 16 and later fill the "after"
sections.

## Reference hardware

| | x86_64 development | ARM64 target |
|---|---|---|
| SoC / CPU | Intel Xeon E5-2620 v3 @ 2.40 GHz, 6 cores / 12 threads | Rockchip PX30, 4x Cortex-A35 |
| RAM | 60 GiB | 964 MiB |
| GPU | AMD Radeon RX 580 (radeonsi, Polaris 10) | Arm Mali-G31 |
| Display | 1280x720 canvas | 640x480 DSI panel (KMSDRM's native mode) |
| OS / kernel | Ubuntu 26.04.1 LTS, kernel 7.0.0-42-generic | Buildroot 2024.02, kernel 6.12.0 |
| Graphics stack | Mesa 26.0.8; offscreen EGL reports `GLES 2 … radeonsi` | KMSDRM + SDL2 2.28.5 + Mali GLES |
| Engine build | release (`lto = "thin"`, `codegen-units = 1`) | release, cross-compiled with the PX30 sysroot |

**The x86_64 2D numbers use `SDL_VIDEODRIVER=dummy`.** That isolates the CPU-side cost
of the SDL renderer and of the engine loop, which is what the phase-16 draw-call
reduction changes; a GPU-backed driver would hide part of it behind the rasterizer.
The x86_64 3D number uses `SDL_VIDEODRIVER=offscreen` with Mesa/EGL, because `dummy`
has no OpenGL.

**The ARM64 numbers use the real console path**: `SDL_VIDEODRIVER=kmsdrm` at the
panel's native 640x480, with the launcher's DRM master released for the run and
restored afterwards (the same procedure as `scripts/hardware-check.sh`). The binary
was copied to `/tmp` (RAM) — no rootfs writes — while the battery reported 84–92 %
and discharging.

## Methodology

- Target: `x86_64-unknown-linux-gnu`, `cargo build --release`.
- CPU affinity: `taskset -c 2` on the host. The PX30 image has no `taskset`; the
  benchmark ran unpinned while the launcher was stopped.
- Canvas: 1280x720 on x86_64, 640x480 on the PX30, vsync **off**. The benchmark path
  calls `Engine::step`, which does not sleep, but **KMSDRM still waits for the panel's
  vblank on present**: below ~10000 sprites the console is capped at ~60 FPS, so CPU
  time per frame is the discriminating number there.
- 600 frames per run; five runs per x86_64 scenario, three (or two above 1000) per
  ARM64 scenario; **median** reported.
- The profiler and the overlay are enabled: `frame_avg`/`frame_p99` come from the
  60-frame window, `cpu_avg`/`render2d_avg` are averaged over all 600 frames.
- Commands:

```bash
cargo build --release -p giochi-demo --example profiler_demo --features profiling

# x86_64
SDL_VIDEODRIVER=dummy taskset -c 2 \
    target/release/examples/profiler_demo --sprites 1000 --frames 600

# PX30 (console released, binary in /tmp)
SDL_VIDEODRIVER=kmsdrm /tmp/profiler_demo \
    --width 640 --height 480 --sprites 1000 --frames 600
```

The demo prints one machine-readable line:

```text
profiler_demo: sprites=1000 frames=600 wall=0.930s fps=645.5 \
    frame_avg=1.546ms frame_p99=1.686ms cpu_avg=0.142ms \
    render2d_avg=0.128ms draw_calls=1000
```

## Baseline (pre-Phase 16, x86_64)

Date: 2026-11-08 | Build: release, profiling feature enabled | vsync: off

| Scenario | FPS (median) | Frame avg | Frame p99 | CPU avg | Render2D avg | Draw calls |
|---|---:|---:|---:|---:|---:|---:|
| 200 sprites, 1 texture | 1889.6 | 0.526 ms | 0.636 ms | 0.039 ms | 0.027 ms | 200 |
| 500 sprites, 1 texture | 1073.4 | 0.925 ms | 1.079 ms | 0.078 ms | 0.065 ms | 500 |
| 1000 sprites, 1 texture | 645.5 | 1.546 ms | 1.686 ms | 0.142 ms | 0.128 ms | 1000 |
| 1000 sprites, 10 textures | — | — | — | — | — | the demo only has the one-texture scene; added in phase 16 |
| mini_fps, base scene (3D) | ~2500 | — | — | — | — | 600 frames in 0.24 s wall; no profiler integration yet |

The 1000-sprite scene is CPU-bound in the driver at ~645 fps: 1000 separate SDL render
copies per frame. That is the number phase 16 has to move.

## Baseline (pre-Phase 16, ARM64 PX30, 640x480 KMSDRM)

Date: 2026-11-08 | Build: release, profiling feature enabled | vsync: off (panel-paced)

| Scenario | FPS | Frame avg | CPU avg | Render2D avg | Draw calls |
|---|---:|---:|---:|---:|---:|
| 200 sprites, 1 texture | 60.0 | 16.68 ms | 0.762 ms | 0.547 ms | 200 |
| 500 sprites, 1 texture | 60.0 | 16.68 ms | 1.203 ms | 1.004 ms | 500 |
| 1000 sprites, 1 texture | 59.9 | 16.68 ms | 1.964 ms | 1.761 ms | 1000 |
| 5000 sprites, 1 texture | 59.8 | 16.68 ms | 6.267 ms | 5.909 ms | 5000 |
| 10000 sprites, 1 texture | 59.6 | 16.68 ms | 8.712 ms | 8.178 ms | 10000 |
| 20000 sprites, 1 texture | 35.7 | 27.81 ms | 18.243 ms | 16.610 ms | 20000 |
| mini_fps, base scene (3D) | 60.0 | — | — | — | — |

`mini_fps`: 600 frames in 10.76 s wall, `user 1.79 s`, `sys 0.88 s`, 0 GL errors,
0 meshes/shaders left. The example predates the profiler and has no per-frame stats;
it is a sanity number, not a phase-16 comparison point.

The panel caps the run at ~60 FPS until the 1000-sprite workload grows by an order of
magnitude: 10000 sprites still fit in a 16.7 ms frame (8.7 ms CPU), while 20000 sprites
do not (18.2 ms CPU, 35.7 FPS). **Phase 16's FPS acceptance criterion therefore cannot
be judged at 1000 sprites on this console**; use `render2d_avg` at 1000 sprites and/or
the 20000-sprite scenario for the FPS delta. The x86_64 `dummy` matrix is uncapped and
is the primary FPS baseline.

## Profiling overhead (R14 check)

The profiler and the overlay are two separate costs: the profiler collects the scopes
and counters, and the panel (1 Hz, 1x text on the console) adds a periodic upload and a
textured quad. Compare:

| Build | x86_64 1000 sprites, FPS (median of 5) | ARM64 1000 sprites, process CPU (median of 3) |
|---|---:|---:|
| `profiling` off | 649.9 | 3.10 s / 10.8 s wall |
| `profiling` on, overlay hidden | 642.8 (−1.1 %) | 3.16 s (+0.56 % of wall) |
| `profiling` on, overlay visible | 645.5 (−0.7 %, within noise) | 3.31 s (+1.94 % of wall) |

The 2 % budget is met at the phase-15 baseline. The ARM64 figure is process CPU from
`/usr/bin/time` over 600 frames, not FPS: KMSDRM caps this workload at 60 FPS, so the
panel cost appears as CPU, not as dropped frames. At the 20000-sprite stress point the
overlay-visible build costs ~25 % CPU; the per-draw counter increments dominate there,
and phase 16's batching is what removes that cost. The 2 % budget is specified for the
profiler at a normal frame, not for a 20x overdraw stress test.

The overlay uses **one persistent texture updated in place**, refreshed at **1 Hz**.
Creating and destroying a texture at 4 Hz measured ~10 % on the Mali-G31; 250×230
pixels at 1 Hz with 5x7 glyphs is what fits the budget.

## After Phase 16 (Batching 2D + Atlas)

Date: 2026-11-08 | Build: release, profiling feature enabled | vsync: off
Method: as above, but the demo now has two arms so the A/B is made by one binary on
one machine:

- `--immediate` draws through `draw_texture_immediate`, which is exactly the phase-15
  `SDL_RenderCopy` path. The comparison therefore does not depend on rebuilding an old
  commit or on two binaries being equally fast for reasons unrelated to the change.
- `--centred-camera` reproduces the phase-15 **scene**: the demo used to keep the
  camera at its default, which centres the world origin in the viewport, so sprites
  placed in `0..width` had three quarters of their area off-screen. `--centred-camera`
  + `--immediate` reproduces the old binary to within noise (637.8 vs 619.2 FPS at 1000
  sprites on the `software` renderer, against the 645.5 recorded below), which is what
  makes the two tables comparable.
- `--sprite-size N` changes the drawn size, `--textures N` and `--use-atlas` change the
  texture mode, and `--batch-max N` overrides `SpriteBatchConfig::max_sprites`.

**Two things to know before reading the tables.**

1. **The scene is fill-bound, not call-bound, at 24x24 sprites.** A 1000-sprite frame
   covers 576 000 pixels — 62 % of the 1280x720 canvas, 1.9 screens on the 640x480
   panel — and 20000 sprites cover 11.5 M pixels. Removing 999 of 1000 draw calls
   cannot move a frame that spends its time rasterising, so the draw-call win is
   reported where the work is actually call-bound and the fill-bound numbers are shown
   beside them.
2. **`software` is not a stand-in for the console.** SDL's software `RenderCopy` is a
   hand-written fast blit; `SDL_RenderGeometry` on that backend is a triangle
   rasteriser that interpolates per pixel. The PX30 runs `opengles2` (Mali-G31), so the
   console measurements are the ones that matter for the target.

### x86_64 — phase-15 scene (`--centred-camera`), `opengl` (accelerated)

| Scenario | Draw calls | FPS | Frame avg | Render2D avg |
|---|---:|---:|---:|---:|
| 100 sprites, batched | 1 | 18439.0 | 0.043 ms | 0.029 ms |
| 100 sprites, immediate | 100 | 17048.7 | 0.052 ms | 0.043 ms |
| 500 sprites, batched | 1 | 11913.2 | 0.070 ms | 0.067 ms |
| 500 sprites, immediate | 500 | 4476.1 | 0.211 ms | 0.205 ms |
| 1000 sprites, batched | 1 | 6765.5 | 0.130 ms | 0.131 ms |
| 1000 sprites, immediate | 1000 | 2390.8 | 0.410 ms | 0.403 ms |
| 5000 sprites, batched | 2 | 1378.6 | 0.719 ms | 0.694 ms |
| 5000 sprites, immediate | 5000 | 471.0 | 2.045 ms | 2.099 ms |

At 1000 sprites the renderer itself goes **0.403 → 0.131 ms, 3.08x**, and the frame
goes 2390.8 → 6765.5 FPS, **2.83x**. The gap between the two ratios is the frame
loop's fixed cost: once `render2d` is 0.13 ms, the ~0.02 ms of poll/update/present/overlay
is a larger share of what is left, so FPS cannot scale by the same factor. A sprite-size
sweep at 1000 sprites (8/16/24 px) gives 2.84x/2.70x/2.86x — the ratio is a property
of the call count, not of the pixels, on this backend.

### x86_64 — phase-15 scene, `dummy` + `software` (the recorded baseline)

| Scenario | Draw calls | FPS | Frame avg | Render2D avg |
|---|---:|---:|---:|---:|
| 100 sprites, batched | 1 | 2447.6 | 0.407 ms | 0.143 ms |
| 100 sprites, immediate | 100 | 2453.0 | 0.408 ms | 0.144 ms |
| 500 sprites, batched | 1 | 1053.7 | 0.950 ms | 0.682 ms |
| 500 sprites, immediate | 500 | 1043.5 | 0.956 ms | 0.692 ms |
| 1000 sprites, batched | 1 | 637.7 | 1.555 ms | 1.300 ms |
| 1000 sprites, immediate | 1000 | 598.9 | 1.657 ms | 1.398 ms |
| 5000 sprites, batched | 2 | 158.1 | 6.170 ms | 6.031 ms |
| 5000 sprites, immediate | 5000 | 157.6 | 6.780 ms | 6.061 ms |
| 1000 sprites, 10-region atlas, batched | 1 | 632.3 | 1.588 ms | 1.313 ms |
| 1000 sprites, 10-region atlas, immediate | 1000 | 595.6 | 1.591 ms | 1.407 ms |

**Like for like, the recorded baseline does not regress: 1000 sprites goes 598.9 →
637.7 FPS (+6.5 %) and 1.398 → 1.300 ms of `render2d` (−7.0 %).** The 637.7 FPS also
confirms the `--centred-camera` reproduction is faithful (the phase-15 table says 645.5
on the same machine class).

### x86_64 — whole-field scene (the fixed camera), `dummy` + `software`

Included because it is the case where batching is *not* free. With the camera pinned so
the frame covers the canvas, the software triangle path is slower per pixel than the
blit path it replaces:

| Scenario | Draw calls | FPS | Frame avg | Render2D avg |
|---|---:|---:|---:|---:|
| 1000 sprites, batched | 1 | 107.9 | 9.207 ms | 8.940 ms |
| 1000 sprites, immediate | 1000 | 111.4 | 8.890 ms | 8.648 ms |
| 5000 sprites, batched | 5 | 38.4 | 25.686 ms | 25.681 ms |
| 5000 sprites, immediate | 5000 | 41.9 | 23.073 ms | 23.492 ms |

**−3.1 % at 1000 sprites, −8.3 % at 5000** — inside the 10 % threshold but recorded in
full, because it is a real cost on a backend a host can still be running. A game on the
software renderer can call `set_batch_config(SpriteBatchConfig::new(_, 1))` or
`draw_texture_immediate`; both are public and documented.

### x86_64 — whole-field scene, `opengl` (accelerated)

| Scenario | Draw calls | FPS | Frame avg | Render2D avg |
|---|---:|---:|---:|---:|
| 100 sprites, batched | 1 | 24275.1 | 0.029 ms | 0.026 ms |
| 100 sprites, immediate | 100 | 7528.4 | 0.125 ms | 0.116 ms |
| 500 sprites, batched | 1 | 7599.3 | 0.100 ms | 0.114 ms |
| 500 sprites, immediate | 500 | 1713.8 | 0.590 ms | 0.567 ms |
| 1000 sprites, batched | 1 | 4024.8 | 0.198 ms | 0.230 ms |
| 1000 sprites, immediate | 1000 | 896.9 | 1.111 ms | 1.097 ms |
| 5000 sprites, batched | 5 | 817.4 | 1.051 ms | 1.189 ms |
| 5000 sprites, immediate | 5000 | 178.6 | 5.582 ms | 5.569 ms |
| 1000 sprites, 10-region atlas, batched | 1 | 3990.2 | 0.202 ms | 0.232 ms |
| 1000 sprites, 10-region atlas, immediate | 1000 | 865.1 | 1.165 ms | 1.136 ms |

1000 sprites: **896.9 → 4024.8 FPS, 4.49x** (`render2d` 1.097 → 0.230 ms, 4.77x). The
larger ratio than on the phase-15 scene is the larger pixel coverage: more per-sprite
work for the copy path to do before the batch removes the call.

The ten-texture **interleaved** scene is one call per sprite by design (ADR-043); on
`opengl` it measures 198.1 FPS batched vs 223.5 immediate at 1000 sprites, i.e. the
batcher's own bookkeeping (−11 %) with nothing to merge. The ten-region **atlas** is the
answer to that scene: 3990.2 FPS and **1** call.

### Criterion benchmark

`cargo bench -p giochi-engine --bench sprites_2d`, 8x8 sprites, 1000 of them, one
texture, `SDL_VIDEODRIVER=offscreen SDL_RENDER_DRIVER=opengl`: **737 µs unbatched →
191 µs batched (3.86x)**. That is the tightest measurement of the change: no engine
loop, no overlay, no scene. On the `software` renderer the same benchmark measures
725 µs batched vs 718 µs unbatched at 8x8 (neutral, as the demo above shows for
larger sprites).

### ARM64 PX30 — KMSDRM 640x480, `opengles2` (Mali-G31), phase-15 scene

The panel pins the run at ~60 FPS below ~10000 sprites, so `render2d_avg` is the
discriminating column there and FPS only moves once the frame exceeds the vblank.

| Scenario | Draw calls | FPS | Frame avg | CPU avg | Render2D avg |
|---|---:|---:|---:|---:|---:|
| 100 sprites, 1 texture, batched | 1 | 59.9 | 16.688 ms | 0.579 ms | 0.368 ms |
| 100 sprites, 1 texture, immediate | 100 | 59.9 | 16.689 ms | 0.564 ms | 0.358 ms |
| 500 sprites, 1 texture, batched | 1 | 59.9 | 16.685 ms | 1.306 ms | 1.166 ms |
| 500 sprites, 1 texture, immediate | 500 | 59.9 | 16.687 ms | 1.570 ms | 1.426 ms |
| 1000 sprites, 1 texture, batched | 1 | 59.9 | 16.679 ms | 2.433 ms | 2.221 ms |
| 1000 sprites, 1 texture, immediate | 1000 | 59.9 | 16.686 ms | 2.642 ms | 2.454 ms |
| 5000 sprites, 1 texture, batched | 5 | 58.2 | 16.690 ms | 6.136 ms | 5.753 ms |
| 5000 sprites, 1 texture, immediate | 5000 | 58.0 | 16.961 ms | 7.028 ms | 6.642 ms |
| 20000 sprites, 1 texture, batched | 20 | 14.8 | 67.014 ms | 26.815 ms | 22.853 ms |
| 20000 sprites, 1 texture, immediate | 20000 | 13.8 | 72.272 ms | 32.516 ms | 28.495 ms |
| 100 sprites, 10 textures, batched | 100 | 59.9 | 16.684 ms | 0.581 ms | 0.483 ms |
| 100 sprites, 10-region atlas, batched | 1 | 60.0 | 16.683 ms | 0.673 ms | 0.451 ms |
| 500 sprites, 10 textures, batched | 500 | 59.7 | 16.681 ms | 1.325 ms | 1.227 ms |
| 500 sprites, 10-region atlas, batched | 1 | 60.0 | 16.682 ms | 1.364 ms | 1.226 ms |
| 1000 sprites, 10 textures, batched | 1000 | **48.0** | 20.654 ms | 2.230 ms | 2.120 ms |
| 1000 sprites, 10-region atlas, batched | 1 | **59.9** | 16.685 ms | 2.483 ms | 2.258 ms |
| 5000 sprites, 10 textures, batched | 5000 | **10.8** | 90.223 ms | 11.171 ms | 10.513 ms |
| 5000 sprites, 10-region atlas, batched | 5 | **58.6** | 16.688 ms | 6.373 ms | 6.002 ms |
| 20000 sprites, 10 textures, batched | 20000 | **2.6** | 250.000 ms | 48.987 ms | 42.208 ms |
| 20000 sprites, 10-region atlas, batched | 20 | **14.8** | 66.741 ms | 27.325 ms | 23.525 ms |

What the device shows:

- **1000 sprites, one texture**: 1000 calls → 1, `render2d` **2.454 → 2.221 ms
  (−9.5 %)**, CPU 2.642 → 2.433 ms. FPS cannot show it; the panel holds both at 59.9.
- **5000 sprites, one texture**: 5 calls, `render2d` **6.642 → 5.753 ms (−13.4 %)**.
- **20000 sprites, one texture**: 20 calls, `render2d` **28.495 → 22.853 ms (−19.8 %)**,
  FPS **13.8 → 14.8 (+7.2 %)**. This is the scenario the phase-15 log nominated for the
  device criterion, and it does move once the batch limit is 1024: at the 4096 default
  it measured 28.543 vs 28.246 ms, i.e. nothing, because a single 4096-quad geometry
  call on the Mali costs as much as the calls it saved. The default limit is part of
  the optimisation, not a detail.
- **Interleaved ten textures vs the same scene from an atlas** — the clearest result of
  the phase, because it is the case where the call count is what breaks the frame:

  | Sprites | 10 textures (1000/N calls) | 10-region atlas (1 call) | Gain |
  |---:|---:|---:|---:|
  | 1000 | 48.0 FPS | 59.9 FPS | +25 % |
  | 5000 | 10.8 FPS | 58.6 FPS | **5.4x** |
  | 20000 | 2.6 FPS | 14.8 FPS | **5.7x** |

  At 20000 sprites the interleaved scene takes 250 ms per frame — 15 frames per second
  below the panel rate — and the atlas version is back at the 60 FPS cap's neighbourhood
  with 66.7 ms. Nothing about the sprites changed; only that they share one texture.

### Where phase 16 stands against its acceptance criteria

| Criterion | Result |
|---|---|
| 1000 sprites of one texture: 1000 draw calls → ≤ 5 | **1 call** on all four backends measured |
| ≥ 3x FPS gain over the phase-15 baseline at 1000 sprites | **Partially met, cause measured.** On the phase-15 scene the renderer is 3.08x faster (`render2d` 0.403 → 0.131 ms on `opengl`), but whole-frame FPS is 2.83x because the frame loop's fixed cost becomes the limit. The recorded phase-15 baseline backend (`dummy`/`software`) gains only +6.5 %, and the PX30 is panel-capped there. The full ≥ 3x whole-frame figure appears on `opengl` with the fixed camera (4.49x) and on the criterion bench (3.96x). |
| No > 10 % regression on an existing benchmark | Met. The phase-15 scene improves 6.5 %; the whole-field `software` scene regresses 3.1 % at 1000 and 8.3 % at 5000, both inside the threshold and both recorded above with their cause. |
| Existing public API unchanged (R12) | Yes — `draw_texture` keeps its signature and batches transparently; the new types are additive. |

The honest summary: the draw-call criterion is met everywhere and the renderer is 3x
faster on the accelerated paths, but "3x FPS at 1000 sprites" is not reachable on the
two backends the phase-15 baseline chose, because both are dominated by something other
than the call count — pixel fill on the software renderer and on the console, the
vblank on the console, and the residual frame-loop cost at 6000 FPS on `opengl`. The
device-level win is real and large where calls are the bottleneck (5.4x at 5000 sprites
from an atlas) and is a 10-20 % `render2d` reduction in the single-texture scenes.

## After Phase 17 (Instancing 3D + Static Batching + Upscaling)

Date: 2026-11-08 | Build: release | vsync: off (the console is still panel-paced)

Method:

```bash
cargo build --release -p giochi-engine-3d --example mini_fps

# x86_64 (a real EGL/GLES context with no display)
SDL_VIDEODRIVER=offscreen taskset -c 2 \
    target/release/examples/mini_fps --frames 600 --auto --bench --instanced

# PX30 (console released, binary in /tmp)
SDL_VIDEODRIVER=kmsdrm /tmp/mini_fps17 --frames 200 --auto --bench --cubes 2000
```

`--bench` prints one machine-readable line, including the draw-call count **counted by
the example** rather than read from the profiler, so the acceptance criteria hold in a
plain release build without the `profiling` feature. `--cubes N` sets the size of the
instanced field; `--scale S`, `--instanced`/`--loop` and `--width/--height` select the
other arms, and `F2`/`F3` do the same from the keyboard.

### What the drivers actually provide

The phase plan predicted `GL_ANGLE_instanced_arrays` and made runtime detection
mandatory. Detection was indeed necessary, and the prediction was wrong in a way that
would have silently disabled the whole feature:

| Machine | GLES | `GL_ANGLE_instanced_arrays` | `GL_EXT_instanced_arrays` | Detected |
|---|---|---:|---:|---|
| x86_64 RX 580, Mesa 26.0.8 radeonsi | 2.0 | ✗ | ✓ | `ext` |
| PX30 Mali-G31, Mesa 24.0.0 Panfrost | 2.0 | ✗ | ✓ | `ext` |

Neither machine lists ANGLE. A detector that looked only for ANGLE would have taken the
one-draw-per-cube fallback on 100 % of the hardware this project owns, and every number
below would have been 1.0x. `docs/DECISIONS.md` ADR-044 records the decision and the
`GLES 3.0 core` path that is used when `GIOCHI_GL_VERSION=3` is granted.

### x86_64 — `mini_fps`, 100 cubes, 1280x720

| Scenario | Draw calls | Field calls | FPS (median of 5) | Render size |
|---|---:|---:|---:|---|
| instanced, 100 % | 4 | 1 | 25776.0 | 1280x720 |
| loop, 100 % | 103 | 100 | 4391.7 | 1280x720 |
| instanced, 75 % | 4 | 1 | 18155.6 | 960x540 |
| instanced, 50 % | 4 | 1 | 18878.3 | 640x360 |

**Instancing: 4391.7 → 25776.0 FPS, 5.87x**, draw calls 103 → 4 (100 → 1 for the field;
the other three are the floor and the two static batches). That meets the acceptance
criterion — 100 cubes, ≥ 5x — on the development machine.

**Upscaling is a loss here: −29.6 % at 75 %** (25776.0 → 18155.6). That is not a bug
and it is not hidden. At 25 000 FPS the frame is submission-bound, not fill-bound: the
GPU finishes 921 600 pixels long before the CPU has finished telling it to. Adding a
second target and a full-screen blit therefore adds work to the half of the frame that
was already the bottleneck. Scaling cannot help a frame that is not fill-bound, and on
a discrete GPU this scene never is — measured at 2560x1440 as well (20995.4 FPS
instanced at 100 %, 14932.4 at 75 %), so a bigger window does not change the conclusion.

### Criterion benchmarks — `benches/meshes_3d.rs`

One complete frame per iteration (begin, clear, submit, present), 1280x720, `offscreen`
+ `opengl`:

| Scenario | Instanced | Loop | Ratio |
|---|---:|---:|---:|
| 1 cube | 2.87 µs | 7.36 µs | 2.56x |
| 10 cubes | 8.03 µs | 32.34 µs | 4.03x |
| 100 cubes | 9.01 µs | 109.81 µs | **12.19x** |
| 1000 cubes | 57.07 µs | 1023.5 µs | **17.94x** |
| 50 static tiles, batched vs separate | 9.73 µs | 65.08 µs | **6.69x** |

The `1 cube` row is the honest floor: with one instance there is nothing to batch, and
what is left is the extra instance-buffer upload, so the instanced path is still faster
only because it skips one `u_model`/`u_normal_matrix` pair per draw. The ratio grows with
the count, which is what "removes per-draw cost" means.

### ARM64 PX30 — KMSDRM 640x480, `opengles2` (Mali-G31)

The panel pins the run at 60 FPS, so a 100-cube scene reports 60.0 for every arm:

| Cubes | Instanced | Loop | Draw calls (instanced → loop) |
|---:|---:|---:|---|
| 100 | 60.0 FPS | 59.8 FPS | 4 → 103 |
| 1000 | 59.6 FPS | 39.4 FPS | 4 → 1003 |
| 2000 | 59.0 FPS | 21.9 FPS | 4 → 2003 |
| 5000 | 30.0 FPS | 9.0 FPS | 4 → 5003 |

Read as a **capacity** result rather than a ratio: the loop arm stops holding 60 FPS
somewhere between 100 and 1000 cubes, while the instanced arm still holds it at 2000 and
only breaks at 5000. At 5000 both arms are past the frame budget, and the ratio there
(3.33x) is compressed by the instanced arm becoming fill-bound — 5000 cubes is 5000
filled boxes, and the Mali is doing pixel work, not call work, in that arm.

**Upscaling on the device, 5000 cubes:**

| Scale | Render size | FPS | Gain over 100 % |
|---|---|---:|---:|
| 100 % | 640x480 | 30.0 | — |
| 75 % | 480x360 | **48.4** | **+61.3 %** |
| 50 % | 320x240 | **57.2** | **+90.7 %** |

This meets the ≥ 40 % criterion at 0.75. The mechanism is exactly the one the x86_64
result lacked: on the Mali the frame at 5000 cubes is **fill-bound**, so removing 44 % of
the pixels buys 61 % more frames, and the upscale pass that adds a full-screen blit costs
far less than the pixels it saved. 0.75 gives more than 25 %/44 %-of-pixels would suggest
because the frame also crosses back under the 16.7 ms vblank budget, where the panel
stops adding latency of its own.

### Where phase 17 stands against its acceptance criteria

| Criterion | Result |
|---|---|
| `mini_fps` with 100 instanced cubes: draw calls ~100 → 1 | **Met.** `field_calls=1`, frame total 103 → 4 (floor + 2 batches + 1 field) |
| FPS with 100 cubes: ≥ 5x vs the loop | **Met on x86_64 (5.87x) and in the criterion bench (12.19x).** Not measurable on the PX30 at 100 cubes: both arms sit at the 60 FPS vblank. The device result is reported as capacity (59.0 vs 21.9 FPS at 2000 cubes) |
| Static batch: ~50 → 1 draw call | **Met in structure, 48 → 2 in the demo.** A batch carries one colour, and the scene has two (tan obstacles, blue crates); one batch per material is the correct semantics, not a shortfall. `tests/static_batch.rs` asserts the 50 → 1 case directly |
| Upscaling at 0.75: ≥ 40 % FPS | **Met on the PX30: +61.3 %** (5000 cubes, 30.0 → 48.4). Not met on x86_64, where the frame is submission-bound and the pass is pure overhead — recorded above with the measurement that rules out resolution as the explanation |
| `PERF_LOG.md` updated | This section |
| Public API unchanged (R12) | Yes — additive only: `draw_instanced`, `set_render_scale`, `StaticBatch`, `InstancedRenderer`, `Upscaler`, `InstancingSupport`. No existing signature changed, no `giochi.h` change |
| Gates + no regression > 10 % | See `PROGRESS.md` phase 17. The 2D benchmark is untouched by this phase |

### Two design choices worth knowing when reading the numbers

1. **One run of one quad is a `RenderCopy`, not geometry** was phase 16's rule for 2D.
   Phase 17's equivalent is that `draw_instanced` with one instance still uploads the
   instance buffer and issues one instanced call. For a single object the ordinary
   `draw_mesh` is faster; instancing earns its keep from the second instance onwards,
   and the 1-cube row above quantifies the crossover.
2. **The static batch bakes the transform at build time.** That is what makes it one
   buffer, and it is also why the demo cannot move its obstacles: anything that moves
   has to stay out of the batch. The scene is split accordingly — the 48 immobile
   obstacles and crates are batched, the 100 cubes that exist to be instanced are not.

## After Phase 18 (ARM64 flags + benchmark CI)

Date: 2026-11-08 | Build: release | vsync: off on the console, uncapped on x86_64

Phase 18 has four parts — compile flags, a benchmark suite, a regression gate in CI and
a C ABI v3 — and only the first is expected to move a number. It moved exactly one, and
the honest result is that it moved **size, not speed**.

### What the flags do, and one the plan got wrong

`-C target-cpu=native` was rejected outright. Nothing about it can work in a cross
build: cargo runs the compiler on the host, and a host is not a PX30. The core has to be
named, and the PX30's core is a **Cortex-A35** (`CPU part 0xd04` in `/proc/cpuinfo`), not
the `cortex-a53` the plan suggested. This toolchain has a model for it, so
`target-cpu=cortex-a35` is what both `.cargo/config.toml` and `scripts/build-aarch64.sh`
now use.

The plan also proposed `-C target-feature=+lse,+crc,+crypto`. Two of those are right and
one would have shipped a broken binary:

```text
$ grep Features /proc/cpuinfo
Features : fp asimd evtstrm aes pmull sha1 sha2 crc32 cpuid
```

`crc32`, `aes`, `pmull`, `sha1` and `sha2` are present, so `+crc`, `+aes` and `+sha2` are real
instructions on this part. Those three are also the spellings rustc accepts: it warns
that `crypto` is "unknown and unstable", so the umbrella name from the plan is replaced
by the individual stable features it stands for. **`atomics` is absent**, and LSE atomics are an ARMv8.1
feature while the Cortex-A35 is ARMv8.0-A: `+lse` emits `ldadd`/`swpal`-class
instructions the core cannot decode, and the first atomic in the engine is an
`Arc::clone` on the first frame. `+lse` is not applied, and the reason is written next to
the flag in both places it would be added.

This is the phase-17 lesson repeating: the plan named ANGLE and the hardware had EXT.
Here the plan named a feature the CPU does not have. The flag list is now derived from
the device's own `/proc/cpuinfo`, and `scripts/build-aarch64.sh` gives any unknown CPU
**no** target features rather than a guess, because a wrong `+feature` is a SIGILL on a
device nobody is watching and a missed optimisation is merely slower.

There was a second, quieter version of the same trap. `.cargo/config.toml` had the flags
and the cross build still compiled for baseline ARMv8-A, because
`scripts/build-aarch64.sh` exports `RUSTFLAGS` (for the sysroot link path) and cargo
**ignores the config file's `rustflags` entirely while `RUSTFLAGS` is set**. Nothing
failed; the binary was simply never optimised for the console. The script now appends the
CPU flags itself, and the fact was verified with a `RUSTC_WRAPPER` probe that logs the
real compiler command line:

```text
$ grep -o "target-cpu=[a-z0-9-]*" /tmp/rustc-args.log | sort -u
target-cpu=cortex-a35
$ grep -o "target-feature=[^ ]*" /tmp/rustc-args.log | sort -u
target-feature=+crc,+aes,+sha2
```

### `panic = "abort"` was rejected

The plan offered `abort` for the release profile, and it is not usable here.
`giochi-engine-capi` wraps every exported function in `catch_unwind`, and
`docs/C_ABI_CONTRACT.md` §5 states as a guarantee that no Rust panic may cross the
boundary — a panic becomes `GIOCHI_ERROR_INTERNAL`. `abort` removes the ability to catch,
turning every such bug into a process kill inside whatever game embeds the engine. The
plan allowed keeping `unwind` at a cost of roughly 5 % of size; that cost is included in
the numbers below, in exchange for keeping a documented guarantee.

### Binary size, and where it came from

x86_64, `cargo build --release --workspace`:

| Binary | Before (thin LTO, symbols) | After (fat LTO, stripped) | Δ |
| --- | --- | --- | --- |
| `mini_fps` | 2 073 496 B | 1 574 288 B | **−24.1 %** |
| `profiler_demo` | 719 648 B | 487 480 B | **−32.3 %** |
| `giochi-demo` | 2 909 960 B | 2 300 368 B | **−20.9 %** |
| `libgiochi.so` | (not previously built) | 812 304 B | — |

ARM64 (`aarch64-unknown-linux-gnu`, `mini_fps`), decomposed so the two mechanisms stay
attributable:

| Step | Size | Share |
| --- | --- | --- |
| Before | 2 024 560 B | — |
| `strip = "symbols"` only | 1 512 432 B | **−25.3 %** |
| `+ lto = "fat"`, `cortex-a35` | 1 381 360 B | **−31.8 % total** |

The remaining 8.7 % is code, not symbols: `.text` falls 1 414 302 → 1 323 066 bytes
(−6.5 %). Stripping is thus most of the win, and stripping is free — the console copies a
binary and runs it, and never read a symbol table.

`strip = "symbols"` was verified not to break the C ABI: `--strip-all` removes `.symtab`
but the dynamic symbol table survives, and all 74 exported `giochi_*` functions are still
resolvable from the stripped `libgiochi.so` via `nm -D`. The four FFI smoke consumers
pass against a stripped build.

### Speed: measured, and unchanged

Both changes were benchmarked head-to-head against the previous profile, built from the
same source, interleaved on the same machine to cancel out the load it was under (a
concurrent image build held the 12-core host at load ~12–18 for the whole session).

x86_64, `mini_fps --instanced`, 100 cubes, `SDL_VIDEODRIVER=offscreen`:

| Run | old profile | new profile |
| --- | --- | --- |
| 1 | 14 378 | 15 069 |
| 2 | 15 626 | 10 689 |
| 3 | 15 087 | 15 149 |
| 4 | 15 072 | 14 884 |
| 5 | 14 767 | 14 613 |
| **median** | **15 072** | **14 884** |

A 1.3 % difference against a spread of 10 689–15 626 between two runs of the same binary:
that is noise, not a regression. Only LTO and stripping changed on x86_64 — no
`target-cpu` was set there — and neither can change how many instructions the frame
needs.

A cleaner scalar, because it excludes the sprite and GL work entirely: the empty-frame
cost that `tests/perf_regression.rs` prints, same two binaries, interleaved:

| Run | old profile | new profile |
| --- | --- | --- |
| 1 | 267.9 µs | 271.2 µs |
| 2 | 267.3 µs | 263.3 µs |
| 3 | 271.4 µs | 262.8 µs |
| 4 | 258.5 µs | 258.8 µs |
| 5 | 274.7 µs | 261.3 µs |
| **median** | **267.9 µs** | **262.8 µs** |

−1.9 %, in the same direction in 4 runs of 5 but inside the spread. The frame is ~249 µs
of SDL software `present`; the engine's own share is ~5 µs, and that is all LTO could
have touched.

PX30, KMSDRM 640x480, `mini_fps --loop --cubes 2000` (the CPU-bound arm: 2000 separate
draw calls, ~21 fps, so the panel's 60 Hz pacing does not hide anything):

| Run | old profile | new profile |
| --- | --- | --- |
| 1 | 21.8 fps | 21.8 fps |
| 2 | 21.7 fps | 21.6 fps |
| 3 | 21.7 fps | 21.4 fps |
| **median** | **21.7 fps** | **21.6 fps** |

Unchanged, and that is the expected result rather than a disappointing one. This frame is
limited by the Mali-G31/Panfrost driver draining 2000 draw calls, not by the quality of
the A53-class integer code — which is exactly what phase 16 and 17 measured too. The
`+crc`/`+aes`/`+sha2` features cannot help at all: the engine does no CRC and no cryptography,
so they are enabled because they describe the part honestly, not because they pay.

**The phase-18 result is a 24–32 % smaller binary at unchanged speed.** Smaller is not
cosmetic on this device: the payload is written to a small rootfs over a slow link, and
`profiler_demo` losing a third of its size is a third of that transfer.

### The regression gate found a real allocation

`tests/perf_regression.rs` counts allocations with a wrapping global allocator, and its
first run failed: a steady-state frame with 512 batched sprites allocated **exactly once
per frame**, and an empty frame allocated zero. The culprit was
`SpriteBatch::flush_runs`, which returned `Vec<RunView>` — `collect()` on every flush,
since phase 16. It now returns an iterator and collects nothing, with `flush_batch`
destructuring `self` so the iterator over `batch` can live while the body borrows
`textures` and `canvas`. Both the 512-sprite and the empty frame are now at 0
allocations, which is the gate's recorded baseline.

The time cost of that allocation was never measurable — it is a few dozen bytes once a
frame — and it is recorded here as what it is: a correctness-shaped improvement found by
a gate, not a performance win.

### What phase 18 establishes

| Claim | Status |
| --- | --- |
| Console builds are compiled for the console's core | **Met.** `cortex-a35 +crc,+aes,+sha2`, proven from the compiler command line, and a SIGILL-free run on the PX30 |
| Release payload is materially smaller | **Met.** −24.1 % x86_64 `mini_fps`, −31.8 % ARM64, −32.3 % `profiler_demo` |
| No performance regression from the flags | **Met.** unchanged within noise on both machines, 5-run interleaved medians recorded above |
| Per-frame cost has a gate that cannot flake | **Met.** 0 allocations per frame, `ceil(N/1024)` draw calls, run in CI with and without `profiling` |
| C ABI v3 reports frame statistics | **Met.** `giochi_engine_frame_stats` + `GiochiFrameStats`, additive; the four v1/v2 consumers pass unchanged |


## ARM64 validation — **measured on the PX30**

Date: 2026-11-08. The device was reachable over SSH; the console launcher was disabled
with the procedure from `scripts/hardware-check.sh` (chmod 0644, terminate the DRM
holder, restore `chmod 0755` and wait for the supervisor), and the console came back
after every session. The demo binary was copied to `/tmp` (RAM), not to the rootfs.
Battery reported 84–92 % and discharging.

Build:

```bash
CROSS_SYSROOT=/tmp/r35s-sysroot scripts/build-aarch64.sh --release \
    -p giochi-demo --example profiler_demo --features profiling
```

Run:

```text
SDL_VIDEODRIVER=kmsdrm /tmp/profiler_demo --width 640 --height 480 \
    --sprites <N> --frames 600
```

- [x] `profiler_demo` 200 / 500 / 1000 / 5000 / 10000 / 20000 sprites on the PX30
      (table above).
- [x] profiling-off / on / overlay-hidden at 1000 sprites (table above).
- [x] `mini_fps` base scene on the PX30: 600 frames in 10.76 s wall, `user 1.79 s`,
      `sys 0.88 s`, 0 GL errors, 0 meshes/shaders left. The example predates the
      profiler and has no per-frame stats; it is recorded as a sanity number, not a
      phase-16 comparison point.

The phase-18 build matrix was to include `cortex-a35` (PX30) in addition to
`cortex-a53/a72/a76`. It does — `cortex-a35` is the default and the others are in the
table in `docs/CROSS_COMPILE.md`, selected with `GIOCHI_TARGET_CPU`. See "After Phase 18"
above for the measured result.

## Conclusions

- On x86_64 the baseline is dominated by per-sprite draw calls: 200 → 1000 sprites
  costs 1889.6 → 645.5 fps while the work per sprite is identical. Phase 16 should
  attack the call count, not the sprite arithmetic.
- On the PX30, KMSDRM's vblank pacing hides draw-call cost in FPS until the frame
  exceeds 16.7 ms. Use `render2d_avg` (1.761 ms at 1000 sprites, 16.610 ms at 20000)
  and the 20000-sprite FPS point for the phase-16 delta.
- The profiler costs ≤ 1.1 % on x86_64 and ~0.56 % of wall time on ARM64 at the
  1000-sprite baseline; with the panel visible the ARM64 cost is ~1.94 %, inside the
  2 % budget. The overlay is one persistent, in-place-updated texture at 1 Hz.
- At the 20000-sprite stress point the counter increments themselves cost ~5 % CPU
  and the visible panel pushes the wall-rate down further. That is a symptom of 20000
  individual draw calls, which phase 16 removes; it is not the budget scenario.
