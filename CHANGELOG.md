# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project adheres
to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.1] - 2026-11-08

### Fixed — a fifth audit found two more

This pass read the scene lifecycle at the C boundary and the asset loader.

**A panic inside `giochi_engine_run_scene` wedged the scene handle forever.** `guard`
turns a panic into `GIOCHI_ERROR_INTERNAL`, which is right — what was wrong is everything
after it. The unwind skipped the line that clears the scene's `running` mark, so the handle
refused `giochi_scene_destroy` and every further run, with no reset anywhere; and it skipped
`run_loop`'s stack cleanup, so the adapter stayed on the stack with `on_stack == 1` and the
deferred `on_exit` was never delivered. A `Drop` guard now clears the mark on every exit
path, and `run_loop` catches the panic internally, empties the stack and delivers the exits
**before** recording the error — the message comes last because the user's `on_exit` may
call `giochi_*` functions, and every one of those clears the error on entry. This is the
same class of bug as the run-in-progress mark, fixed the same way.

**An asset that changed size between `stat` and `read` panicked.** `read_asset` copied with
`buffer[..bytes.len()]` after sizing the buffer from a stale `asset_size`, so a file that
*grew* between the two syscalls produced `range end index out of range` — a panic in safe
code, reachable from C (`giochi_asset_read`) and from the Lua host. `read_asset_vec` had the
same race acknowledged only by a `debug_assert`, which the shipped release build compiles
out. Both now verify the read length against the statted size and return `GIOCHI_ERROR_ASSET`
("changed size between stat and read") instead.

### Fixed — a fourth audit found six more

This pass read the parts the first three never opened: the Lua value conversions, the 2D
input snapshot, the core frame loop's clock, and the tuning path of the balance game. It
also started from a test that was failing in this tree, which is how the first one below
surfaced.

**Every run began by simulating up to a quarter-second that never happened.** `run_current`
did not restart `FrameClock`, whose `last` is set in `Engine::new`. So the first `tick()` of
a run reported everything between construction and the loop — window and renderer creation,
opening the mixer, and every asset the caller loaded before starting — and that was
integrated as if it had been a frame. A 3-frame run after 100 ms of set-up reported **13**
fixed steps and 0.131 s of gameplay. On a console that is the ball already rolling, the
player already fallen, and a first `fps()` figure that is fiction. The scene-transition path
already reset the clock for exactly this reason; the start of the run is the same event and
had been missed.

**A Lua script could crash the host with a colour string.** `parse_hex_color` matched on
`digits.len()` — a **byte** count — and then sliced with byte ranges. `"日日"` is 6 bytes
and 2 characters, so it passed the `6 | 8` arm and the slice at `0..2` split a character:
`byte index 2 is not a char boundary`, a Rust panic raised from inside a callback. The
binding's own design note says a script gets errors, not crashes. The string is now checked
to be ASCII hexadecimal before it is sliced, which is what a hex colour is by definition.

**A quit on one thread stopped another thread's engine.** `QUIT_REQUESTED` was a
process-global `AtomicBool` while the engine is thread-local (ADR-012), so two threads each
owning an engine shared one quit flag: `giochi_engine_request_quit` on thread A ended
thread B's loop, and inside a callback `giochi_engine_is_running` answered from the other
thread's state. This is the identical defect `RUN_IN_PROGRESS` had, found the identical way
(a test asserting another thread cannot see this thread's flag) — and the comment beside
`RUN_IN_PROGRESS` had been pointing at it since that fix.

**Held keys and buttons never came back after the window lost focus.** SDL delivers no
`KeyUp` for a key held when focus is taken away, and the second audit had already changed
`apply_event`'s catch-all without noticing that `WindowEvent::FocusLost` fell into it. A
player who alt-tabbed mid-stride kept walking for the rest of the process's life, and a
held mouse button kept reading as a drag. `FocusLost` now clears both snapshot's held state
and deliberately does **not** synthesise release edges: a game bound to "release to jump"
would fire it on an alt-tab, and the engine cannot tell the two apart.

**`BalanceConfig::sanitised` guarded one field of eight, and its own doc was wrong.** It
claimed `fixed_step` "is the one that matters", which a test disproved. `fall_duration: 0.0`
— the obvious spelling of "respawn immediately" — never satisfies the `fall_timer > 0.0`
that decrements it, so the ball never respawned, stayed off the board, and `falls` climbed
**once per frame** (77 and counting after one idle second). `ball_radius: 0.0` divided the
rolling angle to infinity and made the spin quaternion `NaN`, which draws an invisible
ball. `platform_size: -4.0` made the ball "fallen" while still at the centre — no round
ever started — and `0.0` made `edge_proximity` a `0/0`. A `NaN` `damping` poisoned the
velocity and then the position. Every field is now brought into a runnable range, with a
warning per substitution, and `Ball::has_fallen`/`edge_proximity` no longer trust a config
that did not come through `sanitised`.

**The light let a `NaN` ambient through.** `DirectionalLight::sanitised` used
`ambient.clamp(0.0, 1.0)`, and `f32::clamp` returns `self` for `NaN` — unlike `f32::max`,
which is why `specular` and `shininess` were already safe. The `NaN` reached
`u_ambient_strength`, `gl_FragColor` became `NaN` for every fragment, and the frame went
black: exactly the failure the function documents itself as preventing. It is reachable from
C, where `GiochiDirectionalLight` has no constructor and a forgotten field is whatever the
stack held. All three scalars are now made finite, with a finite ceiling on `specular` and
on the exponent.

Also repaired: `giochi-engine-lua/tests/lua_smoke.rs` asserted that `quit()` stops the run
in **at least 3 display frames**, which is not a property of the code — `update` runs per
fixed step and a frame absorbs every step its elapsed time covers, including the first
frame's work. It failed on this machine at 2 frames, where the first `present` cost ~120 ms.
It now asserts what the contract says: the quit condition fired (`update` ran at least 3
times) and the run stopped far short of the 500-frame limit.

### Added — `giochi_renderer3d_set_light`

Phase 13 introduced `BuiltinShader::Phong`, whose one input is a light. Part II shipped
without a function to set that light, so a C consumer got the engine's default sun and had
no way to move it: the shader was advertised, the struct was in `giochi.h` and the contract,
and the function that connects them did not exist. `docs/C_ABI_CONTRACT.md` had already
fixed the C form in §9.4a, so this is the implementation of something already specified
rather than a signature invented now.

```c
int32_t giochi_renderer3d_set_light(GiochiEngine3D* engine,
                                    const GiochiDirectionalLight* light);
```

The light is engine state, not a per-draw argument, for the same reason `draw_mesh` keeps
`GiochiColor3D` by value: GLES 2.0 uniform limits make it cheaper to upload once per change.
Sanitising happens on entry exactly as the Rust `RenderContext3D::set_light` does, so a
caller cannot produce a black frame by leaving `shininess` at zero (`pow(x, 0)` is 1
everywhere) or by passing a zero-length direction (which normalises to NaN).

`GiochiDirectionalLight` was **also missing from the generated header** even though it was
in the contract and `giochi.h` claimed to be the contract. `cbindgen.toml` did not list
`giochi-engine-3d` in `include`, so the type was never emitted. Both are covered now:
`c_smoke_3d.c` static-asserts `sizeof(GiochiDirectionalLight) == 88` (and 28/32 for
`GiochiVec3`/`GiochiColor3D`), so the *C compiler* is what checks the layout in §9, and
`python_smoke_3d.py` passes one across the boundary from a second FFI implementation.
Verified to fail against the old header: it does not compile.

### Fixed — a CI failure that was a real bug, not a flake

`unknown_handles_are_reported_not_crashing` started failing on CI and passing locally,
which is the shape of a flake and was not one. `RUN_IN_PROGRESS` — the flag that refuses a
re-entrant `giochi_engine_run` — was a **process-global** atomic while the engine itself is
thread-local (ADR-012). Two consequences:

* a run on thread A made thread B's `giochi_engine_run_scene` fail with "already running on
  this thread" — false, and nothing the caller could act on;
* the capi's own tests raced each other, because one test entering a run flipped the flag
  while a parallel test asserted an unrelated error code. That is how it surfaced.

The flag is now thread-local, and the set/reset pair became a scope guard: a panic inside
the loop used to unwind past the reset and leave the thread permanently "running", after
which every later run and step was refused as re-entrant with no way to recover. Two
regression tests cover both — a run on another thread, and a panic inside one — and both
were verified to fail against the old code.

### Fixed — a second audit found four more

**A 2D engine and a 3D engine could not coexist, and said so in the wrong language.** Both
create paths ask SDL for video initialisation, and SDL allows one `EventPump` per process,
so whichever came second failed with `an `EventPump` instance is already alive` — an
internal type the caller has never used, and no statement of the rule that applies
(ADR-037: a renderer is picked, not mixed). Each side now refuses with a message naming the
rule and the call that would fix it. Asserted from C in both directions.

**The 3D C ABI dereferenced any non-NULL pointer.** `giochi_engine3d_begin_frame`,
`_end_frame`, `giochi_renderer3d_clear` and `_draw_mesh` checked only for NULL, under a
`SAFETY` comment claiming the pointer was "asserted to be the thread's current engine by the
same rule the other functions use" — a check that did not exist. Because `create` refuses a
second engine per thread there is exactly one valid pointer at any moment, so it can be
compared: a stale or foreign pointer now yields `GIOCHI_ERROR_INVALID_HANDLE` instead of a
use-after-free.

**`release_all` reported success when a resource had leaked.** It answered "are the
registries empty", which is a different question from "were the GL objects deleted": a
caller holding an `Rc` clone made `destroy_*` log a leak while the entry disappeared
anyway. It now counts the references first and returns `false` when any will leak — the
behaviour its own documentation already promised.

**A mistyped argument in the 3D examples meant "no frame limit".** Unknown flags were
ignored and an unparseable number became `None`, so `--frames=3oo` produced an unattended
run that never terminates — a CI job timeout rather than a message. `--frames`,
`--trace`, `--obj` and `--quadro` now reject a missing or malformed value, unknown flags
are an error, and the message lists the flags actually accepted instead of pointing at a
`--help` that does not exist.

### Fixed — an audit found four real defects

**Every 3D mesh and shader leaked its GL objects.** `destroy_mesh` and `destroy_shader`
read the resource with `HandleRegistry::get`, which **clones** the `Rc`, and then asked
`Rc::try_unwrap` for ownership — with a strong count of two that can never succeed. The
registry entry disappeared, the VAO, VBO, EBO and program stayed alive in the driver, and
the only trace was a `log::warn!` that no example initialises a logger to show. The
examples printing `released: 0 meshes and 0 shaders left` were counting registry entries,
so they read as confirmation of a cleanup that had not happened. `release_all` had the
same bug, being built on the same two calls. A new `HandleRegistry::take` removes and
returns the sole reference; `remove` is gone, because it is the shape that invites the
mistake. Regression test: `destroying_a_resource_really_deletes_its_gl_objects` asks the
driver with `glIsBuffer`/`glIsProgram` before and after, and was verified to fail against
the old code.

**`destroy_sound` freed a `Mix_Chunk` that could still be playing.** SDL_mixer keeps the
chunk pointer for as long as a channel plays it, so dropping it mid-playback is a
use-after-free in the mixer thread — garbage audio, then a crash. `destroy_music` had
always halted first; the sound path had been missed. It now halts the effect channels
first, and the doc explains why that has to be all of them: the `sdl2` crate cannot say
*which* chunk a channel holds.

**A `fixed_step` of zero or less hung the balance game forever.** `BalanceConfig`'s fields
are public and `BalanceGame::update` loops `while accumulator >= fixed_step`, subtracting
the step: zero never progresses and a negative one raises the accumulator, so the game
froze with no error and no frame. `BalanceConfig::sanitised` clamps it to at least 1 ms and
`BalanceGame::new` applies it.

**`draw_line` bypassed the coordinate guard.** It converted endpoints with `as i32` while
every other draw path in the module goes through `to_sdl_coord`, which maps a non-finite
value to 0 and clamps the rest. Rust's `as` is saturating, so an infinite endpoint became
`i32::MAX`/`i32::MIN` and was handed to SDL's integer line clipping. (`NaN` mapped to 0
either way, so the comment in the fix says so rather than overstating the case.)

Also corrected: `destroy_music`'s doc claimed playback stopped only for the track being
destroyed. It always stops, because SDL_mixer exposes no way to ask which `Mix_Music` is
playing; the code was right and the sentence was not.

## [0.2.0] - 2026-11-07

3D arrives, and with it part II of the C ABI. Nothing in part I changes: a 2D program
built against 0.1.x keeps working, and `giochi_abi_version()` now says 2.

### Added — part II of the C ABI (`GIOCHI_ABI_VERSION 2`)

The 3D engine is an **opaque pointer the caller owns**, unlike the 2D singleton, because it
holds a GL context whose layout has no C representation and changes between releases
(ADR-039).

- `giochi_engine3d_create` / `_destroy` / `_begin_frame` / `_end_frame`.
- `giochi_camera3d_new` / `_look_at`, `giochi_transform3d_identity`,
  `giochi_transform3d_from_translation`. The last two exist because a zeroed
  `GiochiTransform3D` has **zero scale** and draws nothing — the trap a C caller walks into
  and a Rust one cannot.
- `giochi_mesh_create_cube`, `giochi_mesh_load_obj`, `giochi_mesh_destroy`,
  `giochi_shader_builtin`, `giochi_shader_load`, `giochi_shader_destroy`.
  The four that take no engine act on the most recently created engine on the calling
  thread; a second engine on the same thread is refused rather than allowed to retarget
  them (ADR-041).
- `giochi_renderer3d_clear`, `giochi_renderer3d_draw_mesh`.
- `GiochiWindowConfig`, `GiochiVec3`, `GiochiQuat`, `GiochiColor3D`,
  `GiochiTransform3D`, `GiochiCamera3D` in `giochi.h`, with their documented sizes.
- `GIOCHI_SHADER_FLAT`, `GIOCHI_SHADER_TEXTURED`, `GIOCHI_SHADER_PHONG`.

`giochi_renderer3d_draw_mesh` takes a `GiochiColor3D` the contract draft did not have.
A mesh in part II is geometry with no material, so without it every draw would take the
background colour and be invisible. Part II had not shipped, which is exactly when a
contract is allowed to be fixed rather than worked around (contract §10.7).

### Added — 3D rendering (`giochi-engine-3d`)

- GLES 2.0 over EGL, 3.0 opt-in, with the requested-version fix for drivers that ignore
  `SDL_GL_MAJOR_VERSION` (ADR-038).
- `Vec3`, `Quat`, `Mat4`, `Transform3D`, `Camera3D`, `CameraController`.
- Mesh pipeline: cube, plane, UV sphere and OBJ loading, with and without VAOs.
- Directional Blinn-Phong lighting, `DirectionalLight`, `Mat4::normal_matrix`.
- `cube_demo`, `mini_fps`, `ball_balance` — the last one built to measure input latency on
  a real console.

### Changed

- `GIOCHI_ABI_VERSION` is `2`. The rule for a consumer is "at least the version I was
  built for", never equality; `c_smoke.c` asserts it that way so a part I program does not
  break the day a part is added.
- `README.md` and `docs/C_ABI.md` describe both parts.
- `scripts/hardware-check.sh` deploys the 3D examples and the 3D C consumer, and refuses
  to run at all on a battery that cannot carry a session — a brown-out on the PX30 wipes
  the rootfs, which is where the deploy lives.

### Fixed

- `Mat4::transform_point` does not divide by `w` (it mirrors `glam`'s `transform_point3`,
  which asserts an affine bottom row), so on a projection matrix it returned clip
  coordinates that look like ordinary numbers. `Mat4::project_point` divides and returns
  `None` behind the eye; `transform_point` is now documented as affine-only.
- The balance game measured frame work *after* the buffer swap, so every frame was
  charged the vsync wait and the hitch indicator reported the panel refreshing instead of
  the game stuttering. Real cost on the Mali-G31: 1.0 ms p50, 3.8 ms p99.
- Its two latency indicators were outside the camera's field of view on the console's 4:3
  panel, which is narrower than the 16:9 window they were placed against.
- Phase 13's acceptance criterion was unobservable: `mini_fps` drew 2 m obstacles from a
  1.7 m eye, so no top face was ever on screen. It now draws low crates and reports the
  count.

### Added — Phase 13, directional lighting

- `DirectionalLight`: one light per frame, held by the renderer. `#[repr(C)]`, primitive
  members only, with the reserved tail rule R3 requires, so phase 14 can expose it
  without a redesign (`docs/C_ABI_CONTRACT.md` §9.4a pins it at 88 bytes). `set_light`
  sanitises on assignment: a zero-length direction becomes a warning and a fallback
  instead of a NaN the driver renders as black, and `shininess` is floored at 1 because
  `pow(x, 0)` paints a disc rather than a highlight.
- `shaders/phong.vert` / `phong.frag`: GLSL ES 1.00 Blinn-Phong — ambient, diffuse,
  specular, world-space varyings, half-vector specular.
- `Mat4::normal_matrix()`: the inverse-transpose of the upper 3×3, computed on the CPU
  because GLSL ES 1.00 has no `inverse()`. Without it a non-uniformly scaled object
  shades as though it had a different shape.
- `BuiltinShader::Phong` and `BuiltinShader::reads_lighting`.
- `giochi-engine-3d/tests/lighting.rs`: 7 tests that read the framebuffer back and assert
  the shading rather than the call sequence.
- `mini_fps` now draws with Phong and an explicit sun; it prints the light parameters.
- `docs/3D.md` §5.1 "Lighting": the direction convention, the normal matrix, a table of
  every uniform with the symptom of getting it wrong, and how the tests verify it.
- ADR-040 records the three choices that are not obvious: one light per frame, `direction`
  follows the photons, and a CPU normal matrix.

### Fixed — the balance game on the console

Two bugs that only a 640×480 panel could show, both found by running the game on the
PX30 rather than on a desktop.

- **The frame-work clock was read after the swap.** KMSDRM's `end_frame` blocks until
  vsync, so every frame was charged the 16.7 ms it waited: the hitch indicator sat
  permanently on its own threshold and the reported percentiles described the display
  rather than the game. Measured cost on the Mali-G31 is 1.0 ms p50 / 3.8 ms p99, against
  the 16.7 ms it used to claim. `scripts/hardware-check.sh` now fails if the reported p99
  is anywhere near a refresh interval.
- **Both latency indicators were outside the camera's field of view.** The console panel
  is 4:3 and therefore narrower than a 1280×720 window; the hitch cube projected to
  ndc −1.2 and the input cube left the frame at full stick deflection. The camera and the
  marker anchors moved into `game::BalanceView`, and `every_marker_stays_on_screen`
  projects them at both aspect ratios across the whole stick range.
- The `--trace` field is now called `work`, not `dt`, because it is the game's own cost
  and not a frame interval.

### Added — `Mat4::project_point`

A correct projection: divides by `w` and returns `None` for a point behind the eye.
`Mat4::transform_point` does not divide — it mirrors `glam`'s `transform_point3`, which
asserts an affine bottom row — so on a projection matrix it returns clip coordinates that
look like ordinary numbers. That is how the off-screen markers went unnoticed; the
affine-only restriction is now documented on `transform_point`.

### Added — `giochi_engine_3d::game::BalanceView`

The balance game's camera and marker layout, extracted from the example so that "are the
indicators on screen" became a test instead of an opinion.

### Fixed — the phase-13 acceptance criterion could not be observed

The criterion is that a cube's top face reads brighter than a side face. It could not be,
and nothing said so: `mini_fps` drew 2-metre obstacles from a walker whose eyes are at
1.7 m, so every top face sat above the camera and the player only ever saw sides. The
lighting tests asserted the behaviour on a cube they placed themselves, which is a good
unit test and not the acceptance check.

- `mini_fps` now also draws 24 **low crates** (0.6 m, between the obstacles) and reports the
  count, so a top face is genuinely on screen while walking and CI can assert it is.
- `a_crates_top_face_reads_brighter_than_its_side_the_phase_13_acceptance_check` measures
  it with the example's own numbers — crate size, eye height, and the exact light
  `mini_fps` sets — projecting a point on the top face and one on the front face with
  `Mat4::project_point` and sampling a patch around each, rather than guessing where the
  faces landed. Its first version had the two patches 6 px apart, so both sampled the seam
  and reported a blend; the framing is now asserted (`separation > 2 * patch + 2`) and the
  threshold is a ratio (3.3× measured against 1.5× required) rather than a difference.

### Fixed — 3D by the lighting work

- `read_pixels`-based assertions initially passed for the wrong reason: the sample bands
  sat outside the sphere's silhouette, so they compared "lit surface" against "black
  background". The probe is now a patch that is provably inside the silhouette, and the
  camera distance is documented with the arithmetic.

### Added — `ball_balance`, a game for judging input latency

- `giochi-engine-3d::game::BalanceGame`: the balance-board simulation, with no GL in it.
  2D in the platform's own tangent basis, sub-stepped to 1/120 s so a slow frame gives
  the same result as two fast ones. 13 tests pin the direction of each axis, the edge,
  the respawn and the sub-stepping.
- `mesh::primitives::sphere(radius, rings, segments)`: a UV sphere generated in code, so
  the ball costs no asset to deploy. 5 tests, including outward winding on every triangle.
- `examples/ball_balance.rs`: the playable game. A blue cube follows the stick with no
  physics at all, the platform one frame behind it, then the ball — so a delay can be
  attributed instead of merely noticed. A hitch-indicator cube turns amber and red as the
  frame budget is passed. `--frames`/`--trace`/`--auto`/`--quadro`.
- `RenderContext3D::create_sphere_mesh`.
- CI runs the game headless and asserts the ball finished where the script pushed it, plus
  a capture that must contain both a ball and a platform by colour.
- `docs/3D.md` §7: how to read the three indicators and the `--trace` output.

### Fixed — 3D by the balance game

- The X axis of a tilt was inverted: a rotation about +Z by a positive angle raises the
  +X side, so rolling right needs the negative angle. The Z axis was already correct,
  which is what made the asymmetry easy to write and hard to see.
- `ball_balance` read the drawable size from the render context, which only learns it
  inside `begin_frame`; the first frame got the initial `(1, 1)` and `--quadro` wrote a
  1×1 BMP. It asks the window instead.

### Added — 3D, phase 12: first-person camera and `mini_fps`

- `CameraController`: WASD, Q/E, sprint, relative mouse look. Yaw and pitch are stored as
  clamped angles rather than an integrated quaternion, which is what stops the view
  drifting over a long session.
- `AxisInput` decouples the movement axes from the keyboard, so a scripted path drives
  the same code the manual check does. This is not a convenience: `KeyboardState` is only
  populated by real SDL events, and the first `--auto` implementation walked nowhere while
  still printing 60.0 fps and exiting successfully.
- `examples/mini_fps.rs`: textured floor, 24 tilted obstacle cubes, wireframe toggle,
  fps once per second, `--frames`/`--auto`/`--quadro`.
- `RenderContext3D::set_wireframe` and `Platform::sdl_mouse`; `KEY_E`, `KEY_Q` and
  `KEY_LEFT_SHIFT` added to the 2D input module (additive).

### Verified — phase 12 on real hardware (PX30, Mali-G31)

- 300 frames with the frame cap: **60.0 fps** sustained, no GL error.
- 1200 frames uncapped: **57.1 fps** in 21 s, position walking from (0, 1.7, 8) to
  (38.2, 1.7, -101.0).
- A frame captured on the device shows the sky, the floor and the obstacles in depth.

### Verified — phases 10 and 11 on real hardware

Confirmed on the Rockchip PX30 (Mali-G31/Panfrost, Mesa 24.0.0), which is the device
these phases exist for:

- 300 frames at GLES 2 and 200 at GLES 3, no GL error in either;
- the no-VAO path runs (300 frames, no error), which is the path GLES 2.0 embedded
  devices may take and no desktop can exercise;
- 1200 frames in 21 s (**57.1 fps**), 19.9 MB resident;
- a frame captured **on the device** shows the cube 209x209 px, centred at (322,238) on
  a 640x480 panel whose centre is (320,240) — the camera took its aspect from the real
  drawable, not from an assumed 16:9;
- the OBJ loader parses on the device's libc, and a missing file reports its path.

### Added — 3D, phase 11: mesh pipeline

- `mesh`: `Vertex` (`#[repr(C)]` + `Pod`, 32 bytes, interleaved position/normal/uv),
  `Mesh` with a VBO, an EBO and an **optional** vertex array object, plus procedural
  `cube`/`plane` and a `tobj`-based OBJ loader.
- `shader`: `Program` with full `glGetShaderInfoLog`/`glGetProgramInfoLog` on failure,
  cached uniform locations, and `BuiltinShader::{Flat, Textured}` read from
  `giochi-engine-3d/shaders/*.vert|frag` with `include_str!` (R10).
- `render3d`: `create_mesh`/`destroy_mesh`, `create_builtin_shader`/`load_shader`/
  `destroy_shader`, `draw_mesh`, `draw_mesh_colored` and `release_all`.
- `examples/cube_demo.rs` draws a tumbling cube, with `--quadro` to capture a BMP and
  `--obj` to draw a model file instead — the loader is the only code here that parses
  untrusted input, so it needs a path that actually runs on a console.

### Fixed — two cube faces were wound backwards

`+X` and `-X` wound clockwise seen from outside, so with culling on the cube looked
hollow. The test that checked only the first face passed; the one that checks all six
found both. There is now a test per face, and it compares the geometric normal against
the declared one.

### Fixed — the OBJ normal pass underflowed on multi-object files

The derived-normals pass walked the whole index list instead of only the indices just
appended, so a second object subtracted a `base` larger than its own indices. Debug
builds panicked; release builds computed normals for the wrong vertices.

### Added — the VAO fallback is tested

`glGenVertexArrays` is core in GLES 3.0 only; GLES 2.0 needs
`GL_OES_vertex_array_object` or nothing. No developer machine lacks VAOs, so
`GIOCHI_FORCE_NO_VAO=1` forces the per-draw attribute path and CI runs the pipeline tests
and the example through it. Both paths light the same pixels.

### Added — CI exercises the 3D pipeline

Under `SDL_VIDEODRIVER=offscreen` (the `dummy` driver has no OpenGL): shader compilation,
a rasterised cube, the no-VAO fallback, and a frame capture whose BMP dimensions are
asserted so an all-black image cannot pass.

### Added — 3D, phase 10: maths

- `math3d::{Vec3, Quat, Mat4, Transform3D, Camera3D}`, all `#[repr(C)]`, with `glam` as
  an implementation detail that never appears in a public field.
- `Mat4` is column-major so `as_ptr()` goes straight to `glUniformMatrix4fv`. The
  constructors return `Err` for a degenerate frustum instead of producing a matrix that
  renders nothing.
- `Camera3D::aspect == 0.0` means *unresolved*, and projection fails rather than
  guessing 16:9 — on a console the panel is the truth (ADR-035).
- `RenderContext3D::begin_frame` now takes the window and sets the GL viewport from the
  **real drawable size**, exposing `drawable_size()`/`aspect()` for the camera.
- 95 tests in the crate (was 27), including 9 layout assertions that lock the documented
  struct sizes and prove no SIMD type leaked into the surface.

### Fixed — the projection used the Vulkan depth range

`glam::perspective_rh` maps depth to `0..1`; OpenGL ES expects `-1..1`. The first
version used the wrong one: every matrix looked plausible, no GL error was raised, and
geometry would only break as it approached the near plane. Caught by a test asserting
the NDC depth at the near and far planes; `math3d` now uses the `_gl` variants and
`tests/math3d_integration.rs` pins the range.

### Fixed — the viewport was never set

`begin_frame` cleared and presented without calling `glViewport`, so the viewport stayed
at whatever the driver defaulted to. On KMSDRM the drawable is the panel and not the
requested size (ADR-035), so a scene would have been drawn into a corner and clipped.

### Added — 3D, phase 9 (extension v2.1)

3D arrives as a **separate crate**, not a render backend: `giochi-engine-3d` drives
GLES through EGL while `giochi-engine` keeps `sdl2::render`. A game picks one. ADR-037
supersedes ADR-005 for the 3D half only.

- `context`: GLES context creation through SDL2 (`configure_attributes` before the
  window, because `SDL_GL_*` is process-global), `glow` loading, swap, explicit
  `GlState` applied every frame.
- `render3d`: `RenderContext3D` with `begin_frame`/`end_frame`, `take_gl_error` and
  `read_pixels` for asserting on pixels instead of on the absence of errors.
- `math3d`: `Color3D`, `#[repr(C)]`, 32 bytes, NaN-safe clamping.
- `docs/C_ABI_CONTRACT.md` part II (§8–§12): the complete 3D type set, ABI version 2
  specified, phase 14 function table declared — written before the code, per R11.
- `docs/3D.md`: driver table, troubleshooting, per-phase state.
- ADR-037 (separate crate), ADR-038 (GLES 2.0 default, 3.0 opt-in), ADR-039 (opaque
  engine pointer in the 3D ABI).
- 2D crate additions are strictly additive: `Window::new_opengl`, `Window::sdl_window`,
  `Platform::video_subsystem`. No existing signature changed.

**Validated on the PX30** (Mali-G31/Panfrost, Mesa 24.0.0): 300 frames with no GL
error at either GLES version, 1200 frames in 20.57 s (58.3 fps).

### Fixed — GLES version request ignored by SDL2/KMSDRM

Asking for a GLES 2.0 context on this driver produced `OpenGL ES 3.1` regardless.
Reporting the driver's answer made `supports_gles3()` untrustworthy: a caller that
requested the 2.0 floor could have a 3.0-only path activate. Now a 2.0 request always
reports 2.0, and a 3.0 request reports 3.0 only when the driver confirms it, otherwise
2.0 with a warning. `docs/KMSDRM.md` §8.8.

### Changed — CI runs the 3D path

`SDL_VIDEODRIVER=dummy` has no OpenGL; `offscreen` has the full EGL/GLES stack. The
smoke job runs the 3D integration tests and the example under `offscreen`, plus one run
with `GIOCHI_GL_VERSION=3` to exercise the opt-in path.

### Changed — release tooling

- `scripts/verify-release-artifact.sh` unpacks a release archive and *runs* it: layout,
  version stated by the header, exported ABI symbols, the demo, the Lua game and a C
  consumer compiled against the archive's own `include/` and `lib/` — then all three
  again with the PNGs deleted, which is the codec-less console case. `release.yml` runs
  it on the packaged x86_64 archive before attaching it, and checks the ARM64 archive's
  layout and ELF architecture, so "well-formed tarball" can no longer stand in for
  "working tarball".
- CI lints the workflow files with `actionlint` (pinned to 1.7.7). Every job there runs
  shell that `shellcheck` never sees, and a YAML mistake otherwise only shows up as a
  red X after the push that was meant to prove the change works.

## [0.1.1] - 2026-11-07

Everything here came from the first run on real console hardware. The 0.1.0 release sat
in draft precisely because these defects would have shipped in it.

### Changed — release tooling

- `scripts/check-version.sh` verifies that the version in `Cargo.toml` is echoed by
  the member pins, the Rust constants, the generated header, the docs, the changelog
  and `Cargo.lock`. It runs in `gates.sh` and in CI, so a bump cannot leave a release
  whose artefacts disagree about what they are.
- `CONTRIBUTING.md` documents the release procedure and the six files a bump touches.

### Documentation — hardware traps corrected

- §8.1 of [docs/KMSDRM.md](docs/KMSDRM.md) prescribed `chmod -x` and `kill -TERM` on
  `giochi-init`. Both were wrong and cost three failed runs:
  - BusyBox applies the umask to a symbolic mode and this image runs `umask 0077`, so
    `chmod -x` produced `-rw-r-xr-x` — still executable, and since everything runs as
    root `[ -x ]` stayed true, so the supervisor respawned the launcher mid-test. Use
    `chmod 0644` and verify.
  - `/etc/inittab` registers the supervisor as `::respawn:`, so killing it only makes
    BusyBox start a new one, and its launcher child is orphaned with DRM master still
    open. Killing the launcher once is enough; the supervisor treats SIGTERM as a
    deliberate exit and zeroes its crash quota.
  - SIGTERM to the launcher is honoured *sometimes*; its `SigCgt` includes signal 15.
    Escalate to SIGKILL after a grace period.
- §8.7 records the run that proves the BMP fallback is load-bearing on this image:
  `player.png` fails with "Unsupported image format" and `player.bmp` loads.

### Verified on real hardware

The first run on an actual console — a Rockchip PX30 handheld (Buildroot 2024.02,
kernel 6.12, aarch64, 640x480 DSI panel, glibc 2.38, no X11, no Wayland) — found four
portability bugs that neither the desktop suite nor CI could have caught. All four are
fixed; the measurements are in [docs/KMSDRM.md](docs/KMSDRM.md) §8.

### Fixed — hardware validation

- **A codec-less `libSDL2_image` killed texture loading entirely.** On this image
  `libSDL2_image` links only `libSDL2`, `libm` and `libc`: PNG and JPEG are compiled as
  "not supported" stubs. Because `IMG_Init` fails when *any* requested flag is
  unavailable, asking for `PNG | JPG` made the engine declare the whole subsystem dead —
  even though BMP, TGA, PNM and QOI need no flags and worked all along. The subsystem is
  now opened with no flags and each optional decoder is probed separately, warning once
  per missing format. ADR-034.
- **KMSDRM is fullscreen-only, and the log said otherwise.** Requesting 1280x720 on a
  640x480 panel opens 640x480 with no error and no scaling, but startup printed the
  *configured* size — the worst pairing for debugging a cropped scene. The engine now
  reports the real drawable and logs `requested WxH, backend opened WxH` when they
  differ. ADR-035.
- **The driver name is `KMSDRM`, in capitals.** `x11` and `wayland` are lower case, so
  the natural `strcmp(driver, "kmsdrm")` never matched on a console and the mistake was
  invisible on a desktop. Documented, and the C smoke test now compares case-insensitively.
- **The ARM64 sysroot cross build leaked host packages.** Two independent omissions:
  the CMake toolchain file set `CMAKE_SYSROOT` without `CMAKE_FIND_ROOT_PATH`, and
  pkg-config still answered with host x86_64 `.pc` files, so SDL2 enabled
  X11/Wayland/EGL/PulseAudio/sndio and the aarch64 linker was handed host sonames. Both
  are set now (`cmake/aarch64-linux-toolchain.cmake`, `scripts/build-aarch64.sh`).

### Added — hardware validation

- Every asset is now committed twice, as `player.png`/`player.bmp` and
  `checker.png`/`checker.bmp`, with identical pixels. On the R35S a plain checkout
  previously could not run its own demo at all. The demo, the Lua reference game and
  both ABI smoke tests fall back to the BMP when a build has no PNG decoder;
  `scripts/png-to-bmp.py` regenerates the twins (`--check` compares bytes and
  `scripts/gates.sh` runs it) and `giochi-engine/tests/asset_formats.rs` decodes both
  through SDL. ADR-036.
- `scripts/hardware-check.sh` repeats the whole hardware run — cross-build, deploy,
  release DRM master, demo, frame capture, Lua game, C consumer, restore the console —
  so validating the next unit does not need the archaeology this section documents.
- `docs/KMSDRM.md` §8 records what the run actually measured, including the two traps
  that cost the most time: a shipped console's own launcher already holds DRM master
  (so the "stop your X server" message is misleading there), and the supervisor gives up
  permanently after five crashes in a minute, leaving the screen black with no recovery
  short of signalling `giochi-init`.
- Measured on that device: 300 demo frames at 59.4 fps, a 1200-frame Lua game at
  59.5 fps with **zero live textures** afterwards, `c_smoke: OK (abi 1, engine 0.1.0)`,
  and 21.9 MB resident.

## [0.1.0] - 2026-11-07

The first release: a working 2D engine in Rust on SDL2 whose public surface is
C-shaped end to end, so a C, C++, Lua, Python, Go, Zig or C# binding is a wrapper
rather than a rewrite. Runs on `x86_64-unknown-linux-gnu` and
`aarch64-unknown-linux-gnu`; auto-selects KMSDRM on a bare Linux console and
falls back to the desktop driver elsewhere.

### Verified on the real runners

`ci.yml`, `cross-compile.yml` and `release.yml` are green on GitHub, including the
ARM64 link and the packaged `v0.1.0` draft release with both archives.

### Fixed — CI on the runner, after the workflows started working

- `ubuntu-latest` moved from 24.04 to 26.04, and the ARM64 multiarch recipe broke with
  it: `libcrypt-dev:amd64 Breaks libcrypt-dev:arm64 (!= 1:4.5.1-1)`, because the ports
  suite was hard-coded to `noble` while the host had become something newer. The recipe
  now lives in `scripts/install-arm64-deps.sh`, derives the suite from the machine's
  `/etc/os-release`, and the two jobs that need matching indexes are pinned to
  `ubuntu-24.04` rather than tracking a moving target.

### Fixed — false minimum Rust version

- `rust-version` declared **1.75**, which no combination of the pinned dependencies can
  satisfy: `mlua 0.10` requires 1.79 and `size_of` in the prelude requires 1.80. Probed
  with `cargo +1.79 build --workspace` (fails) and `cargo +1.82 build --workspace`
  (passes), so the declared floor is now **1.82** and the README says the same. Anyone on
  an older toolchain gets cargo's clear message instead of a confusing failure inside a
  dependency. `clippy.toml` carried the same stale number and is aligned. ADR-033.
- Correcting the MSRV immediately unlocked a lint that had been suppressed by the false
  1.75 floor: `read_asset_vec` used the pre-`Result::inspect` idiom (stable since 1.76).
  It is now `inspect`, so the length check reads as a check rather than a rewrite.

### Fixed — CI never passed

Every workflow run on `main` failed, from phase 6 onwards. Four causes:

- **The X11 headers were missing.** SDL2's bundled CMake configure enables the X11
  video backend whenever it can see any X11 library, then requires `Xext.h` and the
  rest. The apt list never installed them; the build worked locally only because a
  desktop dev machine has them.
- **The host SDL2 satellites were missing.** `bundled` builds core SDL2 only, so
  linking the `cdylib` failed with `cannot find -lSDL2_mixer`. This was already
  documented for ARM64 and had never been tested on x86_64.
- **The package list was duplicated across four jobs**, which is how both omissions
  survived. Replaced by `scripts/install-deps.sh`, a single list.
- **Three workflows had been replaced by cut-down versions**, and the `git add -A`
  commits of phases 7 and 8 swept them in: `ci.yml` without the `ffi`, `smoke` and
  cross-check jobs, `cross-compile.yml` with a single job, `release.yml` ARM64-only.
  Restored to what the documentation describes.

- `tests/api_guide.rs` asserted that `play_sound(0, 0)` returns `INVALID_HANDLE`, but a
  machine without a sound card returns `AUDIO` (ADR-021). It passed on a desktop and
  would have failed on every runner; it now accepts both documented codes.

Verified by mirroring the entire CI matrix in a clean `ubuntu:24.04` container: 16/16
steps pass (fmt, clippy, build, 209 tests, doc, header, C and Python FFI, demo, five
examples, `mini_game.lua`).

### Added — post-release audit

- `giochi-engine/tests/c_shape.rs`: the C-shaped rules (R1, R3) are now checked
  mechanically over the whole crate source and run as part of `cargo test` — every
  public type `#[repr(C)]`, every `#[repr(C)]` struct ending in `_reserved: [u32; N ≥ 4]`,
  no public field holding `Rc`/`Arc`/`Box`/`String`/`Vec`/`HashMap`, and no public
  function with generic parameters or an explicit lifetime. The one deliberate
  exception (`GiochiError`, required by R4) is an allow-list entry that must cite the
  rule that allows it. ADR-032.

### Fixed — post-release audit

- `FrameClock` was public without `#[repr(C)]` **and** without `_reserved`, violating
  R1 and R3; `SceneManager` had `_reserved` but no `#[repr(C)]`, violating R1. Both are
  `#[repr(C)]` now, and `FrameClock` gained its reserved tail.
- `giochi-engine/src/lib.rs` still announced that unimplemented functions panic with
  `todo!("fase N")`, which stopped being true at phase 5.
- `assets/manager.rs` and `render/texture.rs` referred to work "landing in phase 7" and
  "the ResourceManager's job in phase 4"; both now describe the finished behaviour.

### Added — phase 8 (Lua binding)

- **`giochi-engine-lua`** is a working binding, not a stub: a `giochi` module for Lua
  5.4 (mLua, vendored) with `giochi.app`, the frame callbacks, textures, sounds, music,
  input, drawing, the camera and named key/mouse/axis/button tables. The crate is
  `#![forbid(unsafe_code)]`, like the core.
- **Capabilities instead of a global engine** (ADR-030): `update(time, input, audio)`
  and `render(gfx)` receive exactly what the engine lends them, and the values stop
  working when the callback returns, so a script cannot outlive its borrow. The
  texture registry is `Rc`-shared, which is why `Texture` can free itself.
- **`Texture` freed by `Drop` *and* by `:destroy()`** (ADR-031); `Sound`/`Music` are
  app-scoped because the mixer belongs to the engine. Both rules are documented and
  asserted — `app:texture_count() == 0` at the end of the reference game.
- **Errors end the run**: a Lua error inside a callback stops the loop and is
  re-raised by `app:run`, so a broken script never leaves a window on screen.
- `giochi-demo lua <script> [args...]`, with `arg[0]`/`arg[1..]` per Lua convention.
- `giochi-engine-lua/examples/mini_game.lua`: a complete space shooter (movement,
  shooting, drones, collisions, lives, game over, synthesised beep) in ~250 lines,
  with `--auto`, `--frames` and test-only `--lives`/`--ram`/`--spawn`/`--speed` flags.
  It returns a summary table so tests assert on the outcome instead of parsing stdout.
- `docs/LUA_BINDING.md`: the module reference, the capability table, handle lifetimes,
  design notes and the test matrix.
- `tests/lua_smoke.rs` and `tests/mini_game_script.rs`; a Lua smoke run in CI and in
  the release workflow, which now ships `lua/mini_game.lua` with the binary.

### Added — phase 7 (C ABI)

- **`libgiochi`**: `giochi-engine-capi` is now a real `cdylib` + `staticlib` exposing
  54 functions — lifecycle, statistics, textures, drawing, camera, keyboard, mouse,
  gamepad, sound, music, asset reading and scene callbacks. Every fallible call
  returns an `int32_t` code; handles are opaque `uint64_t` where `0` is never valid.
- **`giochi.h`**, generated by `cbindgen` from the Rust crate and never edited by
  hand. `scripts/gen-header.sh --check` fails when the committed copy is stale.
- **Thread-local error slot** with `giochi_get_last_error()`, cleared on entry to
  every call so a successful call always leaves it empty.
- **Panic guard** on every export: an unwind becomes `GIOCHI_ERROR_INTERNAL` with
  its message instead of undefined behaviour. Calls before `giochi_engine_init` or
  from another thread report `GIOCHI_ERROR_NOT_INITIALIZED` instead of racing.
- **C scenes** as a callback table (`GiochiSceneCallbacks`) plus `user_data`.
  `giochi_engine_run_scene` brackets exactly one `on_enter`/`on_exit` pair and leaves
  the stack empty, so the same handle can be run again.
- `giochi_engine_step(frame_time)` for hosts that own the pacing, plus
  `giochi_engine_frames`/`steps`/`elapsed`/`timestep` statistics and
  `giochi_engine_video_driver`.
- `giochi_texture_create_memory` decodes an encoded image from memory, for targets
  without a filesystem.
- `scripts/ffi-smoke.sh`: builds `libgiochi`, compiles `tests/c_smoke.c` with
  `-Wall -Wextra -Werror` against the generated header, runs it, then runs
  `tests/python_smoke.py`. Both are real consumers; both run in CI.
- `docs/C_ABI.md`: the practical guide to consuming the ABI, including the
  "which subsystem may a callback use" table and a section per target language.
- CI job `ffi`, and a header check in the `gates` job.

### Changed — phase 7

- `giochi_texture_destroy`, `giochi_sound_destroy`, `giochi_music_destroy` and
  `giochi_scene_destroy` return `int32_t` instead of `void`: a double destroy is an
  error the contract already required to be *reported*, and a `void` function cannot
  report it.
- `giochi-engine-capi`'s library target is named `giochi`, so the artefacts are
  `libgiochi.so` / `libgiochi.a` as documented, and Rust consumers `use giochi::*`.
- `RenderContext` stores its `TextureRegistry` behind `Rc` and exposes
  `shared_textures()`, so the C ABI can create and destroy textures from inside a
  callback without borrowing the engine (ADR-028).
- `Engine::step(frame_time)` added; `run_current` and `step` now share one
  implementation of the frame body (ADR-029).

### Added — phase 6 (polish)

- `docs/ARCHITECTURE.md`: layer diagram, the dependency rules that keep the crates
  acyclic, ownership and lifetime rules, the mandated per-frame order, a table of
  the mechanisms that enforce the C-shaped surface, the failure policy and the
  testing strategy.
- `docs/KMSDRM.md`: how the driver is negotiated, environment hints, the
  `/dev/dri` permission problem and how to fix it, a table of the typical failure
  messages, the runtime libraries needed on the target and a verification recipe.
- `docs/API_GUIDE.md`: task-oriented guide covering the five-minute example,
  configuration, scenes and transitions, inspecting a running loop, screen-space
  versus world-space drawing, the input edge semantics, audio and silent mode,
  assets, math, the complete error-code table, and recommended game patterns.
- `giochi-engine/tests/api_guide.rs`: compiles and executes every example in
  `API_GUIDE.md`, so the guide cannot drift from the code.
- GitHub Actions: `ci.yml` (fmt, clippy `-D warnings`, build, test, doc on x86_64;
  ARM64 cross-check; demo and example smoke runs), `cross-compile.yml` (dedicated
  ARM64 build that inspects the produced binaries) and `release.yml` (tagged
  x86_64 and aarch64 archives attached to a draft GitHub release).
- Repository housekeeping: issue templates, a pull-request template and Dependabot
  configuration.
- `scripts/build-aarch64.sh`: a full ARM64 cross **build** (not just `cargo check`)
  that resolves the SDL2 satellite libraries from `CROSS_SYSROOT` or the
  Debian/Ubuntu multiarch directory, fails with actionable instructions when they
  are absent, and asserts the produced binaries are AArch64 ELF. Run by
  `cross-compile.yml` and `release.yml`.
- `docs/CROSS_COMPILE.md` rewritten: the two scripts and what each one proves, the
  verified multiarch recipe, console sysroots, the linker requirement and the
  satellite-library requirement.
- `README.md` rewritten for 0.1.0: badges, prerequisites, native and cross builds,
  the KMSDRM driver switch, the examples, and links to every guide.

### Fixed — phase 6

- **The primary target could not be built.** `cargo check` never links, so the
  per-phase gate was green while `aarch64-unknown-linux-gnu` failed to produce a
  single binary. Two causes, both fixed: `.cargo/config.toml` had no
  `[target.aarch64-unknown-linux-gnu] linker`, so Cargo handed ARM64 objects to the
  host `cc` (`rust-lld: --fix-cortex-a53-843419 is only supported on AArch64`); and
  nothing provided the ARM64 `libSDL2_image`/`libSDL2_mixer`/`libSDL2_ttf` that the
  `image`/`mixer`/`ttf` features link against, because `bundled` builds core SDL2
  only. Verified end to end in a clean `ubuntu:24.04` container, which now produces
  an `ELF 64-bit LSB pie executable, ARM aarch64`. See ADR-027.
- `docs/CROSS_COMPILE.md` claimed `scripts/check-aarch64.sh --release` performed a
  full compile and link. It never did — `--release` was forwarded to `cargo check`.

### Added — phase 5

- Phase 5 — scenes and the `Engine` facade: `trait Scene` with
  `on_enter`/`on_exit`/`update`/`render`/`transition`, `SceneManager` with
  `push`/`pop`/`replace`/`clear` (calling `on_enter`/`on_exit` exactly once per
  scene), and `Engine`, which owns the platform, renderer, audio, input and scene
  stack and drives them through the fixed-timestep loop.
- `Engine::run`, `run_current`, `push_scene`/`replace_scene`/`pop_scene`,
  `set_frame_limit`, `reset_quit`, `run_stats` and a `Drop` that releases scenes,
  audio, cached resources and textures in dependency order.
- `SceneTransition` (`Continue`/`Quit`/`Replace`/`Push`/`Pop`) applied by the engine
  **after** the frame is presented, so a scene is never freed mid-render, plus a
  runaway guard that reports a scene transitioning every frame.
- `RunStats` (`frames`, `steps`, `elapsed`) with derived `fps`, `step_rate` and
  `frame_time_ms`.
- `giochi-demo` rebuilt on `Engine` + `Scene`: movable sprite, bouncing companion,
  beep, background grid and `--verify` for one-frame pipeline checks.
- Example `mini_game`: a complete space shooter (movement, shooting, collision,
  lives, score, progressive difficulty, game over, procedurally generated shot
  sound) using every module, with `--frames` and `--auto`.
- `tests/engine_smoke.rs`: headless integration test of the loop, the scene stack
  lifecycle and the transition paths.

### Changed — phase 5

- `Scene::update` now receives `&mut AudioSystem` in addition to `Time` and
  `&Input` (ADR-024). Scenes had no legal way to play a sound they had loaded.

### Fixed — phase 5

- `SceneTransition::Quit` broke out of the loop without stopping it, leaving
  `Engine::is_running()` true; it now requests a quit so the state is coherent.

### Added — phase 4

- Phase 4 — assets and audio: `asset_size`/`read_asset` reading into a
  caller-owned buffer (never truncating, 64 MiB cap) and `ResourceManager`, a
  path-keyed texture cache where loading the same path twice performs **one**
  upload. `RenderContext` gains `load_texture`, `cached_texture`,
  `clear_resource_cache` and access to the cache.
- `AudioSystem` on top of SDL_mixer: sound effects and streamed music behind
  opaque `SoundHandle`/`MusicHandle`, with `play_sound`, `play_music`,
  `pause_music`, `resume_music`, `stop_music`, master volume clamped to `0..=128`
  and `music_state`.
- A machine without an audio device now runs in **silent mode**: loading and
  playing return `GIOCHI_ERROR_AUDIO` while the engine keeps running (ADR-021).
  `AudioSystem::silent()` makes that path testable without a sound card.
- Example `audio_test`: synthesises a 16-bit stereo WAV at runtime (no committed
  binary asset), plays it on `Space`, adjusts volume with the arrow keys, mutes
  with `M`, and supports `--beep`/`--frames` for scripted verification.
- `tests/assets_audio_smoke.rs`: headless coverage of the loader, the cache
  (two loads → one texture, stale-handle reload, cache clear) and the sound
  lifecycle on a real device.

### Added — phase 3

- Phase 3 — input: `KeyboardState`, `MouseState` and `GamepadState` aggregated by
  `Input`, which snapshots one consistent frame view via `Input::update(&[Event])`
  (called before the scene's update). Keyboard and mouse expose down/pressed/
  released queries; the mouse also reports position, per-frame delta and wheel.
- A key pressed **and** released between two frames reports both edges, so a fast
  tap is never lost; SDL key auto-repeat does not re-trigger `pressed`.
- Gamepad support: the lowest-indexed device SDL maps as a game controller is
  player one, with live axis/button queries normalised and clamped to `[-1.0, 1.0]`
  and device add/remove handling. A missing controller subsystem degrades to a
  warning instead of an error.
- `Input::without_gamepad()`, `apply_event`, `begin_frame` and
  `keyboard_mut()`/`mouse_mut()` so tests, replays and the future C ABI can inject
  input through the same state machines.
- Example `input_test`: WASD/arrows move, `Space` cycles the colour, the right
  button drags the square, clicks leave fading markers, the wheel resizes it and a
  gamepad's stick/D-pad also moves it.
- `tests/input_smoke.rs`: headless end-to-end input test injecting real SDL events
  through `Platform::push_event`.

### Fixed — phase 3

- The mouse delta was double-counted by adding both the position difference and
  SDL's `xrel`/`yrel`; the engine now uses SDL's relative motion only.

### Added — phase 2

- Phase 2 — 2D rendering: `RenderContext` (owns the SDL canvas, the texture
  registry and the current camera), `clear`/`draw_rect`/`draw_texture`/
  `draw_sprite`/`present`, a float-rect → `SDL_Rect` conversion that rounds and
  clamps instead of wrapping, and `read_pixels` for screenshot tooling.
- `TextureRegistry`: opaque `u64` texture handles backed by `HandleRegistry`,
  `create_texture(path)`, `create_texture_from_rgba`, `destroy_texture`,
  `dimensions`/`size`, `contains`, `len`, and a `Drop` that frees everything.
- `Camera::world_to_screen`, `screen_to_world` (exact inverse),
  `world_rect_to_screen` and `viewport_center`, with `zoom` sanitised to `1.0` for
  non-finite/non-positive values.
- `Sprite::world_rect`, `translate` and `is_drawable`; `Vec2` operators
  (`Add`/`Sub`/`Mul<f32>`) plus `length`/`distance`/`is_finite`; `Rect`
  `intersection`/`contains`/`center`.
- Examples `hello_rect` and `moving_sprite`, with `--frames N` for smoke runs, and
  `assets/player.png`/`assets/checker.png`.
- `tests/render_smoke.rs`: headless renderer test that verifies drawn pixels via
  `read_pixels`, invalid-handle and missing-asset errors, and that the live texture
  count returns to zero.

### Changed — phase 2

- `sdl2` is built with the `unsafe_textures` feature so textures can live in an
  opaque handle registry without a public lifetime (ADR-017). `RenderContext` now
  consumes the `Window` and owns it (ADR-018).

### Added — phase 1

- Phase 1 — platform and game loop: `Platform` (SDL2 video/timer/events with a
  shared `Rc` context), `Window` (fullscreen desktop, size, drawable size, title,
  fullscreen query), `Time::advance` with NaN/negative sanitisation, `FrameClock`,
  and `GameLoop` with a 1/60 s fixed timestep, a 0.25 s catch-up clamp and
  `QuitSignal` handling for `Escape`/window close.
- KMSDRM driver negotiation: `kmsdrm` is selected only when `SDL_VIDEODRIVER` is
  unset, with a logged fallback to the desktop driver, and a descriptive
  `/dev/dri` diagnostic when KMSDRM fails for an explicitly selected driver
  (ADR-015).
- `giochi-demo` now opens a 1280x720 window that closes on `Escape` or window
  close, with a `--frames N` flag for deterministic runs.
- `tests/platform_smoke.rs`: headless (dummy driver) integration test covering
  platform init, window creation, argument validation and the full
  Escape → `QuitSignal` → loop-stopped path.

### Fixed — phase 1

- A fixed timestep coarser than `MAX_FRAME_TIME` could never advance the
  simulation; the frame clamp is now `max(MAX_FRAME_TIME, fixed_timestep)`.
- The demo's frame limiter subtracted the *whole previous frame period* from the
  budget, so sleep and work were both charged to the same target and the loop
  settled at half the intended rate (~8 ms instead of 16.6 ms per frame). Sleeping
  is now derived from the current iteration's work time, via
  `GameLoop::sleep_ms`.

### Added — phase 0

- Phase 0 — bootstrap: four-crate workspace (`giochi-engine`,
  `giochi-engine-capi`, `giochi-engine-lua`, `giochi-demo`), C-shaped value types
  (`Vec2`, `Rect`, `Color`, `Sprite`, `Camera`, `Time`, `EngineConfig`,
  `WindowConfig`), crate-private `HandleRegistry<T>`, `GiochiError` with the
  numeric code table, and stub signatures for every module of phases 1–8.
- `docs/C_ABI_CONTRACT.md`: authoritative C ABI v1 specification, including
  projected struct layouts, error codes and the full function table.
- `docs/DECISIONS.md`: sixteen ADRs covering the workspace split, handles,
  error model, Lua binding, 2D-only scope and the ABI layout rules.
- `docs/CROSS_COMPILE.md`, `scripts/gates.sh`, `scripts/check-aarch64.sh` and the
  ARM64 CMake toolchain file that makes the bundled SDL2 build cross-compile.
- `giochi-engine/tests/abi_layout.rs`: byte-size and alignment assertions for
  every `#[repr(C)]` type in the contract.

[Unreleased]: https://github.com/xtrempkch-droid/GIOCHI-ENGINE-3D/compare/v0.1.0...main
[0.1.0]: https://github.com/xtrempkch-droid/GIOCHI-ENGINE-3D/releases/tag/v0.1.0
