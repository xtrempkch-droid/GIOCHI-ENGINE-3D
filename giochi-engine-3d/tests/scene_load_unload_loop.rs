//! Loading and unloading a scene a hundred times must not leak GPU objects
//! (DOD criterion 1).
//!
//! **Ativado na Fase S4.**
//!
//! # What "VRAM delta < 1 %" can honestly mean on GLES 2.0
//!
//! GLES 2.0 has no query for texture or buffer memory, and `glGetIntegerv` reports limits,
//! not usage. What *is* exact is the set of objects the renderer owns: every mesh,
//! texture and shader is a registry entry with a name, and a leak is an entry that
//! survives the unload. So the measurable quantity is:
//!
//! ```text
//! delta = (bytes after cycle N) - (bytes after cycle 1)
//! bytes = sum of texture_size(handle) * 4 + vertices/indices reported by the mesh
//! ```
//!
//! This is an **estimate of the resident set**, and `docs/STABILITY_DECISIONS.md` says so.
//! It catches the leak that matters (an object that is never destroyed) and cannot catch
//! driver-internal fragmentation — which no portable API could.
//!
//! # Skipping instead of failing
//!
//! The GL tests in this crate skip cleanly when no driver is available (`docs/3D.md` §5)
//! because a machine without EGL is a normal developer machine, not a broken one. The
//! skip is counted in the run output by the S4 gate, so "0 executed and 1 skipped" cannot
//! be mistaken for "passed".

mod common;

use std::sync::MutexGuard;

use giochi_engine::platform::sdl::Platform;
use giochi_engine::platform::window::{Window, WindowConfig};
use giochi_engine_3d::context::{self, GlesVersion};
use giochi_engine_3d::shader::BuiltinShader;
use giochi_engine_3d::RenderContext3D;

const SIZE: u32 = 64;

type Gl = (MutexGuard<'static, ()>, Platform, Window, RenderContext3D);

fn headless() -> Option<Gl> {
    let guard = common::platform_guard();
    common::set_headless_driver();
    let platform = Platform::new().ok()?;
    context::configure_attributes(&platform, GlesVersion::Gles2).ok()?;
    let window = Window::new_opengl(&platform, WindowConfig::new(SIZE, SIZE), "lifecycle").ok()?;
    let render = RenderContext3D::new(context::create(&window).ok()?);
    Some((guard, platform, window, render))
}

/// A 32x32 RGBA texture: 4 KiB, enough that 100 leaked copies would be visible in the
/// estimate long before a driver's allocation granularity obscures it.
fn rgba() -> Vec<u8> {
    let mut pixels = vec![255u8; 32 * 32 * 4];
    for (index, pixel) in pixels.chunks_exact_mut(4).enumerate() {
        pixel[0] = (index % 255) as u8;
        pixel[3] = 255;
    }
    pixels
}

/// One load → render a frame → unload cycle, returning the estimated resident bytes.
///
/// Five meshes, three textures and two shaders, which is the "complex scene" the DOD
/// names. Everything is destroyed through the public API; nothing is `release_all`d, so a
/// missing `destroy_*` in the unload path shows up instead of being masked.
fn cycle(render: &mut RenderContext3D, window: &Window) -> u64 {
    let fixed = render
        .create_builtin_shader(BuiltinShader::Flat)
        .expect("a builtin shader");
    let lit = render
        .create_builtin_shader(BuiltinShader::Phong)
        .expect("a builtin shader");

    let meshes: Vec<_> = [
        render.create_cube_mesh(1.0),
        render.create_cube_mesh(2.0),
        render.create_sphere_mesh(0.5, 12, 8),
        render.create_plane_mesh(10.0, 4),
    ]
    .into_iter()
    .map(|mesh| mesh.expect("a primitive mesh"))
    .collect();

    let textures: Vec<_> = (0..3)
        .map(|_| {
            render
                .create_texture_from_rgba(32, 32, &rgba())
                .expect("a procedural texture")
        })
        .collect();

    render.clear(window, giochi_engine_3d::math3d::Color3D::BLACK);
    let bytes = resident_bytes(render, &meshes, &textures);

    for texture in textures {
        assert!(render.destroy_texture(texture), "texture destroyed");
    }
    for mesh in meshes {
        assert!(render.destroy_mesh(mesh), "mesh destroyed");
    }
    assert!(render.destroy_shader(lit), "shader destroyed");
    assert!(render.destroy_shader(fixed), "shader destroyed");

    bytes
}

/// The estimate: texture bytes plus the mesh index/vertex counts the renderer reports.
fn resident_bytes(render: &RenderContext3D, meshes: &[u64], textures: &[u64]) -> u64 {
    let texture_bytes: u64 = textures
        .iter()
        .filter_map(|handle| render.texture_size(*handle))
        .map(|(width, height)| u64::from(width) * u64::from(height) * 4)
        .sum();
    let mesh_bytes: u64 = meshes
        .iter()
        .map(|handle| {
            // A vertex is 8 floats (position, normal, uv) and an index is a u32; the
            // exact stride is `Vertex`'s, and it is stable by ABI.
            u64::from(render.mesh_vertex_count(*handle)) * 32
                + u64::from(render.mesh_index_count(*handle)) * 4
        })
        .sum();
    texture_bytes + mesh_bytes
}

#[test]
#[ignore = "Ativado na Fase S4"]
fn a_hundred_load_unload_cycles_hold_their_gpu_memory() {
    let Some((_guard, _platform, window, mut render)) = headless() else {
        eprintln!("skipped: no GL driver");
        return;
    };

    let baseline = (
        render.mesh_count(),
        render.texture_count(),
        render.shader_count(),
    );

    let first = cycle(&mut render, &window);
    assert!(first > 0, "the estimate measured nothing");

    for cycle_index in 1..100 {
        let bytes = cycle(&mut render, &window);
        let delta = bytes.abs_diff(first) as f64 / first as f64;
        assert!(
            delta < 0.01,
            "cycle {cycle_index}: {bytes} bytes vs {first} on cycle 1 ({:.2}%)",
            delta * 100.0
        );
    }

    // The object registries are the exact measure; the byte estimate is the proxy.
    assert_eq!(
        (
            render.mesh_count(),
            render.texture_count(),
            render.shader_count()
        ),
        baseline,
        "an object outlived its unload"
    );
}

#[test]
#[ignore = "Ativado na Fase S4"]
fn release_all_empties_every_registry() {
    let Some((_guard, _platform, window, mut render)) = headless() else {
        eprintln!("skipped: no GL driver");
        return;
    };
    cycle(&mut render, &window);
    assert!(render.release_all(), "release_all reports failure");
    assert_eq!(render.mesh_count(), 0);
    assert_eq!(render.texture_count(), 0);
    assert_eq!(render.shader_count(), 0);
}
