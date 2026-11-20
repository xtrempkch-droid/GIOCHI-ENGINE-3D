# Stability decisions (S1–S5)

Decisions of the Stability Pack, one section per phase, in the order they were taken.
[STABILITY_DOD.md](STABILITY_DOD.md) says *what* must hold; this file says *why* it holds
that way and what was rejected. Out-of-scope findings go to
[IDEAS_STABILITY.md](../IDEAS_STABILITY.md), not here.

The pack adds **no dependency and no feature**. That is a constraint, not a coincidence:
stability work that grows the graph is harder to trust on the console, and every phase here
is arithmetic, file handling or measurement.

---

## S1 — the ruler, and why each threshold is that number

### 1. Twenty criteria, one command each, status measured on the day

The DOD is only useful if every row can be executed. The rule applied when writing it: if a
criterion could not be turned into a command plus a number, it was **reformulated** until
it could, and the reformulation is recorded in the row ("reformulated from …"). Three rows
were rewritten:

| Original wording | Reformulated as | Why |
|---|---|---|
| "sem jitter perceptível" | per-frame camera displacement within ±25 % of its mean, and 0.5 units across 30 vs 60 fps | "perceptible" is not measurable; the property that causes jitter is |
| "nenhum panic/unwrap em lib" | no panic on the reload and teardown paths (DOD #3, #4, #10, #1) | 714 raw matches, mostly in `#[cfg(test)]` modules; a global audit is another project |
| "VRAM delta < 1 %" | estimated resident bytes from the object registries | GLES 2.0 exposes no memory counter; the estimate catches the leak that matters |

### 2. The VRAM estimate: what it can and cannot see

`scene_load_unload_loop` computes

```text
bytes = Σ texture_size(handle).w × .h × 4     (RGBA8, the only format the API creates)
      + Σ mesh_vertex_count(handle) × 32      (position + normal + uv, 8 floats)
      + Σ mesh_index_count(handle) × 4        (u32 indices)
```

for the objects the cycle itself created. It **can** see an object that survives unload,
which is the leak this engine can actually produce (a `destroy_*` missing from a path). It
**cannot** see driver-internal fragmentation, a stale GL name the registry already forgot,
or a texture the driver keeps alive for another context. Those are stated limits, and the
registry counts are asserted separately because they are exact.

Rejected alternative: querying `GL_TEXTURE_FREE_MEMORY_MGA` or a similar vendor extension.
Not portable to Mali/Panfrost, which is the target.

### 3. Determinism is asserted on positions, not on the solver's internals

The test drives the public `PhysicsWorld3D` and compares final positions. It does not
compare velocities, angular velocities or contact manifolds: those are Rapier's business,
and pinning them would make the test fail on a legitimate solver improvement.

The tolerance is **1e-4**, which is the plan's number. Measured on 2026-11-20, the actual
deviation at 30/60/120 fps is exactly **0.0** — the accumulator + fixed step is bit-exact.
That is a stronger result than the threshold requires, and it is recorded because a future
change that makes it 1e-5 would still pass while being a regression in kind.

### 4. The 144 fps finding is a step-count problem, not a solver problem

`144 × (1/60)` frames at `1/144` s each sum to `0.99999994` in f32. The accumulator never
reaches the 60th step, so a 1 s run integrates 59 steps at 144 fps and 60 at 30/60/120.

Two candidate fixes, for S2 to choose between (not decided here):

1. **Epsilon in the accumulator comparison** (`accumulator + EPS >= timestep`), with EPS
   scaled to the timestep. Cheapest, but it can run one extra step when the frame delta is
   genuinely just under the step.
2. **Accumulate in integer nanoseconds.** Exact by construction, costs a conversion per
   frame, and removes the whole class of residue bugs. Preferred if S2 finds no other
   float-accumulator dependency.

Either way, the acceptance test is the one committed in S1
(`the_accumulator_does_not_lose_a_step_to_float_residue`), which fails today and names the
step count rather than a position, so the fix is judged on the property and not on a
tolerance.

### 5. `max_substeps` is documented as 8, and the world uses 4

The S2 contract in the master plan says `max_substeps = 4` and allows raising it to 8. The
existing `PhysicsWorld3D` already clamps at its own `MAX_SUBSTEPS` (4) and discards the
remainder after a hitch. The DOD's bound is **8** so S2 can raise it if a 200 ms hitch
(editor recompile, asset load) needs it without the test changing; the test asserts
`steps <= 8`, not `== 4`.

### 6. No production code was touched in S1

The phase is tests and documentation only. The one place this was tempting was the
percentile measurement for DOD #7: `mini_fps` does not print p50/p99, and wiring the
profiler into it is a five-line change. It was **not** done, for two reasons:

- S1's acceptance is "nada de produção foi tocado"; an example is code users run.
- S5a is where `docs/STABILITY_METRICS.md` is filled with p50/p95/p99 by contract, so
  doing it here would duplicate S5's work in a phase that is not allowed to change
  behaviour.

The baseline therefore records **mean frame time and draw calls** (real, measured) and
names the percentile column as S5a's. `ball_balance` already prints the full percentile
set, and its numbers are in the baseline as the reference implementation.

### 7. Which crate owns `abi_layout_3d`

The master prompt names `giochi-engine-capi/tests/abi_layout_3d.rs`. The file is actually
**`giochi-engine-3d/tests/abi_layout_3d.rs`**, because it asserts the Rust-side `#[repr(C)]`
layout of the 3D types; the capi crate has `rust_ffi_3d.rs` for the boundary itself. The DOD
uses the real command. The stale path in the prompt is recorded in `IDEAS_STABILITY.md`
rather than "fixed" by moving a working test.

### 8. Skeleton tests use `todo!()` inside `#[ignore]`, not empty bodies

An ignored test with an empty body passes the moment somebody removes the `#[ignore]`,
which is worse than not having it. `todo!("S3: …")` names the missing API and fails loudly.
This is the repository's own convention (`CONTRIBUTING.md`, "Style": *stubs for future
phases use `todo!("fase N: …")`*).

Where the property is **already true**, the test contains the real assertion instead, so S1
also proved several criteria rather than deferring all of them:

| File | Already true on 2026-11-20 | Still open |
|---|---|---|
| `physics_determinism.rs` | 30/60/120 fps bit-identical; 100 runs identical; hitch recovery | 144 fps step residue |
| `transform_sync.rs` | both directions in the same frame; camera convergence | — (all 5 green) |
| `hot_reload_safety.rs` | bad shader rejected; bad JSON refused; future version refused | texture reload; 10 failures |
| `scene_load_unload_loop.rs` | 100 cycles, 0 % delta; `release_all` empties | activation in S4's gate |
| `material_contract.rs` | `Material::DEFAULT` semantics | the whole contract |
| `mesh_collider.rs` | box floor holds a dropped body | trimesh / convex |
| `renderer_state.rs` | public API shape and `Material` size | the cache itself |

### 9. The stability workflow is separate from `ci.yml`

`ci.yml` already runs the workspace in debug. `stability.yml` exists to run the properties
in **release** — where float code and the solver behave differently — and to run the
stability tests as a named group, so "stability is red" is a status a reviewer can see
without reading a 400-line log. It does not duplicate the workspace suite; it runs the
tests the DOD names, plus the regression commands from R7.

The workflow does not gate merges by itself (branch protection is a repository setting).
That is deliberate: a stability job that is required but flaky would be disabled within a
week.

### 10. What "no EGL" means for these tests

`scene_load_unload_loop`, `hot_reload_safety` and the S5 state test need a GL context. A
developer machine without EGL is normal; `docs/3D.md` §5 establishes that these tests skip
cleanly. The skip prints `skipped: no GL driver` and returns, so it is visible in
`--nocapture` output. The stability gate runs on CI where the offscreen driver exists, and
the skip is a known limitation on a bare console (KMSDRM is the only driver there, and the
test honours `SDL_VIDEODRIVER` if it is already set — `tests/common/mod.rs`).

---

## S2 — physics determinism (2026-11-20)

### 1. Epsilon in the accumulator, not integer nanoseconds

§S1.4 left two candidates. Epsilon was chosen:

| Candidate | Verdict |
|---|---|
| **Epsilon relative to the step** (`ACCUMULATOR_EPSILON = 1e-4`) | **chosen** — one comparison, no new state, error bounded by 1e-4 of a step (1.7 µs of simulated time), and the residue to absorb is 5.7e-6 of a step, 17× inside the tolerance |
| Integer nanoseconds | exact, but it means changing the accumulator's type, converting every `delta`, and keeping a second representation of the step that has to agree with `IntegrationParameters::dt`; the property is already reached without it |

The decisive argument was the measurement: the residue is a *rounding* of `1/144`, so
accumulating the same f32 deltas more precisely (f64 or nanoseconds) does **not** help —
`144 × f32(1/144)` is `0.99999994` no matter how you store the sum. Only a tolerance (or a
delta that is not f32) changes the outcome. Nanoseconds would only help if the *frame
delta* were computed in nanoseconds upstream, which is a renderer change this phase does
not need.

A second risk was checked before committing: a tolerance can run a step too early. The
guard is bounded and tested — `the_tolerance_never_adds_a_step_when_the_time_is_genuinely_short`
asserts that a delta two tolerances short of a step runs **zero** steps.

The leftover after subtraction is clamped with `.max(0.0)`. Without it the tolerance leaves
a negative accumulator (≈ −1e-7), which is a small debt against the next step; clamping
keeps the accumulator a measure of "time owed" rather than "time borrowed".

### 2. `max_substeps` stays 4

§S1.5 allowed raising it to 8. It was not raised:

- The existing hitch guard is a documented behaviour (`MAX_SUBSTEPS` is public, and
  `the_accumulator_caps_the_steps_a_stall_can_run` asserts it). Raising it changes how much
  simulated time a 0.25 s frame can catch up on, from 4 to 8 steps (66 ms → 133 ms).
- Nothing measured asks for it. The DOD's criterion is "a 200 ms hitch does not explode",
  and the existing test proves recovery with 4. If a real editor recompile or asset load
  produces a 200 ms frame on the handheld, that is a measurement for S5/phase 46, not a
  guess now.

The DOD keeps the assertion at `steps <= 8` so the constant can move without the ruler
breaking; the constant itself is 4 and is documented as such.

### 3. No `physics/fixed_timestep.rs` type — a deliberate deviation from the phase list

The master prompt lists a `FixedTimestep` struct with `accumulator`, `max_substeps` and a
`step(dt, world) -> StepStats` signature, plus `interpolation_alpha`. It was **not** added:

- `PhysicsWorld3D` already owns the accumulator, the step clamp, the substep cap and the
  post-hitch discard. Extracting them into a second type would either duplicate the state
  (two accumulators that must agree) or move five private fields and change no observable
  behaviour.
- R1/R8 are explicit: no refactor that a measurement did not ask for. The property
  (determinism, DOD #2) is now met and tested; the file layout gains nothing.
- `StepStats` as a return type would be a public API change for a value no caller uses
  today (`step` already returns the substep count).

The extraction is recorded in `IDEAS_STABILITY.md` as a refactor, not silently dropped.

### 4. Interpolated rendering was not added, and why that is honest

The phase list also asks for `previous_position`/`current_position` per body and a
`draw_scene(alpha)` that renders between them. Not implemented:

- **No DOD criterion measures judder.** Criterion 9 measures the *camera*, which
  interpolates its own position and passes at 30 and 60 fps. Interpolating rigid bodies is
  a smoother animation, i.e. a feature.
- It changes `BodySlot` (per-body storage), which is exactly the kind of change that
  propagates to the C ABI (R6) and to the Racer's car. Doing it because a phase list said so
  — with no criterion and no measurement — is the scope creep R1 exists to stop.
- The trigger to revisit is written down in `IDEAS_STABILITY.md`: a frame-time or
  positional measurement showing visible judder when the render rate is below the physics
  rate (the handheld at 30 fps with 60 Hz physics). That is a phase-45 measurement, not an
  S2 one.

### 5. The order is `sync_from_transforms` → `step` → `sync_to_transforms`

Fixed here, enforced by `tests/transform_sync.rs`:

1. **Scene → physics** before the step, so an editor drag or a respawn enters the solver in
   the same frame instead of a frame late.
2. **Step**, which consumes that state.
3. **Physics → scene** after the step, so the renderer sees the result the solver just
   produced rather than the previous frame's.

**Both are needed in the same frame**: doing only the second is a one-frame lag the editor
shows as an object passing through a wall before the collider catches up; doing only the
first makes the renderer a frame stale. The tests assert the value read back, not just that
the callback ran.

### 6. What changed in production code, and what was left alone

Two files changed: `physics/mod.rs` (the constant and its documentation) and
`physics/world.rs` (the comparison, the clamp, plus two unit tests). No struct layout
changed, so **the C ABI does not move** (R6) — `cargo test -p giochi-engine-3d --test abi_layout_3d`
and the capi suite are unchanged and green.

`MAX_FRAME_DELTA` (0.25 s), `FIXED_TIMESTEP` (1/60) and the 4-substep cap are untouched:
the phase is about the *comparison*, and widening the envelope would be a second,
unmeasured change.

### 7. The Racer was the regression test that mattered

A change to when a step runs could move the car. Measured after the fix: `giochi-racer
--frames 600 --auto` produces the same 10.000 s of race time and the **same 196 sampled
colours** in the frame capture, and the crate's 19 physics/spec tests are green. That is a
stronger signal than the fps number, which is noise at 5 900 fps on this GPU.

## S3 — to be filled (material contract + mesh collider)

- Where the contract lives (`material_contract/`) and why it is not a second `Material`.
- Fallback precedence when a texture path exists but fails to decode.
- Trimesh vs convex: exact rule (static and large → trimesh; dynamic and small → convex),
  the 64k-vertex chunking, and the measured setup cost on `track_easy`.

## S4 — to be filled (hot reload safety + asset lifecycle)

- `Result` vs `bool` for the reload API. The DOD deliberately does not name a type.
- Last-known-good: what is snapshotted (path + hash), and where.
- Drop order (`drop_meshes → drop_textures → drop_shaders → drop_physics_bodies →
  drop_scene`) and the rule that drops are deferred to the end of the frame, never inside a
  render pass.

## S5 — to be filled (C ABI sync + renderer, data-driven)

- The S5b decision, with the numbers from `STABILITY_METRICS.md` that produced it.
- If optimised: what the profiler pointed at, what changed, and the p99 delta.
- If not optimised: "renderer is not a hotspot", with draw calls and p99, recorded with
  the same care as an optimisation — skipping work is a result.
