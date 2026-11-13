#!/usr/bin/env bash
# Cross-check the workspace for aarch64-unknown-linux-gnu.
#
# The bundled SDL2 is a CMake project. When CMake configures it as a native
# build it probes the host for optional libraries and emits x86_64 linker flags
# that the aarch64 linker rejects. `cmake/aarch64-linux-toolchain.cmake` makes
# that build a real cross build; this script points `sdl2-sys` at it through the
# target-suffixed variable the `cmake` crate reads.
#
# Requirement: the aarch64 target and the aarch64 GCC toolchain
# (gcc-aarch64-linux-gnu) must be installed. See docs/CROSS_COMPILE.md.
#
# Optional: CROSS_SYSROOT=/path/to/sysroot for a sysroot that provides ARM64
# audio/udev headers — without it those backends are simply disabled.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

if ! rustup target list --installed | grep -qx 'aarch64-unknown-linux-gnu'; then
    echo "error: aarch64-unknown-linux-gnu target is not installed." >&2
    echo "       run: rustup target add aarch64-unknown-linux-gnu" >&2
    exit 1
fi

if ! command -v aarch64-linux-gnu-gcc >/dev/null 2>&1; then
    echo "error: aarch64-linux-gnu-gcc not found." >&2
    echo "       install: sudo apt install gcc-aarch64-linux-gnu g++-aarch64-linux-gnu" >&2
    exit 1
fi

export CMAKE_TOOLCHAIN_FILE_aarch64_unknown_linux_gnu="$repo_root/cmake/aarch64-linux-toolchain.cmake"

cargo check --workspace --target aarch64-unknown-linux-gnu "$@"

# As features opcionais ficam de fora do `--workspace` acima, então um crate que
# só compila com elas passaria sem ser visto. `giochi-net/laminar` é exatamente
# esse caso: o transporte padrão tem zero dependência, e a feature puxa 12
# crates que precisam cross-compilar para o aparelho. Verificado à parte, de
# propósito — `--all-features` no workspace ligaria também `profiling` e o
# `tester` do laminar, que não são o que o aparelho usa.
# A descoberta por mDNS e a feature que mais arrisca a portabilidade: `mdns-sd`
# puxa `if-addrs` e `socket2`, que fazem syscalls de enumeracao de interface.
# Se ela nao compilar para o aparelho, o substituto e broadcast UDP (ver
# docs/REDE.md) -- mas isso so se decide com o resultado deste passo.
cargo check -p giochi-net --features laminar,mdns --target aarch64-unknown-linux-gnu "$@"
