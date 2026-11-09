//! Integration test for the phase-16 sprite batching and texture atlas.
//!
//! Runs against SDL's `dummy` driver with the software renderer, so it works in a
//! container without a display. One test on purpose: SDL allows one `EventPump` per
//! process and cargo runs the tests of one binary concurrently.

use giochi_engine::{
    Color, Platform, Rect, RenderContext, SpriteBatchConfig, TextureAtlas, WindowConfig,
};

fn asset(name: &str) -> String {
    format!("{}/../assets/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// A solid RGBA texture, so a pixel read tells which texture is on top.
fn solid(render: &RenderContext, color: [u8; 4]) -> u64 {
    let pixels: Vec<u8> = color.iter().copied().cycle().take(8 * 8 * 4).collect();
    render
        .create_texture_from_rgba(8, 8, &pixels)
        .expect("texture")
}

fn read(render: &mut RenderContext, x: f32, y: f32) -> [u8; 4] {
    let bytes = render
        .read_pixels(Rect::new(x, y, 1.0, 1.0))
        .expect("readable");
    [bytes[0], bytes[1], bytes[2], bytes[3]]
}

#[test]
fn batching_reduces_draw_calls_without_changing_pixels() {
    std::env::set_var("SDL_VIDEODRIVER", "dummy");
    std::env::set_var("SDL_RENDER_DRIVER", "software");

    let platform = Platform::new().expect("dummy driver");
    let window = platform
        .create_window(WindowConfig::new(64, 64), "giochi batching")
        .expect("window");
    let mut render = RenderContext::new(window).expect("renderer");

    let red = solid(&render, [220, 40, 50, 255]);
    let blue = solid(&render, [40, 80, 220, 255]);

    // --- 1000 sprites of one texture collapse to one draw call -----------
    render.clear(Color::BLACK);
    for index in 0..1000 {
        let x = (index % 40) as f32 * 1.5;
        let y = (index / 40) as f32 * 1.5;
        render
            .draw_texture(red, Rect::new(x, y, 8.0, 8.0), Rect::ZERO)
            .expect("draw");
    }
    assert_eq!(
        render.sprite_batch().len(),
        1000,
        "every submitted sprite must be queued"
    );
    assert_eq!(
        render.sprite_batch().draw_calls(),
        1,
        "one texture, one run, one backend call"
    );

    // `read_pixels` flushes; the batch must be empty afterwards and the queue drawing
    // must have produced the same pixels the immediate path would have.
    let batched = read(&mut render, 4.0, 4.0);
    assert_eq!(batched, [220, 40, 50, 255]);
    assert!(render.sprite_batch().is_empty(), "the read must flush");

    render.clear(Color::BLACK);
    for index in 0..1000 {
        let x = (index % 40) as f32 * 1.5;
        let y = (index / 40) as f32 * 1.5;
        render
            .draw_texture_immediate(red, Rect::new(x, y, 8.0, 8.0), Rect::ZERO)
            .expect("draw");
    }
    assert_eq!(
        read(&mut render, 4.0, 4.0),
        batched,
        "batching must not change the pixels"
    );

    // --- submission order is preserved across textures --------------------
    render.clear(Color::BLACK);
    render
        .draw_texture(red, Rect::new(0.0, 0.0, 32.0, 32.0), Rect::ZERO)
        .expect("red");
    render
        .draw_texture(blue, Rect::new(16.0, 16.0, 32.0, 32.0), Rect::ZERO)
        .expect("blue");
    assert_eq!(
        render.sprite_batch().draw_calls(),
        2,
        "a texture change starts a new run"
    );
    assert_eq!(
        read(&mut render, 24.0, 24.0),
        [40, 80, 220, 255],
        "the later draw must stay on top"
    );

    // A, B, A is three runs: the batch never reorders across a texture change.
    render.clear(Color::BLACK);
    for (texture, x) in [(red, 0.0), (blue, 8.0), (red, 16.0)] {
        render
            .draw_texture(texture, Rect::new(x, 0.0, 16.0, 16.0), Rect::ZERO)
            .expect("draw");
    }
    assert_eq!(render.sprite_batch().draw_calls(), 3);

    // --- culling: a sprite entirely outside the window is not queued ------
    render.clear(Color::BLACK);
    render
        .draw_texture(red, Rect::new(-64.0, -64.0, 8.0, 8.0), Rect::ZERO)
        .expect("off-screen draw is not an error");
    render
        .draw_texture(red, Rect::new(200.0, 0.0, 8.0, 8.0), Rect::ZERO)
        .expect("off-screen draw is not an error");
    assert!(
        render.sprite_batch().is_empty(),
        "off-screen sprites are skipped"
    );

    // --- a full batch flushes itself --------------------------------------
    render.clear(Color::BLACK);
    render.set_batch_config(SpriteBatchConfig::new(2, 3));
    for index in 0..10 {
        let x = index as f32 * 4.0;
        render
            .draw_texture(red, Rect::new(x, 0.0, 4.0, 4.0), Rect::ZERO)
            .expect("draw");
    }
    assert!(
        render.sprite_batch().len() <= 3,
        "the batch must flush at its limit, queued {}",
        render.sprite_batch().len()
    );
    render.set_batch_config(SpriteBatchConfig::default());

    // --- sprite + camera still goes through the batch ---------------------
    render.clear(Color::BLACK);
    render
        .draw_sprite(&giochi_engine::Sprite::new(red, 0.0, 0.0, 16.0, 16.0))
        .expect("sprite");
    assert_eq!(render.sprite_batch().draw_calls(), 1);
    // The default camera centres the world origin, so the sprite sits mid-window.
    assert_eq!(read(&mut render, 32.0, 32.0), [220, 40, 50, 255]);

    // --- atlas: two images, one texture, still one draw call --------------
    let player_path = asset("player.png");
    let checker_path = asset("checker.png");
    let atlas = render
        .load_atlas(&[player_path.as_str(), checker_path.as_str()])
        .expect("atlas");
    assert_eq!(atlas.len(), 2);
    assert!(atlas.contains("player"));
    assert!(atlas.contains("checker"));
    assert!(
        !atlas.region("player").is_empty(),
        "the player region must have pixels"
    );
    let (width, height) = atlas.size();
    assert!(width.is_power_of_two() && height.is_power_of_two());
    assert_eq!(render.texture_size(atlas.texture()), Some((width, height)));

    let player = atlas.region("player");
    let checker = atlas.region("checker");
    assert_eq!(
        player.pixel_rect(),
        Rect::new(player.x as f32, player.y as f32, 32.0, 32.0)
    );
    assert_eq!(
        checker.pixel_rect(),
        Rect::new(checker.x as f32, checker.y as f32, 64.0, 64.0)
    );

    render.clear(Color::BLACK);
    // Reset the cumulative counters here so the assertion below is about this loop, not
    // about every sprite the test has drawn since the renderer was created.
    render.begin_frame_stats();
    for index in 0..100 {
        let x = (index % 10) as f32 * 6.0;
        let y = (index / 10) as f32 * 6.0;
        let region = if index % 2 == 0 { player } else { checker };
        render
            .draw_texture(
                atlas.texture(),
                Rect::new(x, y, 6.0, 6.0),
                region.pixel_rect(),
            )
            .expect("atlas draw");
    }
    assert_eq!(
        render.sprite_batch().draw_calls(),
        1,
        "every atlas region shares one texture, so the whole scene is one call"
    );
    assert_eq!(
        render.frame_counters().sprites_drawn,
        if cfg!(feature = "profiling") { 100 } else { 0 },
        "the counter exists exactly when the profiling feature does"
    );

    // The unknown region is empty and draws nothing; the known one draws pixels.
    assert!(atlas.region("nope").is_empty());
    render.clear(Color::BLACK);
    render
        .draw_texture(
            atlas.texture(),
            Rect::new(0.0, 0.0, 32.0, 32.0),
            player.pixel_rect(),
        )
        .expect("draw");
    let pixel = read(&mut render, 8.0, 8.0);
    assert_ne!(
        pixel[..3],
        [0, 0, 0],
        "the player region must paint something"
    );

    // --- in-memory atlas: generated content needs no file -----------------
    let generated = [255u8, 0, 0, 255].repeat(4 * 4);
    let generated_atlas = TextureAtlas::from_rgba_images(&render, &[("red", 4, 4, &generated)])
        .expect("in-memory atlas");
    assert_eq!(generated_atlas.len(), 1);
    let region = generated_atlas.region("red");
    assert_eq!(
        region.pixel_rect(),
        Rect::new(region.x as f32, region.y as f32, 4.0, 4.0)
    );
    render.clear(Color::BLACK);
    render
        .draw_texture(
            generated_atlas.texture(),
            Rect::new(40.0, 40.0, 24.0, 24.0),
            region.pixel_rect(),
        )
        .expect("draw");
    assert_eq!(render.sprite_batch().draw_calls(), 1);
    assert_eq!(read(&mut render, 48.0, 48.0), [255, 0, 0, 255]);

    // --- teardown ---------------------------------------------------------
    render.present();
    assert!(render.sprite_batch().is_empty());
    assert!(render.destroy_texture(generated_atlas.texture()));
    assert!(render.destroy_texture(atlas.texture()));
    assert!(render.destroy_texture(red));
    assert!(render.destroy_texture(blue));
    assert_eq!(render.texture_count(), 0, "no texture may leak");
}
