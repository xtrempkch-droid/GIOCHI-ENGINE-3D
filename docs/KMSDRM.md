# Running on a Linux console with KMSDRM

GIOCHI-ENGINE's production target is a Linux box that boots straight into the game:
no X11, no Wayland, no compositor. SDL2's `kmsdrm` video driver talks to
DRM/KMS directly, which is exactly what a console needs.

---

## 1. How the driver is chosen

The engine never guesses twice. `Platform::new` negotiates once:

| Situation | Result |
|---|---|
| `SDL_VIDEODRIVER` set by the operator | Used verbatim. Never replaced, never retried. A failure is a hard error. |
| `SDL_VIDEODRIVER` unset | The engine sets it to `kmsdrm` and tries. |
| Engine-selected `kmsdrm` fails | A warning, the variable is unset, and SDL's default desktop driver is retried. |
| That also fails | `GiochiError::Platform` with the driver name, SDL's message, the DRM devices present and the relevant hints. |

`Platform::video_driver_choice()` reports which of these happened
(`Explicit`, `AutoKmsdrm`, `AutoFallback`), and the resolved driver is available
from `video_driver_name()`.

**Production builds should pin the driver** so a missing DRM device fails loudly
instead of quietly running on a fallback:

```bash
SDL_VIDEODRIVER=kmsdrm ./giochi-demo
```

---

## 2. Environment hints SDL2 honours

| Variable | Purpose |
|---|---|
| `SDL_VIDEODRIVER=kmsdrm` | Select the KMSDRM backend explicitly. |
| `SDL_KMSDRM_REQUIRE_DRM_MASTER=1` | Require DRM master. Needed when the engine owns the console and must page-flip. On a desktop session that already holds master, this makes the backend fail — which is useful in production and annoying in development. |
| `SDL_KMSDRM_DEVICE_INDEX=<n>` | Pick `/dev/dri/cardN` when the machine has more than one DRM device (common on ARM SBCs with a separate HDMI controller). |
| `SDL_VIDEO_EGL_DRIVER` | EGL driver name (e.g. `libEGL.so.1`), if the vendor ships a specific one. |
| `SDL_VIDEO_GL_DRIVER` | GL/GLES library used for the EGL context (e.g. `libGLESv2.so.2`). |

These names are exported as constants from `giochi_engine::platform::sdl`
(`HINT_KMSDRM_REQUIRE_DRM_MASTER`, `HINT_KMSDRM_DEVICE_INDEX`,
`HINT_VIDEO_EGL_DRIVER`, `HINT_VIDEO_GL_DRIVER`, `KMSDRM_VIDEO_DRIVER`,
`SDL_VIDEODRIVER_ENV`), so a launcher does not have to hardcode strings.

---

## 3. Permissions

The process must be able to open the DRM device and the render node:

```bash
ls -l /dev/dri
# crw-rw----+ 1 root video  226,   1 card0
# crw-rw----+ 1 root render 226, 128 renderD128

sudo usermod -aG video,render "$USER"
# log out and back in for the group change to take effect
```

Check with `id` that both groups are present. On a locked-down console image, a
udev rule granting the game user access to the card node is the usual alternative.

If the backend cannot open the device, the engine's error message names the
devices that *are* present, which distinguishes "no permission" from "no device".

---

## 4. Typical failure messages, and what they mean

**`kmsdrm not available`**

SDL2 was built without the KMSDRM backend. The bundled SDL2 in this repository is
configured with `SDL_KMSDRM=ON` (verify with
`grep SDL_KMSDRM target/<triple>/debug/build/sdl2-sys-*/out/build/CMakeCache.txt`).
If you build against a system SDL2, install `libdrm-dev`, `libgbm-dev` and
`libegl1-mesa-dev` before building it.

**`/dev/dri is not accessible (Permission denied)`**

Group membership, as in §3.

**`DRM devices found in /dev/dri: ... If another process holds DRM master`**

Another process owns the console: a desktop session, or — on a shipped console — the
launcher that booted it, which holds `/dev/dri/card0` open for its whole life (see
§8.1 for how to find it). Either stop it, switch to a virtual console
(`Ctrl+Alt+F3`), or unset `SDL_KMSDRM_REQUIRE_DRM_MASTER` while developing.

**`failed to initialise the SDL2 video subsystem: No available video device`**

Nothing matched: no DRM, no X11, no Wayland. In a container, mount `/dev/dri` and
grant `--device /dev/dri`.

---

## 5. Runtime libraries on the target

`SDL_KMSDRM_SHARED=ON` makes the bundled SDL2 `dlopen` its GPU support at runtime,
which is why the cross build links without ARM64 copies of those libraries. The
target root filesystem must therefore provide:

* `libdrm.so.2`
* `libgbm.so.1`
* `libEGL.so.1`
* `libGLESv2.so.2` (or whatever `SDL_VIDEO_GL_DRIVER` names)

The SDL2 satellites are **not** bundled — they are linked against at build time
(see [CROSS_COMPILE.md](CROSS_COMPILE.md)) and must be present at run time in the
same ABI versions:

* `libSDL2_image-2.0.so.0`
* `libSDL2_mixer-2.0.so.0`
* `libSDL2_ttf-2.0.so.0`

plus their own dependencies (`libpng16`, `libjpeg`, `libtiff`, `libwebp`,
`libfreetype`, `libharfbuzz`, `libvorbisfile`, `libmodplug`, `libmpg123`,
`libopusfile`, `libfluidsynth`, …). `ldd` on the built binary lists the exact set.

A minimal console image should also ship ALSA (`libasound.so.2`) for the mixer; a
missing sound stack is not fatal — the engine starts in silent mode.

---

## 6. Frame pacing on a console

KMSDRM page-flips on vblank, so the display rate is whatever the panel runs at
(typically 60 Hz). The engine's fixed timestep is independent of that:

* `Engine::new` derives the fixed step from `EngineConfig::target_fps` (60 by
  default), so a 50 Hz panel still simulates at 60 Hz;
* `RenderContext::with_vsync(window, true)` is what the engine uses by default; the
  `vsync` field of `EngineConfig` controls it;
* the loop sleeps only the remainder of the frame budget
  (`GameLoop::sleep_ms`), so a vsync-blocked `present` does not double-sleep.

On a console the practical result is one `present` per vblank and a stable 60 Hz
simulation, which is the configuration `mini_game` and the demo were validated with.

---

## 7. Quick verification recipe

On the console itself:

```bash
# 1. The device is there and readable
ls -l /dev/dri

# 2. SDL sees the backend (fails loudly if not)
SDL_VIDEODRIVER=kmsdrm ./giochi-demo --frames 60

# 3. The engine reports the driver it actually used
RUST_LOG=info SDL_VIDEODRIVER=kmsdrm ./giochi-demo --verify
```

`--verify` runs a single frame through the full `Engine` pipeline and prints the
live texture and sound counts plus whether audio is available, which is the fastest
way to confirm that a deployment is wired correctly.

---

## 8. Measured on real hardware

Everything below was observed on a **Rockchip PX30** handheld (Buildroot 2024.02,
kernel 6.12, aarch64, 640x480 DSI panel, glibc 2.38, no X11 and no Wayland). Each
item cost time to diagnose and none is predictable from the sections above.

### 8.1 A shipped console already holds DRM master

The device boots into its own launcher, which keeps `/dev/dri/card0` and
`renderD128` open for its whole life. `kmsdrm` then fails with the "another process
holds DRM master" message from §4, even though nothing resembling a desktop session
is running — so that message does **not** only mean "stop your X server".

Find the holder without `fuser` (BusyBox images often lack it):

```bash
for p in /proc/[0-9]*; do for f in $p/fd/*; do
    case "$(readlink "$f" 2>/dev/null)" in /dev/dri/*) echo "${p#/proc/} $(cat $p/comm)";; esac
done; done | sort -u
```

On our image `giochi-init` restarts the launcher within a second, so releasing the
console for a test needs a wider window than killing it. Making the launcher
non-executable is what the supervisor honours (`[ -x "$LAUNCHER" ] ||` falls into a
wait loop that polls every `LAUNCHER_WAIT` seconds, 30 by default).

Three details of that recipe are counter-intuitive, and each one cost a failed run:

**`chmod -x` is not enough on this image.** BusyBox applies the umask to a symbolic
mode, and the image runs with `umask 0077`, so `chmod -x` produced `-rw-r-xr-x` —
still executable, and since everything runs as `root` the supervisor's `[ -x ]` stayed
true and it respawned the launcher mid-test. Use an explicit mode and verify it:

```bash
chmod 0644 /opt/giochi/apps/launcher/bin/giochi-launcher
[ -x /opt/giochi/apps/launcher/bin/giochi-launcher ] && echo "still executable"
```

**Do not kill `giochi-init` to release the console.** `/etc/inittab` registers it as
`::respawn:/usr/bin/giochi-init`, so a kill only makes BusyBox start a fresh copy. A
supervisor that was blocked in `"$LAUNCHER"` leaves its launcher child orphaned
(`PPid: 1`) and that orphan keeps DRM master, so the sequence "kill init, kill
launcher, kill init again" chases its own tail. Killing the launcher once is enough:
the supervisor classifies exit status 143 (SIGTERM) as a deliberate exit, zeroes its
crash quota and goes to wait for the launcher to reappear.

**SIGTERM to the launcher usually works but is not guaranteed.** Its `SigCgt` includes
signal 15, and on the same unit it sometimes exited on TERM and sometimes ignored it.
Give it a grace period and then escalate to SIGKILL; one KILL is one of the
supervisor's five crashes, which is well inside tolerance.

The whole sequence, measured to leave the screen free:

```bash
chmod 0644 /opt/giochi/apps/launcher/bin/giochi-launcher
kill -TERM <launcher-pid>     # wait; if /dev/dri is still held, kill -KILL it
# ... run the engine against /dev/dri/card0 ...
chmod 0755 /opt/giochi/apps/launcher/bin/giochi-launcher
rm -f /run/giochi/launcher-parado
# the supervisor's own poll brings the launcher back within LAUNCHER_WAIT
```

`scripts/hardware-check.sh` does exactly this and confirms success by checking that
something holds `card0` again — a restore that only re-enables the file without
verifying the holder is how a script reports success on a dead screen.

**The trap: a broken test binary can leave the console dead.** The supervisor gives
up after `RESTART_MAX=5` failures inside `RESTART_WINDOW=60` seconds, and then parks
in `while :; do sleep 3600; done` — it stays alive (the kernel needs an init) but
never starts the launcher again. The screen goes black and no amount of rebooting
the *app* helps. Deploying a test binary that crashes on startup five times will do
exactly this. Recovery is a fresh `kill -TERM` on `giochi-init`, which re-enters
`main` with a fresh quota. Leave a marker when this happens rather than guessing
later — on failure the supervisor writes `quedas_repetidas` into `$PARADA_FILE`.

### 8.2 The driver name is `KMSDRM`, in capitals

SDL2's bootstrap for this backend is registered as `"KMSDRM"`, while `x11` and
`wayland` are lower case. `strcmp(driver, "kmsdrm") == 0` therefore never matches on
a console, and the mistake is invisible on a desktop. Compare case-insensitively;
`giochi_engine_video_driver()`, `app:video_driver()` and `Engine::video_driver_name`
all return SDL's own spelling.

### 8.3 KMSDRM is fullscreen-only: the requested size is ignored

Asking for 1280x720 on a 640x480 panel **opens 640x480**. There is no window manager
to negotiate a window size and no scaling. Consequences:

* `giochi_render_size` / `RenderContext::size()` return the real drawable, which is
  the panel mode, not `EngineConfig::width/height`;
* the engine logs both when they differ
  (`requested 1280x720, backend opened 640x480`), because a scene laid out for the
  configured size is silently cropped otherwise;
* a screenshot taken with `--quadro` is the panel's size, not the configured one.

Design scenes against the reported size. Do not assume the configured size survives.

### 8.4 `libSDL2_image` may be built without any external codec

On this image `libSDL2_image` links only `libSDL2`, `libm` and `libc`: PNG and JPG
are compiled as "not supported" stubs, and `IMG_Init(PNG)` fails. Because
`IMG_Init` fails when *any* requested flag is unavailable, the engine used to
declare the whole subsystem dead — even though BMP, TGA, PNM and QOI need no flags
and kept working. Since ADR-034 the subsystem is opened with no flags and each
optional decoder is probed separately, so this target logs a warning and still
decodes BMP.

Check what an image actually has before blaming the engine:

```bash
objdump -p /usr/lib/libSDL2_image-2.0.so.0 | grep NEEDED   # no libpng/libjpeg = no PNG/JPG
```

Ship BMP assets to such a target, or rebuild `libSDL2_image` with `libpng`.

The repository does the former without giving up PNG on capable builds: every
asset under `assets/` is committed twice, `player.png` **and** `player.bmp`, and the
demo, the Lua example and both ABI smoke tests try the PNG first and fall back to the
BMP when the decoder is missing. `scripts/png-to-bmp.py` regenerates the twins (the
pixels are identical, so nothing looks different on a desktop), `--check` fails the
gate when one drifts, and `giochi-engine/tests/asset_formats.rs` decodes both through
SDL. Before this, a plain checkout of the repository could not run its own demo on
this device at all — only a hand-converted BMP renamed to `.png` worked.

### 8.5 Numbers from that run

Measured with the release ARM64 build described in
[CROSS_COMPILE.md](CROSS_COMPILE.md), 640x480 panel, vsync off and the default
60 Hz timestep:

| Measurement | Result |
| --- | --- |
| `giochi-demo --frames 300` | 59.4 fps, 303 steps |
| `giochi-demo lua mini_game.lua --auto --frames 1200` | 59.5 fps, 16 points, 0 live textures |
| Resident memory (`VmRSS`) | 21.9 MB (`VmPeak` 181 MB) |
| C ABI consumer (`c_smoke`) | `OK (abi 1, engine 0.1.x)` |
| Audio | available (ALSA, `/dev/snd/pcmC0D0p`) |

The texture count returning to zero after a 1200-frame game is the leak check: the
Lua binding frees textures on `Drop`, and the registry reports nothing left behind.

### 8.6 Repeating the run without the archaeology

`scripts/hardware-check.sh` performs everything §8.1–§8.5 describes, in the order
that actually works, and restores the console afterwards even on Ctrl-C or on a
failure:

```bash
scripts/hardware-check.sh --host root@10.55.0.2 --sysroot /tmp/r35s-sysroot
```

It cross-builds, deploys to `/opt/giochi`, frees DRM by parking the launcher,
runs the demo, a frame capture, the Lua reference game and the C consumer, brings
the captured frame back for inspection, prints the battery, and then re-enables the
launcher and confirms the console came back. Use `--skip-build` to reuse an existing
`target/aarch64-unknown-linux-gnu/release`, and `--skip-c-smoke` when no
`aarch64-linux-gnu-gcc` is available.

Every remote snippet sticks to busybox `sh`/`awk`/`ps`/`readlink`: the target image
has no bash, no `python3`, no `timeout` and no `fuser`.

### 8.7 The BMP fallback is not a theory

A second run on the same unit checked which asset variant actually loads, with the
demo logging at `debug`:

```text
DEBUG giochi_demo] textura 'player.png' indisponível: asset error: failed to load
                   texture 'assets/player.png': Unsupported image format
DEBUG giochi_demo] textura carregada: player.bmp
```

So on this image the PNG is genuinely undecodable and the committed BMP twin
(ADR-036) is what keeps the demo running — including from the release tarball, whose
`assets/` ships both files.
