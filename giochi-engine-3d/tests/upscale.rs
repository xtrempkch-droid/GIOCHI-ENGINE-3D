//! Rendering below the window size and stretching the result.
//!
//! An upscaler that silently does nothing, or that leaves the off-screen target bound,
//! produces a **black screen** — and a black screen also passes "the frame is not
//! empty" only if the assertion is written carelessly. These tests therefore check that
//! the scene is still visible, that the presented image is the window's size, and that
//! no GL error was raised.
//!
//! ```text
//! SDL_VIDEODRIVER=offscreen cargo test -p giochi-engine-3d --test upscale
//! ```

mod common;

use std::sync::MutexGuard;

use giochi_engine::platform::sdl::Platform;
use giochi_engine::platform::window::{Window, WindowConfig};
use giochi_engine_3d::context::{self, GlesVersion};
use giochi_engine_3d::math3d::{Camera3D, Color3D, Transform3D, Vec3};
use giochi_engine_3d::shader::BuiltinShader;
use giochi_engine_3d::{DirectionalLight, RenderContext3D};

const WIDTH: u32 = 160;
const HEIGHT: u32 = 120;

fn headless_context() -> Option<(MutexGuard<'static, ()>, Platform, Window)> {
    // Serialise the tests that need a Platform: SDL allows one EventPump per process and
    // a concurrent `Platform::new` used to be mistaken for "no GL driver".
    let guard = common::platform_guard();
    std::env::set_var("SDL_VIDEODRIVER", "offscreen");

    let platform = Platform::new().ok()?;
    context::configure_attributes(&platform, GlesVersion::Gles2).ok()?;
    let window = Window::new_opengl(&platform, WindowConfig::new(WIDTH, HEIGHT), "upscale").ok()?;
    Some((guard, platform, window))
}

/// Number of pixels that are not the clear colour.
fn lit_pixels(pixels: &[u8]) -> usize {
    pixels
        .chunks_exact(4)
        .filter(|px| px[0] > 8 || px[1] > 8 || px[2] > 8)
        .count()
}

/// Draws one cube filling most of the frame and returns the presented pixels.
fn frame_at(render: &mut RenderContext3D, window: &Window, cube: u64, shader: u64) -> Vec<u8> {
    let camera = Camera3D::from_parts(Vec3::new(0.0, 0.0, 3.0), Vec3::ZERO, Vec3::UP)
        .with_clip_planes(0.1, 100.0);

    // `begin_frame` applies the state and clears; `RenderContext3D::clear` would also
    // present the frame, leaving the draw outside the render target.
    render.begin_frame(window);
    render
        .draw_mesh_colored(
            cube,
            shader,
            &Transform3D::IDENTITY,
            &camera,
            Color3D::new(0.9, 0.9, 0.9, 1.0),
        )
        .expect("draw");
    render.read_pixels(WIDTH, HEIGHT).expect("pixels")
}

#[test]
fn a_scaled_frame_still_shows_the_scene_and_is_window_sized() {
    let Some((_guard, _platform, window)) = headless_context() else {
        eprintln!("skipped: no GL driver (SDL_VIDEODRIVER=offscreen)");
        return;
    };
    let mut render = RenderContext3D::new(context::create(&window).expect("context"));
    let cube = render.create_cube_mesh(2.5).expect("cube");
    let shader = render
        .create_builtin_shader(BuiltinShader::Phong)
        .expect("shader");
    render.set_clear_color(Color3D::BLACK);
    render.set_light(DirectionalLight::sun());

    // Reference: full resolution.
    assert_eq!(render.render_scale(), None);
    let full = frame_at(&mut render, &window, cube, shader);
    assert_eq!(full.len(), (WIDTH * HEIGHT * 4) as usize);
    let full_lit = lit_pixels(&full);
    assert!(full_lit > 2_000, "the reference frame is nearly empty");

    // Scaled: the scene renders at 75 %, the picture presented is still the window's.
    render.set_render_scale(Some(0.75));
    assert_eq!(render.render_scale(), Some(0.75));
    let scaled = frame_at(&mut render, &window, cube, shader);

    assert_eq!(
        scaled.len(),
        (WIDTH * HEIGHT * 4) as usize,
        "the presented image must be the window size, not the render size"
    );
    assert_eq!(render.render_size(), (120, 90), "160x120 at 0.75 is 120x90");
    assert_eq!(
        render.drawable_size(),
        (120, 90),
        "the camera must see the small buffer"
    );

    let scaled_lit = lit_pixels(&scaled);
    assert!(
        scaled_lit > full_lit * 8 / 10,
        "the upscaled frame lost the scene: {scaled_lit} lit pixels against {full_lit}"
    );

    let error = render.take_gl_error();
    assert_eq!(error, 0, "GL error 0x{error:04x} while upscaling");
}

#[test]
fn every_scale_produces_a_visible_frame() {
    let Some((_guard, _platform, window)) = headless_context() else {
        eprintln!("skipped: no GL driver (SDL_VIDEODRIVER=offscreen)");
        return;
    };
    let mut render = RenderContext3D::new(context::create(&window).expect("context"));
    let cube = render.create_cube_mesh(1.0).expect("cube");
    let shader = render
        .create_builtin_shader(BuiltinShader::Phong)
        .expect("shader");
    render.set_clear_color(Color3D::BLACK);

    for scale in [1.0, 0.75, 0.5, 0.25] {
        render.set_render_scale(Some(scale));
        let pixels = frame_at(&mut render, &window, cube, shader);
        assert_eq!(pixels.len(), (WIDTH * HEIGHT * 4) as usize);
        assert!(
            lit_pixels(&pixels) > 1_500,
            "scale {scale} produced a nearly empty frame ({} lit pixels)",
            lit_pixels(&pixels)
        );
        let error = render.take_gl_error();
        assert_eq!(error, 0, "GL error 0x{error:04x} at scale {scale}");
    }
}

#[test]
fn a_scale_of_one_is_the_same_as_no_scale() {
    let Some((_guard, _platform, window)) = headless_context() else {
        eprintln!("skipped: no GL driver (SDL_VIDEODRIVER=offscreen)");
        return;
    };
    let mut render = RenderContext3D::new(context::create(&window).expect("context"));
    let cube = render.create_cube_mesh(1.0).expect("cube");
    let shader = render
        .create_builtin_shader(BuiltinShader::Phong)
        .expect("shader");

    // 1.0 would mean "upscale from the same size to the same size", i.e. an extra
    // full-screen pass that costs a copy and buys nothing.
    render.set_render_scale(Some(1.0));
    assert_eq!(render.render_scale(), None);

    // Returning to full resolution must release the target, not leave it bound.
    render.set_render_scale(Some(0.5));
    let _ = frame_at(&mut render, &window, cube, shader);
    render.set_render_scale(None);
    assert_eq!(render.render_scale(), None);
    let pixels = frame_at(&mut render, &window, cube, shader);
    assert!(lit_pixels(&pixels) > 1_500);
    assert_eq!(render.drawable_size(), (WIDTH, HEIGHT));
    assert_eq!(render.take_gl_error(), 0);
}

#[test]
fn a_nonsense_scale_falls_back_to_full_resolution() {
    let Some((_guard, _platform, window)) = headless_context() else {
        eprintln!("skipped: no GL driver (SDL_VIDEODRIVER=offscreen)");
        return;
    };
    let mut render = RenderContext3D::new(context::create(&window).expect("context"));

    // A caller who passed `--scale oops` through a cast, or a config parsed badly,
    // should get a normal frame rather than the blurriest one the engine can make.
    render.set_render_scale(Some(f32::NAN));
    assert_eq!(render.render_scale(), None);
    render.set_render_scale(Some(-1.0));
    assert_eq!(render.render_scale(), None);
    render.set_render_scale(Some(0.0));
    assert_eq!(render.render_scale(), None);
}

#[test]
fn releasing_everything_also_releases_the_render_target() {
    let Some((_guard, _platform, window)) = headless_context() else {
        eprintln!("skipped: no GL driver (SDL_VIDEODRIVER=offscreen)");
        return;
    };
    let mut render = RenderContext3D::new(context::create(&window).expect("context"));
    let cube = render.create_cube_mesh(1.0).expect("cube");
    let shader = render
        .create_builtin_shader(BuiltinShader::Phong)
        .expect("shader");
    render.set_render_scale(Some(0.5));
    let _ = frame_at(&mut render, &window, cube, shader);

    // `release_all` is the "is anything left" question; a framebuffer that survives it
    // is a leak the driver would only reclaim when the context dies.
    assert!(render.release_all());
    assert_eq!(render.take_gl_error(), 0);
}

#[test]
fn a_second_scaled_frame_clears_the_previous_depth() {
    let Some((_guard, _platform, window)) = headless_context() else {
        eprintln!("skipped: no GL driver (SDL_VIDEODRIVER=offscreen)");
        return;
    };
    let mut render = RenderContext3D::new(context::create(&window).expect("context"));
    let cube = render.create_cube_mesh(2.5).expect("cube");
    let shader = render
        .create_builtin_shader(BuiltinShader::Phong)
        .expect("shader");
    render.set_clear_color(Color3D::BLACK);
    render.set_light(DirectionalLight::sun());
    render.set_render_scale(Some(0.5));

    let camera = Camera3D::from_parts(Vec3::new(0.0, 0.0, 3.0), Vec3::ZERO, Vec3::UP)
        .with_clip_planes(0.1, 100.0);

    let draw_at = |render: &mut RenderContext3D, z: f32| {
        // `begin_frame` applies the state and clears; `clear` would present.
        render.begin_frame(&window);
        render
            .draw_mesh_colored(
                cube,
                shader,
                &Transform3D::from_translation(Vec3::new(0.0, 0.0, z)),
                &camera,
                Color3D::new(0.9, 0.9, 0.9, 1.0),
            )
            .expect("draw");
        render.read_pixels(WIDTH, HEIGHT).expect("pixels")
    };

    let near = draw_at(&mut render, 1.0);
    assert!(lit_pixels(&near) > 2_000, "the reference frame is empty");

    // The second frame draws the cube farther away, at a depth the first frame's
    // geometry already covers. If the depth buffer was not cleared (or depth writes
    // were left disabled by the upscale pass) every fragment fails `GL_LEQUAL` and the
    // frame is the clear colour.
    let far = draw_at(&mut render, -2.0);
    // The far cube is smaller on screen than the near one, so the threshold is lower;
    // what matters is that it is not the clear colour (0 without the depth clear).
    assert!(
        lit_pixels(&far) > 300,
        "the second scaled frame is empty: depth was not cleared / written"
    );
}
