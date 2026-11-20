# Definition of Done — Stability Pack (S1–S5)

This is the **ruler**: what "stable" means for this engine, how each property is measured,
and what number counts as passing. A phase of the Stability Pack closes when its criteria
move from ⬜/🟡 to ✅ with a measurement pasted into
[STABILITY_METRICS.md](STABILITY_METRICS.md), not when the code "looks right".

The Stability Pack adds no features. Every criterion below is a property the engine
already claims or that the Racer (phases 41–46) needs in order not to debug engine bugs
and game bugs at the same time.

**Legend**

| Mark | Meaning |
|---|---|
| ✅ | Satisfied **and measured** on this machine; the command is in the row |
| 🟡 | Measured, with a **named gap** in the notes — the phase that closes it is named |
| ⬜ | Not yet measured; the phase that will is named |
| ⏳ | Needs hardware or a destructive action; manual, with the procedure recorded |

Status column reflects **2026-11-20**: S1 closed, S2 closed. The machine is x86_64 with an
AMD Radeon RX 580 (radeonsi) for GL, which is a desktop GPU and not the Racer's target —
so frame-time criteria are recorded here and re-measured on the handheld in phase 46.

---

## 1–20

### 1. A complex scene loads and unloads 100 times without leaking

- **Command:** `SDL_VIDEODRIVER=offscreen cargo test -p giochi-engine-3d --test scene_load_unload_loop -- --ignored`
- **Threshold:** estimated resident bytes after cycle 100 within **1 %** of cycle 1, and
  the object registries (`mesh_count`, `texture_count`, `shader_count`) exactly back to
  their baseline.
- **Why the estimate:** GLES 2.0 has no memory-usage query. `docs/STABILITY_DECISIONS.md`
  §S1 records the formula and what it can and cannot catch.
- **Status:** 🟡 — measured 2026-11-20: **0 % delta over 100 cycles, registries exact**.
  Test stays `#[ignore]`d until S4 moves it into the stability gate (S4 owns the CI hook).

### 2. Physics is deterministic across framerates

- **Command:** `SDL_VIDEODRIVER=offscreen cargo test --release -p giochi-engine-3d --features physics-3d --test physics_determinism`
- **Threshold:** final position within **1e-4** at 30, 60, 120 and 144 fps, over 10 s of
  simulated time; 100 identical runs bit-for-bit equal.
- **Status:** ✅ **closed in S2** (2026-11-20). Before S2: 30/60/120 fps were bit-identical
  but 144 fps lost one fixed step to f32 residue (`144 × 1/144 = 0.99999994`), which is a
  0.164 m difference one second after the drop. `ACCUMULATOR_EPSILON`
  (`giochi-engine-3d/src/physics/mod.rs`) absorbs the residue; after the fix all four
  framerates and 240 fps run **600 steps over 10 s and 60 over 1 s**, with **0.0
  deviation** at both horizons, in debug and release, and 100/100 runs identical.
  `the_accumulator_does_not_lose_a_step_to_float_residue` is active (not ignored) and is
  the acceptance test; `the_tolerance_never_adds_a_step_when_the_time_is_genuinely_short`
  is the guard against over-correcting.

### 3. A shader with a syntax error leaves the previous shader active

- **Command:** `SDL_VIDEODRIVER=offscreen cargo test -p giochi-engine-3d --test hot_reload_safety -- --ignored`
- **Threshold:** the reload reports failure; `shader_count` unchanged; no GL error; the
  next frame renders.
- **Status:** ✅ measured 2026-11-20 with the current `bool` API. S4 may change the
  signature to `Result`; the observable property is what is asserted, so the test survives
  the change.

### 4. A scene with invalid JSON does not replace the current one

- **Command:** same as #3.
- **Threshold:** `Scene::from_json` returns `Err`; the caller's `Scene` is untouched; a
  scene from a future `version` is refused, not misread.
- **Status:** ✅ measured 2026-11-20 (both the malformed-JSON and the future-version cases).

### 5. The C ABI layout tests pass after any struct change

- **Command:** `cargo test -p giochi-engine-3d --test abi_layout_3d && cargo test -p giochi-engine-capi`
- **Threshold:** 10 layout assertions green for the 3D types; the C and Python consumers
  green in CI (`c_smoke_3d.c`, `python_smoke_3d.py`).
- **Status:** ✅ measured 2026-11-20 (`10 passed`). Note the 3D layout test lives in
  `giochi-engine-3d/tests/`, not in the capi crate — the phase table in the prompt names the
  wrong crate; this row is the correct command.

### 6. Editor Play mode survives a scene with physics and materials

- **Command:**
  `SDL_VIDEODRIVER=offscreen cargo run -p giochi-editor -- --scene giochi-editor/assets/scenes/default.scene --frames 8 --play-frames 3 --config /tmp/giochi-s1-cfg`
- **Threshold:** exit 0 and `Played 3 frame(s) on <adapter>` in the log. The prompt's
  `--headless-test-play-mode` flag does not exist; this is the equivalent that does
  (`--play-frames` opens the preview window and returns). Recorded in `IDEAS_STABILITY.md`
  as a possible ergonomic flag, not implemented here.
- **Status:** ✅ measured 2026-11-20: `Played 3 frame(s) on AMD Radeon RX 580 … (1 mesh(es),
  1 instance(s), 0 unsupported)`.

### 7. Frame time p99 stays under 25 ms in `mini_fps`

- **Command:** `SDL_VIDEODRIVER=offscreen cargo run -p giochi-engine-3d --example mini_fps -- --frames 600 --auto --bench --instanced`
- **Threshold:** p99 < **25 ms** (60 Hz budget plus one frame of slack) and draw calls
  reported per category.
- **Status:** 🟡 — **mean** measured 2026-11-20: 600 frames in 0.057 s, 4 draw calls (1
  instanced field + 2 static batches + floor). `mini_fps` does not print percentiles; the
  reference implementation that does is `ball_balance` (p50 0.05 ms, p95 0.08, p99 0.09,
  worst 0.13 on this GPU). S5a wires the same percentiles into `mini_fps` and fills the
  column.

### 8. An editor edit reaches physics in the same frame, and physics reaches the scene

- **Command:** `SDL_VIDEODRIVER=offscreen cargo test -p giochi-engine-3d --features physics-3d --test transform_sync`
- **Threshold:** position read back in the same frame matches within 1e-6, in both
  directions; a raycast immediately after `sync_from_transforms` hits the edited body.
- **Status:** ✅ measured 2026-11-20, active since S2 (5/5 in debug and release).

### 9. The chase camera does not oscillate at 30 or 60 fps

- **Reformulated from:** "sem jitter perceptível", which cannot be asserted.
- **Threshold:** with a constant-velocity target, the per-frame eye displacement in the
  second half of a 90-frame run stays within **±25 %** of its own mean, and the eye is
  within **0.5 units** after 2 s at 30 fps versus 60 fps.
- **Command:** same as #8 (the camera tests live in `transform_sync.rs` because the property
  is transform interpolation, not rendering).
- **Status:** ✅ measured 2026-11-20.

### 10. A missing texture falls back to the material's base colour

- **Command:** `SDL_VIDEODRIVER=offscreen cargo test -p giochi-engine-3d --test material_contract -- --ignored`
- **Threshold:** every row of the documented fallback table resolves without an error, and
  `Material::DEFAULT` stays a plain matte white.
- **Status:** 🟡 — the `Material` half is asserted and green today; the `MaterialContract`
  type (glTF ↔ scene ↔ shader) does not exist yet. S3 owns it.

### 11. A static track collides from its mesh, not from a box approximation

- **Command:** `SDL_VIDEODRIVER=offscreen cargo test -p giochi-engine-3d --features physics-3d --test mesh_collider -- --ignored` plus `cargo test -p giochi-racer`
- **Threshold:** a body dropped from 10 m rests on the floor (`0.3 < y < 0.8`); the shipped
  `track_easy` keeps the physics car inside; a mesh above 64k vertices is split into
  chunks.
- **Status:** 🟡 — the box-collider behaviour is measured (the drop test passes, and
  `giochi-racer/tests/physics_track.rs` keeps the car on the road); **trimesh/convex
  colliders do not exist**. S3 owns them.

### 12. `cargo test --workspace --release` passes

- **Command:** `SDL_VIDEODRIVER=offscreen cargo test --workspace --release`
- **Threshold:** 0 failures.
- **Status:** ✅ measured 2026-11-20 as part of the S1 gates (see
  [PROGRESS.md](../PROGRESS.md) §Stability).

### 13. `cargo clippy --workspace --all-targets -- -D warnings` is clean

- **Status:** ✅ measured 2026-11-20, exit 0.

### 14. No `panic!`/`unwrap()`/`expect()` on reachable runtime paths

- **Reformulated.** The global form is not testable in this repository and is **out of
  scope** (see "Fora de escopo"): a grep finds **714** raw matches across the six crates'
  `src/`, and the overwhelming majority are inside `#[cfg(test)]` modules or after an
  invariant the type system cannot express. Converting all of them is a separate project,
  not a stabilization phase.
- **What is enforced instead:** the paths the Stability Pack touches — shader/scene/texture
  reload (#3, #4, #10) and renderer teardown (#1) — must never panic. Those are covered by
  tests that assert an `Err`/`false` and a still-usable renderer.
- **Command:** `SDL_VIDEODRIVER=offscreen cargo test -p giochi-engine-3d --test hot_reload_safety --test scene_load_unload_loop`
- **Status:** 🟡 — 3 of the 5 reload scenarios are green today; the texture-reload and
  ten-failures scenarios are S4's.

### 15. The physics tests pass in `--release`

- **Command:** `SDL_VIDEODRIVER=offscreen cargo test --release -p giochi-engine-3d --features physics-3d --test physics_determinism`
- **Why release:** debug builds carry overflow checks and different float code paths;
  a solver that only agrees in debug is not deterministic.
- **Status:** ✅ measured 2026-11-20, re-measured after S2's fix (4/4 in release).

### 16. `mini_fps`, `physics_3d_demo` and `ball_balance` run 300 frames without panicking

- **Commands:**
  - `SDL_VIDEODRIVER=offscreen cargo run -p giochi-engine-3d --example mini_fps -- --frames 300 --auto`
  - `SDL_VIDEODRIVER=offscreen cargo run -p giochi-engine-3d --features physics-3d --example physics_3d_demo -- --frames 300 --auto`
  - `SDL_VIDEODRIVER=offscreen cargo run -p giochi-engine-3d --example ball_balance -- --frames 300 --auto`
- **Status:** ✅ measured 2026-11-20; all three exit 0 with no GL error. `physics_3d_demo`
  reports `bloqueado_pela_parede=true apoiado_no_chao=true`; `ball_balance` reports
  `0/300 frames over 20 ms`.

### 17. A complex scene saves and reopens identically

- **Command:** `cargo test -p giochi-engine-3d scene::`
- **Threshold:** round-trip through JSON and through a file preserves hierarchy, ids,
  transforms and materials.
- **Status:** ✅ — the round-trip tests exist since phase 27 (`scene/tests.rs`:
  `round_trip_through_json_preserves_everything`,
  `round_trip_through_a_file_preserves_hierarchy_and_ids`, plus the transform and emissive
  round-trips) and are in the workspace suite.

### 18. `giochi.h` matches the layout the tests assert

- **Command:** `scripts/gen-header.sh --check && cargo test -p giochi-engine-3d --test abi_layout_3d`
- **Status:** ✅ measured 2026-11-20 (`giochi.h is up to date`; 10 layout tests green).

### 19. Upscaling, instancing and particles do not glitch at 75 %

- **Command:** `SDL_VIDEODRIVER=offscreen cargo run -p giochi-engine-3d --example mini_fps -- --frames 600 --auto --bench --instanced --scale 0.75`
- **Threshold:** the run exits 0 with no GL error, draw calls unchanged, and the existing
  pixel tests (`tests/upscale.rs`, `tests/instancing.rs`, particles render tests) stay green.
- **Status:** 🟡 — measured 2026-11-20: at 0.75 the frame is 960×540, **4 draw calls, no GL
  error**; on this submission-bound GPU the pass costs 10 % (10 448 → 9 517 fps), which is
  the documented behaviour (`docs/3D.md` §6.1). "No visual glitch" at 0.75 has pixel tests
  for 1.00 and the structural tests for the pass; a dedicated 0.75 pixel assertion is S5's.

### 20. The committed assets work in a clean build

- **Command:** `git clone . /tmp/giochi-clean && cd /tmp/giochi-clean && cargo build --release --workspace --examples`
- **Threshold:** exit 0 with no network access to anything but the cargo registry, and the
  demos find their assets.
- **Status:** ⏳ — the destructive form (`git clean -xdf` in place) is deliberately not run
  here because it would delete the local `target/` (a 20-minute rebuild). The safe clone
  form is recorded; running it is part of the S1→S2 transition on a clean machine.

---

## Fora de escopo (não é a DOD)

These are real, and deliberately **not** criteria of the Stability Pack. They go to
[IDEAS_STABILITY.md](../IDEAS_STABILITY.md) if someone wants them.

- **`unwrap`/`expect`/`panic!` across the whole codebase** (714 raw matches). The global
  audit is a separate project; #14 covers the paths this pack touches.
- **Fuzzing** the glTF/OBJ loaders, the scene parser or hot reload with thousands of
  malformed files.
- **24-hour soak testing** for leaks that only appear after hours.
- **VRAM measured in bytes from the driver.** No portable GLES 2.0 query exists; #1 uses an
  object/estimate model and says so.
- **Frame-time parity with the handheld on x86_64.** The numbers here are a desktop GPU;
  phase 46 re-measures on the R35S/x55.
- **`clippy::unwrap_used` as a lint.** It would fail on test code and on 700 call sites.
- **A `--headless-test-play-mode` editor flag.** #6 uses the flag that exists.
- **Renderer refactor for its own sake.** S5b decides; if the profiler does not point at
  the renderer, `renderer/` is not split.
- **New dependencies.** The pack adds none; `docs/STABILITY_DECISIONS.md` records that as a
  constraint, not a coincidence.

---

## How to use this document

1. Before a phase, read the rows it owns.
2. Run the row's command. Paste the number into
   [STABILITY_METRICS.md](STABILITY_METRICS.md) under the phase's heading.
3. Flip the mark to ✅ only when the number is inside the threshold **on this machine**.
4. If a threshold turns out to be unreachable, do **not** force it: record the measured
   value and the reason in [STABILITY_DECISIONS.md](STABILITY_DECISIONS.md) and change the
   threshold here in the same commit, so the ruler and the reality move together.
