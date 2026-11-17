//! Phase 39's acceptance: the scene reaches the real 3D renderer.
//!
//! A GL window with no display (`SDL_VIDEODRIVER=offscreen`, a real EGL/GLES context), the
//! scene from `assets/scenes/default.scene`, three frames, and the pixels read back. This is
//! the only test in the editor that proves the preview draws the scene rather than opening a
//! window and showing the clear colour.
//!
//! One `#[test]` per binary: SDL allows one `Platform` and one `Sdl` per process, and `cargo
//! test` gives each `#[test]` its own thread — the same constraint `viewport_render.rs` and
//! the engine's own render tests document.
//!
//! Skips itself when the driver cannot give a GL context (a console with KMSDRM and no
//! EGL, for instance) rather than failing: the test asserts a property of the *renderer*, and
//! a machine without one has nothing to say about it. The skip prints why.

use std::path::PathBuf;

use giochi_editor::play::{MeshSpec, PlayPlan};
use giochi_engine::platform::{Platform, Window, WindowConfig};
use giochi_engine_3d::context::{self, GlesVersion};
use giochi_engine_3d::math3d::{Camera3D, Color3D};
use giochi_engine_3d::render3d::RenderContext3D;
use giochi_engine_3d::scene::Scene;
use giochi_engine_3d::shader::BuiltinShader;

const WIDTH: u32 = 200;
const HEIGHT: u32 = 150;

#[test]
fn the_scene_reaches_the_three_d_renderer() {
    if std::env::var_os("SDL_VIDEODRIVER").is_none() {
        std::env::set_var("SDL_VIDEODRIVER", "offscreen");
    }
    let Ok(platform) = Platform::new() else {
        eprintln!("skipped: no SDL platform");
        return;
    };
    if context::configure_attributes(&platform, GlesVersion::Gles2).is_err() {
        eprintln!("skipped: no GL attributes");
        return;
    }
    let Ok(window) = Window::new_opengl(&platform, WindowConfig::new(WIDTH, HEIGHT), "play-render")
    else {
        eprintln!("skipped: no GL window");
        return;
    };
    let Ok(gl) = context::create(&window) else {
        eprintln!("skipped: no GL context");
        return;
    };
    let mut render = RenderContext3D::new(gl);

    // The scene the editor ships, read from disk: the plan, the loader and this renderer all
    // meet here.
    let scene = Scene::load_from_path(scene_path()).expect("the default scene loads");
    let plan = PlayPlan::from_scene(&scene, None);
    assert!(!plan.instances.is_empty(), "the scene has geometry");
    assert!(plan.unsupported.is_empty(), "{:?}", plan.unsupported);

    let shader = render
        .create_builtin_shader(BuiltinShader::Phong)
        .expect("shader");
    let mut meshes = Vec::new();
    for spec in &plan.meshes {
        let handle = match spec {
            MeshSpec::Obj { path } => render.load_obj_mesh(path).expect("obj"),
            _ => {
                let primitive = spec.primitive_values().expect("a primitive");
                match primitive {
                    giochi_engine_3d::scene::Primitive::Cube { size } => {
                        render.create_cube_mesh(size).expect("cube")
                    }
                    giochi_engine_3d::scene::Primitive::Sphere {
                        radius,
                        rings,
                        segments,
                    } => render
                        .create_sphere_mesh(radius, rings, segments)
                        .expect("sphere"),
                    giochi_engine_3d::scene::Primitive::Plane { size, subdivisions } => {
                        render.create_plane_mesh(size, subdivisions).expect("plane")
                    }
                }
            }
        };
        meshes.push(handle);
    }
    render.set_light(plan.light);
    render.set_clear_color(Color3D::new(0.0, 0.0, 0.0, 1.0));

    // The scene's own camera, which the plan derived from the camera node.
    let mut camera: Camera3D = plan.camera.to_camera3d(WIDTH, HEIGHT);
    camera = camera.with_drawable_size(WIDTH, HEIGHT);

    render.begin_frame(&window);
    for instance in &plan.instances {
        let index = plan
            .meshes
            .iter()
            .position(|spec| spec == &instance.mesh)
            .expect("uploaded");
        render
            .draw_mesh_colored(
                meshes[index],
                shader,
                &instance.transform,
                &camera,
                instance.color,
            )
            .expect("drawn");
    }
    let pixels = render.read_pixels(WIDTH, HEIGHT).expect("pixels");
    render.end_frame(&window);
    drop(render);
    drop(window);

    // Anything the cube drew is not the black clear colour. A frame that is entirely black
    // means the camera is looking at nothing, which is exactly the failure a "did it not
    // panic?" test would miss.
    let lit = pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[0] > 20 || pixel[1] > 20 || pixel[2] > 20)
        .count();
    let total = pixels.len() / 4;
    // The default scene's camera sits ~7.8 units from a unit cube, so at 200×150 the cube is
    // about 18 px across — roughly 120 lit pixels, which is what this measured when it was
    // written. The bar is well below that: the test is about "the geometry reached the
    // framebuffer", not about the exact projection, which `giochi-engine-3d` tests on its own.
    assert!(
        lit > 50,
        "{lit} of {total} pixels are lit; the cube did not reach the framebuffer"
    );
}

fn scene_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/scenes/default.scene")
}
