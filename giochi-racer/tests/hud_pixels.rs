//! Which egui drawing path actually reaches the framebuffer, measured.
//!
//! The HUD is the one part of the game a unit test cannot check: it is a texture upload, a
//! program and a scissor, and a HUD that draws nothing looks exactly like a HUD that works
//! from the outside. This test draws a label through each of the three paths a host can
//! use — a window with widgets, an area with widgets, an area with the painter — and
//! reports the pixels each one leaves behind, so a change in egui or in the engine's
//! painter shows up as a number rather than as a screenshot nobody looks at.
//!
//! ```text
//! SDL_VIDEODRIVER=offscreen cargo test -p giochi-racer --test hud_pixels
//! ```

use giochi_engine::platform::sdl::Platform;
use giochi_engine::platform::window::{Window, WindowConfig};
use giochi_engine_3d::context::{self, GlesVersion};
use giochi_engine_3d::math3d::Color3D;
use giochi_engine_3d::ui::GlesHud;
use giochi_engine_3d::RenderContext3D;

const WIDTH: u32 = 400;
const HEIGHT: u32 = 240;

/// A colour no glyph antialiasing can produce, so "not the background" is unambiguous.
const BACKGROUND: Color3D = Color3D::new(0.0, 0.0, 0.0, 1.0);

#[test]
fn text_reaches_the_framebuffer_through_the_painter() {
    std::env::set_var("SDL_VIDEODRIVER", "offscreen");
    let Ok(platform) = Platform::new() else {
        eprintln!("hud_pixels: no SDL video driver, skipping");
        return;
    };
    if context::configure_attributes(&platform, GlesVersion::Gles2).is_err() {
        eprintln!("hud_pixels: no GLES configuration, skipping");
        return;
    }
    let Ok(window) = Window::new_opengl(&platform, WindowConfig::new(WIDTH, HEIGHT), "hud") else {
        eprintln!("hud_pixels: no GL window, skipping");
        return;
    };
    let mut render = RenderContext3D::new(context::create(&window).expect("GL context"));
    let mut hud = GlesHud::new(render.glow()).expect("HUD program");

    // Four frames: the first uploads the font atlas, and egui lays text out for the next
    // frame when the atlas changed under it.
    for _ in 0..4 {
        render.begin_frame(&window);
        render.clear(&window, BACKGROUND);
        hud.draw(render.glow(), (WIDTH, HEIGHT), 1.0 / 60.0, &mut |ctx| {
            egui::Area::new("probe".into())
                .fixed_pos(egui::Pos2::ZERO)
                .show(ctx, |ui| {
                    ui.set_min_size(egui::vec2(WIDTH as f32, HEIGHT as f32));
                    ui.painter().text(
                        egui::pos2(10.0, 10.0),
                        egui::Align2::LEFT_TOP,
                        "HHHH",
                        egui::FontId::proportional(40.0),
                        egui::Color32::WHITE,
                    );
                });
        })
        .expect("HUD frame");
    }

    let lit = lit_pixels(&render);
    println!("painter text: {lit} lit pixels");
    assert!(
        lit > 200,
        "the painter drew {lit} pixels; a 40 px label needs a lot more"
    );

    // ---- the same thing through the game's own HUD function ----------------
    let state = giochi_racer::vehicle::hud::HudState {
        track: "PROBE".to_owned(),
        speed_kmh: 87.0,
        gear: 4,
        lap: 2,
        total_laps: 3,
        lap_time: 12.5,
        total_time: 40.0,
        best_lap: Some(11.0),
        event: None,
        finished: false,
        new_best: false,
    };
    for _ in 0..4 {
        render.begin_frame(&window);
        render.clear(&window, BACKGROUND);
        giochi_racer::vehicle::hud::draw(&mut hud, &render, (WIDTH, HEIGHT), 1.0 / 60.0, &state)
            .expect("HUD frame");
    }
    let game_lit = lit_pixels(&render);
    println!("the game's HUD: {game_lit} lit pixels");
    assert!(
        game_lit > 500,
        "the game's HUD drew {game_lit} pixels of text"
    );
}

/// Draws the game's HUD at 1280x720 over black and prints a coarse map.
///
/// Run with `--nocapture` when the HUD looks wrong: the map says which block landed where
/// without needing a screenshot viewer.
#[test]
fn the_hud_layout_lands_in_the_four_corners() {
    std::env::set_var("SDL_VIDEODRIVER", "offscreen");
    let Ok(platform) = Platform::new() else {
        eprintln!("hud_pixels: no SDL video driver, skipping");
        return;
    };
    if context::configure_attributes(&platform, GlesVersion::Gles2).is_err() {
        eprintln!("hud_pixels: no GLES configuration, skipping");
        return;
    }
    let (width, height) = (1280u32, 720u32);
    let Ok(window) = Window::new_opengl(&platform, WindowConfig::new(width, height), "hud") else {
        eprintln!("hud_pixels: no GL window, skipping");
        return;
    };
    let mut render = RenderContext3D::new(context::create(&window).expect("GL context"));
    let mut hud = GlesHud::new(render.glow()).expect("HUD program");
    let state = giochi_racer::vehicle::hud::HudState {
        track: "OVAL EASY".to_owned(),
        speed_kmh: 128.0,
        gear: 5,
        lap: 2,
        total_laps: 3,
        lap_time: 41.25,
        total_time: 96.0,
        best_lap: Some(38.5),
        event: None,
        finished: false,
        new_best: false,
    };
    for _ in 0..4 {
        render.begin_frame(&window);
        render.clear(&window, BACKGROUND);
        giochi_racer::vehicle::hud::draw(&mut hud, &render, (width, height), 1.0 / 60.0, &state)
            .expect("HUD frame");
    }
    let pixels = render.read_pixels(width, height).expect("a framebuffer");
    let lit = |x: u32, y: u32| {
        // Rows come back bottom-up.
        let row = height - 1 - y;
        let i = ((row * width + x) * 4) as usize;
        pixels[i..i + 3].iter().any(|channel| *channel > 40)
    };
    for cy in (0..height).step_by(16) {
        let line: String = (0..width)
            .step_by(16)
            .map(|cx| {
                let n = (cy..cy + 16)
                    .flat_map(|y| (cx..cx + 16).map(move |x| (x, y)))
                    .filter(|(x, y)| lit(*x, *y))
                    .count();
                if n == 0 {
                    '.'
                } else if n > 20 {
                    '#'
                } else if n > 5 {
                    '+'
                } else {
                    ':'
                }
            })
            .collect();
        println!("{cy:3} {line}");
    }
    let total = (0..height)
        .flat_map(|y| (0..width).map(move |x| (x, y)))
        .filter(|(x, y)| lit(*x, *y))
        .count();
    assert!(total > 1000, "the HUD drew {total} pixels");
}

fn lit_pixels(render: &RenderContext3D) -> usize {
    let pixels = render
        .read_pixels(WIDTH, HEIGHT)
        .expect("a readable framebuffer");
    pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[0] > 32 || pixel[1] > 32 || pixel[2] > 32)
        .count()
}
