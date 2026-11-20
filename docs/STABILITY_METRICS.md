# Stability metrics — baseline and deltas

The numbers behind [STABILITY_DOD.md](STABILITY_DOD.md). One section per phase; S1 is the
baseline, later phases append a column rather than rewriting the first one, so the delta is
always visible.

## Machine

| | |
|---|---|
| CPU / OS | x86_64, Linux 7.0.0-43-generic (Ubuntu) |
| GPU | **AMD Radeon RX 580** (radeonsi, polaris10, ACO) |
| Driver | Mesa, GLES 2 via EGL (`SDL_VIDEODRIVER=offscreen`) |
| Build | `--release` where a number is compared; `dev` only where noted |

This is **not** the Racer's target. The handheld (Mali-G31/G52, KMSDRM) is measured in
phase 46; the thresholds in the DOD are chosen to be meaningful on both, and the point of
the baseline is the **delta**, not the absolute value on a desktop GPU.

## How to measure

Every row below is reproducible with the committed code and no arguments beyond the ones
shown. Timings are per-process wall clock; run each command three times and take the
median when comparing phases.

```bash
# Frame time and draw calls, 600 frames on the instanced field
SDL_VIDEODRIVER=offscreen cargo run --release -p giochi-engine-3d --example mini_fps -- \
    --frames 600 --auto --bench --instanced

# Same at 75 % render scale (the upscaling pass)
SDL_VIDEODRIVER=offscreen cargo run --release -p giochi-engine-3d --example mini_fps -- \
    --frames 600 --auto --bench --instanced --scale 0.75

# Physics: 24 bodies, the player blocked by a wall and resting on the floor
SDL_VIDEODRIVER=offscreen cargo run --release -p giochi-engine-3d --features physics-3d \
    --example physics_3d_demo -- --frames 300 --auto

# Percentiles (p50/p95/p99/worst), the reference implementation in this workspace
SDL_VIDEODRIVER=offscreen cargo run --release -p giochi-engine-3d --example ball_balance -- \
    --frames 300 --auto

# Editor Play mode
SDL_VIDEODRIVER=offscreen cargo run --release -p giochi-editor -- \
    --scene giochi-editor/assets/scenes/default.scene --frames 8 --play-frames 3 \
    --config /tmp/giochi-metrics-cfg

# The Racer on the shipped track (headless, scripted driver)
SDL_VIDEODRIVER=offscreen cargo run --release -p giochi-racer -- \
    --frames 600 --auto --quadro /tmp/race.bmp
```

`mini_fps --bench` prints one line; the numbers below are extracted from it and from the
demo's own summary. `ball_balance` prints `frame work: mean … p50 … p95 … p99 … worst` and
`N/300 frames over 20 ms`.

## S1 — baseline (2026-11-20)

### mini_fps, 1280×720, 100 instanced cubes, 600 frames

| Scale | Wall | fps | Draw calls | Field calls | Instanced | Render size |
|---|---:|---:|---:|---:|---:|---|
| 1.00 | 0.057 s | 10 448.6 | 4 | 1 | 1 | 1280×720 |
| 0.75 | 0.063 s | 9 516.8 | 4 | 1 | 1 | 960×540 |
| 0.50 | 0.064 s | 9 317.6 | 4 | 1 | 1 | 640×360 |

Reading: **4 draw calls** for a scene with 24 obstacles, 24 crates, 100 instanced cubes and
a floor — the static batches and the instanced field already collapse to one call each.
The GPU is **submission-bound** at this size (10 000 fps means the CPU is the limit), which
is why lowering the resolution does not help; the documented fill-bound case is the PX30
(`docs/PERF_LOG.md`). Mean frame time is **0.095 ms** at scale 1.00.

Percentiles for this scenario are **not yet emitted** by `mini_fps` (DOD #7); S5a adds them.
The closest existing percentile measurement is `ball_balance` below.

### physics_3d_demo, 300 frames

| Measurement | Value |
|---|---|
| Frames / simulated time | 300 / 5.0 s |
| fps | 60.0 (vblank-locked) |
| Bodies | 24 |
| Player | `(5.18, 0.91, -0.18)`, `bloqueado_pela_parede=true`, `apoiado_no_chao=true` |
| Boxes still moving | 0 |
| Top of the stack | 4.49 |

### ball_balance, 300 frames (frame work, not wall clock)

| Metric | Value |
|---|---|
| mean | 0.05 ms |
| p50 | 0.05 ms |
| p95 | 0.08 ms |
| p99 | **0.09 ms** |
| worst | 0.13 ms |
| Frames over 20 ms | **0/300** |

This is the DOD #7 shape of answer, on the scene where it is already instrumented. p99 of
0.09 ms against a 25 ms threshold is not a close call on this GPU; the interesting case is
the handheld.

### Editor Play mode

`Played 3 frame(s) on AMD Radeon RX 580 Series … (1 mesh(es), 1 instance(s), 0
unsupported)` — exit 0.

### Physics determinism (DOD #2), debug and release

| fps | Frames for 10 s | Fixed steps that ran | Deviation vs 60 fps |
|---:|---:|---:|---:|
| 30 | 300 | 600 | **0.0** |
| 60 | 600 | 600 | reference |
| 120 | 1200 | 600 | **0.0** |
| 144 | 1440 | **599** | 0.0 (at rest) / **0.164 m** after 1 s |

100 runs at 60 fps: **bit-for-bit identical**. The 144 fps row is the S2 defect: one step
lost to f32 residue in `144 × (1/144) = 0.99999994`.

### GPU object lifecycle (DOD #1)

| Cycle | Estimated resident bytes | Meshes | Textures | Shaders |
|---:|---:|---:|---:|---:|
| 1 | 21 152 | baseline | baseline | baseline |
| 100 | 21 152 | baseline | baseline | baseline |
| Delta | **0 %** | 0 | 0 | 0 |

The estimate is `Σ(texture w×h×4) + Σ(vertices×32 + indices×4)` for the objects the cycle
owns: four meshes (cube, cube, sphere, plane) and three 32×32 textures.

## S2 — physics determinism (2026-11-20)

### The fix, in one number

`ACCUMULATOR_EPSILON = 1e-4` (relative to the step) makes the accumulator run a step when it
is within 0.01 % of one. Measured residue to absorb at 144 fps: **9.8e-8 s**, about 5.7e-6
of a step. The tolerance can pull at most `1e-4` of a step (1.7 µs of simulated time) early,
and the leftover is clamped to zero so it cannot become a debt.

### Step count and deviation, after the fix

| fps | Frames for 1 s | Steps at 1 s | Deviation at 1 s | Frames for 10 s | Steps at 10 s | Deviation at 10 s |
|---:|---:|---:|---:|---:|---:|---:|
| 30 | 30 | 60 | 0.0 | 300 | 600 | 0.0 |
| 60 | 60 | 60 | reference | 600 | 600 | reference |
| 120 | 120 | 60 | 0.0 | 1200 | 600 | 0.0 |
| 144 | 144 | **60** | **0.0** | 1440 | **600** | **0.0** |
| 240 | 240 | 60 | 0.0 | 2400 | 600 | 0.0 |

Before the fix the 144 fps row read **59 / 0.164 m** at 1 s and **599** at 10 s. 100 runs at
60 fps remain bit-for-bit identical.

### Regression against the S1 baseline

| Scenario | S1 | S2 | Δ |
|---|---:|---:|---:|
| `physics_3d_demo`, 300 frames | 60.0 fps, player `(5.18, 0.91, -0.18)`, wall-blocked, floor-supported | same | **0 %** |
| `giochi-racer`, 600 frames on `track_easy` | 10.000 s of race time, 196 sampled colours | 10.000 s, **196** colours | **0 %** |
| `mini_fps`, 600 frames | 4 draw calls, 10 448.6 fps | unchanged (no renderer change) | 0 % |
| Physics unit suite | 9 tests | **11** (two new accumulator tests) | +2 |

The Racer's frame signature is identical, which matters more than the fps number: the fix
changed *when* a step runs by at most 1.7 µs, and the deterministic scripted driver lands on
the same frame of the same lap.

## S3 — to be filled

Columns: frame time in `mini_fps` before/after the material contract (−10 % allowed for the
collider setup, none expected at runtime), and the collider setup cost for `track_easy`
(milliseconds once, not per frame).

## S4 — to be filled

Columns: cycle 1 vs cycle 100 bytes with the texture reload path in place, and the reload
latency for a shader (must not block the render thread).

## S5 — to be filled

Columns: this is where DOD #7 and #19 get their percentiles. The S5b decision
(optimise / do not optimise) is recorded in `docs/STABILITY_DECISIONS.md`, and if the
answer is "do not optimise", these columns stay identical to S1 **on purpose**.

## Template for a new measurement

```markdown
### <scenario>, <frames> frames, <build>

| Metric | S<n-1> | S<n> | Δ |
|---|---:|---:|---:|
| mean frame time | | | |
| p50 | | | |
| p95 | | | |
| p99 | | | |
| draw calls | | | |
| frames over 20 ms | | | |

Command:
`…`
Reading: <one sentence: is this better, worse, or unchanged, and does the profiler say why>
```
