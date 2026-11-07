/*
 * C smoke test for the GIOCHI-ENGINE C ABI.
 *
 * A real C consumer: it includes only `giochi.h`, links against libgiochi, drives a
 * scene through the callback table and checks the error path. Built and run by
 * scripts/ffi-smoke.sh (and by CI with SDL_VIDEODRIVER=dummy).
 *
 *   cc -std=c11 -Wall -Wextra -Werror -I giochi-engine-capi/include \
 *      giochi-engine-capi/tests/c_smoke.c -L target/debug -lgiochi -o c_smoke
 */
#include <ctype.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "giochi.h"

static int failures;

/* SDL2 registers this backend as "KMSDRM", unlike "x11" or "wayland", so a
 * case-sensitive compare silently misses the console this test exists for. */
static int contains_ignore_case(const char *haystack, const char *needle)
{
    const size_t n = strlen(needle);
    const size_t h = strlen(haystack);
    for (size_t i = 0; i + n <= h; i++) {
        size_t j = 0;
        while (j < n
               && tolower((unsigned char)haystack[i + j])
                      == tolower((unsigned char)needle[j])) {
            j++;
        }
        if (j == n) {
            return 1;
        }
    }
    return 0;
}

#define CHECK(condition)                                                       \
    do {                                                                       \
        if (!(condition)) {                                                    \
            fprintf(stderr, "FAIL %s:%d: %s\n        %s\n", __FILE__, __LINE__, \
                    #condition, giochi_get_last_error());                      \
            failures++;                                                        \
        }                                                                      \
    } while (0)

#define CHECK_CODE(expression)                                                 \
    do {                                                                       \
        int32_t code_ = (expression);                                          \
        if (code_ != GIOCHI_ERROR_OK) {                                        \
            fprintf(stderr, "FAIL %s:%d: %s returned %d (%s)\n", __FILE__,      \
                    __LINE__, #expression, (int)code_, giochi_get_last_error()); \
            failures++;                                                        \
        }                                                                      \
    } while (0)

/* Frame counter shared with the scene callbacks. */
struct scene_state {
    int entered;
    int exited;
    int updates;
    int renders;
    int quit_after_renders;
    GiochiTexture texture;
};

static void on_enter(void *user_data)
{
    struct scene_state *state = (struct scene_state *)user_data;
    state->entered++;

    /* Resource loading belongs here: on_enter runs outside the engine's frame. */
    state->texture = giochi_texture_create("assets/player.png");
    CHECK(state->texture != 0);
    CHECK(giochi_texture_width(state->texture) == 32);
    CHECK(giochi_texture_height(state->texture) == 32);
}

static void on_exit(void *user_data)
{
    struct scene_state *state = (struct scene_state *)user_data;
    state->exited++;
    if (state->texture != 0) {
        CHECK_CODE(giochi_texture_destroy(state->texture));
        state->texture = 0;
    }
}

static void update(void *user_data, const GiochiTime *time)
{
    struct scene_state *state = (struct scene_state *)user_data;
    state->updates++;

    CHECK(time != NULL);
    if (time != NULL) {
        CHECK(time->delta > 0.0f);
    }
    /* `is_running` turns 0 once a quit is requested, and the remaining fixed steps
       of that frame still run, so only check it before the threshold. */
    if (state->quit_after_renders == 0 || state->renders < state->quit_after_renders) {
        CHECK(giochi_engine_is_running() == 1);
    }

    /* Keyboard and mouse are readable here, and an unmapped key is not an error. */
    (void)giochi_input_key_down(32 /* SDLK_SPACE */);
    (void)giochi_input_mouse_x();
    CHECK(giochi_gamepad_connected() == 0 || giochi_gamepad_connected() == 1);

    if (state->quit_after_renders > 0 && state->renders >= state->quit_after_renders) {
        giochi_engine_request_quit();
    }
}

static void render(void *user_data)
{
    struct scene_state *state = (struct scene_state *)user_data;
    state->renders++;

    GiochiColor background = {12, 14, 22, 255, {0, 0, 0, 0}};
    CHECK_CODE(giochi_render_clear(background));

    GiochiRect bar = {10.0f, 10.0f, 100.0f, 24.0f, {0, 0, 0, 0}};
    GiochiColor foreground = {40, 160, 90, 255, {0, 0, 0, 0}};
    CHECK_CODE(giochi_render_draw_rect(&bar, foreground));
}

/* A second scene used to prove the stack is emptied between runs. */
static void noop_update(void *user_data, const GiochiTime *time)
{
    (void)user_data;
    (void)time;
    giochi_engine_request_quit();
}

static void noop_render(void *user_data)
{
    (void)user_data;
}

int main(void)
{
    /* ---- version ---------------------------------------------------------- */
    CHECK(giochi_abi_version() == 1);
    CHECK(GIOCHI_ABI_VERSION == 1);
    CHECK(giochi_engine_version() > 0);
    CHECK(giochi_get_last_error() != NULL);
    CHECK(giochi_get_last_error()[0] == '\0');

    /* ---- before init ------------------------------------------------------ */
    CHECK(giochi_engine_run() == GIOCHI_ERROR_NOT_INITIALIZED);
    CHECK(strstr(giochi_get_last_error(), "not initialised") != NULL);

    /* ---- init ------------------------------------------------------------- */
    GiochiEngineConfig config = {
        .width = 320,
        .height = 240,
        .target_fps = 60,
        .vsync = 0,
        .title = "c_smoke",
        ._reserved = {0, 0, 0, 0, 0, 0, 0, 0},
    };
    CHECK_CODE(giochi_engine_init(&config));
    CHECK(giochi_get_last_error()[0] == '\0');
    CHECK(giochi_engine_video_driver() != NULL);

    uint32_t width = 0, height = 0;
    CHECK_CODE(giochi_render_size(&width, &height));
    CHECK(width > 0 && height > 0);
    /* A windowed backend honours the requested size. KMSDRM is fullscreen-only:
       it binds the panel's native mode and cannot be 320x240. Demanding the
       requested size would make this test fail on the console it exists for. */
    if (!contains_ignore_case(giochi_engine_video_driver(), "kmsdrm")) {
        CHECK(width == 320 && height == 240);
    }

    /* NULL out-parameters are allowed. */
    CHECK_CODE(giochi_render_size(NULL, NULL));

    /* ---- assets ----------------------------------------------------------- */
    size_t size = 0;
    CHECK_CODE(giochi_asset_size("assets/player.png", &size));
    CHECK(size > 0);

    uint8_t *buffer = malloc(size);
    CHECK(buffer != NULL);
    if (buffer != NULL) {
        size_t read = 0;
        CHECK_CODE(giochi_asset_read("assets/player.png", buffer, size, &read));
        CHECK(read == size);
        /* An image signature, to prove the bytes are really the image and not
           zeroes. BMP counts: SDL_image decodes it with no external library,
           and a console image built without libpng has nothing else. */
        CHECK(read > 8
              && ((buffer[0] == 0x89 && buffer[1] == 'P')
                  || (buffer[0] == 'B' && buffer[1] == 'M')));

        GiochiTexture from_memory = giochi_texture_create_memory(buffer, read);
        CHECK(from_memory != 0);
        CHECK(giochi_texture_width(from_memory) == 32);
        CHECK_CODE(giochi_texture_destroy(from_memory));
        free(buffer);
    }

    /* A buffer that is too small fails instead of truncating. */
    uint8_t tiny[4] = {0};
    size_t read_small = 0;
    CHECK(giochi_asset_read("assets/player.png", tiny, sizeof(tiny), &read_small)
          == GIOCHI_ERROR_ASSET);
    /* A double destroy is reported. */
    CHECK(giochi_texture_destroy(4242) == GIOCHI_ERROR_INVALID_HANDLE);

    /* ---- scene ------------------------------------------------------------ */
    struct scene_state state = {0, 0, 0, 0, 5, 0};

    GiochiSceneCallbacks callbacks = {
        .user_data = &state,
        .on_enter = on_enter,
        .on_exit = on_exit,
        .update = update,
        .render = render,
        ._reserved = {0, 0, 0, 0},
    };

    /* A table without render is rejected. */
    GiochiSceneCallbacks incomplete = callbacks;
    incomplete.render = NULL;
    CHECK(giochi_scene_create(&incomplete) == 0);
    CHECK(strstr(giochi_get_last_error(), "update and render") != NULL);

    GiochiScene scene = giochi_scene_create(&callbacks);
    CHECK(scene != 0);

    CHECK_CODE(giochi_engine_run_scene(scene));
    CHECK(state.entered == 1);
    CHECK(state.exited == 1);
    CHECK(state.renders >= 5);
    CHECK(state.updates > 0);
    CHECK(giochi_engine_scene_depth() == 0);
    CHECK(giochi_engine_is_running() == 0);
    CHECK(giochi_engine_frames() >= 5);

    /* ---- stepping --------------------------------------------------------- */
    GiochiSceneCallbacks noop = {
        .user_data = NULL,
        .on_enter = NULL,
        .on_exit = NULL,
        .update = noop_update,
        .render = noop_render,
        ._reserved = {0, 0, 0, 0},
    };
    GiochiScene quick = giochi_scene_create(&noop);
    CHECK(quick != 0);
    CHECK_CODE(giochi_engine_run_scene(quick));
    CHECK_CODE(giochi_scene_destroy(quick));

    uint64_t before = giochi_engine_frames();
    CHECK_CODE(giochi_engine_step(1.0f / 60.0f));
    CHECK(giochi_engine_frames() == before + 1);

    /* ---- teardown --------------------------------------------------------- */
    CHECK_CODE(giochi_scene_destroy(scene));
    CHECK(giochi_scene_destroy(scene) == GIOCHI_ERROR_INVALID_HANDLE);

    giochi_engine_shutdown();
    giochi_engine_shutdown(); /* idempotent */
    CHECK(giochi_engine_run() == GIOCHI_ERROR_NOT_INITIALIZED);

    if (failures == 0) {
        printf("c_smoke: OK (abi %u, engine %u.%u.%u)\n", giochi_abi_version(),
               giochi_engine_version() >> 16, (giochi_engine_version() >> 8) & 0xFF,
               giochi_engine_version() & 0xFF);
        return 0;
    }
    fprintf(stderr, "c_smoke: %d failure(s)\n", failures);
    return 1;
}
