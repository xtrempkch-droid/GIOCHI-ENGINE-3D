#!/usr/bin/env bash
# Install the native build dependencies for x86_64 Linux (Debian/Ubuntu).
#
# One list, used by every workflow. The list used to be copy-pasted into four jobs,
# and all four were missing the X11 headers: SDL2's CMake configure detects the X11
# video backend on a runner that has any X11 library, then fails with
# "Missing Xext.h, maybe you need to install the libxext-dev". The engine never uses
# X11 itself, but `bundled` builds all of SDL2, so the backend is configured and its
# headers are required. Duplicating the list is what let that slip through; a single
# script cannot drift between jobs.
#
# Usage:
#   sudo scripts/install-deps.sh            # build dependencies (default)
#   sudo scripts/install-deps.sh --cross    # adds the ARM64 cross toolchain
set -euo pipefail

cross=0
if [[ "${1:-}" == "--cross" ]]; then
    cross=1
fi

if [[ "$(id -u)" != "0" ]]; then
    echo "error: run as root (sudo scripts/install-deps.sh)" >&2
    exit 1
fi

# SDL2 is built from source by the `bundled` feature, so its *build* dependencies are
# what matter here. The engine links SDL2_image/SDL2_mixer/SDL2_ttf + SDL2 itself at
# runtime, but those come from the same `bundled`/system build.
packages=(
    # Toolchain
    build-essential
    cmake
    pkg-config
    # bindgen, used by sdl2-sys
    clang
    libclang-dev
    # The `image`, `mixer` and `ttf` features of the sdl2 crate link the *system*
    # SDL2 satellites. `bundled` builds core SDL2 only, so these must be installed or
    # the link fails with "cannot find -lSDL2_mixer". This is the same requirement
    # docs/CROSS_COMPILE.md documents for ARM64; it applies to the host too.
    libsdl2-image-dev
    libsdl2-mixer-dev
    libsdl2-ttf-dev
    # SDL2 core backends
    libasound2-dev
    libpulse-dev
    libudev-dev
    # KMSDRM: the production video backend
    libdrm-dev
    libgbm-dev
    libgl1-mesa-dev
    libegl1-mesa-dev
    # X11 backends. Configured whenever the headers are present, and *required* by
    # SDL2's CMake when it can see any X11 library at all.
    libx11-dev
    libxext-dev
    libxinerama-dev
    libxcursor-dev
    libxi-dev
    libxrandr-dev
    libxss-dev
    libxkbcommon-dev
    # Wayland backend, for developers on a Wayland desktop
    libwayland-dev
    libdecor-0-dev
    # Diagnostics used by the smoke scripts
    file
    ca-certificates
    curl
    # `python_smoke.py` and `scripts/png-to-bmp.py` both need an interpreter, and
    # `scripts/gates.sh` lints the shell scripts it relies on.
    python3
    shellcheck
)

if [[ "$cross" == "1" ]]; then
    packages+=(gcc-aarch64-linux-gnu g++-aarch64-linux-gnu)
fi

echo "==> installing ${#packages[@]} packages"
apt-get update -qq
DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends "${packages[@]}"

echo "==> done"
