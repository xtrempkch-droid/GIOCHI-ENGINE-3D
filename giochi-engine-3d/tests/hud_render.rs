//! Pixel-level test for the egui HUD over the 3D scene.
//!
//! A HUD that draws nothing is indistinguishable from a HUD that works, from the outside:
//! no GL error, no log line, just a scene with no overlay. This test reads the framebuffer
//! and asserts that the top-left corner — where the test puts its window — is neither the
//! clear colour nor uniform, which is only true if text and a panel actually landed there.
//!
//! ```text
//! SDL_VIDEODRIVER=offscreen cargo test -p giochi-engine-3d --features hud --test hud_render
//! ```

mod common;

use giochi_engine::platform::sdl::Platform;
use giochi_engine::platform::window::{Window, WindowConfig};
use giochi_engine_3d::context::{self, GlesVersion};
use giochi_engine_3d::math3d::{Camera3D, Color3D};
use giochi_engine_3d::ui::GlesHud;
use giochi_engine_3d::RenderContext3D;

const WIDTH: u32 = 320;
const HEIGHT: u32 = 200;

#[test]
fn the_hud_paints_pixels_on_top_of_the_scene() {
    let _guard = common::platform_guard();
    std::env::set_var("SDL_VIDEODRIVER", "offscreen");

    let platform = match Platform::new() {
        Ok(platform) => platform,
        Err(_) => {
            eprintln!("hud_render: no SDL video driver, skipping");
            return;
        }
    };
    if context::configure_attributes(&platform, GlesVersion::Gles2).is_err() {
        eprintln!("hud_render: no GLES configuration, skipping");
        return;
    }
    let window = match Window::new_opengl(&platform, WindowConfig::new(WIDTH, HEIGHT), "hud") {
        Ok(window) => window,
        Err(error) => {
            eprintln!("hud_render: no GL window ({error}), skipping");
            return;
        }
    };
    let mut render = RenderContext3D::new(context::create(&window).expect("GL context"));
    let mut hud = GlesHud::new(render.glow()).expect("HUD program");

    let camera = Camera3D::from_parts(
        giochi_engine_3d::math3d::Vec3::new(0.0, 0.0, 3.0),
        giochi_engine_3d::math3d::Vec3::ZERO,
        giochi_engine_3d::math3d::Vec3::UP,
    )
    .with_drawable_size(WIDTH, HEIGHT);
    let clear = Color3D::new(0.1, 0.2, 0.3, 1.0);

    // Two frames: the first is where egui builds and uploads the font atlas, so a
    // single-frame test would pass vacuously with zero textures.
    let mut draws = 0;
    for _ in 0..2 {
        render.begin_frame(&window);
        render.clear(&window, clear);
        let (width, height) = render.drawable_size();
        draws = hud
            .draw(render.glow(), (width, height), 1.0 / 60.0, &mut |ctx| {
                egui::Window::new("hud")
                    .anchor(egui::Align2::LEFT_TOP, egui::vec2(4.0, 4.0))
                    .title_bar(false)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.label("HUD OVER 3D");
                        ui.label("second line");
                    });
            })
            .expect("HUD frame");
        // `--` no scene mesh needed: the point is the overlay, and an empty scene keeps the
        // probe about the HUD rather than about what the camera happens to see.
        let pixels = render
            .read_pixels(WIDTH, HEIGHT)
            .expect("framebuffer readback");
        if draws > 0 {
            assert_hud_pixels(&pixels, &camera, clear);
        }
        render.end_frame(&window);
    }

    assert!(
        draws > 0,
        "the HUD tessellated nothing: egui produced no mesh at all"
    );
    hud.destroy(render.glow());
}

/// Fails unless the top-left corner has more than one colour and is not just the clear
/// colour.
///
/// `glReadPixels` returns row 0 at the **bottom**, so "top left" is the last rows.
fn assert_hud_pixels(pixels: &[u8], _camera: &Camera3D, clear: Color3D) {
    let clear_rgb = [
        (clear.r * 255.0).round() as u8,
        (clear.g * 255.0).round() as u8,
        (clear.b * 255.0).round() as u8,
    ];
    let mut colours = std::collections::HashSet::new();
    for y in 0..96usize {
        for x in 0..180usize {
            let row = HEIGHT as usize - 1 - y;
            let base = (row * WIDTH as usize + x) * 4;
            let pixel = &pixels[base..base + 3];
            // Ignore the clear colour: the scene behind the HUD is empty, so those pixels
            // are "nothing was drawn here".
            if pixel != clear_rgb {
                colours.insert((pixel[0], pixel[1], pixel[2]));
            }
        }
    }
    assert!(
        colours.len() > 3,
        "the HUD region has {} non-background colours; the overlay did not paint",
        colours.len()
    );
}
