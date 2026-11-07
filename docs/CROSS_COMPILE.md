# Cross-compiling to ARM64

> Phase 0 note: this page documents the toolchain plumbing that already exists in
> the repository. Phase 6 expands it with the full command matrix and the CI
> integration.

## Target

Primary production target: `aarch64-unknown-linux-gnu` (Linux consoles booting
straight into KMSDRM). Development target: `x86_64-unknown-linux-gnu`.

## Requirements

```bash
rustup target add aarch64-unknown-linux-gnu
sudo apt install gcc-aarch64-linux-gnu g++-aarch64-linux-gnu
```

Because SDL2 is built from source (`sdl2` feature `bundled`), a C toolchain and
CMake are required in addition to the Rust target.

## The CMake toolchain file

`cmake/aarch64-linux-toolchain.cmake` exists because SDL2's CMake project would
otherwise configure itself as a *native* build while cross-compiling. In that
mode it probes the host for optional libraries and produces flags such as
`-lsamplerate` that resolve to x86_64 sonames, which the aarch64 linker rejects:

```text
/usr/bin/aarch64-linux-gnu-ld: skipping incompatible /usr/lib/x86_64-linux-gnu/libsamplerate.so
```

The toolchain file sets `CMAKE_SYSTEM_NAME`/`CMAKE_SYSTEM_PROCESSOR`, the ARM64
compilers, and restricts `find_*` to the sysroot. SDL2's optional dependencies
that the sysroot does not provide are disabled instead of being resolved from the
host.

## Running the check

```bash
scripts/check-aarch64.sh
```

The script exports `CMAKE_TOOLCHAIN_FILE_aarch64_unknown_linux_gnu` (the
target-suffixed variable the `cmake` build helper reads) and runs
`cargo check --workspace --target aarch64-unknown-linux-gnu`.

An optional sysroot that provides ARM64 audio/udev headers can be supplied with
`CROSS_SYSROOT=/path/to/sysroot scripts/check-aarch64.sh`. Without it, SDL2 builds
without ALSA/udev; that is enough to validate that the engine compiles for ARM64,
but a production build should use a complete sysroot.

## `cross`

`Cross.toml` configures `cross` for the same target, installing the ARM64
`libasound2-dev`/`libudev-dev` packages inside the container. It is an
alternative to the local cross toolchain, not a replacement for the gate above.
