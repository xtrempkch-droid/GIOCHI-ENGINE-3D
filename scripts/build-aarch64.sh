#!/usr/bin/env bash
# Cross-build the workspace for aarch64-unknown-linux-gnu, including the link.
#
# `scripts/check-aarch64.sh` only runs `cargo check`, which never links. This
# script performs the real build, so a broken linker configuration or a missing
# ARM64 library is caught here instead of in a release.
#
# The `sdl2` crate builds core SDL2 from source (`bundled`), but the `image`,
# `mixer` and `ttf` features link against the *system* SDL2_image, SDL2_mixer and
# SDL2_ttf. Those must exist for aarch64. Two supported ways to provide them:
#
#   1. Multiarch packages on a Debian/Ubuntu host (see docs/CROSS_COMPILE.md):
#        sudo dpkg --add-architecture arm64
#        sudo apt install libsdl2-{image,mixer,ttf}-dev:arm64
#      They land in /usr/lib/aarch64-linux-gnu, which this script finds by
#      default.
#
#   2. A sysroot from the target device or vendor SDK, pointed at with
#        CROSS_SYSROOT=/path/to/sysroot scripts/build-aarch64.sh
#
# The build is compiled for the console's own core, not for a baseline ARMv8-A. Which
# core is `GIOCHI_TARGET_CPU`, defaulting to the PX30's Cortex-A35; see the table in
# docs/CROSS_COMPILE.md for the other boards, and pass it for anything else:
#
#   GIOCHI_TARGET_CPU=cortex-a72 scripts/build-aarch64.sh --release
#
# This has to be done here rather than only in `.cargo/config.toml`: cargo ignores the
# `rustflags` in that file as soon as `RUSTFLAGS` is set in the environment, which this
# script does below in order to add the sysroot to the link search path. A phase-18
# build was verified to be compiling for generic ARMv8-A for exactly that reason -- the
# flags were in the config file, the script had hidden them, and nothing failed.
#
# Usage:
#   scripts/build-aarch64.sh             # debug
#   scripts/build-aarch64.sh --release   # release
#   scripts/build-aarch64.sh -p giochi-demo
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

target=aarch64-unknown-linux-gnu

# Code generation for the console CPU. `native` cannot work in a cross build: the
# compiler runs on the host, so it would describe the host.
GIOCHI_TARGET_CPU="${GIOCHI_TARGET_CPU:-cortex-a35}"
case "$GIOCHI_TARGET_CPU" in
    cortex-a35 | cortex-a53 | cortex-a55 | cortex-a72 | cortex-a76 | cortex-a78)
        # ARMv8-A cores whose vendor builds ship the optional CRC and crypto
        # instructions -- the PX30 reports `aes pmull sha1 sha2 crc32` in /proc/cpuinfo.
        # LSE atomics are deliberately absent: they are ARMv8.1 and the A35 is ARMv8.0-A,
        # so `+lse` produces instructions the core cannot decode. See
        # docs/CROSS_COMPILE.md.
        GIOCHI_TARGET_FEATURES="${GIOCHI_TARGET_FEATURES:-+crc,+aes,+sha2}"
        ;;
    *)
        # Anything unknown gets no target features: a wrong `+feature` is a SIGILL on a
        # device nobody is watching, and a missed optimisation is merely slower.
        GIOCHI_TARGET_FEATURES="${GIOCHI_TARGET_FEATURES:-}"
        ;;
esac
satellites=(libSDL2_image.so libSDL2_mixer.so libSDL2_ttf.so)

die() {
    echo "error: $*" >&2
    exit 1
}

rustup target list --installed | grep -qx "$target" ||
    die "the $target target is not installed. run: rustup target add $target"

command -v aarch64-linux-gnu-gcc >/dev/null 2>&1 ||
    die "aarch64-linux-gnu-gcc not found. install: sudo apt install gcc-aarch64-linux-gnu g++-aarch64-linux-gnu"

# Where are the ARM64 SDL2 satellite libraries?
lib_dir=""
if [[ -n "${CROSS_SYSROOT:-}" ]]; then
    [[ -d "$CROSS_SYSROOT" ]] || die "CROSS_SYSROOT=$CROSS_SYSROOT does not exist"
    for candidate in \
        "$CROSS_SYSROOT/usr/lib/$target" \
        "$CROSS_SYSROOT/usr/lib/aarch64-linux-gnu" \
        "$CROSS_SYSROOT/lib/aarch64-linux-gnu" \
        "$CROSS_SYSROOT/usr/lib"; do
        if [[ -e "$candidate/libSDL2_mixer.so" || -e "$candidate/libSDL2_mixer.a" ]]; then
            lib_dir="$candidate"
            break
        fi
    done
else
    for candidate in /usr/lib/aarch64-linux-gnu /usr/lib64; do
        if [[ -e "$candidate/libSDL2_mixer.so" ]]; then
            lib_dir="$candidate"
            break
        fi
    done
fi

if [[ -z "$lib_dir" ]]; then
    missing=()
    for lib in "${satellites[@]}"; do
        [[ -e "/usr/lib/aarch64-linux-gnu/$lib" ]] || missing+=("$lib")
    done
    {
        echo "error: the ARM64 SDL2 satellite libraries are missing:${missing[*]:+ ${missing[*]}}"
        echo
        echo "  The 'image', 'mixer' and 'ttf' features of the sdl2 crate link against the"
        echo "  system SDL2_image, SDL2_mixer and SDL2_ttf; the 'bundled' feature only builds"
        echo "  core SDL2. For aarch64 they must come from an ARM64 sysroot, the target device,"
        echo "  or Debian/Ubuntu multiarch packages."
        echo
        echo "  Debian/Ubuntu host:"
        echo "    sudo dpkg --add-architecture arm64"
        echo "    # add http://ports.ubuntu.com/ubuntu-ports for arm64, see docs/CROSS_COMPILE.md"
        echo "    sudo apt update && sudo apt install libsdl2-{image,mixer,ttf}-dev:arm64"
        echo
        echo "  Console SDK:"
        echo "    CROSS_SYSROOT=/path/to/sysroot $0 $*"
    } >&2
    exit 1
fi

export CMAKE_TOOLCHAIN_FILE_aarch64_unknown_linux_gnu="$repo_root/cmake/aarch64-linux-toolchain.cmake"

# Always set, so `.cargo/config.toml` cannot be the thing that decides whether the
# console build is optimised for the console. The CPU flags are repeated here rather
# than left to the config file, which cargo ignores while this variable is set.
target_rustflags="-C target-cpu=$GIOCHI_TARGET_CPU"
if [[ -n "$GIOCHI_TARGET_FEATURES" ]]; then
    target_rustflags="$target_rustflags -C target-feature=$GIOCHI_TARGET_FEATURES"
fi
export RUSTFLAGS="${RUSTFLAGS:-} $target_rustflags"

# A plain sysroot is not in gcc's default search path; a multiarch directory is.
if [[ -n "${CROSS_SYSROOT:-}" ]]; then
    export RUSTFLAGS="$RUSTFLAGS -Lnative=$lib_dir"

    # SDL2's CMake probes X11, Wayland, EGL, PulseAudio and sndio through
    # pkg-config, which knows nothing about CMAKE_FIND_ROOT_PATH. Left alone it
    # answers with the *host's* x86_64 .pc files, so the bundled SDL2 enables
    # those subsystems and the aarch64 linker is then handed flags that resolve
    # to host sonames. Pointing pkg-config at the sysroot alone makes those
    # probes fail, which is correct: the target does not have them.
    export PKG_CONFIG_SYSROOT_DIR="$CROSS_SYSROOT"
    export PKG_CONFIG_LIBDIR="$CROSS_SYSROOT/usr/lib/pkgconfig:$CROSS_SYSROOT/usr/share/pkgconfig"
fi

echo "==> target:      $target"
echo "==> cpu:         $GIOCHI_TARGET_CPU ${GIOCHI_TARGET_FEATURES:-(no extra target features)}"
echo "==> satellites:  $lib_dir"
echo "==> profile:     $([[ " $* " == *" --release "* ]] && echo release || echo debug)"

cargo build --target "$target" "$@"

# Prove the artefacts really are ARM64; a mis-set linker can quietly emit host
# binaries when nothing imports the target architecture explicitly.
echo
echo "==> produced binaries:"
target_dir="${CARGO_TARGET_DIR:-$repo_root/target}"
found=0
for bin in "$target_dir/$target"/*/giochi-demo; do
    [[ -f "$bin" ]] || continue
    found=$((found + 1))
    if command -v file >/dev/null 2>&1; then
        file "$bin"
        file "$bin" | grep -q 'ARM aarch64' ||
            die "$bin is not an AArch64 executable"
    else
        size=$(stat -c %s "$bin")
        [[ "$size" -gt 0 ]] || die "$bin is empty"
        echo "$bin ($size bytes; install 'file' to inspect the ELF header)"
    fi
done

[[ "$found" -gt 0 ]] || die "no giochi-demo binary was produced for $target"
echo
echo "aarch64 build OK."
