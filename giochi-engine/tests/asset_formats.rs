//! Guards the BMP twins shipped next to the PNG assets.
//!
//! The PNG decoder in SDL_image is optional at build time (ADR-034): the GIOCHI
//! console image links a `libSDL2_image` whose PNG and JPEG entry points are
//! "not supported" stubs. The repository therefore keeps `player.bmp` and
//! `checker.bmp` beside the PNGs, and the demo and the Lua example fall back to
//! them.
//!
//! A stale or malformed twin would surface only on the console — the one place
//! where it is expensive to investigate. This test decodes both variants through
//! SDL itself and compares the resulting sizes, so a mismatch fails CI instead.

use giochi_engine::{Engine, EngineConfig};

fn asset(name: &str) -> String {
    format!("{}/../assets/{name}", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn every_png_asset_has_a_bmp_twin_of_identical_size() {
    // Headless: this test decodes files, it does not present frames.
    std::env::set_var("SDL_VIDEODRIVER", "dummy");
    let mut engine = Engine::new(EngineConfig::new(320, 200, "asset formats")).expect("engine");

    for (stem, expected) in [("player", (32, 32)), ("checker", (64, 64))] {
        let png = engine
            .render_mut()
            .create_texture(&asset(&format!("{stem}.png")))
            .unwrap_or_else(|error| panic!("{stem}.png must decode: {error}"));
        let bmp = engine
            .render_mut()
            .create_texture(&asset(&format!("{stem}.bmp")))
            .unwrap_or_else(|error| panic!("{stem}.bmp must decode: {error}"));

        assert_eq!(
            engine.render().texture_size(png),
            Some(expected),
            "{stem}.png changed size; regenerate {stem}.bmp with scripts/png-to-bmp.py"
        );
        assert_eq!(
            engine.render().texture_size(bmp),
            Some(expected),
            "{stem}.bmp is stale; regenerate it with scripts/png-to-bmp.py"
        );

        assert!(engine.render_mut().destroy_texture(png));
        assert!(engine.render_mut().destroy_texture(bmp));
    }
}
