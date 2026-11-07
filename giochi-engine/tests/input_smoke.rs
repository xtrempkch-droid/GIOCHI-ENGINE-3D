//! Headless integration test for the input pipeline.
//!
//! Uses SDL's `dummy` video driver and injects real SDL events through
//! `Platform::push_event`, so the event → `Input::update` → query path is exercised
//! end to end rather than only the pure state machines.
//!
//! One test per binary: SDL allows a single `EventPump` per process.

use giochi_engine::input::keyboard::{KEY_A, KEY_ESCAPE, KEY_W};
use giochi_engine::input::mouse::MOUSE_RIGHT;
use giochi_engine::{quit_signal, GameLoop, Input, Platform, WindowConfig};
use sdl2::event::Event;
use sdl2::keyboard::{Keycode, Mod};

fn key_down(keycode: Keycode) -> Event {
    Event::KeyDown {
        timestamp: 0,
        window_id: 0,
        keycode: Some(keycode),
        scancode: None,
        keymod: Mod::NOMOD,
        repeat: false,
    }
}

#[test]
fn input_pipeline_without_a_display() {
    std::env::set_var("SDL_VIDEODRIVER", "dummy");

    let platform = Platform::new().expect("dummy driver must initialise");
    let _window = platform
        .create_window(WindowConfig::new(320, 200), "giochi-input-test")
        .expect("window must be created");

    let mut input = Input::new(&platform);
    let mut game_loop = GameLoop::new();

    // The controller subsystem must at least be queryable; the dummy driver has no
    // controllers, so the gamepad reports "not connected" without failing.
    assert!(platform.game_controller_subsystem().is_ok());
    assert!(!input.gamepad().is_connected());

    // --- a key press arrives through the real event queue -------------------
    platform
        .push_event(key_down(Keycode::W))
        .expect("SDL must accept the key event");

    let events = platform.poll_events();
    input.update(&events);

    assert!(input.key_down(KEY_W), "W must be held after the event");
    assert!(input.key_pressed(KEY_W), "the pressed edge must be visible");
    assert!(!input.key_down(KEY_A));

    // --- the edge clears on the next frame ----------------------------------
    input.update(&[]);
    assert!(input.key_down(KEY_W));
    assert!(!input.key_pressed(KEY_W));

    // --- mouse button events reach the mouse state -------------------------
    platform
        .push_event(Event::MouseButtonDown {
            timestamp: 0,
            window_id: 0,
            which: 0,
            mouse_btn: sdl2::mouse::MouseButton::Right,
            clicks: 1,
            x: 40,
            y: 30,
        })
        .expect("SDL must accept the mouse event");

    let events = platform.poll_events();
    input.update(&events);

    assert!(input.mouse_down(MOUSE_RIGHT));
    assert_eq!(input.mouse().position(), (40.0, 30.0));

    // --- Escape still stops the loop through the shared QuitSignal ----------
    platform
        .push_event(key_down(Keycode::Escape))
        .expect("SDL must accept the key event");

    for event in platform.poll_events() {
        input.update(std::slice::from_ref(&event));
        game_loop.handle_quit_signal(quit_signal(&event));
    }

    assert!(input.key_down(KEY_ESCAPE));
    assert!(!game_loop.is_running(), "Escape must stop the loop");
}
