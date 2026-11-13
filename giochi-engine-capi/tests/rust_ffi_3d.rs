//! End-to-end test of part II of the C ABI (3D, `GIOCHI_ABI_VERSION 2`), from Rust.
//!
//! Everything here calls the exported `extern "C"` functions the way a C program would,
//! and takes the layout of the value types from `giochi.h` rather than from the engine
//! crate — a struct whose field order drifted would fail here and nowhere else.
//!
//! It needs a real EGL/GLES driver. `SDL_VIDEODRIVER=offscreen` provides one with no
//! display; `dummy` has no OpenGL at all, and there every test skips rather than fails,
//! because the machine is not lying about the ABI.
//!
//! ```text
//! SDL_VIDEODRIVER=offscreen cargo test -p giochi-engine-capi --test rust_ffi_3d
//! ```

use std::ffi::{CStr, CString};

use giochi::{
    giochi_abi_version, giochi_camera3d_look_at, giochi_camera3d_new, giochi_engine3d_begin_frame,
    giochi_engine3d_create, giochi_engine3d_destroy, giochi_engine3d_end_frame,
    giochi_get_last_error, giochi_gl_texture_create, giochi_gl_texture_create_rgba,
    giochi_gl_texture_destroy, giochi_gl_texture_height, giochi_gl_texture_width,
    giochi_mesh_create_cube, giochi_mesh_destroy, giochi_mesh_load_obj, giochi_renderer3d_clear,
    giochi_renderer3d_draw_mesh, giochi_renderer3d_draw_mesh_textured, giochi_renderer3d_set_light,
    giochi_shader_builtin, giochi_shader_destroy, giochi_shader_load,
    giochi_transform3d_from_translation, giochi_transform3d_identity, GiochiCamera3D,
    GiochiColor3D, GiochiDirectionalLight, GiochiEngine3D, GiochiQuat, GiochiTransform3D,
    GiochiVec3, GiochiWindowConfig, GIOCHI_ABI_VERSION, GIOCHI_ERROR_ASSET,
    GIOCHI_ERROR_INVALID_ARGUMENT, GIOCHI_ERROR_INVALID_HANDLE, GIOCHI_ERROR_NOT_INITIALIZED,
    GIOCHI_ERROR_OK, GIOCHI_SHADER_FLAT, GIOCHI_SHADER_PHONG, GIOCHI_SHADER_TEXTURED,
};

/// The message of the thread's last error, as a `String`.
fn last_error() -> String {
    // SAFETY: `giochi_get_last_error` never returns NULL and the pointer stays valid until
    // the next call on this thread; this reads it immediately.
    unsafe { CStr::from_ptr(giochi_get_last_error()) }
        .to_string_lossy()
        .into_owned()
}

fn window_config() -> GiochiWindowConfig {
    let mut config: GiochiWindowConfig = unsafe { std::mem::zeroed() };
    config.width = 320;
    config.height = 240;
    config
}

fn color(r: f32, g: f32, b: f32, a: f32) -> GiochiColor3D {
    let mut c: GiochiColor3D = unsafe { std::mem::zeroed() };
    c.r = r;
    c.g = g;
    c.b = b;
    c.a = a;
    c
}

/// Creates the engine, or returns `None` when this machine has no GL driver.
///
/// The distinction matters on CI: a runner without Mesa should report "skipped", not a
/// failure that says the ABI is broken.
fn engine_or_skip() -> Option<*mut GiochiEngine3D> {
    std::env::set_var("SDL_VIDEODRIVER", "offscreen");

    // SDL2 allows one `EventPump` per *process*, not per thread. Cargo runs the tests of
    // this binary in parallel threads, and a test's engine owns the pump until its
    // thread ends, so the second test to call `create` used to fail — and was reported
    // as "no GL driver", silently skipping a test that never ran. Wait for the pump
    // instead: `create` failing for that reason is the one case that is not a skip.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        let engine = giochi_engine3d_create(&window_config());
        if !engine.is_null() {
            return Some(engine);
        }

        let message = last_error();
        if message.contains("EventPump") && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
            continue;
        }

        assert!(
            !message.is_empty(),
            "create failed without saying why, so a consumer cannot report it"
        );
        eprintln!("skipped: no OpenGL ES driver ({message})");
        return None;
    }
}

/// Releases the engine if it was created, so a failing test does not leave it installed
/// for the next one in this binary (there is one engine per thread, §10.5).
fn release(engine: Option<*mut GiochiEngine3D>) {
    if let Some(engine) = engine {
        giochi_engine3d_destroy(engine);
    }
}

#[test]
fn destroying_a_stale_engine_pointer_is_reported_not_freed_twice() {
    let Some(engine) = engine_or_skip() else {
        return;
    };
    giochi_engine3d_destroy(engine);

    // The slot is empty now. A second call with the same pointer value must be rejected:
    // the old code only checked a *non-NULL* mismatch, so this reconstructed the `Box`
    // again and freed the allocation twice.
    giochi_engine3d_destroy(engine);
    let message = last_error();
    assert!(
        message.contains("already destroyed") || message.contains("not the live one"),
        "a double destroy must explain itself, got: {message}"
    );

    // The rejection must not have left the thread wedged: a fresh engine still works.
    let Some(fresh) = engine_or_skip() else {
        panic!("the thread could not create an engine after a rejected double destroy");
    };
    giochi_engine3d_destroy(fresh);
}

#[test]
fn a_part_two_consumer_still_works_after_part_three() {
    // This test was written against part II. Part III (frame statistics) raised the number
    // to 3 and this assertion was relaxed from `== 2` to `>= 2` -- which is the whole
    // compatibility claim, demonstrated rather than asserted: the consumer needed no other
    // change. The rule for callers is ">= what I was built for", never "==".
    const { assert!(GIOCHI_ABI_VERSION >= 2) };
    assert_eq!(giochi_abi_version(), GIOCHI_ABI_VERSION);
    assert!(giochi_abi_version() >= 1);
}

#[test]
fn the_value_types_have_the_layout_giochi_h_documents() {
    // These are the sizes `docs/C_ABI_CONTRACT.md` §9 names. A field added in the middle of
    // one of them shifts everything after it and silently corrupts a C caller, which is
    // why they are asserted here as well as in the 3D crate's own layout test: this one
    // goes through the re-exports the header was generated from.
    assert_eq!(std::mem::size_of::<GiochiVec3>(), 28);
    assert_eq!(std::mem::size_of::<GiochiQuat>(), 32);
    assert_eq!(std::mem::size_of::<GiochiColor3D>(), 32);
    assert_eq!(std::mem::size_of::<GiochiTransform3D>(), 104);
    assert_eq!(std::mem::size_of::<GiochiCamera3D>(), 116);
    assert_eq!(std::mem::size_of::<GiochiWindowConfig>(), 44);
    assert_eq!(std::mem::size_of::<GiochiDirectionalLight>(), 88);

    for alignment in [
        std::mem::align_of::<GiochiVec3>(),
        std::mem::align_of::<GiochiQuat>(),
        std::mem::align_of::<GiochiColor3D>(),
        std::mem::align_of::<GiochiTransform3D>(),
        std::mem::align_of::<GiochiCamera3D>(),
        std::mem::align_of::<GiochiWindowConfig>(),
        std::mem::align_of::<GiochiDirectionalLight>(),
    ] {
        assert_eq!(alignment, 4, "a SIMD type would raise this to 8 or 16");
    }
}

#[test]
fn everything_is_refused_before_an_engine_exists() {
    // No engine has been created in this test yet, so every mesh and shader call must name
    // the missing engine rather than dereference a null slot.
    let mut handle = 4242u64;

    assert_eq!(
        giochi_mesh_create_cube(1.0, &mut handle),
        GIOCHI_ERROR_NOT_INITIALIZED
    );
    assert_eq!(
        handle, 4242,
        "a failed call must not write the out parameter"
    );
    assert!(
        last_error().contains("giochi_engine3d_create"),
        "the message must name the call that fixes it: {}",
        last_error()
    );

    assert_eq!(giochi_mesh_destroy(1), GIOCHI_ERROR_NOT_INITIALIZED);
    assert_eq!(
        giochi_shader_builtin(GIOCHI_SHADER_FLAT, &mut handle),
        GIOCHI_ERROR_NOT_INITIALIZED
    );
    assert_eq!(giochi_shader_destroy(1), GIOCHI_ERROR_NOT_INITIALIZED);
}

#[test]
fn null_pointers_are_rejected_without_dereferencing() {
    assert_eq!(
        giochi_mesh_load_obj(std::ptr::null(), std::ptr::null_mut()),
        GIOCHI_ERROR_INVALID_ARGUMENT
    );
    assert_eq!(
        giochi_mesh_create_cube(1.0, std::ptr::null_mut()),
        GIOCHI_ERROR_INVALID_ARGUMENT
    );
    assert_eq!(
        giochi_shader_load(std::ptr::null(), std::ptr::null(), std::ptr::null_mut()),
        GIOCHI_ERROR_INVALID_ARGUMENT
    );
    assert_eq!(
        giochi_shader_builtin(GIOCHI_SHADER_PHONG, std::ptr::null_mut()),
        GIOCHI_ERROR_INVALID_ARGUMENT
    );
    assert_eq!(
        giochi_renderer3d_clear(std::ptr::null_mut(), color(0.0, 0.0, 0.0, 1.0)),
        GIOCHI_ERROR_INVALID_ARGUMENT
    );
    assert_eq!(
        giochi_engine3d_begin_frame(std::ptr::null_mut()),
        GIOCHI_ERROR_INVALID_ARGUMENT
    );
    assert_eq!(
        giochi_engine3d_end_frame(std::ptr::null_mut()),
        GIOCHI_ERROR_INVALID_ARGUMENT
    );

    // Destroying NULL is a no-op, so a caller can bail out of any branch safely.
    giochi_engine3d_destroy(std::ptr::null_mut());
}

#[test]
fn an_unknown_builtin_shader_is_reported_not_guessed() {
    let mut handle = 0u64;
    assert_eq!(
        giochi_shader_builtin(7, &mut handle),
        GIOCHI_ERROR_INVALID_ARGUMENT
    );
    assert_eq!(handle, 0);
    assert!(
        last_error().contains("GIOCHI_SHADER_PHONG"),
        "the message must list the valid ids: {}",
        last_error()
    );
}

#[test]
fn a_missing_obj_is_an_asset_error() {
    let Some(engine) = engine_or_skip() else {
        return;
    };

    let path = CString::new("/nonexistent/cube.obj").expect("no interior NUL");
    let mut handle = 0u64;
    assert_eq!(
        giochi_mesh_load_obj(path.as_ptr(), &mut handle),
        GIOCHI_ERROR_ASSET
    );
    assert_eq!(handle, 0, "a failed load must not hand out a handle");
    assert!(!last_error().is_empty());

    release(Some(engine));
}

#[test]
fn the_full_3d_cycle_creates_draws_and_tears_down_cleanly() {
    let Some(engine) = engine_or_skip() else {
        return;
    };

    // A second engine on the same thread would retarget the functions that take no engine,
    // so it is refused rather than allowed.
    assert!(giochi_engine3d_create(&window_config()).is_null());
    assert!(
        last_error().contains("already exists"),
        "creating a second engine must say why: {}",
        last_error()
    );

    let mut cube = 0u64;
    assert_eq!(giochi_mesh_create_cube(1.0, &mut cube), GIOCHI_ERROR_OK);
    assert_ne!(cube, 0);

    let mut shader = 0u64;
    assert_eq!(
        giochi_shader_builtin(GIOCHI_SHADER_PHONG, &mut shader),
        GIOCHI_ERROR_OK
    );
    assert_ne!(shader, 0);

    let mut camera: GiochiCamera3D = giochi_camera3d_new();
    assert_eq!(camera.aspect, 0.0, "0.0 means ask the drawable (ADR-035)");

    let mut eye: GiochiVec3 = unsafe { std::mem::zeroed() };
    eye.x = 1.6;
    eye.y = 1.6;
    eye.z = 2.4;
    let mut origin: GiochiVec3 = unsafe { std::mem::zeroed() };
    origin.y = 0.0;
    giochi_camera3d_look_at(&mut camera, eye, origin);

    let mut model: GiochiTransform3D = giochi_transform3d_identity();
    assert_eq!(model.scale.x, 1.0);

    let background = color(0.07, 0.09, 0.14, 1.0);
    let tint = color(0.85, 0.62, 0.35, 1.0);

    // The one input GIOCHI_SHADER_PHONG reads. Passed by pointer, so this also exercises
    // the struct-by-reference half of the boundary from Rust.
    let mut light: GiochiDirectionalLight = GiochiDirectionalLight::sun();
    light.direction = GiochiVec3 {
        x: -0.4,
        y: -1.0,
        z: -0.3,
        _reserved: [0; 4],
    };
    light.shininess = 32.0;
    assert_eq!(giochi_renderer3d_set_light(engine, &light), GIOCHI_ERROR_OK);
    assert_eq!(
        giochi_renderer3d_set_light(engine, std::ptr::null()),
        GIOCHI_ERROR_INVALID_ARGUMENT
    );
    assert!(
        last_error().contains("light must not be NULL"),
        "the message must name the argument: {}",
        last_error()
    );

    for frame in 0..3 {
        assert_eq!(giochi_engine3d_begin_frame(engine), GIOCHI_ERROR_OK);
        assert_eq!(giochi_renderer3d_clear(engine, background), GIOCHI_ERROR_OK);

        // Turn the cube a little each frame, in case a model matrix is ignored.
        let angle = frame as f32 * 0.35;
        model.rotation = GiochiQuat {
            x: 0.0,
            y: (angle * 0.5).sin(),
            z: 0.0,
            w: (angle * 0.5).cos(),
            _reserved: [0; 4],
        };

        assert_eq!(
            giochi_renderer3d_draw_mesh(engine, cube, shader, &model, &camera, tint),
            GIOCHI_ERROR_OK,
            "draw failed: {}",
            last_error()
        );
        assert_eq!(giochi_engine3d_end_frame(engine), GIOCHI_ERROR_OK);
    }

    // A NULL model is not silently the identity: a caller that forgot it would get the whole
    // scene piled at the origin with no error to explain it.
    assert_eq!(
        giochi_renderer3d_draw_mesh(engine, cube, shader, std::ptr::null(), &camera, tint),
        GIOCHI_ERROR_INVALID_ARGUMENT
    );
    assert!(last_error().contains("giochi_transform3d_identity"));

    assert_eq!(
        giochi_renderer3d_draw_mesh(engine, cube, shader, &model, std::ptr::null(), tint),
        GIOCHI_ERROR_INVALID_ARGUMENT
    );

    // Unknown handles are reported, not drawn as nothing.
    assert_eq!(
        giochi_renderer3d_draw_mesh(engine, 0, shader, &model, &camera, tint),
        GIOCHI_ERROR_INVALID_HANDLE
    );
    assert_eq!(
        giochi_renderer3d_draw_mesh(engine, cube, 0, &model, &camera, tint),
        GIOCHI_ERROR_INVALID_HANDLE
    );

    // A camera with up parallel to the view direction has no view matrix; the failure has
    // to come out of the draw, not as a blank frame.
    let mut degenerate: GiochiCamera3D = giochi_camera3d_new();
    let mut up: GiochiVec3 = unsafe { std::mem::zeroed() };
    up.y = 1.0;
    giochi_camera3d_look_at(&mut degenerate, up, unsafe { std::mem::zeroed() });
    assert_eq!(
        giochi_renderer3d_draw_mesh(engine, cube, shader, &model, &degenerate, tint),
        GIOCHI_ERROR_INVALID_ARGUMENT
    );

    // Teardown. A double destroy is a caller bug and must be visible.
    assert_eq!(giochi_mesh_destroy(cube), GIOCHI_ERROR_OK);
    assert_eq!(giochi_mesh_destroy(cube), GIOCHI_ERROR_INVALID_HANDLE);
    assert_eq!(giochi_shader_destroy(shader), GIOCHI_ERROR_OK);
    assert_eq!(giochi_shader_destroy(shader), GIOCHI_ERROR_INVALID_HANDLE);

    giochi_engine3d_destroy(engine);

    // The pointer is dead now: a caller that kept a copy must get a code, not a
    // use-after-free. This is the check `validate_engine` exists for.
    assert_eq!(
        giochi_renderer3d_set_light(engine, &light),
        GIOCHI_ERROR_INVALID_HANDLE
    );

    // And after destroy the mesh functions have no engine again.
    let mut after = 0u64;
    assert_eq!(
        giochi_mesh_create_cube(1.0, &mut after),
        GIOCHI_ERROR_NOT_INITIALIZED
    );
}

#[test]
fn a_gl_texture_created_from_c_draws_on_a_mesh() {
    let Some(engine) = engine_or_skip() else {
        return;
    };

    // 2x2 RGBA: red and blue, so a black frame means "the sampler was not bound".
    let pixels: [u8; 16] = [
        255, 0, 0, 255, 0, 0, 255, 255, 255, 0, 0, 255, 0, 0, 255, 255,
    ];
    let texture = giochi_gl_texture_create_rgba(pixels.as_ptr(), 2, 2);
    assert_ne!(texture, 0, "texture create failed: {}", last_error());
    assert_eq!(giochi_gl_texture_width(texture), 2);
    assert_eq!(giochi_gl_texture_height(texture), 2);

    let mut mesh = 0u64;
    assert_eq!(giochi_mesh_create_cube(1.0, &mut mesh), GIOCHI_ERROR_OK);
    let mut shader = 0u64;
    assert_eq!(
        giochi_shader_builtin(GIOCHI_SHADER_TEXTURED, &mut shader),
        GIOCHI_ERROR_OK
    );

    assert_eq!(giochi_engine3d_begin_frame(engine), GIOCHI_ERROR_OK);
    assert_eq!(
        giochi_renderer3d_clear(engine, color(0.0, 0.0, 0.0, 1.0)),
        GIOCHI_ERROR_OK
    );

    let mut camera = giochi_camera3d_new();
    camera.aspect = 1.0;
    let model = giochi_transform3d_from_translation(GiochiVec3 {
        x: 0.0,
        y: 0.0,
        z: -4.0,
        _reserved: [0; 4],
    });
    assert_eq!(
        giochi_renderer3d_draw_mesh_textured(
            engine,
            mesh,
            shader,
            texture,
            &model,
            &camera,
            color(1.0, 1.0, 1.0, 1.0),
        ),
        GIOCHI_ERROR_OK,
        "textured draw failed: {}",
        last_error()
    );
    assert_eq!(giochi_engine3d_end_frame(engine), GIOCHI_ERROR_OK);

    assert_eq!(giochi_gl_texture_destroy(texture), GIOCHI_ERROR_OK);
    assert_eq!(
        giochi_gl_texture_destroy(texture),
        GIOCHI_ERROR_INVALID_HANDLE,
        "a double destroy must be reported"
    );
    assert_eq!(giochi_mesh_destroy(mesh), GIOCHI_ERROR_OK);
    assert_eq!(giochi_shader_destroy(shader), GIOCHI_ERROR_OK);
    giochi_engine3d_destroy(engine);
}

#[test]
fn gl_texture_functions_reject_bad_input_with_codes() {
    // No engine needed for the argument checks; the handle ones need one, so this test
    // takes the same skip as the renderer tests.
    let Some(engine) = engine_or_skip() else {
        return;
    };

    assert_eq!(giochi_gl_texture_create(std::ptr::null()), 0);
    assert!(last_error().contains("path"), "{}", last_error());

    assert_eq!(
        giochi_gl_texture_create(CString::new("/nonexistent/giochi.png").unwrap().as_ptr()),
        0
    );
    assert!(!last_error().is_empty());

    assert_eq!(
        giochi_gl_texture_create_rgba(std::ptr::null(), 2, 2),
        0,
        "NULL pixels are rejected"
    );
    assert_eq!(giochi_gl_texture_create_rgba([0u8; 16].as_ptr(), 0, 2), 0);
    assert_eq!(giochi_gl_texture_destroy(0), GIOCHI_ERROR_INVALID_HANDLE);
    assert_eq!(giochi_gl_texture_width(0), 0);
    assert_eq!(giochi_gl_texture_height(0), 0);

    // A NULL engine is a caller mistake in the call itself, and the mesh API reports it
    // the same way.
    assert_eq!(
        giochi_renderer3d_draw_mesh_textured(
            std::ptr::null_mut(),
            1,
            1,
            1,
            std::ptr::null(),
            std::ptr::null(),
            color(1.0, 1.0, 1.0, 1.0)
        ),
        GIOCHI_ERROR_INVALID_ARGUMENT
    );

    giochi_engine3d_destroy(engine);
}

#[test]
fn translating_a_transform_does_not_zero_its_scale() {
    // The trap this helper exists for: a C caller writing `{0}` gets scale (0,0,0) and
    // draws nothing. `from_translation` must leave the scale at one.
    let mut offset: GiochiVec3 = unsafe { std::mem::zeroed() };
    offset.x = 1.0;
    offset.y = 2.0;
    offset.z = 3.0;

    let model = giochi_transform3d_from_translation(offset);

    assert_eq!(model.position.x, 1.0);
    assert_eq!(model.position.y, 2.0);
    assert_eq!(model.position.z, 3.0);
    assert_eq!(model.scale.x, 1.0);
    assert_eq!(model.scale.y, 1.0);
    assert_eq!(model.scale.z, 1.0);
}
