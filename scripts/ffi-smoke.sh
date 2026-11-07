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

echo
echo "FFI smoke tests passed."
