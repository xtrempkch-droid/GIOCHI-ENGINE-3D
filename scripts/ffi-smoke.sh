#!/usr/bin/env bash
# Build libgiochi and run the C and Python smoke tests against it.
#
#   scripts/ffi-smoke.sh            # debug
#   scripts/ffi-smoke.sh --release  # release
#
# `tests/rust_ffi.rs` is part of `cargo test`, so it is not repeated here. These two
# are the ones that prove a real consumer can use the library: the C program links
# `libgiochi.so` and includes only `giochi.h`, and the Python program uses nothing but
# the standard library's `ctypes`.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

profile="debug"
cargo_flags=()
if [[ "${1:-}" == "--release" ]]; then
    profile="release"
    cargo_flags+=(--release)
fi

# Phase 18c: build with the `profiling` feature to prove that the frame statistics
# (introduced in `GIOCHI_ABI_VERSION 3`, current value 4) report real measurements
# through the C ABI and not just zeros. The consumers assert
# both states without being told which one they are in, so the same script covers both.
#
#   GIOCHI_CAPI_FEATURES=profiling scripts/ffi-smoke.sh
if [[ -n "${GIOCHI_CAPI_FEATURES:-}" ]]; then
    cargo_flags+=(--features "$GIOCHI_CAPI_FEATURES")
    echo "==> features:    $GIOCHI_CAPI_FEATURES"
fi

target_dir="${CARGO_TARGET_DIR:-$repo_root/target}/x86_64-unknown-linux-gnu/$profile"

echo "==> building libgiochi ($profile)"
cargo build -p giochi-engine-capi --target x86_64-unknown-linux-gnu "${cargo_flags[@]}"

if [[ ! -f "$target_dir/libgiochi.so" ]]; then
    echo "error: $target_dir/libgiochi.so was not produced" >&2
    exit 1
fi

echo "==> header is up to date"
scripts/gen-header.sh --check

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

echo "==> C smoke test"
cc -std=c11 -Wall -Wextra -Werror \
    -I giochi-engine-capi/include \
    giochi-engine-capi/tests/c_smoke.c \
    -L "$target_dir" -lgiochi \
    -Wl,-rpath,"$target_dir" \
    -o "$work/c_smoke"
SDL_VIDEODRIVER="${SDL_VIDEODRIVER:-dummy}" "$work/c_smoke"

echo "==> Python smoke test"
if ! command -v python3 >/dev/null 2>&1; then
    echo "    python3 not found; skipped"
else
    SDL_VIDEODRIVER="${SDL_VIDEODRIVER:-dummy}" \
        python3 giochi-engine-capi/tests/python_smoke.py "$target_dir"
fi

# Part II (3D) needs a real GL driver. `dummy` has none at all -- SDL refuses the window
# with "OpenGL support is either not configured in SDL" -- so this section forces
# `offscreen`, which provides a full EGL/GLES stack with no display attached. Both tests
# exit 77 when the driver has no GL, so a machine without Mesa reports "skipped" instead
# of failing and blaming the ABI.
echo "==> C 3D smoke test"
cc -std=c11 -Wall -Wextra -Werror \
    -I giochi-engine-capi/include \
    giochi-engine-capi/tests/c_smoke_3d.c \
    -L "$target_dir" -lgiochi \
    -Wl,-rpath,"$target_dir" \
    -o "$work/c_smoke_3d"
if SDL_VIDEODRIVER=offscreen "$work/c_smoke_3d"; then :; else
    status=$?
    if [[ "$status" == 77 ]]; then
        echo "    3D skipped: no OpenGL ES driver on this machine"
    else
        exit "$status"
    fi
fi

echo "==> Python 3D smoke test"
if ! command -v python3 >/dev/null 2>&1; then
    echo "    python3 not found; skipped"
else
    if SDL_VIDEODRIVER=offscreen \
        python3 giochi-engine-capi/tests/python_smoke_3d.py "$target_dir"; then :; else
        status=$?
        if [[ "$status" == 77 ]]; then
            echo "    3D skipped: no OpenGL ES driver on this machine"
        else
            exit "$status"
        fi
    fi
fi

echo
echo "FFI smoke tests passed."
