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
#   scripts/build-aarch64.sh --container --release -p giochi-racer
#
# ## `--container`: build at the device's glibc, not the host's
#
# A sysroot carries the device's SDL2, not the device's libc: when the linker runs on
# the host it records the *host's* symbol versions, and the binary then refuses to start
# on a console whose glibc is older (`version 'GLIBC_2.43' not found`). Measured on
# 2026-11-18: host 2.43, R35S 2.41. `--container` reruns this same script inside
# `debian:13-slim` (glibc 2.41), mounting the repository read-only and the output
# directory writable, which is the procedure in docs/CROSS_COMPILE.md §"The host's
# glibc is the ceiling".
#
#   GIOCHI_BUILD_IMAGE=debian:13-slim      # the image to run (any glibc <= the device's)
#   CROSS_SYSROOT=/path/to/sysroot         # required with --features sdl2-system
#   GIOCHI_TARGET_GLIBC=2.41               # fail if the result needs more than this
#
# `GIOCHI_TARGET_GLIBC` is checked after every build, container or not, and the produced
# binaries' real floor is printed either way.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

die() {
    echo "error: $*" >&2
    exit 1
}

# `--container` is a wrapper, not a cargo flag: take it out before `"$@"` reaches cargo.
container=0
cargo_args=()
for arg in "$@"; do
    case "$arg" in
        --container) container=1 ;;
        *) cargo_args+=("$arg") ;;
    esac
done
if ((${#cargo_args[@]})); then
    set -- "${cargo_args[@]}"
else
    set --
fi

target=aarch64-unknown-linux-gnu

# `sdl2-sys` needs `deps/` to exist before its build script runs; see that script.
"$repo_root/scripts/create-cargo-deps-dirs.sh" >/dev/null

# Code generation for the console CPU. `native` cannot work in a cross build: the
# compiler runs on the host, so it would describe the host. Resolved here, before the
# container branch, so the value forwarded into the container is the one the inner
# build would use; the inner process repeats the feature derivation from it below.
GIOCHI_TARGET_CPU="${GIOCHI_TARGET_CPU:-cortex-a35}"

# The inner process of `--container`, so the wrapper runs once.
if ((container)) && [[ "${GIOCHI_IN_CONTAINER:-}" != 1 ]]; then
    command -v docker >/dev/null 2>&1 ||
        die "docker is required for --container (it is how the build runs at the device's glibc)"

    [[ -n "${GIOCHI_TARGET_GLIBC:-}" ]] || GIOCHI_TARGET_GLIBC=2.41
    image="${GIOCHI_BUILD_IMAGE:-debian:13-slim}"
    out_dir="${GIOCHI_CONTAINER_OUT:-$repo_root/target/container-aarch64}"
    mkdir -p "$out_dir/cargo-home" "$out_dir/target"

    # The registry is copied (hardlinks when possible) instead of mounted: cargo writes
    # its package-cache lock there and a read-only home fails the build.
    offline=0
    if [[ -d "${CARGO_HOME:-$HOME/.cargo}/registry" ]]; then
        if [[ ! -d "$out_dir/cargo-home/registry" ]]; then
            cp -al "${CARGO_HOME:-$HOME/.cargo}/registry" "$out_dir/cargo-home/registry" \
                2>/dev/null ||
                cp -a "${CARGO_HOME:-$HOME/.cargo}/registry" "$out_dir/cargo-home/registry"
        fi
        offline=1
    fi

    # `command -v cargo` is the rustup shim (`~/.cargo/bin/cargo`), which cannot run
    # inside the container: rustup is not installed there. Resolve the real toolchain
    # binary through rustup and mount `~/.rustup` (the parent of `toolchains/`).
    if command -v rustup >/dev/null 2>&1; then
        real_cargo="$(rustup which cargo 2>/dev/null || true)"
    else
        real_cargo=""
    fi
    toolchain_bin="$(dirname "${real_cargo:-$(command -v cargo)}")"
    # .../toolchains/<name>/bin -> .../toolchains/<name> -> .../toolchains -> ~/.rustup
    toolchain_name="$(basename "$(dirname "$toolchain_bin")")"
    rustup_home="$(cd "$toolchain_bin/../../.." && pwd)"
    # Cargo inside the container must still find the aarch64 std for this toolchain; it
    # is part of the mounted `~/.rustup/toolchains/<name>/lib/rustlib`.
    [[ -d "$rustup_home/toolchains/$toolchain_name" ]] ||
        die "could not find the rustup toolchain directory (looked in $rustup_home/toolchains)"
    docker_args=(
        run --rm
        -v "$repo_root:/work:ro"
        -v "$out_dir:/out"
        -e GIOCHI_IN_CONTAINER=1
        -e "GIOCHI_TARGET_CPU=$GIOCHI_TARGET_CPU"
        -e "GIOCHI_TARGET_FEATURES=${GIOCHI_TARGET_FEATURES:-}"
        -e "GIOCHI_TARGET_GLIBC=$GIOCHI_TARGET_GLIBC"
        -e CARGO_HOME=/out/cargo-home
        -e RUSTUP_HOME=/rustup
        -e CARGO_TARGET_DIR=/out/target
        -e PKG_CONFIG_ALLOW_CROSS=1
        -e PATH="/rustup/toolchains/$toolchain_name/bin:/usr/local/bin:/usr/bin:/bin"
        -v "$rustup_home:/rustup:ro"
    )
    ((offline)) && docker_args+=(-e CARGO_NET_OFFLINE=true)

    if [[ -n "${CROSS_SYSROOT:-}" ]]; then
        docker_args+=(-v "$CROSS_SYSROOT:/sysroot:ro" -e CROSS_SYSROOT=/sysroot)
    fi

    echo "==> container: $image (glibc <= ${GIOCHI_TARGET_GLIBC:-unknown})"
    echo "==> output:    $out_dir/target"

    exec docker "${docker_args[@]}" "$image" bash -c '
        set -euo pipefail
        # gcc/g++ build the host-side build scripts (SDL2'"'"'s CMake probes them);
        # pkg-config and file are what the wrappers and the ELF checks use.
        #
        # The ARM64 SDL2 satellites (image/mixer/ttf) are not part of the `bundled`
        # feature -- it only builds core SDL2 -- so they are installed here through
        # Debian multiarch. That is what makes `--container` self-contained on a machine
        # that has never seen the console; `CROSS_SYSROOT` still overrides them with the
        # device own libraries when those matter (they do for a version check).
        dpkg --add-architecture arm64
        apt-get update -qq
        apt-get install -y -qq --no-install-recommends \
            gcc g++ gcc-aarch64-linux-gnu g++-aarch64-linux-gnu libc6-dev-arm64-cross \
            pkg-config file cmake make \
            libsdl2-image-dev:arm64 libsdl2-mixer-dev:arm64 libsdl2-ttf-dev:arm64 \
            >/dev/null
        /work/scripts/build-aarch64.sh "$@"
    ' -- "$@"
fi

# Feature flags follow from the CPU chosen above.
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

# Inside the container the toolchain is the mounted rustup one: `cargo` is there but
# `rustup` is not, and the image has the cross compiler installed a few lines down.
if [[ "${GIOCHI_IN_CONTAINER:-}" != 1 ]]; then
    rustup target list --installed | grep -qx "$target" ||
        die "the $target target is not installed. run: rustup target add $target"
fi

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
binaries=()
# Any binary crate in the workspace, not just the demo: `-p giochi-racer` produces no
# `giochi-demo`, and failing on that was a false alarm.
for bin in "$target_dir/$target"/*/giochi-*; do
    [[ -f "$bin" && -x "$bin" ]] || continue
    found=$((found + 1))
    binaries+=("$bin")
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

[[ "$found" -gt 0 ]] || die "no giochi-* binary was produced for $target"

# The host's glibc is the ceiling, not the device's: the linker records the symbol
# versions of whichever libc it resolved against, and the binary then refuses to start
# on a console whose glibc is older (docs/CROSS_COMPILE.md §"The host's glibc is the
# ceiling"). Report the floor always; fail when the caller says what the device has.
glibc_floor() {
    local elf="$1" symbols=""
    if command -v aarch64-linux-gnu-readelf >/dev/null 2>&1; then
        symbols=$(aarch64-linux-gnu-readelf -sW --dyn-syms "$elf" 2>/dev/null || true)
    elif command -v readelf >/dev/null 2>&1; then
        symbols=$(readelf -sW --dyn-syms "$elf" 2>/dev/null || true)
    fi
    grep -oE 'GLIBC_[0-9]+\.[0-9]+' <<<"$symbols" | sort -Vu | tail -1 || true
}

echo
echo "==> glibc floor:"
floor_failed=0
for bin in "${binaries[@]}"; do
    floor=$(glibc_floor "$bin")
    if [[ -z "$floor" ]]; then
        echo "    $(basename "$bin"): no GLIBC_* symbols (statically linked, or no readelf)"
        continue
    fi
    echo "    $(basename "$bin"): needs ${floor#GLIBC_}"
    if [[ -n "${GIOCHI_TARGET_GLIBC:-}" ]]; then
        highest=$(printf '%s\n%s\n' "${floor#GLIBC_}" "$GIOCHI_TARGET_GLIBC" | sort -V | tail -1)
        if [[ "$highest" != "$GIOCHI_TARGET_GLIBC" ]]; then
            echo "    error: needs ${floor#GLIBC_}, the device has $GIOCHI_TARGET_GLIBC" >&2
            floor_failed=1
        fi
    fi
done

if ((floor_failed)); then
    die "the build inherited the host's glibc — rebuild with --container (docs/CROSS_COMPILE.md)"
fi
if [[ -z "${GIOCHI_TARGET_GLIBC:-}" ]]; then
    echo "    (pass GIOCHI_TARGET_GLIBC=<device> to fail instead of report)"
fi

echo
echo "aarch64 build OK."
