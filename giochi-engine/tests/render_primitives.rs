//! Pixel tests for the primitives the launcher needed.
//!
//! These four did not exist until the launcher was ported: `stroke_rect`,
//! `fill_rects`, `draw_line`, `draw_texture_ex` — and the canvas had no blend
//! mode, so every colour with alpha below 255 was drawn solid.
//!
//! One test function, like [`render_smoke`](render_smoke.rs): SDL allows one
//! `EventPump` per process and cargo runs the tests of one binary concurrently.
//! Every assertion reads a pixel, because "no error was returned" is also true
//! for a line drawn in the background colour.

use giochi_engine::{Color, Platform, Rect, RenderContext, Vec2, WindowConfig};

const PRETO: Color = Color::rgb(0, 0, 0);
const VERDE: Color = Color::rgb(0, 255, 0);
const AZUL: Color = Color::rgb(0, 0, 255);
const AMARELO: Color = Color::rgb(255, 255, 0);
const BRANCO: Color = Color::rgb(255, 255, 255);

fn pixel(render: &mut RenderContext, x: f32, y: f32) -> (u8, u8, u8) {
    let bytes = render
        .read_pixels(Rect::new(x, y, 1.0, 1.0))
        .expect("the pixel must be readable");
    (bytes[0], bytes[1], bytes[2])
}

#[test]
fn primitives_and_canvas_blend_write_expected_pixels() {
    std::env::set_var("SDL_VIDEODRIVER", "dummy");
    std::env::set_var("SDL_RENDER_DRIVER", "software");

    let platform = Platform::new().expect("dummy driver must initialise");
    let window = platform
        .create_window(WindowConfig::new(48, 48), "primitivas")
        .expect("window must be created");
    let mut render = RenderContext::new(window).expect("renderer must be created");

    // O backend tem de se identificar, e o nome não é inventado: `SDL_RENDER_DRIVER`
    // inválido cai em silêncio para outro driver, e num aparelho embarcado isso
    // separa "acelerado" de "software". Fica neste teste porque o SDL só aceita
    // uma inicialização por processo — dois testes no mesmo binário rodam em
    // threads diferentes e o segundo falha com "more than one thread".
    assert_eq!(render.renderer_name(), "software");
    assert!(
        !render.renderer_is_accelerated(),
        "software não é acelerado"
    );

    render.clear(PRETO);

    // Linha: `draw_line` usa Bresenham do SDL, então a linha horizontal pinta
    // todos os pixels do caminho.
    render.draw_line(Vec2::new(4.0, 4.0), Vec2::new(43.0, 4.0), VERDE);
    assert_eq!(
        pixel(&mut render, 20.0, 4.0),
        (0, 255, 0),
        "a linha deve estar em y=4"
    );

    // Contorno: pinta a borda e **não** o interior.
    render.stroke_rect(Rect::new(4.0, 20.0, 20.0, 20.0), AZUL);
    assert_eq!(
        pixel(&mut render, 4.0, 20.0),
        (0, 0, 255),
        "o contorno deve pintar o canto"
    );
    assert_eq!(
        pixel(&mut render, 12.0, 30.0),
        (0, 0, 0),
        "o contorno não pode preencher"
    );

    // Lote: dois retângulos numa chamada.
    render.fill_rects(
        &[
            Rect::new(30.0, 30.0, 4.0, 4.0),
            Rect::new(38.0, 30.0, 4.0, 4.0),
        ],
        AMARELO,
    );
    assert_eq!(
        pixel(&mut render, 31.0, 31.0),
        (255, 255, 0),
        "primeiro do lote"
    );
    assert_eq!(
        pixel(&mut render, 39.0, 31.0),
        (255, 255, 0),
        "segundo do lote"
    );

    // Alfa no alvo: branco coberto por vermelho a 50 % tem de dar a média.
    //
    // Sem `set_blend_mode` no canvas, o resultado seria (255,0,0) — vermelho
    // cru. É a diferença entre "a cor foi passada" e "a cor foi composta".
    render.clear(BRANCO);
    render.draw_rect(Rect::new(0.0, 0.0, 48.0, 48.0), Color::new(255, 0, 0, 128));
    let (r, g, b) = pixel(&mut render, 24.0, 24.0);
    assert!(
        (i32::from(r) - 255).abs() <= 2
            && (i32::from(g) - 128).abs() <= 3
            && (i32::from(b) - 128).abs() <= 3,
        "vermelho a 50% sobre branco deve dar ~(255,128,128), veio ({r},{g},{b})"
    );
}
