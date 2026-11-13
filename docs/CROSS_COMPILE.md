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
CMake are required in addition to the Rust target. A build that links the SDL2
already in the target image instead needs neither — see
[Linking the SDL2 already in the image](#linking-the-sdl2-already-in-the-image-sdl2-system).

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
sudo scripts/install-arm64-deps.sh
```

That script is the whole recipe, and CI calls it too:

1. pin the existing (amd64) sources with `Architectures: amd64`, or apt tries to fetch
   arm64 from the amd64 mirror and 404s;
2. `dpkg --add-architecture arm64` and add the `ports.ubuntu.com` mirror;
3. install the ARM64 SDL2 satellites and the cross toolchain.

It derives the ports **suite from the machine's own `/etc/os-release`**. That matters:
hard-coding `noble` on a newer release mixes indexes from two releases and the libc
packages then conflict outright —

```text
libcrypt-dev:amd64 Breaks libcrypt-dev:arm64 (!= 1:4.5.1-1)
```

— which is exactly how the `aarch64 link` job failed once `ubuntu-latest` moved from
24.04 to 26.04. The two jobs that need the multiarch indexes are therefore pinned to
`ubuntu-24.04`, the release the recipe is verified on, instead of tracking
`ubuntu-latest`.

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

This is the path that was used for the first real-hardware run
([KMSDRM.md](KMSDRM.md) §8): the ARM64 build was made against the console's own
sysroot, and the resulting binaries ran on it unchanged. A sysroot normally comes from
the vendor SDK cache; one can also be lifted from the device itself:

```bash
mkdir -p /tmp/console-sysroot
for d in usr/lib usr/include usr/share/pkgconfig lib; do
    ssh root@console "tar -C /$d -cf - ." | tar -C /tmp/console-sysroot/$d -xf -
done
CROSS_SYSROOT=/tmp/console-sysroot scripts/build-aarch64.sh --release
```

**A runtime-only image has no `.pc` files and no headers.** Measured on the R35S
(Buildroot 2024.02): `/usr/lib/libSDL2*.so*` exist, but there is no `sdl2.pc`, no
`/usr/include/SDL2` and no `pkg-config`. `--features sdl2-system` then fails at
`sdl2-sys`'s `pkg-config` probe. What worked was to lift the shared objects and write the
four `.pc` files by hand, with the headers taken from the build host (the `sdl2-sys`
bindings are pre-generated, so only the include path has to exist):

```bash
mkdir -p /tmp/r35s-sysroot/usr/lib/aarch64-linux-gnu /tmp/r35s-sysroot/usr/lib/pkgconfig
cd /tmp/r35s-sysroot/usr/lib/aarch64-linux-gnu
for so in $(ssh root@console 'ls /usr/lib/libSDL2*.so.*'); do
    scp -q root@console:"$so" .
done
ln -sf libSDL2-2.0.so.0       libSDL2.so
ln -sf libSDL2_image-2.0.so.0 libSDL2_image.so
ln -sf libSDL2_mixer-2.0.so.0 libSDL2_mixer.so
ln -sf libSDL2_ttf-2.0.so.0   libSDL2_ttf.so

# sdl2.pc points at the sysroot; PKG_CONFIG_SYSROOT_DIR, which build-aarch64.sh
# already exports, prefixes these paths with the sysroot at link time.
cat > /tmp/r35s-sysroot/usr/lib/pkgconfig/sdl2.pc <<'PC'
prefix=/usr
exec_prefix=${prefix}
libdir=${prefix}/lib
includedir=${prefix}/include

Name: sdl2
Description: Simple DirectMedia Layer
Version: 2.28.5
Libs: -L${libdir} -lSDL2
Cflags: -I${includedir}/SDL2 -D_REENTRANT
PC
# SDL2_image.pc, SDL2_mixer.pc and SDL2_ttf.pc follow the same shape.
cp -r /usr/include/SDL2 /tmp/r35s-sysroot/usr/include/
```

Then, because `--features` on a virtual workspace needs package-qualified names:

```bash
PKG_CONFIG_ALLOW_CROSS=1 CROSS_SYSROOT=/tmp/r35s-sysroot scripts/build-aarch64.sh \
    --release --workspace --examples --no-default-features \
    --features giochi-engine/sdl2-system,giochi-engine-3d/sdl2-system,\
giochi-engine-capi/sdl2-system,giochi-engine-lua/sdl2-system,giochi-demo/sdl2-system
```

`scripts/hardware-check.sh` forwards those two options, so the whole device run is:

```bash
PKG_CONFIG_ALLOW_CROSS=1 scripts/hardware-check.sh \
    --host root@10.55.0.2 --sysroot /tmp/r35s-sysroot \
    --no-default-features --features <the list above>
```

The script looks for the satellites under `$CROSS_SYSROOT/usr/lib/<triple>`,
`$CROSS_SYSROOT/usr/lib/aarch64-linux-gnu`, `$CROSS_SYSROOT/lib/aarch64-linux-gnu`
and `$CROSS_SYSROOT/usr/lib`, and adds a `-Lnative=` flag so `rustc` finds them.

A **minimal** sysroot is enough: the four SDL2 shared objects and their `lib*.so`
symlinks, about 2 MB. Lifting the whole `usr/lib` over the console's USB-Ethernet link
takes minutes and hundreds of megabytes, and it is not needed for either link:

```bash
mkdir -p /tmp/console-sysroot/usr/lib/aarch64-linux-gnu
cd /tmp/console-sysroot/usr/lib/aarch64-linux-gnu
for so in $(ssh root@console 'ls /usr/lib/libSDL2*.so.*'); do
    scp -q root@console:"$so" .
done
# The unversioned names are what -lSDL2_image resolves against.
ln -sf libSDL2-2.0.so.0       libSDL2.so
ln -sf libSDL2_image-2.0.so.0 libSDL2_image.so
ln -sf libSDL2_mixer-2.0.so.0 libSDL2_mixer.so
ln -sf libSDL2_ttf-2.0.so.0   libSDL2_ttf.so
```

`libgiochi.so` is a shared object, so the ARM64 linker accepts unresolved references into
its own dependencies and records them as `DT_NEEDED`; the device's loader resolves them at
start-up, where they already exist. The transitive closure (`libpng16`, `libjpeg`,
`libtiff`, `libwebp`, `libfreetype`, `libharfbuzz`, `libvorbisfile`, `libmodplug`,
`libmpg123`, `libopusfile`, `libfluidsynth`, …) is only required if you link
`libgiochi.a` **statically**, or if you want `ldd`-style verification on the build host.
Verified by cross-building the workspace and both C consumers from a sysroot of exactly
four files.

**A sysroot without libc headers is still enough for the Rust build.** A vendor
rootfs often ships only the shared objects. Cargo needs the ARM64 SDL2 `.so`s and the
`.pc` files, nothing else, so the sysroot works as-is. Compiling a *C* consumer
against it (`giochi-engine-capi/tests/c_smoke*.c`) needs libc headers from the host's
cross toolchain (`/usr/aarch64-linux-gnu/include`) plus
`-Wl,--allow-shlib-undefined -Wl,-rpath-link=$CROSS_SYSROOT/usr/lib/aarch64-linux-gnu`,
because the sysroot's `.so`s reference libc, freetype and harfbuzz, which are not in it.

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

## Linking the SDL2 already in the image (`sdl2-system`)

`bundled` compiles SDL2 from source through CMake, which is what the CI cross jobs do
and what every command above assumes. The alternative is to link the SDL2 the target
image already ships:

```bash
PKG_CONFIG_ALLOW_CROSS=1 \
PKG_CONFIG_LIBDIR=/usr/lib/aarch64-linux-gnu/pkgconfig:/usr/share/pkgconfig \
    scripts/build-aarch64.sh --release \
        --no-default-features --features sdl2-system
```

Three things have to be true, and each of them fails differently:

- **`--no-default-features`.** `sdl2-bundled` is the default, and it does not replace
  `sdl2-system` — cargo unions features, so asking for both enables both. `sdl2-sys`
  gives `bundled` priority and runs CMake anyway.
- **`PKG_CONFIG_ALLOW_CROSS=1`.** Without it pkg-config refuses to answer for a
  foreign target: `... is for a different platform; use PKG_CONFIG_ALLOW_CROSS=1`.
- **`PKG_CONFIG_LIBDIR`, not `PKG_CONFIG_PATH`.** The latter *adds* to the host's
  search path, so pkg-config finds `/usr/lib/x86_64-linux-gnu/pkgconfig/sdl2.pc` first
  and the link resolves to host sonames.

The env plumbing above is not wired into `scripts/build-aarch64.sh` because the CMake
path is the one the cross workflow exercises; when this path is promoted to the default
it should move into the script, behind the same feature detection.

**The feature has to be reachable from every crate that a game depends on.** Cargo
unions features across the whole graph, so a crate that depends on `giochi-engine`
without `default-features = false` re-enables `sdl2-bundled` for everybody downstream,
and the caller has no way to opt out — the manifests look correct and only the
resolution is wrong. That happened to `giochi-engine-3d`, which made the cross-compile
feature unusable from a 3D game: the build silently went to CMake and failed without a
toolchain. `scripts/check-features.sh` now asserts the resolution for every
consumer-facing crate (default → `bundled`, `sdl2-system` → `pkg-config` and never both),
and CI compiles `giochi-engine-capi` against the system SDL2 so the pkg-config path is
executed rather than merely resolved.

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

**Cross isolation takes three settings, and two of them are easy to miss.** Setting
only `CMAKE_SYSROOT` changes the compiler's default search paths; it does **not**
restrict `find_library`/`find_package`, so CMake still walks
`/usr/lib/x86_64-linux-gnu`, finds `libEGL`/`libpulse`/`libwayland` there and hands
the aarch64 linker flags that resolve to host sonames. `CMAKE_FIND_ROOT_PATH` is the
missing half. And SDL2 probes X11, Wayland, EGL, PulseAudio and sndio through
**pkg-config**, which knows nothing about either CMake variable: left alone it answers
with the host's `.pc` files, so the failures come back through a third channel.
`scripts/build-aarch64.sh` therefore exports both `PKG_CONFIG_SYSROOT_DIR` and
`PKG_CONFIG_LIBDIR` (not just `PKG_CONFIG_PATH` — the latter *adds* to the host's
default search path rather than replacing it). With all three in place the probes fail,
which is the correct answer: the target does not have those subsystems.

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
==> cpu:         cortex-a35 +crc,+aes,+sha2
==> satellites:  /usr/lib/aarch64-linux-gnu
==> profile:     release
...
==> produced binaries:
target/aarch64-unknown-linux-gnu/release/giochi-demo: ELF 64-bit LSB pie executable, ARM aarch64, ...

aarch64 build OK.
```

That assertion is deliberate: a mis-set linker can quietly produce host binaries
when nothing imports the target architecture explicitly.

## Choosing the CPU (`GIOCHI_TARGET_CPU`)

The build is compiled for the console's core, not for baseline ARMv8-A.
`-C target-cpu=native` **cannot** be used: cargo runs the compiler on the host, and
during a cross build "native" describes the host, not the device. The core is named
instead, and `GIOCHI_TARGET_CPU` selects it:

```bash
GIOCHI_TARGET_CPU=cortex-a72 scripts/build-aarch64.sh --release
```

If it is not set, it is `cortex-a35`. The value must be one rustc knows:

```bash
rustc --print target-cpus --target aarch64-unknown-linux-gnu | grep cortex-a
```

| Board / SoC | Core | `GIOCHI_TARGET_CPU` | Target features |
| --- | --- | --- | --- |
| Powkiddy X35H (Rockchip PX30 / RK3326) | 4× Cortex-A35 | `cortex-a35` (default) | `+crc,+aes,+sha2` |
| Raspberry Pi 3 / many RK3328 boards | 4× Cortex-A53 | `cortex-a53` | `+crc,+aes,+sha2` |
| Raspberry Pi 4 | 4× Cortex-A72 | `cortex-a72` | `+crc,+aes,+sha2` |
| Raspberry Pi 5 | 4× Cortex-A76 | `cortex-a76` | `+crc,+aes,+sha2` |

Two rules, both of which have already been broken once in this repository:

**Take the feature list from the device, not from a datasheet or a plan.** Read it:

```bash
ssh root@device 'grep Features /proc/cpuinfo'
```

On the PX30 that is `fp asimd evtstrm aes pmull sha1 sha2 crc32 cpuid`. `crc32`, `aes`
and `sha2` are there, so `+crc`, `+aes` and `+sha2` are real. **`atomics` is not**, and LSE
atomics are ARMv8.1 while the A35 is ARMv8.0-A: `+lse` emits instructions the core
cannot decode, so the binary dies with `SIGILL` on the first atomic — which in this
engine is an `Arc::clone` on the first frame, not an exotic corner. An unknown CPU is
given **no** target features by the script on purpose: a wrong `+feature` is a SIGILL
on a device nobody is watching, while a missing one is merely slower.

**`RUSTFLAGS` hides `.cargo/config.toml`.** Cargo ignores the `rustflags` table in
`.cargo/config.toml` completely once `RUSTFLAGS` is set in the environment, and
`scripts/build-aarch64.sh` sets it to add the sysroot to the link search path. That is
why the CPU flags live in the script as well as in the config file, and why a build
that looked correctly configured compiled for baseline ARMv8-A for a while without
anything failing. To confirm what the compiler was actually told:

```bash
CROSS_SYSROOT=... RUSTC_WRAPPER=/tmp/probe.sh scripts/build-aarch64.sh --release
grep -o 'target-cpu=[a-z0-9-]*' /tmp/probe.log | sort -u
```

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
  `scripts/check-aarch64.sh` — the fast, link-free check, on every push.
- [`.github/workflows/cross-compile.yml`](../.github/workflows/cross-compile.yml)
  `link`: the multiarch setup above, then
  `scripts/build-aarch64.sh --workspace --all-targets`. Runs only when engine sources,
  `cmake/`, `scripts/` or the manifests change.
- [`.github/workflows/release.yml`](../.github/workflows/release.yml) `build-aarch64`:
  the same recipe with `--release`, packaged into
  `giochi-engine-aarch64-unknown-linux-gnu.tar.gz` (binary, `include/giochi.h`,
  `lib/libgiochi.{so,a}`, `lua/mini_game.lua`, docs, assets).

All three were verified in a clean `ubuntu:24.04` container, running the exact `run:`
steps of each job, and then on the real runners:

| Workflow | Jobs | Result |
|---|---|---|
| `ci.yml` | gates, ffi, cross-check, smoke | green |
| `cross-compile.yml` | check, link | green after pinning `ubuntu-24.04` |
| `release.yml` | build-x86_64, build-aarch64 | same recipe as `link` |
