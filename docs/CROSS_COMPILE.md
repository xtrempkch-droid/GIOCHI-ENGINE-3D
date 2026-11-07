# Cross-compiling to ARM64

Primary production target: `aarch64-unknown-linux-gnu` (Linux consoles booting
straight into KMSDRM). Development target: `x86_64-unknown-linux-gnu`.

Two scripts, with different costs:

| Script | Does | Use when |
|---|---|---|
| `scripts/check-aarch64.sh` | `cargo check` — type-checks and compiles, **never links** | Every phase gate; fast |
| `scripts/build-aarch64.sh` | Full `cargo build` — compiles **and links**, then asserts the binaries are AArch64 ELF | Before a release; in the `cross-compile` workflow |

The distinction matters. `cargo check` proves the code compiles for ARM64 but not
that it links: a wrong linker or a missing ARM64 library only shows up in the
second script. That is not hypothetical — see ADR-027.

## Requirements

```bash
rustup target add aarch64-unknown-linux-gnu
sudo apt install gcc-aarch64-linux-gnu g++-aarch64-linux-gnu
```

Because SDL2 is built from source (`sdl2` feature `bundled`), a C toolchain and
CMake are required in addition to the Rust target.

### The ARM64 SDL2 satellite libraries

`bundled` builds **core** SDL2 only. The `image`, `mixer` and `ttf` features of
the `sdl2` crate link against the *system* `SDL2_image`, `SDL2_mixer` and
`SDL2_ttf`, so those have to exist for aarch64 before anything links. Without
them the link fails with:

```text
/usr/bin/aarch64-linux-gnu-ld.bfd: cannot find -lSDL2_mixer: No such file or directory
/usr/bin/aarch64-linux-gnu-ld.bfd: cannot find -lSDL2_image: No such file or directory
/usr/bin/aarch64-linux-gnu-ld.bfd: cannot find -lSDL2_ttf: No such file or directory
```

`scripts/build-aarch64.sh` detects that situation and prints what to install.

#### Option A — Debian/Ubuntu multiarch (what CI does)

Verified on Ubuntu 24.04. `dpkg --add-architecture arm64` alone is not enough:
the default Ubuntu sources have no `Architectures:` field, so apt would try to
fetch arm64 from the amd64 mirror and 404. Pin the existing stanzas to `amd64`
first, then add the arm64 ports mirror:

```bash
# 1. Pin the existing (amd64) sources.
sudo sed -i 's/^Types: deb$/Types: deb\nArchitectures: amd64/' \
  /etc/apt/sources.list.d/ubuntu.sources

# 2. Add the ARM64 ports mirror.
sudo dpkg --add-architecture arm64
sudo tee /etc/apt/sources.list.d/ubuntu-arm64.sources >/dev/null <<'EOF'
Types: deb
URIs: http://ports.ubuntu.com/ubuntu-ports
Suites: noble noble-updates noble-security
Components: main universe
Architectures: arm64
EOF

# 3. Install.
sudo apt update
sudo apt install libsdl2-image-dev:arm64 libsdl2-mixer-dev:arm64 libsdl2-ttf-dev:arm64
```

On Debian the mirror is `http://deb.debian.org/debian` with `Suites: bookworm`
and component `main`; on other Ubuntu releases substitute the suite name. The
libraries land in `/usr/lib/aarch64-linux-gnu`, which `aarch64-linux-gnu-gcc`
searches by default and `scripts/build-aarch64.sh` detects automatically.

#### Option B — sysroot from the target device or SDK

Consoles and vendor SDKs ship their own sysroot. Point the build at it:

```bash
CROSS_SYSROOT=/opt/console-sdk/sysroot scripts/build-aarch64.sh --release
CROSS_SYSROOT=/opt/console-sdk/sysroot scripts/check-aarch64.sh
```

The script looks for the satellites under `$CROSS_SYSROOT/usr/lib/<triple>`,
`$CROSS_SYSROOT/usr/lib/aarch64-linux-gnu`, `$CROSS_SYSROOT/lib/aarch64-linux-gnu`
and `$CROSS_SYSROOT/usr/lib`, and adds a `-Lnative=` flag so `rustc` finds them.
The same variable makes `cmake/aarch64-linux-toolchain.cmake` set `CMAKE_SYSROOT`,
so SDL2's own build uses the sysroot for its ALSA/udev probing.

A sysroot must contain the full transitive closure of the satellites
(`libpng16`, `libjpeg`, `libtiff`, `libwebp`, `libfreetype`, `libharfbuzz`,
`libvorbisfile`, `libmodplug`, `libmpg123`, `libopusfile`, `libfluidsynth`, …).
Copying only `libSDL2_mixer.so` and friends is not enough; the linker reports the
missing symbols from the dependencies.

## The linker

`.cargo/config.toml` sets the ARM64 linker:

```toml
[target.aarch64-unknown-linux-gnu]
linker = "aarch64-linux-gnu-gcc"
```

Without it Cargo falls back to `cc` for every target, so the cross build hands
ARM64 objects to the host linker and fails with a confusing message:

```text
rust-lld: error: --fix-cortex-a53-843419 is only supported on AArch64
collect2: error: ld returned 1 exit status
```

Only the host build ignores that entry; `x86_64` behaviour is unchanged.

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

`scripts/check-aarch64.sh` and `scripts/build-aarch64.sh` both export
`CMAKE_TOOLCHAIN_FILE_aarch64_unknown_linux_gnu` — the target-suffixed variable
the `cmake` build helper reads — so no extra flags are needed on the command line.

## Running the build

```bash
scripts/check-aarch64.sh              # fast: check only, part of scripts/gates.sh
scripts/build-aarch64.sh              # debug build + link + ELF assertion
scripts/build-aarch64.sh --release    # release build + link + ELF assertion
scripts/build-aarch64.sh -p giochi-demo
```

A successful run ends with the artefact inspection:

```text
==> target:      aarch64-unknown-linux-gnu
==> satellites:  /usr/lib/aarch64-linux-gnu
==> profile:     release
...
==> produced binaries:
target/aarch64-unknown-linux-gnu/release/giochi-demo: ELF 64-bit LSB pie executable, ARM aarch64, ...

aarch64 build OK.
```

That assertion is deliberate: a mis-set linker can quietly produce host binaries
when nothing imports the target architecture explicitly.

## KMSDRM in a cross build

SDL2 is configured with `SDL_KMSDRM=ON`, and `SDL_KMSDRM_SHARED=ON` makes it load
`libdrm`, `libgbm` and `libEGL` at **runtime** through `dlopen`. That is why the
cross-check links without ARM64 copies of those libraries, and why the generated
objects are genuinely ARM64:

```text
$ file target/aarch64-unknown-linux-gnu/debug/build/sdl2-sys-*/out/build/CMakeFiles/SDL2.dir/src/video/kmsdrm/SDL_kmsdrmdyn.c.o
ELF 64-bit LSB relocatable, ARM aarch64, version 1 (SYSV), not stripped
```

The consequence for deployment: the target root filesystem must provide
`libdrm.so.2`, `libgbm.so.1` and `libEGL.so.1`, plus the SDL2 satellites
(`libSDL2_mixer-2.0.so.0`, `libSDL2_image-2.0.so.0`, `libSDL2_ttf-2.0.so.0`) and
their own dependencies. See [KMSDRM.md](KMSDRM.md) for the deployment checklist.

## `cross`, as an alternative

`Cross.toml` configures [`cross`](https://github.com/cross-rs/cross) for the same
target, installing the ARM64 ALSA, udev and SDL2 satellite packages inside its
container:

```bash
cargo install cross
cross build --release --target aarch64-unknown-linux-gnu
```

It is a convenience for developers without the local toolchain; the scripts above
remain the source of truth and what CI runs.

## In CI

- [`.github/workflows/ci.yml`](../.github/workflows/ci.yml) `cross-check`:
  `scripts/check-aarch64.sh`.
- [`.github/workflows/cross-compile.yml`](../.github/workflows/cross-compile.yml)
  `link`: multiarch setup, then `scripts/build-aarch64.sh --workspace --all-targets`.
- [`.github/workflows/release.yml`](../.github/workflows/release.yml)
  `build-aarch64`: multiarch setup, then
  `scripts/build-aarch64.sh --release --workspace`, packaged into
  `giochi-engine-aarch64-unknown-linux-gnu.tar.gz`.
