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

#[test]
fn the_hud_puts_the_gl_state_back_the_way_it_found_it() {
    // The painter used to force `disable(BLEND)` on the way out, which broke the plausible
    // order "HUD first, particles after": the particle pass asks `GlState::blend` for
    // source-alpha blending once per frame and the HUD had just turned it off, so the
    // particles drew opaque. This asserts the state the caller set is the state it gets
    // back.
    use glow::HasContext;

    let _guard = common::platform_guard();
    std::env::set_var("SDL_VIDEODRIVER", "offscreen");

    let Ok(platform) = Platform::new() else {
        eprintln!("hud_render: no SDL video driver, skipping");
        return;
    };
    if context::configure_attributes(&platform, GlesVersion::Gles2).is_err() {
        eprintln!("hud_render: no GLES configuration, skipping");
        return;
    }
    let Ok(window) = Window::new_opengl(&platform, WindowConfig::new(WIDTH, HEIGHT), "hud state")
    else {
        eprintln!("hud_render: no GL window, skipping");
        return;
    };
    let mut render = RenderContext3D::new(context::create(&window).expect("GL context"));
    let mut hud = GlesHud::new(render.glow()).expect("HUD program");

    render.begin_frame(&window);
    render.clear(&window, Color3D::new(0.1, 0.2, 0.3, 1.0));
    // Re-borrowed after the `&mut` calls, which is why it is not held for the whole test.
    let gl = render.glow();

    // A caller's state, set before the HUD runs: blending on with the engine's own factors.
    // SAFETY: current context, core GLES 2.0.
    unsafe {
        gl.enable(glow::BLEND);
        gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
        gl.enable(glow::DEPTH_TEST);
        gl.enable(glow::CULL_FACE);
    }

    for _ in 0..2 {
        hud.draw(gl, (WIDTH, HEIGHT), 1.0 / 60.0, &mut |ctx| {
            egui::Window::new("state").show(ctx, |ui| ui.label("state"));
        })
        .expect("HUD frame");
    }

    // SAFETY: same context; these are queries.
    let (blend, depth, cull, src, dst) = unsafe {
        (
            gl.is_enabled(glow::BLEND),
            gl.is_enabled(glow::DEPTH_TEST),
            gl.is_enabled(glow::CULL_FACE),
            gl.get_parameter_i32(glow::BLEND_SRC_ALPHA) as u32,
            gl.get_parameter_i32(glow::BLEND_DST_ALPHA) as u32,
        )
    };
    assert!(blend, "the HUD turned blending off");
    assert!(depth, "the HUD turned the depth test off");
    assert!(cull, "the HUD turned face culling off");
    assert_eq!(
        src,
        glow::SRC_ALPHA,
        "the HUD changed the blend source factor"
    );
    assert_eq!(
        dst,
        glow::ONE_MINUS_SRC_ALPHA,
        "the HUD changed the blend destination factor"
    );

    hud.destroy(gl);
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
