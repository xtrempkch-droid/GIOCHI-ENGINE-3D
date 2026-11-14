//! Hot reload of a shader and a mesh, proved by reading the framebuffer.
//!
//! "The reload returned `Ok`" is not evidence: a program that compiled but never got
//! bound, or a mesh that was swapped for a stale buffer, renders exactly as before. These
//! tests draw the same geometry twice, edit the source in between, and assert that the
//! pixels changed — and that a **broken** edit leaves them unchanged while keeping the
//! previous program alive.
//!
//! Skipped cleanly when no GL driver is available (`docs/3D.md` §5).

mod common;

use std::path::{Path, PathBuf};
use std::sync::MutexGuard;

use giochi_engine::platform::sdl::Platform;
use giochi_engine::platform::window::{Window, WindowConfig};
use giochi_engine_3d::context::{self, GlesVersion};
use giochi_engine_3d::hot_reload::ShaderReloader;
use giochi_engine_3d::math3d::{Camera3D, Color3D, Transform3D, Vec3};
use giochi_engine_3d::shader::BuiltinShader;
use giochi_engine_3d::RenderContext3D;

const SIZE: u32 = 64;

fn headless() -> Option<(MutexGuard<'static, ()>, Platform, Window, RenderContext3D)> {
    let guard = common::platform_guard();
    common::set_headless_driver();
    let platform = Platform::new().ok()?;
    context::configure_attributes(&platform, GlesVersion::Gles2).ok()?;
    let window = Window::new_opengl(&platform, WindowConfig::new(SIZE, SIZE), "hot-reload").ok()?;
    let render = RenderContext3D::new(context::create(&window).ok()?);
    Some((guard, platform, window, render))
}

/// A minimal vertex shader, matching the uniforms the renderer actually uploads.
///
/// `u_view_proj` and `u_model` — not `u_mvp`: the renderer sets those two names, and a
/// shader that expects a third draws nothing at all, which reads exactly like "the
/// reload failed".
const VERTEX: &str = "#version 100
precision highp float;
uniform mat4 u_view_proj;
uniform mat4 u_model;
attribute vec3 a_position;
void main() {
    gl_Position = u_view_proj * u_model * vec4(a_position, 1.0);
}
";

/// A fragment shader that paints one fixed colour, ignoring every uniform.
///
/// Ignoring `u_color` is deliberate: the test proves the *program* was replaced, so the
/// colour must come from the source rather than from a uniform the draw call overwrites.
fn fragment(color: &str) -> String {
    format!(
        "#version 100
precision mediump float;
void main() {{
    gl_FragColor = vec4({color}, 1.0);
}}
"
    )
}

/// A temp directory unique to this test binary.
fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "giochi-shader-reload-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// Draws `mesh` with `shader` and returns the frame.
///
/// White, not the clear colour: `draw_mesh` shades with the clear colour, so a black
/// background would make the geometry black on black and the test would "prove" that a
/// reload changed nothing.
fn draw(render: &mut RenderContext3D, window: &Window, mesh: u64, shader: u64) -> Vec<u8> {
    render.set_clear_color(Color3D::new(0.0, 0.0, 0.0, 1.0));
    render.begin_frame(window);

    let camera = Camera3D::new().with_aspect(1.0);
    let model = Transform3D::from_translation(Vec3::new(0.0, 0.0, -5.0));
    let drew = render
        .draw_mesh_colored(mesh, shader, &model, &camera, Color3D::WHITE)
        .expect("draw must succeed");
    assert!(drew, "both handles must be live");

    // The drawable, not `SIZE`: on KMSDRM the window is the panel and the engine ignores
    // the requested size (ADR-035), so a fixed region would sample the wrong corner.
    let (width, height) = render.drawable_size();
    render
        .read_pixels(width, height)
        .expect("read_pixels must succeed")
}

/// The most common non-black colour in `pixels`, as `(r, g, b)`.
fn dominant_colour(pixels: &[u8]) -> (u8, u8, u8) {
    let mut counts: std::collections::HashMap<(u8, u8, u8), usize> =
        std::collections::HashMap::new();
    for pixel in pixels.chunks_exact(4) {
        let key = (pixel[0], pixel[1], pixel[2]);
        if key == (0, 0, 0) {
            continue;
        }
        *counts.entry(key).or_default() += 1;
    }
    counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(colour, _)| colour)
        .expect("the triangle must have drawn something")
}

#[test]
fn a_shader_reloads_in_place_and_a_broken_edit_keeps_the_old_one() {
    let Some((_guard, _platform, window, mut render)) = headless() else {
        eprintln!("no GL driver; skipping");
        return;
    };

    let dir = temp_dir("shader");
    let vertex_path = dir.join("reload.vert");
    let fragment_path = dir.join("reload.frag");
    std::fs::write(&vertex_path, VERTEX).expect("write vertex");
    std::fs::write(&fragment_path, fragment("0.0, 1.0, 0.0")).expect("write fragment");

    let handle = render
        .load_shader(&vertex_path, &fragment_path)
        .expect("the fixture must compile");
    let mesh = render.create_cube_mesh(1.0).expect("cube mesh");

    let before = dominant_colour(&draw(&mut render, &window, mesh, handle));
    assert!(
        before.1 > before.0 && before.1 > before.2,
        "the first shader paints green, got {before:?}"
    );
    assert_eq!(
        render.shader_captures_uniforms(handle),
        Some(true),
        "a file-loaded shader records its uniforms"
    );

    let mut reloader = ShaderReloader::new();
    assert!(reloader.track_shader(handle, &vertex_path, &fragment_path));

    // ---- edit the fragment shader: red ------------------------------------
    std::fs::write(&fragment_path, fragment("1.0, 0.0, 0.0")).expect("rewrite fragment");
    assert_eq!(
        reloader.apply_path(&mut render, &fragment_path),
        1,
        "the fragment path must map to the shader"
    );
    assert_eq!(reloader.stats().shaders_reloaded, 1);
    assert_eq!(render.shader_count(), 1, "a reload must not leak a program");

    let after = dominant_colour(&draw(&mut render, &window, mesh, handle));
    assert!(
        after.0 > after.1 && after.0 > after.2,
        "the reloaded shader paints red, got {after:?}"
    );

    // ---- edit it into a syntax error: nothing changes ---------------------
    std::fs::write(
        &fragment_path,
        "#version 100\nvoid main() { this is not glsl }\n",
    )
    .expect("write a broken fragment");
    assert_eq!(
        reloader.apply_path(&mut render, &fragment_path),
        0,
        "a shader that does not compile must report failure"
    );
    assert_eq!(reloader.stats().shader_failures, 1);
    assert_eq!(
        render.shader_count(),
        1,
        "the failed reload must leave the previous program alive"
    );

    let still_red = dominant_colour(&draw(&mut render, &window, mesh, handle));
    assert_eq!(
        still_red, after,
        "a failed reload must not change what is on screen"
    );

    // ---- a missing file is a failure, not a panic -------------------------
    std::fs::remove_file(&fragment_path).expect("remove fragment");
    assert_eq!(reloader.apply_path(&mut render, &fragment_path), 0);
    assert_eq!(reloader.stats().shader_failures, 2);

    // ---- an untracked path reloads nothing --------------------------------
    assert_eq!(
        reloader.apply_path(&mut render, Path::new("elsewhere/other.frag")),
        0
    );

    reloader.clear();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_mesh_reloads_from_its_obj_file() {
    let Some((_guard, _platform, window, mut render)) = headless() else {
        eprintln!("no GL driver; skipping");
        return;
    };

    let dir = temp_dir("mesh");
    let obj_path = dir.join("quad.obj");
    // Two triangles: a 1x1 quad on the z = 0 plane, facing +z.
    std::fs::write(
        &obj_path,
        "v -1 -1 0\nv 1 -1 0\nv 1 1 0\nv -1 1 0\nvn 0 0 1\nf 1//1 2//1 3//1\nf 1//1 3//1 4//1\n",
    )
    .expect("write obj");

    let mesh = render
        .load_obj_mesh(&obj_path)
        .expect("OBJ fixture must load");
    assert_eq!(render.mesh_vertex_count(mesh), 4);
    assert_eq!(
        render.mesh_source(mesh).as_deref(),
        Some(obj_path.as_path()),
        "the path must be remembered"
    );

    let shader = render
        .create_builtin_shader(BuiltinShader::Flat)
        .expect("flat shader");
    let before = draw(&mut render, &window, mesh, shader);

    // A triangle list of six vertices instead of four: the reload must change the count,
    // which is what proves the buffer was rebuilt rather than reused.
    std::fs::write(
        &obj_path,
        "v -1 -1 0\nv 1 -1 0\nv 0 1 0\nvn 0 0 1\nf 1//1 2//1 3//1\n",
    )
    .expect("rewrite obj");

    let mut reloader = ShaderReloader::new();
    assert!(reloader.track_mesh(mesh, &obj_path));
    assert_eq!(reloader.apply_path(&mut render, &obj_path), 1);
    assert_eq!(reloader.stats().meshes_reloaded, 1);
    assert_eq!(render.mesh_vertex_count(mesh), 3);
    assert_eq!(render.mesh_count(), 1, "a reload must not leak a mesh");

    let after = draw(&mut render, &window, mesh, shader);
    assert_ne!(before, after, "the reloaded mesh must change what is drawn");

    // A broken OBJ keeps the previous mesh.
    std::fs::write(&obj_path, "not an obj at all\n").expect("write broken obj");
    assert_eq!(reloader.apply_path(&mut render, &obj_path), 0);
    assert_eq!(reloader.stats().mesh_failures, 1);
    assert_eq!(
        render.mesh_vertex_count(mesh),
        3,
        "a failed reload must keep the previous geometry"
    );

    // A generated mesh has no file to reload from.
    let cube = render.create_cube_mesh(1.0).expect("cube");
    assert_eq!(render.mesh_source(cube), None);
    assert!(render.reload_mesh(cube).is_err());

    render.destroy_mesh(mesh);
    assert_eq!(
        render.mesh_source(mesh),
        None,
        "destroying the mesh forgets its path"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
