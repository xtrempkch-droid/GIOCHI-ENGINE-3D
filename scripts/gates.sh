#!/usr/bin/env bash
# Run every quality gate required by PROGRESS.md.
#
# Usage:
#   scripts/gates.sh              # all gates
#   SKIP_ARM64=1 scripts/gates.sh # skip the aarch64 cross-check
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

run() {
    echo
    echo "==> $*"
    "$@"
}

run cargo fmt --all --check
run cargo clippy --workspace --all-targets -- -D warnings
run cargo build --workspace --target x86_64-unknown-linux-gnu
# `SDL_VIDEODRIVER=offscreen` is what the 3D integration tests need: it provides a real
# EGL/GLES context with no display attached, while `dummy` has no OpenGL at all. The
# tests skip cleanly on a machine without a GL driver, so this cannot produce a false
# failure on a headless builder.
SDL_VIDEODRIVER="${SDL_VIDEODRIVER:-offscreen}" \
    run cargo test --workspace --target x86_64-unknown-linux-gnu
# The profiler and the per-frame counters are compiled out by default, so a test that
# asserts the counting behaviour needs a build where counting exists. CI runs the same
# two commands.
SDL_VIDEODRIVER="${SDL_VIDEODRIVER:-offscreen}" \
    run cargo test -p giochi-engine --features profiling --target x86_64-unknown-linux-gnu
SDL_VIDEODRIVER="${SDL_VIDEODRIVER:-offscreen}" \
    run cargo test -p giochi-engine-3d --features profiling --target x86_64-unknown-linux-gnu
# Phase 19. `hot_reload` is off by default, so without this line the watcher, the texture
# reload and the shader reload are only type-checked, never run — and the three
# integration tests are skipped by `required-features`.
SDL_VIDEODRIVER="${SDL_VIDEODRIVER:-offscreen}" \
    run cargo test --workspace --features giochi-engine/hot_reload,giochi-engine-3d/hot_reload,giochi-engine-lua/hot_reload,giochi-demo/hot_reload --target x86_64-unknown-linux-gnu
# Phase 20/21. `devtools` is off by default, so without this the console, the debug draw,
# the egui backend and the widget facade are only type-checked.
SDL_VIDEODRIVER="${SDL_VIDEODRIVER:-offscreen}" \
    run cargo test --workspace --features giochi-engine/devtools,giochi-engine/hot_reload,giochi-engine-3d/hot_reload,giochi-engine-lua/hot_reload,giochi-demo/hot_reload --target x86_64-unknown-linux-gnu
run cargo clippy --workspace --all-targets --features giochi-engine/devtools,giochi-engine/hot_reload,giochi-engine-3d/hot_reload,giochi-engine-lua/hot_reload,giochi-demo/hot_reload -- -D warnings
# `laminar` and `mdns` are optional, so a default-feature build never compiles or tests
# their code outside the aarch64 cross-check. Test them here, where the loopback tests
# can actually run.
run cargo test -p giochi-net --all-features --target x86_64-unknown-linux-gnu
run cargo clippy -p giochi-net --all-targets --all-features -- -D warnings
run cargo doc --workspace --no-deps --document-private-items

if command -v cbindgen >/dev/null 2>&1; then
    run scripts/gen-header.sh --check
else
    echo
    echo "==> C header check SKIPPED (cbindgen not installed)"
    echo "    cargo install cbindgen --version 0.27.0 --locked"
fi

# Version fields live in six files the compiler cannot check against each other;
# a release that disagrees with its own header is the failure mode (ADR: see
# CONTRIBUTING, "Releasing").
run scripts/check-version.sh

# Manifests that look right can still resolve to the wrong SDL2: cargo unions features
# across the graph, so one dependency without `default-features = false` re-enables
# `sdl2/bundled` and silently disables `sdl2-system` for every consumer downstream.
run scripts/check-features.sh

# The BMP twins exist so a target without a PNG decoder can still load the assets
# (ADR-036). Byte comparison, because a same-size twin with different pixels would
# pass every other gate.
if command -v python3 >/dev/null 2>&1; then
    run python3 scripts/png-to-bmp.py --check assets/player.png assets/checker.png
else
    echo
    echo "==> asset twin check SKIPPED (python3 not installed)"
fi

# CI calls these scripts rather than reimplementing them, so a broken script fails
# every job at once. Linting them here catches that before the push.
if command -v shellcheck >/dev/null 2>&1; then
    run shellcheck scripts/*.sh
else
    echo
    echo "==> shellcheck SKIPPED (not installed)"
    echo "    sudo apt install shellcheck"
fi

if [[ "${SKIP_ARM64:-0}" == "1" ]]; then
    echo
    echo "==> aarch64 cross-check SKIPPED (SKIP_ARM64=1)"
else
    run scripts/check-aarch64.sh
fi

echo
echo "All gates passed."
