# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project adheres
to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Phase 0 — bootstrap: four-crate workspace (`giochi-engine`,
  `giochi-engine-capi`, `giochi-engine-lua`, `giochi-demo`), C-shaped value types
  (`Vec2`, `Rect`, `Color`, `Sprite`, `Camera`, `Time`, `EngineConfig`,
  `WindowConfig`), crate-private `HandleRegistry<T>`, `GiochiError` with the
  numeric code table, and stub signatures for every module of phases 1–8.
- `docs/C_ABI_CONTRACT.md`: authoritative C ABI v1 specification, including
  projected struct layouts, error codes and the full function table.
- `docs/DECISIONS.md`: fourteen ADRs covering the workspace split, handles,
  error model, Lua binding, 2D-only scope and the ABI layout rules.
- `docs/CROSS_COMPILE.md`, `scripts/gates.sh`, `scripts/check-aarch64.sh` and the
  ARM64 CMake toolchain file that makes the bundled SDL2 build cross-compile.
- `giochi-engine/tests/abi_layout.rs`: byte-size and alignment assertions for
  every `#[repr(C)]` type in the contract.

[Unreleased]: https://github.com/xtrempkch-droid/GIOCHI-ENGINE-3D/commits/main
