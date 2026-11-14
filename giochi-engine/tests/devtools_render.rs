//! The console really draws, proved by reading the framebuffer.
//!
//! "The panel was shown" is not evidence: an egui integration with a wrong vertex layout, a
//! missing font texture or an off-screen clip rect renders perfectly black and reports
//! nothing. So the test renders two frames — console closed, console open — and asserts
//! that the pixels in the panel's area changed.
//!
//! One test per binary: SDL allows a single `EventPump` per process.

use giochi_engine::devtools::LineKind;
use giochi_engine::{Color, Engine, EngineConfig, Rect};

const SIZE: u32 = 480;

fn key(code: sdl2::keyboard::Keycode) -> sdl2::event::Event {
    sdl2::event::Event::KeyDown {
        timestamp: 0,
        window_id: 0,
        keycode: Some(code),
        scancode: None,
        keymod: sdl2::keyboard::Mod::NOMOD,
        repeat: false,
    }
}

/// How many pixels differ from `expected` inside `area` of a full-frame RGBA buffer.
fn changed_in(pixels: &[u8], width: u32, area: Rect, expected: (u8, u8, u8)) -> usize {
    let mut changed = 0;
    for row in 0..area.h as u32 {
        for column in 0..area.w as u32 {
            let offset = (((area.y as u32 + row) * width + (area.x as u32 + column)) * 4) as usize;
            let (r, g, b) = (pixels[offset], pixels[offset + 1], pixels[offset + 2]);
            if (r, g, b) != expected {
                changed += 1;
            }
        }
    }
    changed
}

/// Reads the whole frame. Small enough at 480x480 that a sub-rect read would only add a
/// coordinate conversion to get wrong.
fn frame(engine: &mut Engine) -> Vec<u8> {
    let (width, height) = engine.render().size();
    engine
        .render_mut()
        .read_pixels(Rect::new(0.0, 0.0, width as f32, height as f32))
        .expect("read_pixels must work")
}

#[test]
fn the_console_and_debug_draw_reach_the_framebuffer() {
    std::env::set_var("SDL_VIDEODRIVER", "dummy");

    let mut engine = Engine::new(EngineConfig::new(SIZE, SIZE, "giochi-devtools-test"))
        .expect("the dummy driver must initialise");
    engine.set_clear_color(Color::rgb(0, 0, 0));

    // ---- with everything closed, the frame is the clear colour --------
    engine.step(1.0 / 60.0).expect("step");
    let (width, height) = engine.render().size();
    let panel = Rect::new(0.0, height as f32 * 0.6, width as f32, height as f32 * 0.4);
    let closed = frame(&mut engine);
    assert_eq!(
        changed_in(&closed, width, panel, (0, 0, 0)),
        0,
        "a closed console must not paint anything"
    );

    // ---- F2 opens it, and the next frame is not black there -----------
    assert!(engine
        .devtools_mut()
        .handle_event(&key(sdl2::keyboard::Keycode::F2)));
    engine.step(1.0 / 60.0).expect("step");
    // egui lays the font atlas out on the first frame; a second frame removes any doubt
    // about whether the panel was merely queued.
    engine.step(1.0 / 60.0).expect("step");

    let opened = frame(&mut engine);
    let drawn = changed_in(&opened, width, panel, (0, 0, 0));
    assert!(
        drawn > 200,
        "the console must paint a panel with text in it, only {drawn} pixel(s) changed"
    );

    // ---- a command runs through the same path the keyboard uses -------
    engine.devtools_mut().console_mut().set_input("entities");
    engine.devtools_mut().set_entity_count(3);
    assert!(engine
        .devtools_mut()
        .handle_event(&key(sdl2::keyboard::Keycode::Return)));
    let output = engine.devtools().console().output_text().join("\n");
    assert!(output.contains("> entities"), "{output}");
    assert!(output.contains("entities 3"), "{output}");

    // ---- F3 draws debug geometry, and clearing it removes it ----------
    engine.devtools_mut().debug_draw_mut().set_visible(true);
    engine.devtools_mut().debug_draw_mut().draw_grid_2d(
        Rect::new(0.0, 0.0, 100.0, 100.0),
        20.0,
        Color::rgb(255, 0, 0),
    );
    engine.step(1.0 / 60.0).expect("step");

    let grid_area = Rect::new(0.0, 0.0, 100.0, 2.0);
    let with_grid = frame(&mut engine);
    assert!(
        changed_in(&with_grid, width, grid_area, (0, 0, 0)) > 0,
        "the debug grid must be visible"
    );
    assert_eq!(
        engine.devtools().debug_draw().segment_count(),
        0,
        "the buffer is cleared after each draw"
    );

    // ---- the expanded overlay is built from live state ----------------
    let lines = engine.devtools().overlay_lines().join("\n");
    assert!(lines.contains("ENTITIES 3"), "{lines}");
    assert!(lines.contains("CONSOLE ON"), "{lines}");

    // ---- closing the console stops the painting -----------------------
    assert!(engine
        .devtools_mut()
        .handle_event(&key(sdl2::keyboard::Keycode::F2)));
    engine.devtools_mut().debug_draw_mut().set_visible(false);
    engine.step(1.0 / 60.0).expect("step");
    let reclosed = frame(&mut engine);
    assert_eq!(
        changed_in(&reclosed, width, panel, (0, 0, 0)),
        0,
        "a closed console and hidden debug draw must leave the frame untouched"
    );

    // ---- and the console remembers what happened ----------------------
    let notices = engine
        .devtools()
        .console()
        .output()
        .iter()
        .filter(|line| line.kind() == LineKind::Notice)
        .count();
    assert!(notices >= 1, "opening the console logs a notice");
}
