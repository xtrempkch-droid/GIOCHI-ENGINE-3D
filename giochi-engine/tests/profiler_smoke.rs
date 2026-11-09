//! Headless integration test for the phase-15 profiler and overlay.
//!
//! The test binary only exists when the `profiling` feature is enabled, because the
//! whole point of the default build is that the profiler is a no-op. Run it with:
//!
//! ```text
//! SDL_VIDEODRIVER=dummy cargo test -p giochi-engine --features profiling --test profiler_smoke
//! ```
//!
//! One test per binary: SDL allows a single `EventPump` per process.

#![cfg(feature = "profiling")]

use giochi_engine::{
    AudioSystem, Engine, EngineConfig, Input, Rect, RenderContext, Scene, Sprite, TextureHandle,
    Time,
};

struct Sprites {
    texture: TextureHandle,
    count: u32,
}

impl Scene for Sprites {
    fn update(&mut self, _time: Time, _input: &Input, _audio: &mut AudioSystem) {}

    fn render(&mut self, renderer: &mut RenderContext) {
        for index in 0..self.count {
            let sprite = Sprite::new(self.texture, index as f32 * 8.0, 12.0, 16.0, 16.0);
            renderer.draw_sprite(&sprite).expect("sprite draw");
        }
    }
}

#[test]
fn the_engine_reports_the_frame_it_just_drew() {
    std::env::set_var("SDL_VIDEODRIVER", "dummy");

    let mut config = EngineConfig::new(320, 200, "profiler smoke");
    config.vsync = 0;
    let mut engine = Engine::new(config).expect("engine must start with the dummy driver");

    let pixels = [255u8; 8 * 8 * 4];
    let texture = engine
        .render_mut()
        .create_texture_from_rgba(8, 8, &pixels)
        .expect("texture");
    engine.push_scene(Box::new(Sprites { texture, count: 4 }));

    assert!(
        engine.is_overlay_visible(),
        "profiling builds start visible"
    );

    for _ in 0..5 {
        engine.step(1.0 / 60.0).expect("step");
    }

    let stats = engine.frame_stats();
    assert_eq!(stats.frames, 1);
    assert_eq!(stats.sprites_drawn, 4);
    assert_eq!(
        stats.draw_calls, 1,
        "four sprites of one texture collapse into one backend call; the overlay's \
         own draws happen after the snapshot"
    );
    assert!((stats.frame_time_ms() - 16.666).abs() < 0.01);
    assert!((stats.fps - 60.0).abs() < 0.01, "fps: {}", stats.fps);
    assert!(stats.update_ns > 0, "the update scope must be measured");

    let summary = engine.frame_time_summary();
    assert_eq!(summary.samples, 5);
    assert!(summary.p99_ns >= summary.p50_ns);
    assert!(summary.avg_ns > 0);

    // The panel keeps one persistent texture and updates it in place. After the first
    // refresh the live count is the sprite plus the panel, and it must stay there.
    engine.set_overlay_visible(true);
    engine.step(1.0 / 60.0).expect("step with overlay");
    assert_eq!(
        engine.render().texture_count(),
        2,
        "the sprite and the persistent overlay texture"
    );

    // The panel is throttled to 4 Hz, so wait past the interval and draw once more.
    // Reading a pixel separates "the panel was drawn" from "the draw call was issued".
    std::thread::sleep(std::time::Duration::from_millis(1_050));
    engine.step(1.0 / 60.0).expect("step with overlay");
    assert_eq!(
        engine.render().texture_count(),
        2,
        "refreshing the panel must reuse its texture, not allocate another"
    );
    let pixel = engine
        .render_mut()
        .read_pixels(Rect::new(9.0, 9.0, 1.0, 1.0))
        .expect("read back the overlay");
    assert_eq!(pixel.len(), 4);
    assert_ne!(
        &pixel[..3],
        &[12, 14, 22],
        "the panel must paint over the clear colour"
    );

    engine.toggle_overlay();
    assert!(!engine.is_overlay_visible());
    engine.set_overlay_visible(true);
    assert!(engine.is_overlay_visible());

    drop(engine);
}
