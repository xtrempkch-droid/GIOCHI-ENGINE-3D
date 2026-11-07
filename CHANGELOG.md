# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project adheres
to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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
