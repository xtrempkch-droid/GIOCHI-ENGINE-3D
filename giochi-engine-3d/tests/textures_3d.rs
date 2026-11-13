//! Textures on meshes, proved by reading the framebuffer.
//!
//! The UV attribute and the `textured` shader existed before the texture API, and a
//! scene drawn with `u_texture` bound to nothing is *black*, not an error — so "the
//! draw call succeeded" says nothing. These tests sample the frame and assert the
//! colours the texture contains.
//!
//! Skipped cleanly when no GL driver is available (`docs/3D.md` §5).

mod common;

use std::sync::MutexGuard;

use giochi_engine::platform::sdl::Platform;
use giochi_engine::platform::window::{Window, WindowConfig};
use giochi_engine_3d::context::{self, GlesVersion};
use giochi_engine_3d::math3d::{Camera3D, Color3D, Transform3D, Vec3};
use giochi_engine_3d::shader::BuiltinShader;
use giochi_engine_3d::RenderContext3D;

const SIZE: u32 = 96;
const BACKGROUND: Color3D = Color3D::new(0.0, 0.0, 0.0, 1.0);
/// Red and blue are far apart in both hue and luminance, so "the texture sampled
/// something other than black" cannot be confused with "the tint was applied".
const LEFT: [u8; 3] = [255, 0, 0];
const RIGHT: [u8; 3] = [0, 0, 255];

fn headless() -> Option<(MutexGuard<'static, ()>, Platform, Window, RenderContext3D)> {
    let guard = common::platform_guard();
    std::env::set_var("SDL_VIDEODRIVER", "offscreen");
    let platform = Platform::new().ok()?;
    context::configure_attributes(&platform, GlesVersion::Gles2).ok()?;
    let window = Window::new_opengl(&platform, WindowConfig::new(SIZE, SIZE), "textures").ok()?;
    let render = RenderContext3D::new(context::create(&window).ok()?);
    Some((guard, platform, window, render))
}

/// A 32x32 texture whose left half is red and right half is blue.
///
/// Two colours, not one, because a single-colour texture cannot distinguish "sampled
/// the texture" from "drew the tint" or "sampled one texel everywhere".
fn split_texture(render: &mut RenderContext3D) -> u64 {
    const W: u32 = 32;
    const H: u32 = 32;
    let mut pixels = vec![0u8; (W * H * 4) as usize];
    for y in 0..H {
        for x in 0..W {
            let offset = ((y * W + x) * 4) as usize;
            let colour = if x < W / 2 { LEFT } else { RIGHT };
            pixels[offset..offset + 4].copy_from_slice(&[colour[0], colour[1], colour[2], 255]);
        }
    }

    render
        .create_texture_from_rgba(W, H, &pixels)
        .expect("procedural texture")
}

/// Pixels of `colour` (within a generous tolerance for filtering and lighting).
fn count_colour(pixels: &[u8], colour: [u8; 3]) -> usize {
    pixels
        .chunks_exact(4)
        .filter(|pixel| {
            // The textured shader multiplies by a Lambert term and a white tint, so a
            // texel can darken; requiring the dominant channel keeps the check honest
            // without demanding an exact byte.
            let [r, g, b] = [pixel[0], pixel[1], pixel[2]];
            match colour {
                [255, 0, 0] => r > g.saturating_add(40) && r > b.saturating_add(40) && r > 40,
                _ => b > g.saturating_add(40) && b > r.saturating_add(40) && b > 40,
            }
        })
        .count()
}

#[test]
fn a_procedural_texture_is_sampled_by_a_mesh() {
    let Some((_guard, _platform, window, mut render)) = headless() else {
        eprintln!("skipped: no OpenGL ES driver available");
        return;
    };

    let texture = split_texture(&mut render);
    assert_eq!(render.texture_size(texture), Some((32, 32)));

    let mesh = render.create_cube_mesh(2.0).expect("cube mesh");
    let shader = render
        .create_builtin_shader(BuiltinShader::Textured)
        .expect("textured shader");

    render.set_clear_color(BACKGROUND);
    render.begin_frame(&window);

    let camera = Camera3D::new().with_aspect(1.0);
    let model = Transform3D::from_translation(Vec3::new(0.0, 0.0, -5.0));
    assert!(render
        .draw_mesh_textured(mesh, shader, texture, &model, &camera, Color3D::WHITE)
        .expect("draw"));

    let error = render.take_gl_error();
    assert_eq!(error, 0, "GL error 0x{error:04x} while drawing textured");

    let pixels = render.read_pixels(SIZE, SIZE).expect("read back");
    let red = count_colour(&pixels, LEFT);
    let blue = count_colour(&pixels, RIGHT);
    assert!(
        red > 100,
        "the red half of the texture did not reach the frame ({red} pixels)"
    );
    assert!(
        blue > 100,
        "the blue half of the texture did not reach the frame ({blue} pixels); \
         a black mesh here means the sampler was never bound"
    );

    assert!(render.destroy_texture(texture));
    assert!(render.destroy_mesh(mesh));
    assert!(render.destroy_shader(shader));
}

#[test]
fn a_texture_loads_from_an_image_file() {
    let Some((_guard, _platform, _window, mut render)) = headless() else {
        eprintln!("skipped: no OpenGL ES driver available");
        return;
    };

    // The same twin the 2D asset tests use; 64x64 is what `assets/checker.png` is.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/checker.png");
    let texture = render.create_texture(&path).expect("checker texture");
    assert_eq!(render.texture_size(texture), Some((64, 64)));
    assert!(render.destroy_texture(texture));
    assert_eq!(render.texture_count(), 0);
}

#[test]
fn a_missing_image_is_an_asset_error_not_a_black_texture() {
    let Some((_guard, _platform, _window, mut render)) = headless() else {
        eprintln!("skipped: no OpenGL ES driver available");
        return;
    };

    let missing = std::path::Path::new("/nonexistent/giochi-no-such-texture.png");
    let error = render.create_texture(missing).expect_err("must fail");
    assert!(
        matches!(error, giochi_engine_3d::Error3D::Asset(_)),
        "expected Asset, got {error:?}"
    );
    assert_eq!(render.texture_count(), 0);
}

#[test]
fn an_invalid_texture_handle_draws_nothing_and_reports_no_size() {
    let Some((_guard, _platform, window, mut render)) = headless() else {
        eprintln!("skipped: no OpenGL ES driver available");
        return;
    };

    let mesh = render.create_cube_mesh(1.0).expect("cube mesh");
    let shader = render
        .create_builtin_shader(BuiltinShader::Textured)
        .expect("textured shader");
    let camera = Camera3D::new().with_aspect(1.0);
    let model = Transform3D::from_translation(Vec3::new(0.0, 0.0, -4.0));

    render.begin_frame(&window);
    let drew = render
        .draw_mesh_textured(mesh, shader, 99, &model, &camera, Color3D::WHITE)
        .expect("an unknown handle is Ok(false), not an error");
    assert!(!drew);

    assert_eq!(render.texture_size(99), None);
    assert!(!render.destroy_texture(99), "double destroy is reported");
    assert!(!render.destroy_texture(0), "0 is never a valid handle");
}

#[test]
fn release_all_deletes_textures_too() {
    let Some((_guard, _platform, _window, mut render)) = headless() else {
        eprintln!("skipped: no OpenGL ES driver available");
        return;
    };

    let _ = split_texture(&mut render);
    let _ = split_texture(&mut render);
    assert_eq!(render.texture_count(), 2);

    assert!(
        render.release_all(),
        "nothing was held elsewhere, so it must be clean"
    );
    assert_eq!(render.texture_count(), 0);
}
