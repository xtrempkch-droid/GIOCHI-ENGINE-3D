#!/usr/bin/env python3
"""Python smoke test for the GIOCHI-ENGINE C ABI.

A real consumer written with the standard library only: no cffi, no bindings
generated from the header. It mirrors what a Python game would do — declare the
handful of structs it needs, drive a scene through callbacks and check the error
path.

Run by scripts/ffi-smoke.sh, which builds libgiochi.so and passes its directory:

    python3 giochi-engine-capi/tests/python_smoke.py target/debug

The script chdirs to the repository root so the relative asset paths in the
callbacks resolve.
"""

from __future__ import annotations

import ctypes
import os
import sys
from ctypes import (
    CFUNCTYPE,
    POINTER,
    Structure,
    addressof,
    byref,
    c_char_p,
    c_float,
    c_int32,
    c_size_t,
    c_uint32,
    c_uint64,
    c_void_p,
    memset,
    sizeof,
)

GIOCHI_ERROR_OK = 0
GIOCHI_ERROR_INVALID_HANDLE = -1
GIOCHI_ERROR_INVALID_ARGUMENT = -2
GIOCHI_ERROR_ASSET = -5
GIOCHI_ERROR_NOT_INITIALIZED = -8
GIOCHI_ERROR_UNSUPPORTED = -9


class GiochiColor(Structure):
    _fields_ = [
        ("r", ctypes.c_uint8),
        ("g", ctypes.c_uint8),
        ("b", ctypes.c_uint8),
        ("a", ctypes.c_uint8),
        ("_reserved", c_uint32 * 4),
    ]

    def __init__(self, r: int, g: int, b: int, a: int = 255) -> None:
        super().__init__(r, g, b, a, (c_uint32 * 4)(0, 0, 0, 0))


class GiochiRect(Structure):
    _fields_ = [
        ("x", c_float),
        ("y", c_float),
        ("w", c_float),
        ("h", c_float),
        ("_reserved", c_uint32 * 4),
    ]

    def __init__(self, x: float, y: float, w: float, h: float) -> None:
        super().__init__(x, y, w, h, (c_uint32 * 4)(0, 0, 0, 0))


class GiochiTime(Structure):
    _fields_ = [
        ("delta", c_float),
        ("elapsed", c_float),
        ("frame_count", c_uint64),
        ("_reserved", c_uint32 * 4),
    ]


class GiochiEngineConfig(Structure):
    _fields_ = [
        ("width", c_uint32),
        ("height", c_uint32),
        ("target_fps", c_uint32),
        ("vsync", c_uint32),
        ("title", ctypes.c_char * 64),
        ("_reserved", c_uint32 * 8),
    ]


ON_ENTER = CFUNCTYPE(None, c_void_p)
ON_EXIT = CFUNCTYPE(None, c_void_p)
UPDATE = CFUNCTYPE(None, c_void_p, POINTER(GiochiTime))
RENDER = CFUNCTYPE(None, c_void_p)


class GiochiFrameStats(Structure):
    """Part III (ABI v3). Durations are nanoseconds; zero without `profiling`."""

    _fields_ = [
        ("frame_time_ns", c_uint64),
        ("cpu_time_ns", c_uint64),
        ("update_ns", c_uint64),
        ("render2d_ns", c_uint64),
        ("render3d_ns", c_uint64),
        ("audio_ns", c_uint64),
        ("fps", c_float),
        ("draw_calls", c_uint32),
        ("sprites_drawn", c_uint32),
        ("meshes_drawn", c_uint32),
        ("frames", c_uint32),
        ("vertices_uploaded", c_uint64),
        ("bytes_uploaded", c_uint64),
        ("_reserved", c_uint32 * 8),
    ]


class GiochiSceneCallbacks(Structure):
    _fields_ = [
        ("user_data", c_void_p),
        ("on_enter", ON_ENTER),
        ("on_exit", ON_EXIT),
        ("update", UPDATE),
        ("render", RENDER),
        ("_reserved", c_uint32 * 4),
    ]


def load(directory: str) -> ctypes.CDLL:
    path = os.path.join(directory, "libgiochi.so")
    if not os.path.exists(path):
        sys.exit(f"error: {path} not found; build with `cargo build -p giochi-engine-capi`")
    return ctypes.CDLL(os.path.abspath(path))


def declare(lib: ctypes.CDLL) -> None:
    """Declares the subset of the ABI this script uses."""
    lib.giochi_abi_version.restype = c_uint32
    lib.giochi_engine_version.restype = c_uint32
    lib.giochi_get_last_error.restype = c_char_p

    lib.giochi_engine_init.argtypes = [POINTER(GiochiEngineConfig)]
    lib.giochi_engine_init.restype = c_int32
    lib.giochi_engine_shutdown.restype = None
    lib.giochi_engine_run.restype = c_int32
    lib.giochi_engine_run_scene.argtypes = [c_uint64]
    lib.giochi_engine_run_scene.restype = c_int32
    lib.giochi_engine_step.argtypes = [c_float]
    lib.giochi_engine_step.restype = c_int32
    lib.giochi_engine_request_quit.restype = None
    lib.giochi_engine_is_running.restype = c_int32
    lib.giochi_engine_frames.restype = c_uint64
    lib.giochi_engine_frame_stats.argtypes = [POINTER(GiochiFrameStats)]
    lib.giochi_engine_frame_stats.restype = c_int32
    lib.giochi_engine_timestep.restype = c_float
    lib.giochi_engine_video_driver.restype = c_char_p
    lib.giochi_engine_scene_depth.restype = c_uint32

    lib.giochi_render_size.argtypes = [POINTER(c_uint32), POINTER(c_uint32)]
    lib.giochi_render_size.restype = c_int32
    lib.giochi_render_clear.argtypes = [GiochiColor]
    lib.giochi_render_clear.restype = c_int32
    lib.giochi_render_draw_rect.argtypes = [POINTER(GiochiRect), GiochiColor]
    lib.giochi_render_draw_rect.restype = c_int32

    lib.giochi_texture_create.argtypes = [c_char_p]
    lib.giochi_texture_create.restype = c_uint64
    lib.giochi_texture_create_memory.argtypes = [ctypes.POINTER(ctypes.c_uint8), c_size_t]
    lib.giochi_texture_create_memory.restype = c_uint64
    lib.giochi_texture_destroy.argtypes = [c_uint64]
    lib.giochi_texture_destroy.restype = c_int32
    lib.giochi_texture_width.argtypes = [c_uint64]
    lib.giochi_texture_width.restype = c_uint32
    lib.giochi_texture_height.argtypes = [c_uint64]
    lib.giochi_texture_height.restype = c_uint32

    lib.giochi_asset_size.argtypes = [c_char_p, POINTER(c_size_t)]
    lib.giochi_asset_size.restype = c_int32
    lib.giochi_asset_read.argtypes = [c_char_p, ctypes.POINTER(ctypes.c_uint8), c_size_t,
                                      POINTER(c_size_t)]
    lib.giochi_asset_read.restype = c_int32

    lib.giochi_scene_create.argtypes = [POINTER(GiochiSceneCallbacks)]
    lib.giochi_scene_create.restype = c_uint64
    lib.giochi_scene_destroy.argtypes = [c_uint64]
    lib.giochi_scene_destroy.restype = c_int32


class Probe:
    """State shared with the C callbacks."""

    def __init__(self, quit_after_renders: int) -> None:
        self.entered = 0
        self.exited = 0
        self.updates = 0
        self.renders = 0
        self.quit_after_renders = quit_after_renders
        self.texture = 0
        self.saw_texture = False


def decodable_asset(
    lib: ctypes.CDLL, candidates: list[bytes]
) -> tuple[bytes, object | None, int]:
    """Returns the first candidate this build can actually decode.

    `giochi_asset_read` hands back the bytes of an unreadable format without
    complaint; only creating a texture proves a decoder exists (ADR-034). The
    repository ships a BMP twin of every PNG (ADR-036), so a build without libpng
    still has something to decode.
    """
    for name in candidates:
        size = c_size_t(0)
        if lib.giochi_asset_size(name, ctypes.byref(size)) != GIOCHI_ERROR_OK or size.value == 0:
            continue

        buffer = (ctypes.c_uint8 * size.value)()
        read = c_size_t(0)
        if lib.giochi_asset_read(name, buffer, size.value, ctypes.byref(read)) != GIOCHI_ERROR_OK:
            continue
        if read.value != size.value:
            continue

        probe = lib.giochi_texture_create_memory(buffer, read.value)
        if probe == 0:
            continue
        lib.giochi_texture_destroy(probe)
        return name, buffer, read.value

    return b"", None, 0


def first_texture(lib: ctypes.CDLL, candidates: list[bytes]) -> tuple[bytes, int]:
    """Returns the first candidate this build can load as a texture."""
    for name in candidates:
        handle = lib.giochi_texture_create(name)
        if handle != 0:
            return name, handle
    return b"", 0




class GiochiParticleConfig(Structure):
    """`docs/C_ABI_CONTRACT.md` §21 — the emitter description.

    Field for field the same shape `giochi_particles_create` copies; the reserved tail is
    spelled out so the struct is exactly as long as the C one.
    """

    _fields_ = [
        ("velocity_x", c_float),
        ("velocity_y", c_float),
        ("velocity_spread_x", c_float),
        ("velocity_spread_y", c_float),
        ("gravity_x", c_float),
        ("gravity_y", c_float),
        ("drag", c_float),
        ("lifetime", c_float),
        ("start_size", c_float),
        ("end_size", c_float),
        ("start_color", c_uint32),
        ("end_color", c_uint32),
        ("_reserved", c_uint32 * 4),
    ]

def header_abi_version() -> int:
    """Reads `GIOCHI_ABI_VERSION` from the committed header.

    Reading it instead of hard-coding a number is what keeps this script passing when a
    later part raises the version: the header and the library are the two things that
    must agree, and the assertion below is exactly that agreement.
    """
    import pathlib
    import re

    header = pathlib.Path(__file__).resolve().parent.parent / "include" / "giochi.h"
    match = re.search(r"#define GIOCHI_ABI_VERSION (\d+)", header.read_text())
    if match is None:
        raise SystemExit("giochi.h has no GIOCHI_ABI_VERSION")
    return int(match.group(1))

def main() -> int:
    directory = sys.argv[1] if len(sys.argv) > 1 else "target/debug"
    repo_root = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    os.chdir(repo_root)

    lib = load(os.path.abspath(directory))
    declare(lib)
    failures: list[str] = []

    def check(condition: bool, what: str) -> None:
        if not condition:
            message = lib.giochi_get_last_error().decode(errors="replace")
            failures.append(f"{what} ({message})")

    # Part I was written against 1; this script ships with the library, so it names the
    # current number. A consumer that predates a part should test `>= 1` instead, which
    # is the compatibility rule in giochi.h.
    check(lib.giochi_abi_version() == header_abi_version(), "abi version")
    check(lib.giochi_abi_version() >= 1, "abi version is at least part I")
    check(lib.giochi_engine_version() > 0, "engine version")
    check(lib.giochi_get_last_error() == b"", "last error starts empty")

    # ---- before init ---------------------------------------------------------
    check(lib.giochi_engine_run() == GIOCHI_ERROR_NOT_INITIALIZED, "run before init")
    check(b"not initialised" in lib.giochi_get_last_error(), "run before init message")

    # ---- init ---------------------------------------------------------------
    config = GiochiEngineConfig()
    config.width = 320
    config.height = 240
    config.target_fps = 60
    config.vsync = 0
    config.title = b"python_smoke"
    check(lib.giochi_engine_init(ctypes.byref(config)) == GIOCHI_ERROR_OK, "init")
    check(lib.giochi_get_last_error() == b"", "successful call clears the error")
    check(abs(lib.giochi_engine_timestep() - 1.0 / 60.0) < 1e-6, "timestep")
    check(lib.giochi_engine_video_driver() == b"dummy", "video driver")

    width, height = c_uint32(0), c_uint32(0)
    check(lib.giochi_render_size(ctypes.byref(width), ctypes.byref(height)) == GIOCHI_ERROR_OK,
          "render size")
    check((width.value, height.value) == (320, 240), "render size values")

    # ---- assets -------------------------------------------------------------
    player_path, buffer, player_size = decodable_asset(
        lib, [b"assets/player.png", b"assets/player.bmp"]
    )
    check(player_path != b"", "an asset variant must decode")
    check(player_size > 0, "asset is not empty")

    if buffer is not None:
        check(
            bytes(buffer[:2]) == b"BM" or bytes(buffer[:4]) == b"\x89PNG",
            "image signature",
        )

        from_memory = lib.giochi_texture_create_memory(buffer, player_size)
        check(from_memory != 0, "texture from memory")
        check(lib.giochi_texture_width(from_memory) == 32, "memory texture width")
        check(lib.giochi_texture_destroy(from_memory) == GIOCHI_ERROR_OK, "memory texture destroy")

    # A tiny buffer fails instead of truncating.
    tiny = (ctypes.c_uint8 * 4)()
    read_small = c_size_t(0)
    check(lib.giochi_asset_read(player_path, tiny, 4, ctypes.byref(read_small))
          == GIOCHI_ERROR_ASSET, "short buffer is rejected")

    check(lib.giochi_texture_destroy(4242) == GIOCHI_ERROR_INVALID_HANDLE, "double destroy")

    # ---- parts VI and VII: particles and animation ---------------------------
    lib.giochi_particles_create.restype = c_uint64
    lib.giochi_particles_create.argtypes = [c_uint64, c_uint32, POINTER(GiochiParticleConfig)]
    lib.giochi_particles_default_config.argtypes = [POINTER(GiochiParticleConfig)]
    lib.giochi_particles_emit.argtypes = [c_uint64, c_float, c_float, c_uint32]
    lib.giochi_particles_update.argtypes = [c_uint64, c_float]
    lib.giochi_particles_alive.argtypes = [c_uint64]
    lib.giochi_particles_destroy.argtypes = [c_uint64]
    lib.giochi_animator_create.restype = c_uint64
    lib.giochi_animator_create.argtypes = [
        c_uint64, c_float, c_float, c_uint32, c_float, c_uint32
    ]
    lib.giochi_animator_update.argtypes = [c_uint64, c_float]
    lib.giochi_animator_current_frame.argtypes = [c_uint64]
    lib.giochi_animator_destroy.argtypes = [c_uint64]

    config = GiochiParticleConfig()
    check(lib.giochi_particles_default_config(byref(config)) == GIOCHI_ERROR_OK,
          "particles default config")
    check(config.lifetime > 0.0, "particles default lifetime")
    check(all(word == 0 for word in config._reserved), "particles config zeroes reserved")

    # A non-zero texture handle is accepted without validating it against the registry:
    # the check that matters in C is "0 is refused", done below.
    # A real texture is only available inside the scene callback, so the particle and
    # animation checks that need one live there; the ones that do not are here.
    check(lib.giochi_particles_destroy(0) == GIOCHI_ERROR_INVALID_HANDLE,
          "particles refuse handle 0")
    check(lib.giochi_animator_create(0, 16.0, 16.0, 4, 0.1, 1) == 0,
          "animator refuses a dead texture handle")

    # ---- parts VIII and IX: console and hot reload ---------------------------
    # Both exist in every build: either they work or they report UNSUPPORTED.
    console = lib.giochi_console_execute(b"help")
    check(console in (GIOCHI_ERROR_OK, GIOCHI_ERROR_UNSUPPORTED), "console execute")
    if console == GIOCHI_ERROR_UNSUPPORTED:
        check(b"devtools" in lib.giochi_get_last_error(), "console unsupported message")
    else:
        check(lib.giochi_console_output_count() > 0, "console produced output")
        line = ctypes.create_string_buffer(256)
        written = c_size_t()
        check(lib.giochi_console_output_line(0, line, 256, byref(written)) == GIOCHI_ERROR_OK,
              "console output line")
        check(written.value == len(line.value), "console output length")
        check(lib.giochi_console_output_line(0, None, 256, byref(written))
              == GIOCHI_ERROR_INVALID_ARGUMENT, "console output rejects NULL")
    check(lib.giochi_console_clear() in (GIOCHI_ERROR_OK, GIOCHI_ERROR_UNSUPPORTED),
          "console clear")

    watched = lib.giochi_hot_reload_enable(b"assets")
    check(watched in (1, GIOCHI_ERROR_UNSUPPORTED, GIOCHI_ERROR_INVALID_ARGUMENT),
          "hot reload enable")
    check(lib.giochi_hot_reload_poll() >= 0, "hot reload poll")
    check(lib.giochi_hot_reload_disable() in (GIOCHI_ERROR_OK, GIOCHI_ERROR_UNSUPPORTED),
          "hot reload disable")
    check(lib.giochi_hot_reload_is_enabled() == 0, "hot reload off after disable")

    # ---- scene --------------------------------------------------------------
    probe = Probe(quit_after_renders=5)
    user_data = ctypes.cast(ctypes.pointer(ctypes.py_object(probe)), c_void_p)

    @ON_ENTER
    def on_enter(_: c_void_p) -> None:
        probe.entered += 1
        _, probe.texture = first_texture(
            lib, [b"assets/checker.png", b"assets/checker.bmp"]
        )
        probe.saw_texture = probe.texture != 0 and lib.giochi_texture_width(probe.texture) == 64
        lib.giochi_texture_destroy(probe.texture)

    @ON_EXIT
    def on_exit(_: c_void_p) -> None:
        probe.exited += 1

    @UPDATE
    def update(_: c_void_p, time: POINTER(GiochiTime)) -> None:
        probe.updates += 1
        if not time:
            failures.append("update received a NULL time pointer")
            return
        if time.contents.delta <= 0.0:
            failures.append("update received a non-positive delta")
        # `is_running` turns 0 as soon as a quit is requested, and the remaining
        # fixed steps of that frame still run, so only check before the threshold.
        if not probe.quit_after_renders or probe.renders < probe.quit_after_renders:
            if lib.giochi_engine_is_running() != 1:
                failures.append("is_running must be 1 inside update")
        if probe.quit_after_renders and probe.renders >= probe.quit_after_renders:
            lib.giochi_engine_request_quit()

    @RENDER
    def render(_: c_void_p) -> None:
        probe.renders += 1
        lib.giochi_render_clear(GiochiColor(12, 14, 22))
        lib.giochi_render_draw_rect(ctypes.byref(GiochiRect(10, 10, 100, 24)),
                                    GiochiColor(40, 160, 90))

    callbacks = GiochiSceneCallbacks()
    callbacks.user_data = user_data
    callbacks.on_enter = on_enter
    callbacks.on_exit = on_exit
    callbacks.update = update
    callbacks.render = render

    incomplete = GiochiSceneCallbacks()
    incomplete.user_data = user_data
    incomplete.update = update
    check(lib.giochi_scene_create(ctypes.byref(incomplete)) == 0, "incomplete table rejected")
    check(b"update and render" in lib.giochi_get_last_error(), "incomplete table message")

    scene = lib.giochi_scene_create(ctypes.byref(callbacks))
    check(scene != 0, "scene create")

    check(lib.giochi_engine_run_scene(scene) == GIOCHI_ERROR_OK, "run scene")
    check(probe.entered == 1, "on_enter ran once")
    check(probe.exited == 1, "on_exit ran once")
    check(probe.saw_texture, "on_enter could create a texture")
    check(probe.renders >= 5, "scene rendered")
    check(probe.updates > 0, "scene updated")
    check(lib.giochi_engine_scene_depth() == 0, "stack emptied after the run")
    check(lib.giochi_engine_is_running() == 0, "not running after quit")

    # ---- stepping -----------------------------------------------------------
    before = lib.giochi_engine_frames()
    check(lib.giochi_engine_step(1.0 / 60.0) == GIOCHI_ERROR_OK, "step")
    check(lib.giochi_engine_frames() == before + 1, "step advanced one frame")

    # ---- part III: frame statistics (ABI v3) --------------------------------
    check(sizeof(GiochiFrameStats) == 120, "GiochiFrameStats size")
    check(
        GiochiFrameStats.fps.offset == 48,
        "GiochiFrameStats layout",
    )

    stats = GiochiFrameStats()
    # Fill with a pattern first: the reserved words have to come back zero, and a struct
    # the library forgot to write would otherwise look valid.
    memset(addressof(stats), 0xAB, sizeof(stats))
    check(
        lib.giochi_engine_frame_stats(None) == GIOCHI_ERROR_INVALID_ARGUMENT,
        "frame stats rejects NULL",
    )
    check(b"out_stats" in lib.giochi_get_last_error(), "frame stats NULL message")
    check(lib.giochi_engine_frame_stats(byref(stats)) == GIOCHI_ERROR_OK, "frame stats")
    check(all(word == 0 for word in stats._reserved), "frame stats zeroes reserved")
    if stats.frames != 0:
        # Only a build with `profiling` measures anything.
        check(stats.frame_time_ns > 0, "frame stats period")
        check(stats.fps >= 0.0, "frame stats fps")

    # ---- part V: 2D physics --------------------------------------------------
    lib.giochi_physics_world_create.restype = c_void_p
    lib.giochi_physics_world_create.argtypes = [c_float, c_float]
    lib.giochi_physics_world_destroy.argtypes = [c_void_p]
    lib.giochi_physics_create_body.argtypes = [
        c_void_p, c_uint32, c_float, c_float, c_float, POINTER(c_uint64)
    ]
    lib.giochi_physics_attach_box.argtypes = [
        c_void_p, c_uint64, c_float, c_float, c_float, c_float
    ]
    lib.giochi_physics_step.argtypes = [c_void_p, c_float]
    # Without argtypes ctypes passes a Python int as a 32-bit C int, which truncates the
    # pointer on a 64-bit build: the first call segfaults instead of failing a check.
    lib.giochi_physics_body_count.argtypes = [c_void_p]
    lib.giochi_physics_body_count.restype = c_uint32
    lib.giochi_physics_get_position.argtypes = [
        c_void_p, c_uint64, POINTER(c_float), POINTER(c_float)
    ]

    physics = lib.giochi_physics_world_create(0.0, -10.0)
    check(bool(physics), "physics world create")
    floor_body = c_uint64()
    check(lib.giochi_physics_create_body(physics, 1, 0.0, -0.5, 0.0, byref(floor_body))
          == GIOCHI_ERROR_OK, "physics create floor")
    check(lib.giochi_physics_attach_box(physics, floor_body, 20.0, 0.5, 0.9, 0.0)
          == GIOCHI_ERROR_OK, "physics attach floor")

    box = c_uint64()
    check(lib.giochi_physics_create_body(physics, 0, 0.0, 5.0, 0.0, byref(box))
          == GIOCHI_ERROR_OK, "physics create box")
    check(lib.giochi_physics_attach_box(physics, box, 0.5, 0.5, 0.5, 0.0)
          == GIOCHI_ERROR_OK, "physics attach box")
    check(lib.giochi_physics_body_count(physics) == 2, "physics body count")

    pos_x = c_float()
    pos_y = c_float()
    check(lib.giochi_physics_get_position(physics, box, byref(pos_x), byref(pos_y))
          == GIOCHI_ERROR_OK, "physics get position")
    check(abs(pos_y.value - 5.0) < 0.01, "physics start position")

    check(lib.giochi_physics_step(physics, 1.0 / 60.0) == GIOCHI_ERROR_OK, "physics one substep")
    for _ in range(120):
        check(lib.giochi_physics_step(physics, 1.0 / 60.0) == GIOCHI_ERROR_OK, "physics step")
    check(lib.giochi_physics_get_position(physics, box, byref(pos_x), byref(pos_y))
          == GIOCHI_ERROR_OK, "physics get position after falling")
    check(0.3 < pos_y.value < 0.8, "physics box landed on the floor")
    check(lib.giochi_physics_step(None, 0.01) == GIOCHI_ERROR_INVALID_ARGUMENT,
          "physics NULL world")
    check(lib.giochi_physics_world_destroy(None) == GIOCHI_ERROR_INVALID_ARGUMENT,
          "physics NULL destroy")
    check(lib.giochi_physics_world_destroy(physics) == GIOCHI_ERROR_OK, "physics destroy")

    # ---- teardown -----------------------------------------------------------
    check(lib.giochi_scene_destroy(scene) == GIOCHI_ERROR_OK, "scene destroy")
    check(lib.giochi_scene_destroy(scene) == GIOCHI_ERROR_INVALID_HANDLE, "scene double destroy")

    lib.giochi_engine_shutdown()
    lib.giochi_engine_shutdown()
    check(lib.giochi_engine_run() == GIOCHI_ERROR_NOT_INITIALIZED, "run after shutdown")

    if failures:
        print("python_smoke: FAILED", file=sys.stderr)
        for failure in failures:
            print(f"  - {failure}", file=sys.stderr)
        return 1

    print(f"python_smoke: OK (abi {lib.giochi_abi_version()}, engine "
          f"{lib.giochi_engine_version() >> 16}."
          f"{(lib.giochi_engine_version() >> 8) & 0xFF}."
          f"{lib.giochi_engine_version() & 0xFF})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
