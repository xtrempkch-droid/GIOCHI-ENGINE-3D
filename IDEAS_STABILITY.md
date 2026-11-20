# IDEAS_STABILITY — out of scope for the Stability Pack (S1–S5)

Ideas that appeared while hardening the engine and that are **not** part of S1–S5. This file
exists so they are neither lost nor implemented by accident: the Stability Pack adds no
feature, no dependency and no refactor that a measurement did not ask for (R1, R8).

Nothing here is implemented without the user's approval **and** evidence that it matters.

## Refactors identified but not prioritised

- [ ] `physics/world.rs` (~34 KB) split into `world/`, `step.rs`, `query.rs`. S2 touches
      the step logic; splitting the file is not the same task.
- [ ] `material.rs` split into `material/contract.rs` + `shader_bind.rs`. S3 adds the
      contract in a **new** module instead, so the existing type keeps its ABI layout.
- [ ] `render3d/renderer.rs` (~62 KB) split into more submodules. S5c does this **only** if
      S5b says the renderer is a hotspot.
- [ ] Extract the accumulator into `physics/fixed_timestep.rs` as `FixedTimestep` +
      `StepStats` (the S2 phase list asked for it). Not done in S2 because the world already
      owns that state and extraction changes no observable behaviour —
      `docs/STABILITY_DECISIONS.md` §S2.3.
- [ ] Interpolated rigid-body rendering (`previous_position`/`current_position` per body and
      `draw_scene(alpha)`). Not a DOD criterion; revisit when there is a measurement showing
      judder with the render rate below the physics rate (handheld at 30 fps) —
      `docs/STABILITY_DECISIONS.md` §S2.4. Note this touches `BodySlot` and therefore the
      C ABI question R6 reserves for S5.
- [ ] Hierarchical `Transform` with a cached `Mat4`. Today the scene multiplies parent
      matrices per node per frame; nobody has profiled it.
- [ ] `AssetHandle` with internal refcounting (`Rc`). Would change the teardown story S4
      is defining, so it cannot be done in parallel with S4.

## Optimisations identified but not implemented

- [ ] Occlusion culling. Expensive to do well on GLES 2.0 and unmeasured on the handheld.
- [ ] Material batching per shader. S5c may cover it as part of sort-by-(program, texture);
      not before the profiler says draw state changes are the cost.
- [ ] Texture streaming for large scenes. Much work, little gain at 720×1280.
- [ ] Async asset loading on a dedicated thread. S4 covers hot reload; general streaming is
      a feature, not stability.

## Tests that would be good but are not in the DOD

- [ ] Fuzz the glTF and OBJ loaders with ~1000 malformed files.
- [ ] Fuzz hot reload by writing ~1000 invalid files in a row.
- [ ] 24-hour soak of `mini_fps`, watching RSS.
- [ ] A `clippy::unwrap_used` pass over library targets. The DOD #14 explains why the
      global form is out of scope: 714 raw matches, mostly inside `#[cfg(test)]` modules.

## Findings that are documentation problems, not code problems

- [ ] The master prompt names `giochi-engine-capi/tests/abi_layout_3d.rs`; the file is
      `giochi-engine-3d/tests/abi_layout_3d.rs`. Recorded in the DOD, not "fixed" by moving
      a working test (`docs/STABILITY_DECISIONS.md` §S1.7).
- [ ] The prompt's regression command `giochi-editor --headless-test-play-mode` does not
      exist. The equivalent is `--scene … --frames 8 --play-frames 3`, which is what the
      DOD uses. A dedicated flag would be ergonomic; it is not needed to measure anything.
- [ ] `mini_fps --bench` prints fps and draw calls but not p50/p99, while `ball_balance`
      prints the full percentile set. Unifying them is S5a's measurement work, not a new
      flag to design.

## Things the pack explicitly refuses

- [ ] A new dependency for physics, hashing or measurement. Rapier already provides trimesh
      and convex hulls; the reloader can compare bytes with `std`.
- [ ] Changing the C ABI unless S2–S4 changed a public struct (R6).
- [ ] Refactoring the renderer without a profiler number (R2).
- [ ] Forcing determinism to 0.0. The threshold is 1e-4; a real solver may not be exact and
      the DOD records the measured value instead of demanding a miracle.
