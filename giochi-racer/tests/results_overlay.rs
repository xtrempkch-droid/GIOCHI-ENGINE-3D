//! The results overlay, measured in pixels.
//!
//! A finished race has to *say* it finished. The overlay is painted with `Painter::text`
//! like the rest of the HUD, and a string that never reaches the framebuffer draws exactly
//! nothing — from the outside, identical to a working screen. So this file draws a
//! finished [`HudState`] and a running one on the same surface and compares what is left
//! behind: the results panel adds text in the middle of the screen that a racing HUD does
//! not have.
//!
//! ```text
//! SDL_VIDEODRIVER=offscreen cargo test -p giochi-racer --test results_overlay
//! ```

use giochi_engine::platform::sdl::Platform;
use giochi_engine::platform::window::{Window, WindowConfig};
use giochi_engine_3d::context::{self, GlesVersion};
use giochi_engine_3d::math3d::Color3D;
use giochi_engine_3d::ui::GlesHud;
use giochi_engine_3d::RenderContext3D;
use giochi_racer::vehicle::hud::{self, HudState};

const WIDTH: u32 = 800;
const HEIGHT: u32 = 480;
const BACKGROUND: Color3D = Color3D::new(0.0, 0.0, 0.0, 1.0);

fn state(finished: bool, new_best: bool) -> HudState {
    HudState {
        track: "OVAL EASY".to_owned(),
        speed_kmh: 84.0,
        gear: 3,
        lap: 3,
        total_laps: 3,
        lap_time: 17.25,
        total_time: 52.5,
        best_lap: Some(16.9),
        event: None,
        finished,
        new_best,
    }
}

/// Paints `hud_state` and returns the pixels, or `None` when this machine has no GL.
fn paint(state: &HudState) -> Option<Vec<u8>> {
    std::env::set_var("SDL_VIDEODRIVER", "offscreen");
    let platform = Platform::new().ok()?;
    context::configure_attributes(&platform, GlesVersion::Gles2).ok()?;
    let window = Window::new_opengl(&platform, WindowConfig::new(WIDTH, HEIGHT), "results").ok()?;
    let mut render = RenderContext3D::new(context::create(&window).ok()?);
    let mut hud = GlesHud::new(render.glow()).ok()?;

    // Four frames: the first uploads the font atlas, and egui lays out text for the next
    // frame when the atlas changed under it (same reason as `hud_pixels`).
    let mut pixels = Vec::new();
    for _ in 0..4 {
        render.begin_frame(&window);
        render.clear(&window, BACKGROUND);
        hud::draw(&mut hud, &render, (WIDTH, HEIGHT), 1.0 / 60.0, state).expect("the HUD draws");
        render.end_frame(&window);
        pixels = render
            .read_pixels(WIDTH, HEIGHT)
            .expect("the frame reads back");
    }
    Some(pixels)
}

/// Lit pixels inside the middle band of the screen, where the results panel sits.
///
/// The racing HUD keeps its text in the corners, so this band is empty while racing and
/// full when the panel is up. That is the difference the test measures.
fn lit_in_the_middle(pixels: &[u8]) -> usize {
    let (top, bottom) = (HEIGHT as usize * 35 / 100, HEIGHT as usize * 75 / 100);
    let mut lit = 0;
    for y in top..bottom {
        for x in 0..WIDTH as usize {
            let offset = (y * WIDTH as usize + x) * 4;
            if pixels[offset] > 60 || pixels[offset + 1] > 60 || pixels[offset + 2] > 60 {
                lit += 1;
            }
        }
    }
    lit
}

#[test]
fn the_results_panel_reaches_the_framebuffer() {
    let (Some(racing), Some(finished), Some(record)) = (
        paint(&state(false, false)),
        paint(&state(true, false)),
        paint(&state(true, true)),
    ) else {
        eprintln!("results_overlay: no GL driver, skipping");
        return;
    };

    let racing = lit_in_the_middle(&racing);
    let finished = lit_in_the_middle(&finished);
    let record = lit_in_the_middle(&record);

    assert!(
        finished > racing.max(200),
        "the results panel drew {finished} lit pixels in the middle, a racing frame has {racing}"
    );
    // "NEW BEST" is one more line of text over "RACE COMPLETE / total / best lap".
    assert!(
        record > finished,
        "the new-best line drew nothing ({record} vs {finished} lit pixels)"
    );
}

#[test]
fn a_racing_frame_keeps_the_middle_clear() {
    let Some(racing) = paint(&state(false, false)) else {
        eprintln!("results_overlay: no GL driver, skipping");
        return;
    };
    // The lap banner lives at y=120 of 480, above this band; the corners carry the rest.
    assert!(
        lit_in_the_middle(&racing) < 200,
        "a racing frame has {} lit pixels in the middle of the screen",
        lit_in_the_middle(&racing)
    );
}
