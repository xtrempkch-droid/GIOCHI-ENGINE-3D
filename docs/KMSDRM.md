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

A running desktop session owns the console. Either stop it, switch to a virtual
console (`Ctrl+Alt+F3`), or unset `SDL_KMSDRM_REQUIRE_DRM_MASTER` while developing.

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
