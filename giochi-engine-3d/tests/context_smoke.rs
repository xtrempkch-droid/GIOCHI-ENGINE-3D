//! Integration tests for the 3D context path.
//!
//! These need a real EGL/GLES driver. `SDL_VIDEODRIVER=offscreen` provides one with no
//! display attached; the `dummy` driver does not ("OpenGL support is either not
//! configured in SDL"), so these tests are skipped rather than failed when no GL is
//! available. That distinction matters on CI: a skip is honest, a failure is noise.
//!
//! Run them with:
//!
//! ```text
//! SDL_VIDEODRIVER=offscreen cargo test -p giochi-engine-3d
//! ```

mod common;

use std::sync::MutexGuard;

use giochi_engine::platform::sdl::Platform;
use giochi_engine::platform::window::{Window, WindowConfig};
use giochi_engine_3d::context::{self, GlesVersion};
use giochi_engine_3d::math3d::Color3D;
#[cfg(feature = "profiling")]
use giochi_engine_3d::math3d::{Camera3D, Transform3D, Vec3};
#[cfg(feature = "profiling")]
use giochi_engine_3d::shader::BuiltinShader;
use giochi_engine_3d::RenderContext3D;

/// Returns a headless GL context, or `None` when this machine has no GL driver.
///
/// The guard serialises the tests that need a `Platform`: SDL allows one `EventPump` per
/// process, and a concurrent `Platform::new` fails in a way that is indistinguishable
/// from "no GL driver" unless the tests hold it in turn.
fn headless_context() -> Option<(MutexGuard<'static, ()>, Platform, Window)> {
    let guard = common::platform_guard();

    // The offscreen driver is the only one that gives GL without a display. Setting it
    // here keeps the test independent of the developer's session, and `Platform::new`
    // respects an explicit `SDL_VIDEODRIVER`.
    std::env::set_var("SDL_VIDEODRIVER", "offscreen");

    let platform = Platform::new().ok()?;
    context::configure_attributes(&platform, GlesVersion::Gles2).ok()?;

    let window = Window::new_opengl(&platform, WindowConfig::new(320, 240), "test").ok()?;
    Some((guard, platform, window))
}

#[test]
fn a_context_can_be_created_and_reports_a_driver() {
    let Some((_guard, _platform, window)) = headless_context() else {
        eprintln!("skipped: no OpenGL ES driver available");
        return;
    };

    let gl = context::create(&window).expect("the offscreen driver advertised GL");

    assert!(
        !gl.adapter().is_empty(),
        "the renderer string is how a bug report identifies the driver"
    );
    assert!(
        matches!(gl.version(), GlesVersion::Gles2 | GlesVersion::Gles3),
        "only GLES 2.0 and 3.0 exist"
    );
}

#[test]
fn clearing_a_frame_raises_no_gl_error() {
    let Some((_guard, _platform, window)) = headless_context() else {
        eprintln!("skipped: no OpenGL ES driver available");
        return;
    };

    let mut render = RenderContext3D::new(context::create(&window).expect("context"));

    // Several frames: the first one can legitimately touch uninitialised driver state,
    // and a state bug that only shows on the second frame is the common case.
    for frame in 0..5 {
        render.set_clear_color(Color3D::SKY);
        render.begin_frame(&window);
        render.end_frame(&window);

        let error = render.take_gl_error();
        assert_eq!(error, 0, "GL error 0x{error:04x} on frame {frame}");
    }
}

#[test]
fn the_clear_color_is_actually_written_to_the_surface() {
    let Some((_guard, _platform, window)) = headless_context() else {
        eprintln!("skipped: no OpenGL ES driver available");
        return;
    };

    let mut render = RenderContext3D::new(context::create(&window).expect("context"));
    let want = Color3D::new(0.25, 0.5, 0.75, 1.0);

    render.set_clear_color(want);
    render.begin_frame(&window);

    // Read back one pixel. This is what separates "the call was made" from "the pixel
    // is that colour" -- the same distinction the 2D engine's `--quadro` capture makes.
    let pixels = render.read_pixels(1, 1).expect("read back the framebuffer");

    assert_eq!(pixels.len(), 4);
    // A byte of slack per channel: drivers are allowed to quantise the clear value.
    for (channel, (got, expected)) in pixels.iter().zip(want.to_array()).enumerate() {
        let got = *got as f32 / 255.0;
        assert!(
            (got - expected).abs() < 0.01,
            "channel {channel}: got {got}, expected {expected}"
        );
    }
}

#[cfg(feature = "profiling")]
#[test]
fn the_renderer_counts_meshes_vertices_and_resets_per_frame() {
    let Some((_guard, _platform, window)) = headless_context() else {
        eprintln!("skipped: no OpenGL ES driver available");
        return;
    };

    let mut render = RenderContext3D::new(context::create(&window).expect("context"));
    let mesh = render.create_cube_mesh(1.0).expect("cube mesh");
    let shader = render
        .create_builtin_shader(BuiltinShader::Flat)
        .expect("flat shader");
    let camera = Camera3D::new()
        .with_aspect(1.0)
        .with_clip_planes(0.1, 100.0);
    let model = Transform3D::from_translation(Vec3::new(0.0, 0.0, -5.0));

    render.begin_frame(&window);
    assert_eq!(
        render.frame_counters().draw_calls,
        0,
        "a frame starts at zero"
    );

    assert!(render
        .draw_mesh_colored(mesh, shader, &model, &camera, Color3D::WHITE)
        .expect("draw"));

    let counters = render.frame_counters();
    assert_eq!(counters.draw_calls, 1);
    assert_eq!(counters.meshes_drawn, 1);
    assert!(
        counters.vertices_uploaded > 0,
        "a cube must report its vertices"
    );
    assert!(
        counters.bytes_uploaded >= counters.vertices_uploaded,
        "each vertex is at least one byte"
    );

    render.end_frame(&window);
    render.begin_frame(&window);
    assert_eq!(
        render.frame_counters().draw_calls,
        0,
        "begin_frame must reset the counters"
    );

    assert!(render.destroy_mesh(mesh));
    assert!(render.destroy_shader(shader));
}
