//! Checks that animation frames use the world-space sprite path and source rectangles.

use giochi_engine::animation::{AnimationFrame, PlaybackMode, SpriteAnimation};
use giochi_engine::{Camera, Color, Platform, Rect, RenderContext};

#[test]
fn animation_draws_the_selected_source_region_through_the_camera() {
    std::env::set_var("SDL_VIDEODRIVER", "dummy");
    std::env::set_var("SDL_RENDER_DRIVER", "software");

    let platform = Platform::new().expect("dummy SDL");
    let window = platform
        .create_window(giochi_engine::WindowConfig::new(16, 16), "animation render")
        .expect("window");
    let mut render = RenderContext::new(window).expect("renderer");
    render.set_camera(Camera::with_position(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        0.0,
        0.0,
    ));

    let mut pixels = vec![0; 8 * 4 * 4];
    for y in 0..4 {
        for x in 0..8 {
            let offset = (y * 8 + x) * 4;
            pixels[offset..offset + 4].copy_from_slice(if x < 4 {
                &[255, 0, 0, 255]
            } else {
                &[0, 0, 255, 255]
            });
        }
    }
    let texture = render
        .create_texture_from_rgba(8, 4, &pixels)
        .expect("sprite sheet");
    let frames = vec![
        AnimationFrame::new(Rect::new(0.0, 0.0, 4.0, 4.0), 0.1).expect("red frame"),
        AnimationFrame::new(Rect::new(4.0, 0.0, 4.0, 4.0), 0.1).expect("blue frame"),
    ];
    let mut animation =
        SpriteAnimation::new(texture, frames, PlaybackMode::Loop).expect("animation");
    let destination = Rect::new(-4.0, -4.0, 8.0, 8.0);

    render.clear(Color::BLACK);
    animation
        .draw(&mut render, destination)
        .expect("draw red frame");
    let red = render
        .read_pixels(Rect::new(8.0, 8.0, 1.0, 1.0))
        .expect("red pixel");
    assert_eq!(&red[..4], &[255, 0, 0, 255]);

    animation.update(0.1).expect("advance frame");
    render.clear(Color::BLACK);
    animation
        .draw(&mut render, destination)
        .expect("draw blue frame");
    let blue = render
        .read_pixels(Rect::new(8.0, 8.0, 1.0, 1.0))
        .expect("blue pixel");
    assert_eq!(&blue[..4], &[0, 0, 255, 255]);
}
