#!/usr/bin/env python3
"""Python smoke test for part II of the GIOCHI-ENGINE C ABI (3D, ABI version 2).

A real consumer written with the standard library only: no cffi, no bindings
generated from the header. It declares the handful of `#[repr(C)]` structs it
passes across the boundary and drives the whole 3D cycle — engine, mesh, shader,
camera, three frames, teardown — so a Python script can put a lit cube on the
screen without a line of C.

Run by scripts/ffi-smoke.sh, which builds libgiochi.so and passes its directory:

    python3 giochi-engine-capi/tests/python_smoke_3d.py target/debug

Needs a video driver with OpenGL. `SDL_VIDEODRIVER=offscreen` provides one with no
display. When the driver has none, this exits 77 (the "skipped" convention) rather
than failing: a machine without Mesa is not a machine with a broken ABI.
"""

from __future__ import annotations

import ctypes
import os
import sys
from ctypes import POINTER, Structure, c_char_p, c_float, c_int32, c_uint32, c_uint64, c_void_p

GIOCHI_ERROR_OK = 0
GIOCHI_ERROR_INVALID_HANDLE = -1
GIOCHI_ERROR_INVALID_ARGUMENT = -2
GIOCHI_ERROR_ASSET = -5
GIOCHI_ERROR_NOT_INITIALIZED = -8

GIOCHI_SHADER_FLAT = 0
GIOCHI_SHADER_TEXTURED = 1
GIOCHI_SHADER_PHONG = 2

GIOCHI_ABI_VERSION = 2
SKIP_CODE = 77


class GiochiVec3(Structure):
    """`docs/C_ABI_CONTRACT.md` §9.2 — 28 bytes.

    The reserved tail is spelled out rather than omitted: the C struct has it, and a
    Python struct that is 12 bytes shorter would shift every field after it when passed
    by value.
    """

    _fields_ = [
        ("x", c_float),
        ("y", c_float),
        ("z", c_float),
        ("_reserved", c_uint32 * 4),
    ]


class GiochiQuat(Structure):
    _fields_ = [
        ("x", c_float),
        ("y", c_float),
        ("z", c_float),
        ("w", c_float),
        ("_reserved", c_uint32 * 4),
    ]


class GiochiColor3D(Structure):
    _fields_ = [
        ("r", c_float),
        ("g", c_float),
        ("b", c_float),
        ("a", c_float),
        ("_reserved", c_uint32 * 4),
    ]


class GiochiTransform3D(Structure):
    _fields_ = [
        ("position", GiochiVec3),
        ("rotation", GiochiQuat),
        ("scale", GiochiVec3),
        ("_reserved", c_uint32 * 4),
    ]


class GiochiCamera3D(Structure):
    _fields_ = [
        ("position", GiochiVec3),
        ("target", GiochiVec3),
        ("up", GiochiVec3),
        ("fov_y", c_float),
        ("near_plane", c_float),
        ("far_plane", c_float),
        ("aspect", c_float),
        ("_reserved", c_uint32 * 4),
    ]


class GiochiWindowConfig(Structure):
    _fields_ = [
        ("width", c_uint32),
        ("height", c_uint32),
        ("fullscreen", c_uint32),
        ("_reserved", c_uint32 * 8),
    ]


def vec3(x: float, y: float, z: float) -> GiochiVec3:
    return GiochiVec3(x, y, z, (c_uint32 * 4)(0, 0, 0, 0))


def color(r: float, g: float, b: float, a: float) -> GiochiColor3D:
    return GiochiColor3D(r, g, b, a, (c_uint32 * 4)(0, 0, 0, 0))


def load(directory: str) -> ctypes.CDLL:
    path = os.path.join(directory, "libgiochi.so")
    if not os.path.exists(path):
        sys.exit(f"error: {path} not found; build with `cargo build -p giochi-engine-capi`")
    return ctypes.CDLL(os.path.abspath(path))


def declare(lib: ctypes.CDLL) -> None:
    """Declares the part II subset this script uses.

    Every `argtypes` is explicit on purpose: ctypes defaults a missing one to `int`, which
    truncates a pointer to 32 bits on a 64-bit build and produces a segfault that looks
    like a bug in the library.
    """
    lib.giochi_abi_version.restype = c_uint32
    lib.giochi_get_last_error.restype = c_char_p

    lib.giochi_engine3d_create.argtypes = [POINTER(GiochiWindowConfig)]
    lib.giochi_engine3d_create.restype = c_void_p
    lib.giochi_engine3d_destroy.argtypes = [c_void_p]
    lib.giochi_engine3d_destroy.restype = None
    lib.giochi_engine3d_begin_frame.argtypes = [c_void_p]
    lib.giochi_engine3d_begin_frame.restype = c_int32
    lib.giochi_engine3d_end_frame.argtypes = [c_void_p]
    lib.giochi_engine3d_end_frame.restype = c_int32

    lib.giochi_camera3d_new.restype = GiochiCamera3D
    lib.giochi_camera3d_look_at.argtypes = [POINTER(GiochiCamera3D), GiochiVec3, GiochiVec3]
    lib.giochi_camera3d_look_at.restype = None

    lib.giochi_transform3d_identity.restype = GiochiTransform3D
    lib.giochi_transform3d_from_translation.argtypes = [GiochiVec3]
    lib.giochi_transform3d_from_translation.restype = GiochiTransform3D

    lib.giochi_mesh_create_cube.argtypes = [c_float, POINTER(c_uint64)]
    lib.giochi_mesh_create_cube.restype = c_int32
    lib.giochi_mesh_load_obj.argtypes = [c_char_p, POINTER(c_uint64)]
    lib.giochi_mesh_load_obj.restype = c_int32
    lib.giochi_mesh_destroy.argtypes = [c_uint64]
    lib.giochi_mesh_destroy.restype = c_int32

    lib.giochi_shader_builtin.argtypes = [c_int32, POINTER(c_uint64)]
    lib.giochi_shader_builtin.restype = c_int32
    lib.giochi_shader_destroy.argtypes = [c_uint64]
    lib.giochi_shader_destroy.restype = c_int32

    lib.giochi_renderer3d_clear.argtypes = [c_void_p, GiochiColor3D]
    lib.giochi_renderer3d_clear.restype = c_int32
    lib.giochi_renderer3d_draw_mesh.argtypes = [
        c_void_p,
        c_uint64,
        c_uint64,
        POINTER(GiochiTransform3D),
        POINTER(GiochiCamera3D),
        GiochiColor3D,
    ]
    lib.giochi_renderer3d_draw_mesh.restype = c_int32


def main() -> int:
    directory = sys.argv[1] if len(sys.argv) > 1 else "target/debug"

    lib = load(os.path.abspath(directory))
    declare(lib)
    failures: list[str] = []

    def last_error() -> str:
        return lib.giochi_get_last_error().decode(errors="replace")

    def check(condition: bool, what: str) -> None:
        if not condition:
            failures.append(f"{what} ({last_error()})")

    # ---- version ------------------------------------------------------------
    # The checklist in docs/C_ABI_CONTRACT.md §12 names this file for the version check.
    check(lib.giochi_abi_version() == GIOCHI_ABI_VERSION, "abi version is 2")
    check(GIOCHI_ABI_VERSION == 2, "the struct declarations match ABI 2")

    # ---- the structs must be the size the header documents ------------------
    for name, size in [
        ("GiochiVec3", 28),
        ("GiochiQuat", 32),
        ("GiochiColor3D", 32),
        ("GiochiTransform3D", 104),
        ("GiochiCamera3D", 116),
        ("GiochiWindowConfig", 44),
    ]:
        check(ctypes.sizeof(globals()[name]) == size, f"{name} is {size} bytes")

    # ---- before the engine exists ------------------------------------------
    handle = c_uint64(777)
    check(
        lib.giochi_mesh_create_cube(1.0, ctypes.byref(handle)) == GIOCHI_ERROR_NOT_INITIALIZED,
        "mesh before engine",
    )
    check(handle.value == 777, "a failed call leaves the out parameter alone")
    check("giochi_engine3d_create" in last_error(), "the message names the fix")

    # ---- create -------------------------------------------------------------
    config = GiochiWindowConfig()
    config.width = 320
    config.height = 240

    engine = lib.giochi_engine3d_create(ctypes.byref(config))
    if not engine:
        print(f"skipped: no OpenGL ES driver ({last_error()})", file=sys.stderr)
        return SKIP_CODE

    # A second engine would retarget the four functions that take no engine.
    check(
        not lib.giochi_engine3d_create(ctypes.byref(config)),
        "a second 3D engine is refused",
    )

    # ---- resources ----------------------------------------------------------
    cube = c_uint64(0)
    shader = c_uint64(0)
    check(lib.giochi_mesh_create_cube(1.0, ctypes.byref(cube)) == GIOCHI_ERROR_OK, "cube")
    check(cube.value != 0, "cube handle is not 0")
    check(
        lib.giochi_shader_builtin(GIOCHI_SHADER_PHONG, ctypes.byref(shader)) == GIOCHI_ERROR_OK,
        "phong shader",
    )
    check(shader.value != 0, "shader handle is not 0")

    unused = c_uint64(0)
    check(
        lib.giochi_shader_builtin(99, ctypes.byref(unused)) == GIOCHI_ERROR_INVALID_ARGUMENT,
        "unknown built-in is refused",
    )
    check(unused.value == 0, "a refused built-in hands out no handle")

    missing = c_uint64(0)
    check(
        lib.giochi_mesh_load_obj(b"/nonexistent/cube.obj", ctypes.byref(missing))
        == GIOCHI_ERROR_ASSET,
        "a missing OBJ is an asset error",
    )

    # ---- camera and transform ----------------------------------------------
    camera = lib.giochi_camera3d_new()
    check(camera.aspect == 0.0, "aspect 0.0 means ask the drawable")

    eye = vec3(1.6, 1.6, 2.4)
    origin = vec3(0.0, 0.0, 0.0)
    lib.giochi_camera3d_look_at(ctypes.byref(camera), eye, origin)
    check(abs(camera.position.y - 1.6) < 1e-6, "look_at stored the position")

    model = lib.giochi_transform3d_identity()
    check(model.scale.x == 1.0 and model.scale.z == 1.0, "identity has unit scale")

    moved = lib.giochi_transform3d_from_translation(vec3(1.0, 2.0, 3.0))
    check(moved.position.y == 2.0, "from_translation stored the position")
    check(moved.scale.x == 1.0, "from_translation kept the unit scale")

    # ---- three frames -------------------------------------------------------
    background = color(0.07, 0.09, 0.14, 1.0)
    tint = color(0.85, 0.62, 0.35, 1.0)

    for frame in range(3):
        check(lib.giochi_engine3d_begin_frame(engine) == GIOCHI_ERROR_OK, f"begin {frame}")
        check(
            lib.giochi_renderer3d_clear(engine, background) == GIOCHI_ERROR_OK,
            f"clear {frame}",
        )

        # Turn the cube a little each frame, in case the model matrix is ignored.
        angle = frame * 0.35
        model.rotation = GiochiQuat(0.0, angle, 0.0, 1.0, (c_uint32 * 4)(0, 0, 0, 0))

        check(
            lib.giochi_renderer3d_draw_mesh(
                engine, cube, shader, ctypes.byref(model), ctypes.byref(camera), tint
            )
            == GIOCHI_ERROR_OK,
            f"draw {frame}",
        )
        check(lib.giochi_engine3d_end_frame(engine) == GIOCHI_ERROR_OK, f"end {frame}")

    # ---- the error paths ----------------------------------------------------
    check(
        lib.giochi_renderer3d_draw_mesh(
            engine, cube, shader, None, ctypes.byref(camera), tint
        )
        == GIOCHI_ERROR_INVALID_ARGUMENT,
        "a NULL model is refused",
    )
    check(
        lib.giochi_renderer3d_draw_mesh(
            engine, cube, shader, ctypes.byref(model), None, tint
        )
        == GIOCHI_ERROR_INVALID_ARGUMENT,
        "a NULL camera is refused",
    )
    check(
        lib.giochi_renderer3d_draw_mesh(
            engine, 0, shader, ctypes.byref(model), ctypes.byref(camera), tint
        )
        == GIOCHI_ERROR_INVALID_HANDLE,
        "an unknown mesh is reported",
    )
    check(
        lib.giochi_renderer3d_draw_mesh(
            engine, cube, 0, ctypes.byref(model), ctypes.byref(camera), tint
        )
        == GIOCHI_ERROR_INVALID_HANDLE,
        "an unknown shader is reported",
    )

    # ---- teardown -----------------------------------------------------------
    check(lib.giochi_mesh_destroy(cube.value) == GIOCHI_ERROR_OK, "mesh destroy")
    check(
        lib.giochi_mesh_destroy(cube.value) == GIOCHI_ERROR_INVALID_HANDLE,
        "mesh double destroy",
    )
    check(lib.giochi_shader_destroy(shader.value) == GIOCHI_ERROR_OK, "shader destroy")
    check(
        lib.giochi_shader_destroy(shader.value) == GIOCHI_ERROR_INVALID_HANDLE,
        "shader double destroy",
    )

    lib.giochi_engine3d_destroy(engine)
    check(
        lib.giochi_mesh_create_cube(1.0, ctypes.byref(c_uint64(0)))
        == GIOCHI_ERROR_NOT_INITIALIZED,
        "no engine after destroy",
    )

    if failures:
        print("python_smoke_3d: FAILED", file=sys.stderr)
        for failure in failures:
            print(f"  - {failure}", file=sys.stderr)
        return 1

    print(f"python_smoke_3d: OK (abi {lib.giochi_abi_version()}, 3 frames rendered)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
