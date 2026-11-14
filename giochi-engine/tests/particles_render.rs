//! Verifica batching e tint do sistema de partículas contra o renderer SDL.
//!
//! Uma única janela dummy permite executar este teste sem ecrã físico.

use giochi_engine::particles::{ParticleConfig, ParticleSystem, PointEmitter};
use giochi_engine::{Color, Platform, Rect, RenderContext, Vec2, WindowConfig};

#[test]
fn one_thousand_tinted_particles_share_one_draw_call() {
    std::env::set_var("SDL_VIDEODRIVER", "dummy");
    std::env::set_var("SDL_RENDER_DRIVER", "software");

    let platform = Platform::new().expect("dummy SDL");
    let window = platform
        .create_window(WindowConfig::new(64, 64), "particles render")
        .expect("window");
    let mut render = RenderContext::new(window).expect("renderer");
    let pixels = [255u8; 8 * 8 * 4];
    let texture = render
        .create_texture_from_rgba(8, 8, &pixels)
        .expect("texture");
    let color = Color::rgb(220, 40, 50);
    let config = ParticleConfig {
        lifetime: 1.0,
        start_size: 10.0,
        end_size: 10.0,
        start_color: color,
        end_color: color,
        ..ParticleConfig::default()
    };
    let mut particles =
        ParticleSystem::with_capacity(texture, 1_000, config).expect("particle pool");
    let emitter = PointEmitter::new(Vec2::ZERO).expect("point emitter");

    assert_eq!(particles.emit(&emitter, 1_000).expect("emit"), 1_000);
    assert_eq!(particles.render(&mut render).expect("render"), 1_000);
    assert_eq!(render.sprite_batch().len(), 1_000);
    assert_eq!(
        render.sprite_batch().draw_calls(),
        1,
        "the shared texture must remain one consecutive batch"
    );

    let pixel = render
        .read_pixels(Rect::new(32.0, 32.0, 1.0, 1.0))
        .expect("pixel read");
    assert_eq!(&pixel[..4], &[color.r, color.g, color.b, color.a]);
}
