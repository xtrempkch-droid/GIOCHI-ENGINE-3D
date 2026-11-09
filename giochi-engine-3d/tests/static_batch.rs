//! A static batch must draw exactly what the individual meshes drew.
//!
//! The failure mode that matters is quiet: forget the index offset when concatenating
//! and every piece after the first draws the first piece's vertices, so the frame shows
//! one object where N should be — with no GL error and no missing draw call. The tests
//! below read the framebuffer and count lit pixels, which is the only way to see it.
//!
//! ```text
//! SDL_VIDEODRIVER=offscreen cargo test -p giochi-engine-3d --test static_batch
//! ```

use giochi_engine::platform::sdl::Platform;
use giochi_engine::platform::window::{Window, WindowConfig};
use giochi_engine_3d::context::{self, GlesVersion};
use giochi_engine_3d::math3d::{Camera3D, Color3D, Quat, Transform3D, Vec3};
use giochi_engine_3d::mesh::StaticBatchBuilder;
use giochi_engine_3d::shader::BuiltinShader;
use giochi_engine_3d::{DirectionalLight, RenderContext3D};

const WIDTH: u32 = 160;
const HEIGHT: u32 = 120;

fn headless_context() -> Option<(Platform, Window)> {
    std::env::set_var("SDL_VIDEODRIVER", "offscreen");

    let platform = Platform::new().ok()?;
    context::configure_attributes(&platform, GlesVersion::Gles2).ok()?;
    let window =
        Window::new_opengl(&platform, WindowConfig::new(WIDTH, HEIGHT), "static_batch").ok()?;
    Some((platform, window))
}

/// Number of pixels that are not the black background.
fn lit_pixels(pixels: &[u8]) -> usize {
    pixels
        .chunks_exact(4)
        .filter(|px| px[0] > 8 || px[1] > 8 || px[2] > 8)
        .count()
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

#[test]
fn a_batch_draws_the_same_pixels_as_the_separate_meshes() {
    let Some((_platform, window)) = headless_context() else {
        eprintln!("skipped: no GL driver (SDL_VIDEODRIVER=offscreen)");
        return;
    };
    let mut render = RenderContext3D::new(context::create(&window).expect("context"));

    let cube = render.create_cube_mesh(1.0).expect("cube");
    let shader = render
        .create_builtin_shader(BuiltinShader::Phong)
        .expect("shader");

    let camera = Camera3D::from_parts(Vec3::new(0.0, 2.0, 6.0), Vec3::ZERO, Vec3::UP)
        .with_clip_planes(0.1, 100.0);
    render.set_clear_color(Color3D::BLACK);
    render.set_light(DirectionalLight::sun());

    // Five crates: four around the edge, one raised and rotated. Enough to make an
    // index-offset bug visible as a missing object rather than a slightly different
    // shade.
    let transforms: Vec<Transform3D> = [
        Vec3::new(-1.6, 0.0, 0.0),
        Vec3::new(1.6, 0.0, 0.0),
        Vec3::new(0.0, 0.0, -1.2),
        Vec3::new(0.0, 0.0, 1.2),
        Vec3::new(0.0, 1.4, 0.0),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, position)| {
        Transform3D::from_translation(position)
            .with_rotation(Quat::from_axis_angle(Vec3::UP, index as f32 * 0.4))
    })
    .collect();

    let colour = Color3D::new(0.85, 0.62, 0.35, 1.0);

    // Arm 1: one draw per crate.
    render.begin_frame(&window);
    render.clear(&window, Color3D::BLACK);
    for transform in &transforms {
        render
            .draw_mesh_colored(cube, shader, transform, &camera, colour)
            .expect("draw");
    }
    let separate = render.read_pixels(WIDTH, HEIGHT).expect("pixels");

    // Arm 2: one static batch, built from the CPU-side cube geometry.
    let (vertices, indices) = giochi_engine_3d::mesh::cube(1.0);
    let mut builder = StaticBatchBuilder::with_capacity(
        vertices.len() * transforms.len(),
        indices.len() * transforms.len(),
    );
    for transform in &transforms {
        builder = builder
            .add_mesh(&vertices, &indices, &transform.matrix())
            .expect("add");
    }
    assert_eq!(builder.len(), transforms.len());

    let batch = giochi_engine_3d::mesh::StaticBatch::build(&mut render, builder).expect("batch");
    assert_eq!(batch.pieces(), transforms.len());

    render.begin_frame(&window);
    render.clear(&window, Color3D::BLACK);
    render
        .draw_mesh_colored(
            batch.mesh(),
            shader,
            &Transform3D::IDENTITY,
            &camera,
            colour,
        )
        .expect("draw batch");
    let batched = render.read_pixels(WIDTH, HEIGHT).expect("pixels");

    // A frame with a handful of lit pixels would make the equality assertion below
    // pass for the wrong reason: two empty pictures are also identical.
    assert!(
        lit_pixels(&separate) > 2_000,
        "the reference frame is nearly empty ({} lit pixels)",
        lit_pixels(&separate)
    );
    assert_eq!(
        lit_pixels(&batched),
        lit_pixels(&separate),
        "the batch lights a different number of pixels than the separate draws, \
         which is what a missing index offset looks like"
    );

    let differing = differing_pixels(&separate, &batched, 2);
    let total = WIDTH as usize * HEIGHT as usize;
    assert!(
        differing * 1000 / total < 1,
        "{differing} of {total} pixels differ between the batch and the separate draws"
    );

    // The batch is one mesh, so it is destroyed like any other.
    assert!(render.destroy_mesh(batch.mesh()));
    assert!(render.destroy_mesh(cube));
    assert!(render.destroy_shader(shader));
    assert!(render.release_all(), "nothing may leak");
}

#[test]
fn a_batch_that_replaces_fifty_meshes_is_one_draw() {
    let Some((_platform, window)) = headless_context() else {
        eprintln!("skipped: no GL driver (SDL_VIDEODRIVER=offscreen)");
        return;
    };
    let mut render = RenderContext3D::new(context::create(&window).expect("context"));

    let (vertices, indices) = giochi_engine_3d::mesh::plane(2.0, 1);
    let mut builder = StaticBatchBuilder::new();
    // A 10x5 grid of tiles: the "scenery" case the phase is for.
    for row in 0..5 {
        for column in 0..10 {
            let model = Transform3D::from_translation(Vec3::new(
                column as f32 * 2.0 - 9.0,
                0.0,
                row as f32 * 2.0 - 4.0,
            ))
            .matrix();
            builder = builder.add_mesh(&vertices, &indices, &model).expect("add");
        }
    }

    assert_eq!(builder.len(), 50);
    let batch = giochi_engine_3d::mesh::StaticBatch::build(&mut render, builder).expect("batch");

    assert_eq!(batch.pieces(), 50);
    // 50 separate meshes would be 50 draw calls; the batch is one mesh, drawn once.
    assert_eq!(render.mesh_count(), 1, "the batch is one mesh");
    assert_eq!(
        render.mesh_index_count(batch.mesh()),
        (indices.len() * 50) as u32
    );
    assert_eq!(
        render.mesh_vertex_count(batch.mesh()),
        (vertices.len() * 50) as u32
    );

    assert!(render.release_all());
}

#[test]
fn an_empty_batch_is_an_error_not_an_empty_mesh() {
    let Some((_platform, window)) = headless_context() else {
        eprintln!("skipped: no GL driver (SDL_VIDEODRIVER=offscreen)");
        return;
    };
    let mut render = RenderContext3D::new(context::create(&window).expect("context"));

    // A zero-vertex buffer would draw nothing forever without explaining why, so the
    // builder refuses instead of uploading one.
    let result = giochi_engine_3d::mesh::StaticBatch::build(&mut render, StaticBatchBuilder::new());
    assert!(result.is_err());
    assert_eq!(render.mesh_count(), 0, "nothing may have been uploaded");
}
