//! Headless integration smoke test for the platform layer.
//!
//! SDL's `dummy` video driver lets the full init → window → event → loop path run
//! without a display, so this test also executes in CI containers.
//!
//! The whole file is a single test on purpose: SDL allows only one `EventPump`
//! alive per process, and cargo runs tests of one binary concurrently.

use giochi_engine::{
    codes, quit_signal, GameLoop, Platform, QuitSignal, VideoDriverChoice, WindowConfig,
    FIXED_TIMESTEP,
};

#[test]
fn platform_smoke_without_a_display() {
    std::env::set_var("SDL_VIDEODRIVER", "dummy");

    let platform = Platform::new().expect("the dummy video driver must initialise");

    assert_eq!(
        platform.video_driver_choice(),
        VideoDriverChoice::Explicit,
        "an explicit SDL_VIDEODRIVER must never be replaced"
    );
    assert_eq!(platform.video_driver_name(), "dummy");

    // A zero-sized window is rejected before SDL is asked to create anything.
    let error = platform
        .create_window(WindowConfig::new(0, 200), "giochi-test")
        .expect_err("a zero width must be rejected");
    assert_eq!(error.code(), codes::INVALID_ARGUMENT, "unexpected: {error}");

    let mut window = platform
        .create_window(WindowConfig::new(320, 200), "giochi-test")
        .expect("window creation must succeed with the dummy driver");

    assert_eq!(window.size(), (320, 200));
    assert_eq!(window.drawable_size(), (320, 200));
    assert!(!window.is_fullscreen());
    window
        .set_title("giochi-test-2")
        .expect("title must be set");

    // No events were injected, so nothing asks to quit.
    for event in platform.poll_events() {
        assert_eq!(quit_signal(&event), QuitSignal::None);
    }

    let mut game_loop = GameLoop::new();
    assert!(game_loop.is_running());
    assert_eq!(game_loop.advance(FIXED_TIMESTEP), 1);
    assert!(platform.ticks_ms() < u64::MAX);

    // The real Escape path: an SDL key event is pumped, translated into a quit
    // signal and fed to the loop, which must stop.
    platform
        .push_event(sdl2::event::Event::KeyDown {
            keycode: Some(sdl2::keyboard::Keycode::Escape),
            scancode: None,
            keymod: sdl2::keyboard::Mod::NOMOD,
            repeat: false,
            timestamp: 0,
            window_id: 0,
        })
        .expect("SDL must accept the synthetic key event");

    for event in platform.poll_events() {
        game_loop.handle_quit_signal(quit_signal(&event));
    }

    assert!(
        !game_loop.is_running(),
        "Escape must stop the fixed-timestep loop"
    );
    assert_eq!(game_loop.advance(FIXED_TIMESTEP), 0);
}
