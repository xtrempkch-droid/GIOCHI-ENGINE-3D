//! Headless integration test for the 2D renderer and the texture registry.
//!
//! Runs against SDL's `dummy` video driver, so it works in CI containers without a
//! display. It is a single test on purpose: SDL allows one `EventPump` per process
//! and cargo runs the tests of one binary concurrently.

use giochi_engine::{
    codes, Camera, Color, Platform, Rect, RenderContext, Sprite, Vec2, WindowConfig,
};

/// Colour written into the cleared target.
const BACKGROUND: Color = Color::rgb(10, 20, 30);
/// Colour of the rectangle drawn on top of it.
const MARKER: Color = Color::rgb(200, 40, 60);

fn asset(name: &str) -> String {
    format!("{}/../assets/{name}", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn render_smoke_without_a_display() {
    std::env::set_var("SDL_VIDEODRIVER", "dummy");

    let platform = Platform::new().expect("dummy driver must initialise");
    let window = platform
        .create_window(WindowConfig::new(64, 64), "giochi-render-test")
        .expect("window must be created");
    let mut render = RenderContext::new(window).expect("renderer must be created");

    assert_eq!(render.size(), (64, 64));
    assert_eq!(render.texture_count(), 0);

    // --- textures: create, inspect, destroy -------------------------------
    let checker = render
        .create_texture(&asset("checker.png"))
        .expect("checker.png must load");
    let player = render
        .create_texture(&asset("player.png"))
        .expect("player.png must load");

    assert_eq!(render.texture_size(checker), Some((64, 64)));
    assert_eq!(render.texture_size(player), Some((32, 32)));
    assert_eq!(render.texture_count(), 2);
    assert!(render.textures().contains(checker));

    // Loading a missing file is an asset error, not a panic.
    let missing = render.create_texture(&asset("does-not-exist.png"));
    assert_eq!(
        missing.expect_err("missing file must fail").code(),
        codes::ASSET
    );

    // --- drawing ---------------------------------------------------------
    render.clear(BACKGROUND);
    render.draw_rect(Rect::new(16.0, 16.0, 32.0, 32.0), MARKER);
    render
        .draw_texture(checker, Rect::new(0.0, 0.0, 8.0, 8.0), Rect::ZERO)
        .expect("draw_texture must succeed");

    // Invalid handles are reported, not silently ignored.
    let bad_draw = render.draw_texture(0, Rect::new(0.0, 0.0, 1.0, 1.0), Rect::ZERO);
    assert_eq!(
        bad_draw.expect_err("handle 0 is invalid").code(),
        codes::INVALID_HANDLE
    );

    // The drawn rect is really there: sample inside and outside it.
    let inside = render
        .read_pixels(Rect::new(20.0, 20.0, 1.0, 1.0))
        .expect("read_pixels must work");
    assert_eq!(
        &inside[..4],
        &[MARKER.r, MARKER.g, MARKER.b, 255],
        "the marker rect must be drawn at (20, 20)"
    );

    let outside = render
        .read_pixels(Rect::new(48.0, 48.0, 1.0, 1.0))
        .expect("read_pixels must work");
    assert_eq!(
        &outside[..4],
        &[BACKGROUND.r, BACKGROUND.g, BACKGROUND.b, 255],
        "outside the marker the background must remain"
    );

    // --- camera ----------------------------------------------------------
    let mut camera = Camera::new(Rect::new(0.0, 0.0, 64.0, 64.0)).zoomed(2.0);
    camera.x = 100.0;
    camera.y = 100.0;
    render.set_camera(camera);

    let centre = render.world_to_screen(Vec2::new(100.0, 100.0));
    assert_eq!(centre, Vec2::new(32.0, 32.0));

    let sprite = Sprite::new(checker, 92.0, 92.0, 16.0, 16.0);
    render
        .draw_sprite(&sprite)
        .expect("draw_sprite must succeed");

    // --- teardown --------------------------------------------------------
    render.present();

    assert!(render.destroy_texture(checker));
    assert!(render.destroy_texture(player));
    assert!(
        !render.destroy_texture(checker),
        "double destroy is reported"
    );
    assert!(!render.destroy_texture(0), "handle 0 is never valid");
    assert_eq!(render.texture_count(), 0, "no texture may leak");
}
