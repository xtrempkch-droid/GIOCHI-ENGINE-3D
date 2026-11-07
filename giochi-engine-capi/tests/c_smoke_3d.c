/*
 * C smoke test for part II of the GIOCHI-ENGINE C ABI (3D, GIOCHI_ABI_VERSION 2).
 *
 * A real C consumer: it includes only `giochi.h`, links against libgiochi, creates a 3D
 * engine, draws a lit cube for a few frames, and tears everything down. Built and run by
 * scripts/ffi-smoke.sh.
 *
 *   cc -std=c11 -Wall -Wextra -Werror -I giochi-engine-capi/include \
 *      giochi-engine-capi/tests/c_smoke_3d.c -L target/debug -lgiochi -o c_smoke_3d
 *
 * Exits 77 (the autotools "skipped" convention) when the video driver offers no OpenGL
 * at all, which is what `SDL_VIDEODRIVER=dummy` does. `offscreen` provides a real
 * EGL/GLES context with no display attached, so this test does not need a screen.
 *
 * What it is actually checking, beyond "it did not crash":
 *
 *   - every part II signature in giochi.h compiles and links from C;
 *   - a NULL model or camera is refused instead of being treated as the identity;
 *   - an unknown mesh, shader or built-in id is reported, not guessed;
 *   - the mesh and shader functions, which take no engine argument, find the engine
 *     that giochi_engine3d_create made;
 *   - handles are invalidated by destroy;
 *   - creating a second engine on the same thread is refused.
 */
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include "giochi.h"

#define SKIP_CODE 77

static int failures;

#define CHECK(cond)                                                          \
    do {                                                                     \
        if (!(cond)) {                                                       \
            fprintf(stderr, "FAIL %s:%d: %s\n", __FILE__, __LINE__, #cond);   \
            failures++;                                                      \
        }                                                                    \
    } while (0)

/* Fails the test and prints the engine's own message, which is the only thing that makes
 * a missing GL context or a bad OBJ path diagnosable. */
#define CHECK_OK(expr)                                                       \
    do {                                                                     \
        if ((expr) != GIOCHI_ERROR_OK) {                                     \
            fprintf(stderr, "FAIL %s:%d: %s -> %s\n", __FILE__, __LINE__, #expr, \
                    giochi_get_last_error());                                 \
            failures++;                                                      \
        }                                                                    \
    } while (0)

#define CHECK_CODE(expr, expected)                                           \
    do {                                                                     \
        const int32_t actual_ = (expr);                                      \
        if (actual_ != (expected)) {                                         \
            fprintf(stderr, "FAIL %s:%d: %s -> %d, expected %d (%s)\n",       \
                    __FILE__, __LINE__, #expr, (int)actual_, (int)(expected), \
                    giochi_get_last_error());                                 \
            failures++;                                                      \
        }                                                                    \
    } while (0)

static GiochiVec3 vec3(float x, float y, float z)
{
    GiochiVec3 v;
    memset(&v, 0, sizeof v);   /* the reserved tail must be zero */
    v.x = x;
    v.y = y;
    v.z = z;
    return v;
}

static GiochiColor3D color3d(float r, float g, float b, float a)
{
    GiochiColor3D c;
    memset(&c, 0, sizeof c);
    c.r = r;
    c.g = g;
    c.b = b;
    c.a = a;
    return c;
}

int main(void)
{
    /* ---- version ---------------------------------------------------------- */
    /* Part I consumers were written against 1 and must keep working: the rule is "at
     * least what I was built for", which is why this is not an equality against the
     * number in the header. */
    CHECK(giochi_abi_version() >= 1);
    CHECK(giochi_abi_version() == GIOCHI_ABI_VERSION);
    CHECK(GIOCHI_ABI_VERSION == 2);
    CHECK(giochi_engine_version() > 0);

    /* ---- everything before the engine exists ------------------------------ */
    {
        uint64_t handle = 1234;
        CHECK_CODE(giochi_mesh_create_cube(1.0f, &handle), GIOCHI_ERROR_NOT_INITIALIZED);
        CHECK(handle == 1234);   /* a failed call must not write the out parameter */
        CHECK(strstr(giochi_get_last_error(), "giochi_engine3d_create") != NULL);

        CHECK_CODE(giochi_mesh_create_cube(1.0f, NULL), GIOCHI_ERROR_INVALID_ARGUMENT);
        CHECK_CODE(giochi_shader_builtin(GIOCHI_SHADER_FLAT, NULL),
                   GIOCHI_ERROR_INVALID_ARGUMENT);
        CHECK_CODE(giochi_engine3d_begin_frame(NULL), GIOCHI_ERROR_INVALID_ARGUMENT);
        CHECK_CODE(giochi_engine3d_end_frame(NULL), GIOCHI_ERROR_INVALID_ARGUMENT);
    }

    /* ---- create ----------------------------------------------------------- */
    GiochiWindowConfig window_config;
    memset(&window_config, 0, sizeof window_config);
    window_config.width = 320;
    window_config.height = 240;

    GiochiEngine3D *engine = giochi_engine3d_create(&window_config);
    if (engine == NULL) {
        fprintf(stderr, "skipped: no OpenGL ES driver (%s)\n", giochi_get_last_error());
        return SKIP_CODE;
    }
    CHECK(giochi_get_last_error()[0] == '\0');

    /* A second engine on the same thread would retarget the four functions that take no
     * engine, so it is refused rather than allowed. */
    CHECK(giochi_engine3d_create(&window_config) == NULL);
    CHECK(strstr(giochi_get_last_error(), "already exists") != NULL);

    /* ---- meshes and shaders ----------------------------------------------- */
    uint64_t cube = 0;
    uint64_t shader = 0;

    CHECK_CODE(giochi_mesh_create_cube(1.0f, &cube), GIOCHI_ERROR_OK);
    CHECK(cube != 0);
    CHECK_OK(giochi_shader_builtin(GIOCHI_SHADER_PHONG, &shader));
    CHECK(shader != 0);

    /* The numbers are in the header, so a wrong one must be reported, not mapped onto a
     * neighbouring shader. */
    {
        uint64_t unused = 0;
        CHECK_CODE(giochi_shader_builtin(99, &unused), GIOCHI_ERROR_INVALID_ARGUMENT);
        CHECK(unused == 0);
        CHECK(strstr(giochi_get_last_error(), "not a built-in shader") != NULL);
    }

    /* An OBJ that does not exist is an asset error, not a crash. */
    {
        uint64_t missing = 0;
        CHECK_CODE(giochi_mesh_load_obj("/nonexistent/cube.obj", &missing),
                   GIOCHI_ERROR_ASSET);
        CHECK(missing == 0);
    }

    /* ---- camera and transforms -------------------------------------------- */
    GiochiCamera3D camera = giochi_camera3d_new();
    CHECK(camera.aspect == 0.0f);   /* "ask the drawable", per ADR-035 */
    giochi_camera3d_look_at(&camera, vec3(1.6f, 1.6f, 2.4f), vec3(0.0f, 0.0f, 0.0f));

    /* A zeroed transform has zero scale; the helper is the documented way to get one that
     * draws. */
    GiochiTransform3D model = giochi_transform3d_identity();
    CHECK(model.scale.x == 1.0f && model.scale.y == 1.0f && model.scale.z == 1.0f);
    CHECK(model.position.x == 0.0f);

    GiochiTransform3D moved = giochi_transform3d_from_translation(vec3(1.0f, 2.0f, 3.0f));
    CHECK(moved.position.y == 2.0f);
    CHECK(moved.scale.x == 1.0f);   /* translation must not zero the scale */

    /* ---- drawing ---------------------------------------------------------- */
    const GiochiColor3D background = color3d(0.07f, 0.09f, 0.14f, 1.0f);
    const GiochiColor3D cube_color = color3d(0.85f, 0.62f, 0.35f, 1.0f);

    for (int frame = 0; frame < 3; frame++) {
        CHECK_OK(giochi_engine3d_begin_frame(engine));
        CHECK_OK(giochi_renderer3d_clear(engine, background));

        /* The cube turns a little each frame, which would catch a model matrix that is
         * ignored or applied in the wrong order. */
        const float angle = (float)frame * 0.35f;
        model.rotation = (GiochiQuat){0.0f, angle, 0.0f, 1.0f, {0}};

        CHECK_OK(giochi_renderer3d_draw_mesh(engine, cube, shader, &model, &camera,
                                             cube_color));
        CHECK_OK(giochi_engine3d_end_frame(engine));
    }

    /* ---- the error paths a caller will actually hit ----------------------- */
    CHECK_CODE(giochi_renderer3d_draw_mesh(engine, cube, shader, NULL, &camera, cube_color),
               GIOCHI_ERROR_INVALID_ARGUMENT);
    CHECK(strstr(giochi_get_last_error(), "giochi_transform3d_identity") != NULL);

    CHECK_CODE(giochi_renderer3d_draw_mesh(engine, cube, shader, &model, NULL, cube_color),
               GIOCHI_ERROR_INVALID_ARGUMENT);

    CHECK_CODE(giochi_renderer3d_draw_mesh(engine, 0, shader, &model, &camera, cube_color),
               GIOCHI_ERROR_INVALID_HANDLE);

    CHECK_CODE(giochi_renderer3d_draw_mesh(engine, cube, 0, &model, &camera, cube_color),
               GIOCHI_ERROR_INVALID_HANDLE);

    /* A camera whose up vector is parallel to its view direction has no view matrix, and
     * the failure must come out of the draw rather than as a silently blank frame. */
    {
        GiochiCamera3D degenerate = giochi_camera3d_new();
        giochi_camera3d_look_at(&degenerate, vec3(0.0f, 1.0f, 0.0f), vec3(0.0f, 0.0f, 0.0f));
        CHECK_CODE(
            giochi_renderer3d_draw_mesh(engine, cube, shader, &model, &degenerate, cube_color),
            GIOCHI_ERROR_INVALID_ARGUMENT);
    }

    /* ---- teardown --------------------------------------------------------- */
    CHECK_OK(giochi_mesh_destroy(cube));
    /* A second destroy is a caller bug and must be visible, not ignored. */
    CHECK_CODE(giochi_mesh_destroy(cube), GIOCHI_ERROR_INVALID_HANDLE);
    CHECK_OK(giochi_shader_destroy(shader));
    CHECK_CODE(giochi_shader_destroy(shader), GIOCHI_ERROR_INVALID_HANDLE);

    giochi_engine3d_destroy(engine);

    /* After destroy the mesh functions have no engine again. */
    {
        uint64_t after = 0;
        CHECK_CODE(giochi_mesh_create_cube(1.0f, &after), GIOCHI_ERROR_NOT_INITIALIZED);
    }

    if (failures != 0) {
        fprintf(stderr, "%d check(s) failed\n", failures);
        return 1;
    }
    printf("c_smoke_3d: ok\n");
    return 0;
}
