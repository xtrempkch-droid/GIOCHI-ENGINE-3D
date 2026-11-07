# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project adheres
to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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

### Fixed

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

### Changed

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

### Fixed

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

[Unreleased]: https://github.com/xtrempkch-droid/GIOCHI-ENGINE-3D/commits/main
