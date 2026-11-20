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

## S2 — to be filled (physics determinism + transform sync)

Questions this phase must answer, written here so it does not improvise them:

- Epsilon in the accumulator, or integer nanoseconds? (§S1.4)
- `max_substeps`: keep 4, or raise to 8? (§S1.5) The criterion is a 200 ms hitch.
- Does the interpolated render position need a `previous_position` per body, or is the
  existing `BodyTransform` enough? If it needs storage, that is a `BodySlot` change and
  therefore a C ABI question — see S5a.
- Order of `sync_from_transforms` → `step` → `sync_to_transforms` is fixed; document it
  once in this file and keep the tests as the enforcement.

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
