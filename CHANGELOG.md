# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project adheres
to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
