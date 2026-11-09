//! Instanced drawing: one call draws N copies, and the pixels match the loop.
//!
//! Two things are worth a framebuffer test here. The first is the **instance matrix
//! upload**: a column-major matrix sent as rows puts every copy in the wrong place, and
//! with the identity matrix — the obvious thing to test with — the transpose is
//! invisible. The second is **equivalence**: an instanced frame and a loop of ordinary
//! draws of the same geometry must produce the same image, or the phase-17 benchmark
//! would be comparing two different pictures and the "gain" could be a scene that draws
//! less.
//!
//! These need a real EGL/GLES driver. `SDL_VIDEODRIVER=offscreen` provides one with no
//! display; without it the tests skip rather than fail.
//!
//! ```text
//! SDL_VIDEODRIVER=offscreen cargo test -p giochi-engine-3d --test instancing
//! ```

use giochi_engine::platform::sdl::Platform;
use giochi_engine::platform::window::{Window, WindowConfig};
use giochi_engine_3d::context::{self, GlesVersion, InstancingSupport};
use giochi_engine_3d::math3d::{Camera3D, Color3D, Mat4, Quat, Transform3D, Vec3};
use giochi_engine_3d::shader::BuiltinShader;
use giochi_engine_3d::{DirectionalLight, RenderContext3D};

const WIDTH: u32 = 128;
const HEIGHT: u32 = 96;

/// A headless GL context, or `None` when this machine has no GL driver.
fn headless_context() -> Option<(Platform, Window)> {
    std::env::set_var("SDL_VIDEODRIVER", "offscreen");

    let platform = Platform::new().ok()?;
    context::configure_attributes(&platform, GlesVersion::Gles2).ok()?;
    let window =
        Window::new_opengl(&platform, WindowConfig::new(WIDTH, HEIGHT), "instancing").ok()?;
    Some((platform, window))
}

struct Scene {
    render: RenderContext3D,
    cube: u64,
    loop_shader: u64,
    instanced_shader: u64,
    camera: Camera3D,
    window: Window,
    _platform: Platform,
}

impl Scene {
    fn new() -> Option<Self> {
        let (platform, window) = headless_context()?;
        let mut render = RenderContext3D::new(context::create(&window).ok()?);

        let cube = render.create_cube_mesh(0.5).ok()?;
        let loop_shader = render.create_builtin_shader(BuiltinShader::Phong).ok()?;
        let instanced_shader = render
            .create_builtin_shader(BuiltinShader::Instanced)
            .ok()?;

        // Look down the +Z axis at a row of cubes, from far enough that all of them are
        // in frame.
        let camera = Camera3D::from_parts(Vec3::new(0.0, 0.0, 6.0), Vec3::ZERO, Vec3::UP)
            .with_clip_planes(0.1, 100.0);

        render.set_clear_color(Color3D::BLACK);
        render.set_light(DirectionalLight::sun());

        Some(Self {
            render,
            cube,
            loop_shader,
            instanced_shader,
            camera,
            window,
            _platform: platform,
        })
    }

    /// Renders one frame with `draw` and returns the RGBA pixels.
    fn capture(&mut self, mut draw: impl FnMut(&mut RenderContext3D)) -> Vec<u8> {
        self.render.begin_frame(&self.window);
        self.render.clear(&self.window, Color3D::BLACK);
        draw(&mut self.render);
        self.render
            .read_pixels(WIDTH, HEIGHT)
            .expect("read_pixels after a complete frame")
    }
}

/// Four cubes spread along X, each rotated differently so a wrong matrix is visible.
///
/// The transforms are built once and used by both arms: `matrix()` feeds the instanced
/// draw and the `Transform3D` itself feeds the loop, so the two are the same scene by
/// construction rather than by two people writing the same loop twice.
fn transforms() -> Vec<Transform3D> {
    [-2, -1, 1, 2]
        .into_iter()
        .map(|column| {
            Transform3D::from_translation(Vec3::new(column as f32 * 1.1, 0.0, 0.0))
                .with_rotation(Quat::from_axis_angle(Vec3::UP, column as f32 * 0.6))
        })
        .collect()
}

/// Number of pixels that differ by more than `tolerance` in any channel.
fn differing_pixels(a: &[u8], b: &[u8], tolerance: i32) -> usize {
    a.chunks_exact(4)
        .zip(b.chunks_exact(4))
        .filter(|(left, right)| {
            (0..3).any(|channel| {
                (i32::from(left[channel]) - i32::from(right[channel])).abs() > tolerance
            })
        })
        .count()
}

/// Number of pixels that are not the black background.
fn lit_pixels(pixels: &[u8]) -> usize {
    pixels
        .chunks_exact(4)
        .filter(|px| px[0] > 8 || px[1] > 8 || px[2] > 8)
        .count()
}

const ORANGE: Color3D = Color3D::new(0.9, 0.55, 0.25, 1.0);

#[test]
fn instancing_draws_the_same_picture_as_a_loop_of_draws() {
    let Some(mut scene) = Scene::new() else {
        eprintln!("skipped: no GL driver (SDL_VIDEODRIVER=offscreen)");
        return;
    };

    if !scene.render.supports_instancing() {
        eprintln!(
            "skipped: this driver has no instanced drawing ({}); the fallback's draw \
             call count is asserted by `the_fallback_reports_one_call_per_instance`",
            scene.render.instancing_support().name()
        );
        return;
    }

    let transforms = transforms();
    let matrices: Vec<Mat4> = transforms.iter().map(Transform3D::matrix).collect();
    let (cube, loop_shader, camera) = (scene.cube, scene.loop_shader, scene.camera);

    let loop_pixels = scene.capture(|render| {
        for transform in &transforms {
            let _ = render.draw_mesh_colored(cube, loop_shader, transform, &camera, ORANGE);
        }
    });

    let (cube, instanced_shader, camera) = (scene.cube, scene.instanced_shader, scene.camera);
    let mut calls = 0;
    let instanced_pixels = scene.capture(|render| {
        calls = render
            .draw_instanced(cube, instanced_shader, &matrices, &camera, ORANGE)
            .expect("draw_instanced");
    });

    assert_eq!(
        calls,
        1,
        "instancing must be one draw call for {} instances",
        matrices.len()
    );
    assert!(
        lit_pixels(&instanced_pixels) > 200,
        "the instanced frame is essentially empty ({} lit pixels); a draw that \
         silently did nothing would otherwise pass",
        lit_pixels(&instanced_pixels)
    );

    let differing = differing_pixels(&loop_pixels, &instanced_pixels, 3);
    let total = WIDTH as usize * HEIGHT as usize;
    assert!(
        differing * 100 / total < 2,
        "instanced and loop frames differ on {differing} of {total} pixels; the two \
         arms of the phase-17 benchmark would not be drawing the same picture"
    );
}

#[test]
fn the_fallback_reports_one_call_per_instance() {
    let Some(mut scene) = Scene::new() else {
        eprintln!("skipped: no GL driver (SDL_VIDEODRIVER=offscreen)");
        return;
    };

    // The fallback is what runs on a driver without the extension. It cannot be
    // produced on demand — but its contract can: the count it reports must be the
    // instance count, which is what a caller would notice if it silently claimed one.
    let matrices: Vec<Mat4> = transforms().iter().map(Transform3D::matrix).collect();
    let (cube, shader, camera) = (scene.cube, scene.instanced_shader, scene.camera);
    let mut calls = 0;
    scene.capture(|render| {
        calls = render
            .draw_instanced(cube, shader, &matrices, &camera, ORANGE)
            .expect("draw_instanced");
    });

    let expected = if scene.render.supports_instancing() {
        1
    } else {
        matrices.len() as u32
    };
    assert_eq!(
        calls, expected,
        "the reported draw-call count must match the path actually taken"
    );
    assert_eq!(scene.render.last_instanced_draw_calls(), expected);
}

#[test]
fn the_instance_matrix_is_column_major() {
    let Some(mut scene) = Scene::new() else {
        eprintln!("skipped: no GL driver (SDL_VIDEODRIVER=offscreen)");
        return;
    };
    if !scene.render.supports_instancing() {
        eprintln!("skipped: no instanced drawing on this driver");
        return;
    }

    // Two identical cubes, one translated to the right. If the translation column were
    // uploaded as a row, both would land on top of each other at the origin and the
    // lit-pixel count would match the single-cube frame instead of roughly doubling.
    let centred = Transform3D::IDENTITY.matrix();
    let shifted = Transform3D::from_translation(Vec3::new(2.2, 0.0, 0.0)).matrix();

    let (cube, shader, camera) = (scene.cube, scene.instanced_shader, scene.camera);
    let grey = Color3D::new(0.85, 0.85, 0.85, 1.0);

    let one = scene.capture(|render| {
        let _ = render.draw_instanced(cube, shader, &[centred], &camera, grey);
    });
    let two = scene.capture(|render| {
        let _ = render.draw_instanced(cube, shader, &[centred, shifted], &camera, grey);
    });

    let single = lit_pixels(&one);
    let double = lit_pixels(&two);
    assert!(single > 50, "the single-cube frame must be non-empty");
    assert!(
        double > single * 3 / 2,
        "two separated cubes must light more than one: {double} vs {single}. \
         A transposed matrix stacks them at the origin"
    );
}

#[test]
fn an_empty_batch_costs_nothing() {
    let Some(mut scene) = Scene::new() else {
        eprintln!("skipped: no GL driver (SDL_VIDEODRIVER=offscreen)");
        return;
    };

    let (cube, shader, camera) = (scene.cube, scene.instanced_shader, scene.camera);
    let mut calls = u32::MAX;
    scene.capture(|render| {
        calls = render
            .draw_instanced(cube, shader, &[], &camera, Color3D::WHITE)
            .expect("an empty batch is not an error");
    });

    assert_eq!(calls, 0, "nothing to draw means no draw calls");
}

#[test]
fn an_unknown_handle_draws_nothing() {
    let Some(mut scene) = Scene::new() else {
        eprintln!("skipped: no GL driver (SDL_VIDEODRIVER=offscreen)");
        return;
    };

    let matrices = [Transform3D::IDENTITY.matrix()];
    let camera = scene.camera;
    let cube = scene.cube;
    let shader = scene.instanced_shader;

    let mut from_bad_mesh = u32::MAX;
    let mut from_bad_shader = u32::MAX;
    scene.capture(|render| {
        from_bad_mesh = render
            .draw_instanced(0, shader, &matrices, &camera, Color3D::WHITE)
            .expect("an unknown mesh is not an error");
        from_bad_shader = render
            .draw_instanced(cube, 0, &matrices, &camera, Color3D::WHITE)
            .expect("an unknown shader is not an error");
    });

    assert_eq!(from_bad_mesh, 0);
    assert_eq!(from_bad_shader, 0);
}

#[test]
fn the_reported_support_matches_the_extension_string() {
    let Some((_platform, window)) = headless_context() else {
        eprintln!("skipped: no GL driver (SDL_VIDEODRIVER=offscreen)");
        return;
    };
    let gl = context::create(&window).expect("context");
    let gles3 = gl.supports_gles3();
    // The entry-point table is crate-internal; a caller asks the extensions and the
    // renderer's public accessor, which is the surface a game actually has.
    let extensions: Vec<String> = gl.extensions().to_vec();
    let support = {
        let render = RenderContext3D::new(gl);
        render.instancing_support()
    };

    // The detection's answer has to be one a caller can act on: a core path implies a
    // GLES 3.0 context, and an extension path implies the driver lists it.
    if support == InstancingSupport::Core {
        assert!(gles3, "the core path requires a GLES 3.0 context");
    } else if let Some(extension) = support.extension() {
        assert!(
            extensions.iter().any(|name| name == extension),
            "{support:?} reports {extension} but the driver does not list it"
        );
    }

    println!(
        "instancing support: {} ({:?})",
        support.name(),
        support.extension()
    );
}
