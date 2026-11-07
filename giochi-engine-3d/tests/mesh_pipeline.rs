//! End-to-end mesh pipeline against a real GLES driver.
//!
//! These are the tests that would have caught the first two `cube()` windings being
//! backwards: a unit test can assert the arithmetic, but only a driver can say whether
//! the triangles were actually rasterised. Pixels are read back with `glReadPixels`, so
//! "it drew" is measured rather than inferred from the absence of an error.
//!
//! Skipped cleanly when no GL driver is available (`docs/3D.md` §5).

use giochi_engine::platform::sdl::Platform;
use giochi_engine::platform::window::{Window, WindowConfig};
use giochi_engine_3d::context::{self, GlesVersion};
use giochi_engine_3d::math3d::{Camera3D, Color3D, Transform3D, Vec3};
use giochi_engine_3d::shader::BuiltinShader;
use giochi_engine_3d::RenderContext3D;

/// Colour the frame is cleared with; anything else in a pixel means geometry.
const BACKGROUND: Color3D = Color3D::new(0.0, 0.0, 0.0, 1.0);
/// Colour the cube is drawn with.
const FOREGROUND: Color3D = Color3D::new(1.0, 0.0, 0.0, 1.0);

fn headless(size: u32) -> Option<(Platform, Window, RenderContext3D)> {
    std::env::set_var("SDL_VIDEODRIVER", "offscreen");
    let platform = Platform::new().ok()?;
    context::configure_attributes(&platform, GlesVersion::Gles2).ok()?;
    let window = Window::new_opengl(&platform, WindowConfig::new(size, size), "mesh").ok()?;
    let render = RenderContext3D::new(context::create(&window).ok()?);
    Some((platform, window, render))
}

/// Counts pixels that differ from the clear colour.
fn lit_pixels(render: &RenderContext3D, width: u32, height: u32) -> usize {
    let pixels = render.read_pixels(width, height).expect("read back");
    let background = [0u8, 0, 0, 255];

    pixels
        .chunks_exact(4)
        .filter(|pixel| *pixel != background)
        .count()
}

#[test]
fn the_builtin_shaders_compile_on_a_real_driver() {
    let Some((_platform, _window, mut render)) = headless(64) else {
        eprintln!("skipped: no OpenGL ES driver available");
        return;
    };

    for shader in BuiltinShader::ALL {
        let handle = render
            .create_builtin_shader(shader)
            .unwrap_or_else(|error| panic!("{} shader failed to compile: {error}", shader.name()));

        assert_ne!(handle, 0);
        assert!(render.destroy_shader(handle), "{} destroy", shader.name());
    }

    assert_eq!(render.shader_count(), 0);
}

#[test]
fn an_invalid_shader_fails_with_the_driver_log() {
    let Some((_platform, _window, mut render)) = headless(64) else {
        eprintln!("skipped: no OpenGL ES driver available");
        return;
    };

    let path = std::env::temp_dir().join("giochi-broken.vert");
    let frag = std::env::temp_dir().join("giochi-broken.frag");
    std::fs::write(&path, "#version 100\nvoid main() { this is not glsl }\n").expect("write");
    std::fs::write(
        &frag,
        "#version 100\nprecision mediump float;\nvoid main() {}\n",
    )
    .expect("write");

    let result = render.load_shader(&path, &frag);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&frag);

    let error = result.expect_err("invalid GLSL must not compile");
    let text = error.to_string();

    // Golden rule 5: the driver's own log, and it must be non-empty. A message without
    // it is indistinguishable from "something went wrong".
    assert!(text.contains("vertex"), "the stage must be named: {text}");
    assert!(text.len() > 40, "the driver log looks empty: {text}");
}

#[test]
fn a_cube_is_actually_rasterised() {
    let size = 64;
    let Some((_platform, window, mut render)) = headless(size) else {
        eprintln!("skipped: no OpenGL ES driver available");
        return;
    };

    let mesh = render.create_cube_mesh(2.0).expect("cube mesh");
    let shader = render
        .create_builtin_shader(BuiltinShader::Flat)
        .expect("flat shader");

    render.set_clear_color(BACKGROUND);
    render.begin_frame(&window);

    let camera = Camera3D::new()
        .with_aspect(1.0)
        .with_clip_planes(0.1, 100.0);
    // Push the cube in front of the default camera (which looks down -Z).
    let model = Transform3D::from_translation(Vec3::new(0.0, 0.0, -5.0));

    let drew = render
        .draw_mesh_colored(mesh, shader, &model, &camera, FOREGROUND)
        .expect("draw");
    assert!(drew, "both handles must resolve");

    let error = render.take_gl_error();
    assert_eq!(error, 0, "GL error 0x{error:04x} while drawing the cube");

    let lit = lit_pixels(&render, size, size);
    assert!(
        lit > 200,
        "the cube covered only {lit} of {} pixels -- winding, culling or the \
         projection is wrong",
        size * size
    );

    assert!(render.destroy_mesh(mesh));
    assert!(render.destroy_shader(shader));
}

#[test]
fn a_cube_is_culled_when_it_is_behind_the_camera() {
    let size = 48;
    let Some((_platform, window, mut render)) = headless(size) else {
        eprintln!("skipped: no OpenGL ES driver available");
        return;
    };

    let mesh = render.create_cube_mesh(2.0).expect("cube mesh");
    let shader = render
        .create_builtin_shader(BuiltinShader::Flat)
        .expect("flat shader");
    let camera = Camera3D::new().with_aspect(1.0);

    render.set_clear_color(BACKGROUND);
    render.begin_frame(&window);

    // +Z is behind the default camera, which looks down -Z. Nothing must be drawn;
    // if projection or culling were wrong this would light up the whole frame.
    let behind = Transform3D::from_translation(Vec3::new(0.0, 0.0, 5.0));
    render
        .draw_mesh_colored(mesh, shader, &behind, &camera, FOREGROUND)
        .expect("draw");

    assert_eq!(render.take_gl_error(), 0);
    assert_eq!(
        lit_pixels(&render, size, size),
        0,
        "geometry behind the camera was drawn in front of it"
    );

    render.destroy_mesh(mesh);
    render.destroy_shader(shader);
}

#[test]
fn depth_testing_hides_the_back_faces() {
    // A cube drawn with culling *and* depth off looks like a flat hexagon. Reading the
    // depth buffer directly is not possible in GLES 2.0, so this asserts the observable
    // consequence instead: the centre pixel belongs to the front face, which is the only
    // face whose normal points at the camera.
    let size = 64;
    let Some((_platform, window, mut render)) = headless(size) else {
        eprintln!("skipped: no OpenGL ES driver available");
        return;
    };

    let mesh = render.create_cube_mesh(2.0).expect("cube mesh");
    let shader = render
        .create_builtin_shader(BuiltinShader::Flat)
        .expect("flat shader");
    let camera = Camera3D::new().with_aspect(1.0);

    render.set_clear_color(BACKGROUND);
    render.begin_frame(&window);

    let model = Transform3D::from_translation(Vec3::new(0.0, 0.0, -5.0));
    render
        .draw_mesh_colored(mesh, shader, &model, &camera, FOREGROUND)
        .expect("draw");

    assert_eq!(render.take_gl_error(), 0);

    let pixels = render.read_pixels(size, size).expect("read back");
    // GL returns bottom-up rows; the centre is the same either way.
    let centre = ((size / 2) * size + size / 2) as usize * 4;
    let pixel = &pixels[centre..centre + 4];

    assert_ne!(&pixel[..3], &[0, 0, 0], "the centre of the cube is empty");
    assert!(
        pixel[0] > 100,
        "the front face should be the brightest: red = {}",
        pixel[0]
    );

    render.destroy_mesh(mesh);
    render.destroy_shader(shader);
}

#[test]
fn destroying_a_mesh_twice_reports_false_and_does_not_crash() {
    let size = 32;
    let Some((_platform, _window, mut render)) = headless(size) else {
        eprintln!("skipped: no OpenGL ES driver available");
        return;
    };

    let mesh = render.create_cube_mesh(1.0).expect("cube mesh");

    assert!(render.destroy_mesh(mesh), "first destroy");
    assert!(!render.destroy_mesh(mesh), "second destroy must be a no-op");
    assert!(!render.destroy_mesh(0), "handle 0 is never valid");
    assert_eq!(render.mesh_count(), 0);
}

#[test]
fn release_all_leaves_nothing_behind() {
    let size = 32;
    let Some((_platform, _window, mut render)) = headless(size) else {
        eprintln!("skipped: no OpenGL ES driver available");
        return;
    };

    for _ in 0..3 {
        render.create_cube_mesh(1.0).expect("cube");
    }
    for shader in BuiltinShader::ALL {
        render.create_builtin_shader(shader).expect("shader");
    }

    assert_eq!(render.mesh_count(), 3);
    assert_eq!(render.shader_count(), 2);
    assert!(render.release_all(), "everything should have been released");
    assert_eq!(render.mesh_count(), 0);
    assert_eq!(render.shader_count(), 0);
}

#[test]
fn the_no_vao_fallback_draws_the_same_pixels() {
    // GLES 2.0 without `GL_OES_vertex_array_object` takes the per-draw attribute path,
    // and no development machine has that driver. `GIOCHI_FORCE_NO_VAO=1` makes both
    // paths runnable here: an untested fallback is a fallback that does not work.
    let size = 64;

    let with_vao = {
        std::env::remove_var(giochi_engine_3d::mesh::FORCE_NO_VAO_ENV);
        let Some((_platform, window, mut render)) = headless(size) else {
            eprintln!("skipped: no OpenGL ES driver available");
            return;
        };
        let uses_vao = draw_cube_and_capture(&mut render, &window, size);
        (uses_vao, render)
    };

    let without_vao = {
        std::env::set_var(giochi_engine_3d::mesh::FORCE_NO_VAO_ENV, "1");
        let Some((_platform, window, mut render)) = headless(size) else {
            eprintln!("skipped: no OpenGL ES driver available");
            return;
        };
        let uses_vao = draw_cube_and_capture(&mut render, &window, size);
        std::env::remove_var(giochi_engine_3d::mesh::FORCE_NO_VAO_ENV);
        (uses_vao, render)
    };

    assert!(
        with_vao.0,
        "this driver should have VAOs; the test setup is wrong"
    );
    assert!(!without_vao.0, "the forced path must not create a VAO");

    // Both paths must light the same number of pixels: if the fallback mis-specified an
    // attribute offset the cube would be smaller, skewed or absent.
    let a = lit_pixels(&with_vao.1, size, size);
    let b = lit_pixels(&without_vao.1, size, size);

    assert!(a > 200, "the VAO path drew only {a} pixels");
    assert!(
        (a as i64 - b as i64).abs() < (a as i64 / 20).max(8),
        "the fallback drew {b} pixels against {a} with VAOs; the attribute setup differs"
    );
}

/// Draws the standard cube and returns `(uses_vao, ())`.
fn draw_cube_and_capture(render: &mut RenderContext3D, window: &Window, size: u32) -> bool {
    let mesh = render.create_cube_mesh(2.0).expect("cube mesh");
    let shader = render
        .create_builtin_shader(BuiltinShader::Flat)
        .expect("flat shader");

    render.set_clear_color(BACKGROUND);
    render.begin_frame(window);

    let camera = Camera3D::new().with_aspect(1.0);
    let model = Transform3D::from_translation(Vec3::new(0.0, 0.0, -5.0));
    render
        .draw_mesh_colored(mesh, shader, &model, &camera, FOREGROUND)
        .expect("draw");

    assert_eq!(render.take_gl_error(), 0, "GL error in the fallback path");
    let _ = size;

    // The mesh is queried before destroying it; `uses_vao` is what the test compares.
    let uses_vao = render.mesh_uses_vao(mesh);

    render.destroy_mesh(mesh);
    render.destroy_shader(shader);

    uses_vao
}
