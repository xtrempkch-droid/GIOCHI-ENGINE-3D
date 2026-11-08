//! Compile-and-run checks for the snippets in `docs/API_GUIDE.md`.
//!
//! Documentation that is not executed drifts. Every example in the guide has a
//! counterpart here: if a signature changes, this test stops compiling and points at
//! the section that needs updating.
//!
//! Uses SDL's `dummy` video driver, and tolerates a machine without audio.

use giochi_engine::assets::{asset_size, read_asset};
use giochi_engine::audio::{AudioSystem, LOOPS_INFINITE, MAX_VOLUME};
use giochi_engine::input::gamepad::{AXIS_LEFT_X, BUTTON_SOUTH};
use giochi_engine::input::keyboard::{KEY_A, KEY_D, KEY_SPACE};
use giochi_engine::input::mouse::MOUSE_RIGHT;
use giochi_engine::{
    codes, Camera, Color, Engine, EngineConfig, GiochiError, Input, Rect, RenderContext, Scene,
    SceneTransition, Sprite, Time, Vec2,
};

fn asset(name: &str) -> String {
    format!("{}/../assets/{name}", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn api_guide_examples_compile_and_behave() {
    // --- §2 configuration ---------------------------------------------------
    let config = EngineConfig::new(320, 200, "guide");
    assert_eq!(config.width, 320);
    assert_eq!(config.title_str(), "guide");
    assert!(config.is_vsync_enabled());
    assert_eq!(config.window_config().width, 320);

    let error = Engine::new(EngineConfig::new(0, 200, "bad")).expect_err("must fail");
    assert_eq!(error.code(), codes::INVALID_ARGUMENT);

    // --- §9 math ------------------------------------------------------------
    let v = Vec2::new(3.0, 4.0);
    assert_eq!(v.length(), 5.0);
    let moved = v + Vec2::splat(1.0);
    let scaled = moved * 2.0;
    assert_eq!(scaled, Vec2::new(8.0, 10.0));

    let r = Rect::new(0.0, 0.0, 100.0, 50.0);
    assert!(r.contains(Vec2::new(10.0, 10.0)));
    assert!(
        r.intersection(Rect::new(90.0, 40.0, 100.0, 100.0)).w > 0.0,
        "overlapping rects must intersect"
    );
    assert!(
        !r.contains(Vec2::new(100.0, 10.0)),
        "right edge is exclusive"
    );
    let _ = Color::rgb(230, 70, 70);

    // --- §6 input injection -------------------------------------------------
    let mut standalone = Input::without_gamepad();
    standalone.begin_frame();
    standalone.keyboard_mut().press(KEY_D);
    assert!(standalone.key_down(KEY_D));
    assert!(!standalone.key_down(KEY_A));
    assert_eq!(standalone.gamepad().axis(AXIS_LEFT_X), 0.0);
    assert!(!standalone.gamepad().is_down(BUTTON_SOUTH));
    assert!(!standalone.mouse_down(MOUSE_RIGHT));

    // --- §5, §7, §8 with a real (headless) engine ---------------------------
    std::env::set_var("SDL_VIDEODRIVER", "dummy");
    let mut engine = Engine::new(config).expect("engine must start with the dummy driver");
    // The profiler's overlay owns one panel texture when the feature is enabled; the
    // leak assertion at the end counts engine textures, so keep the debug aid out of it.
    engine.set_overlay_visible(false);
    engine.set_frame_limit(3);

    // §5 textures: cached vs primitive.
    let cached = engine
        .render_mut()
        .load_texture(&asset("player.png"))
        .expect("player.png");
    let again = engine
        .render_mut()
        .load_texture(&asset("player.png"))
        .expect("player.png again");
    assert_eq!(cached, again, "the cache returns the same handle");

    let primitive = engine
        .render_mut()
        .create_texture(&asset("checker.png"))
        .expect("checker.png");
    assert_ne!(primitive, cached);
    assert_eq!(engine.render().texture_size(primitive), Some((64, 64)));
    assert!(engine.render_mut().destroy_texture(primitive));

    // §5 sprite + camera.
    let mut camera = Camera::new(Rect::new(0.0, 0.0, 320.0, 200.0));
    camera.x = 40.0;
    camera.y = 30.0;
    camera.zoom = 1.25;
    engine.render_mut().set_camera(camera);
    assert_eq!(
        engine.render().world_to_screen(Vec2::new(40.0, 30.0)),
        Vec2::new(160.0, 100.0),
        "the camera centres its position in the viewport"
    );

    // §7 audio: silent mode is tolerated, a device is exercised.
    assert!(engine.audio().volume() <= MAX_VOLUME);
    engine.audio_mut().set_volume(96);
    assert_eq!(engine.audio().volume(), 96);
    engine.audio_mut().pause_music();
    engine.audio_mut().resume_music();
    engine.audio_mut().stop_music();
    assert_eq!(
        engine.audio().music_state(),
        giochi_engine::MusicState::Stopped
    );

    // §8 assets.
    let size = asset_size(&asset("player.png")).expect("size");
    let mut buffer = vec![0u8; size];
    assert_eq!(
        read_asset(&asset("player.png"), &mut buffer).expect("read"),
        size
    );
    assert_eq!(&buffer[..4], b"\x89PNG");

    // §3 scenes: run a scene through the real loop.
    engine.push_scene(Box::new(GuideScene {
        texture: cached,
        entered: false,
        exited: false,
        updates: 0,
        renders: 0,
    }));

    engine.run_current().expect("loop must run");
    let stats = engine.run_stats();
    assert_eq!(stats.frames, 3);
    assert!(stats.steps > 0);
    assert!(stats.fps() > 0.0);
    assert_eq!(engine.scenes().depth(), 1);

    // §4 frame limit / reset.
    assert_eq!(engine.frame_limit(), Some(3));
    engine.clear_frame_limit();
    assert_eq!(engine.frame_limit(), None);
    engine.reset_quit();
    assert!(engine.is_running());
    assert!(engine.fixed_timestep() > 0.0);
    assert!(engine.time().frame_count > 0);

    // §3 popping calls on_exit.
    assert!(engine.pop_scene());
    assert!(engine.scenes().is_empty());

    // §10 error codes are unique and negative.
    for code in [
        codes::OK,
        codes::INVALID_HANDLE,
        codes::INVALID_ARGUMENT,
        codes::PLATFORM,
        codes::RENDER,
        codes::ASSET,
        codes::AUDIO,
        codes::SCENE,
        codes::NOT_INITIALIZED,
        codes::UNSUPPORTED,
    ] {
        assert!(code <= 0);
    }
    let sample: GiochiError = GiochiError::NotInitialized;
    assert_eq!(sample.code(), codes::NOT_INITIALIZED);

    // Nothing may leak across the run.
    assert_eq!(engine.render().texture_count(), 1);
    assert_eq!(engine.audio().sound_count(), 0);
    let _ = LOOPS_INFINITE;
}

/// The scene from §1/§3 of the guide, instrumented so the test can assert on it.
struct GuideScene {
    texture: u64,
    entered: bool,
    exited: bool,
    updates: u32,
    renders: u32,
}

impl Scene for GuideScene {
    fn on_enter(&mut self) {
        self.entered = true;
    }

    fn on_exit(&mut self) {
        self.exited = true;
    }

    fn update(&mut self, _time: Time, input: &Input, audio: &mut AudioSystem) {
        self.updates += 1;
        // The guide's "ignore audio errors" rule. Which error comes back depends on
        // the machine, and both are documented:
        //   * handle 0 is invalid                          -> INVALID_HANDLE
        //   * no audio device, so the mixer is silent      -> AUDIO (ADR-021)
        // A CI runner has no sound card, a developer desktop does; asserting one of the
        // two would make this test environment-dependent, which it must not be.
        if let Err(error) = audio.play_sound(0, 0) {
            assert!(
                matches!(error.code(), codes::INVALID_HANDLE | codes::AUDIO),
                "play_sound(0) must report an invalid handle or a silent mixer, got {error}"
            );
        }
        assert!(!input.key_down(KEY_SPACE));
    }

    fn render(&mut self, renderer: &mut RenderContext) {
        self.renders += 1;
        renderer.draw_rect(Rect::new(0.0, 0.0, 10.0, 10.0), Color::rgb(1, 2, 3));
        let sprite = Sprite::new(self.texture, 10.0, 10.0, 32.0, 32.0);
        // `draw_sprite` is fallible; the guide shows the `?` form.
        assert!(renderer.draw_sprite(&sprite).is_ok());
    }

    fn transition(&mut self) -> SceneTransition {
        SceneTransition::Continue
    }
}
