//! Pixel-level acceptance for the viewport: the grid, the axes and a node's box really
//! reach the framebuffer, and nothing is drawn outside the rectangle.
//!
//! `SDL_VIDEODRIVER=dummy` plus `SDL_RENDER_DRIVER=software` gives a real 2D renderer
//! with no display, so the frame can be read back. That is the same trade the engine's
//! `ui_render` and `devtools_render` tests make: asserting "it did not panic" would pass
//! with an empty viewport, which is exactly the failure this phase can have.
//!
//! One test per binary: SDL's `Sdl` is a singleton and `cargo test` runs each `#[test]`
//! on its own thread, so the checks below are functions called from one `#[test]`.

use std::path::PathBuf;

use egui::Rect;
use giochi_editor::editor::{viewport, EditorTheme, OrbitCamera};
use giochi_engine::ui::EguiBackend;
use giochi_engine::{Engine, EngineConfig};
use giochi_engine_3d::scene::Scene;

fn engine(width: u32, height: u32) -> Engine {
    std::env::set_var("SDL_VIDEODRIVER", "dummy");
    std::env::set_var("SDL_RENDER_DRIVER", "software");
    std::env::set_var("SDL_AUDIODRIVER", "dummy");
    Engine::new(EngineConfig::new(width, height, "viewport test")).expect("engine")
}

fn scene() -> Scene {
    // The committed default, read through the real loader: the same file `--scene` opens.
    Scene::load_from_path(default_scene_path()).expect("default scene")
}

fn default_scene_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join("scenes")
        .join("default.scene")
}

fn pixel_at(width: u32, pixels: &[u8], x: u32, y: u32) -> (u8, u8, u8) {
    let index = ((y * width + x) * 4) as usize;
    (pixels[index], pixels[index + 1], pixels[index + 2])
}

#[test]
fn the_viewport_paints_a_frame() {
    grid_axes_and_boxes();
    the_icons_load();
    the_selection_changes_pixels();
    an_empty_viewport_is_only_the_clear_colour();
    the_layout_and_the_viewport_together_paint_a_frame();
}

fn grid_axes_and_boxes() {
    let mut engine = engine(640, 480);
    let theme = EditorTheme::dark();
    engine.render_mut().clear(theme.viewport_color());

    // A rectangle smaller than the window, with margins on every side. Anything the
    // viewport draws must stay inside it; the margins are what tests that.
    let rect = Rect::from_min_size(egui::pos2(80.0, 60.0), egui::vec2(480.0, 360.0));
    let camera = OrbitCamera::default();
    let scene = scene();

    viewport::draw(
        engine.render_mut(),
        rect,
        Some(&scene),
        &camera,
        &[],
        &theme,
    );

    let (width, height, pixels) = engine.render().read_pixels_rgba().expect("read back");
    assert_eq!((width, height), (640, 480));
    assert_eq!(pixels.len(), (width * height * 4) as usize);

    let viewport_color = (theme.viewport.r(), theme.viewport.g(), theme.viewport.b());
    let grid = (theme.grid.r(), theme.grid.g(), theme.grid.b());

    // --- the grid is drawn inside the rectangle ---------------------------
    let mut grid_pixels = 0;
    for y in (rect.top() as u32 + 1)..(rect.bottom() as u32 - 1) {
        for x in (rect.left() as u32 + 1)..(rect.right() as u32 - 1) {
            let pixel = pixel_at(width, &pixels, x, y);
            if pixel == grid {
                grid_pixels += 1;
            }
            assert_ne!(
                pixel,
                (0, 0, 0),
                "no pixel inside the viewport may stay the window's clear black \
                 (the viewport must clear to its own colour)"
            );
        }
    }
    if grid_pixels == 0 {
        let mut counts = std::collections::HashMap::new();
        for y in (rect.top() as u32)..(rect.bottom() as u32) {
            for x in (rect.left() as u32)..(rect.right() as u32) {
                *counts
                    .entry(pixel_at(width, &pixels, x, y))
                    .or_insert(0usize) += 1;
            }
        }
        let mut top: Vec<_> = counts.into_iter().collect();
        top.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
        eprintln!(
            "top colours inside the viewport: {:?}",
            &top[..top.len().min(6)]
        );
    }
    assert!(
        grid_pixels > 500,
        "expected the grid to cover the viewport, found {grid_pixels} grid-coloured pixels"
    );

    // --- the three axes are drawn in three colours ------------------------
    let axes = [
        (0xd0u8, 0x50u8, 0x50u8),
        (0x50, 0xc0, 0x60),
        (0x50, 0x80, 0xe0),
    ];
    for axis in axes {
        let found = (rect.top() as u32..rect.bottom() as u32).any(|y| {
            (rect.left() as u32..rect.right() as u32)
                .any(|x| pixel_at(width, &pixels, x, y) == axis)
        });
        assert!(found, "axis colour {axis:?} is missing from the viewport");
    }

    // --- nothing is drawn outside the rectangle ---------------------------
    for y in 0..height {
        for x in 0..width {
            let inside = rect.contains(egui::pos2(x as f32, y as f32));
            if !inside {
                assert_eq!(
                    pixel_at(width, &pixels, x, y),
                    viewport_color,
                    "the viewport drew outside its rectangle at ({x}, {y})"
                );
            }
        }
    }
}

fn the_icons_load() {
    // `run()` calls this before loading icons; the test exercises the loader itself.
    match sdl2::image::init(sdl2::image::InitFlag::PNG) {
        Ok(context) => std::mem::forget(context),
        Err(error) => panic!("SDL_image PNG: {error}"),
    }
    let ctx = egui::Context::default();
    let icons = giochi_editor::editor::IconSet::load(
        &ctx,
        giochi_editor::editor::IconSet::default_directory(),
    );
    assert!(
        icons.is_complete(),
        "every shipped icon must decode: {icons:?}"
    );
}

fn the_selection_changes_pixels() {
    let mut engine = engine(640, 480);
    let theme = EditorTheme::dark();
    let rect = Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(640.0, 480.0));
    let camera = OrbitCamera::default();
    let scene = scene();

    engine.render_mut().clear(theme.viewport_color());
    viewport::draw(
        engine.render_mut(),
        rect,
        Some(&scene),
        &camera,
        &[],
        &theme,
    );
    let (width, _, unselected) = engine.render().read_pixels_rgba().expect("read");

    engine.render_mut().clear(theme.viewport_color());
    viewport::draw(
        engine.render_mut(),
        rect,
        Some(&scene),
        &camera,
        &[giochi_engine_3d::scene::SceneNodeId(1)],
        &theme,
    );
    let (_, _, selected) = engine.render().read_pixels_rgba().expect("read");

    let changed = unselected
        .as_chunks::<4>()
        .0
        .iter()
        .zip(selected.as_chunks::<4>().0.iter())
        .filter(|(a, b)| a != b)
        .count();
    assert!(
        changed > 100,
        "the selection outline changed {changed} pixels"
    );

    // Sanity: the image is not a single flat colour.
    let distinct = selected
        .as_chunks::<4>()
        .0
        .iter()
        .collect::<std::collections::HashSet<_>>()
        .len();
    assert!(distinct > 3, "{distinct} distinct colours in the frame");
    assert_eq!(width, 640);
}

fn an_empty_viewport_is_only_the_clear_colour() {
    let mut engine = engine(320, 240);
    let theme = EditorTheme::dark();
    engine.render_mut().clear(theme.viewport_color());

    let rect = Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(300.0, 220.0));
    // No scene, but the grid and axes still draw: those are orientation, not content.
    viewport::draw(
        engine.render_mut(),
        rect,
        None,
        &OrbitCamera::default(),
        &[],
        &theme,
    );

    let (width, _, pixels) = engine.render().read_pixels_rgba().expect("read");
    let viewport_color = (theme.viewport.r(), theme.viewport.g(), theme.viewport.b());
    // The corners of the rectangle are outside the projected grid, so they keep the
    // clear colour; the centre must have something on it.
    assert_eq!(pixel_at(width, &pixels, 12, 12), viewport_color);
    let centre = pixel_at(width, &pixels, 160, 120);
    assert_ne!(centre, viewport_color, "the centre must have the grid/axes");
}

fn the_layout_and_the_viewport_together_paint_a_frame() {
    // The whole editor composition, minus the SDL event loop: engine → clear → viewport
    // → egui panels → present. This is what the loop does each frame, and it is the
    // closest a test gets to "the editor shows a scene".
    let mut engine = engine(800, 600);
    let theme = EditorTheme::dark();
    let mut state = giochi_editor::editor::EditorState::new();
    state.new_scene();
    std::env::set_var("SDL_VIDEODRIVER", "dummy");

    engine.render_mut().clear(theme.viewport_color());
    let mut backend = EguiBackend::new();
    let mut metrics = giochi_editor::editor::LayoutMetrics::default();
    let output = backend.run((800, 600), 1.0 / 60.0, &[], &mut |ctx| {
        theme.apply(ctx);
        metrics = giochi_editor::editor::layout::draw(ctx, &mut state, &theme, None);
    });
    // Second pass: the viewport rectangle from the first is what the viewport draws into.
    viewport::draw(
        engine.render_mut(),
        metrics.viewport_rect.expect("viewport"),
        state.scene.as_ref(),
        &state.viewport_camera,
        &[],
        &theme,
    );
    let output_after = backend.run((800, 600), 1.0 / 60.0, &[], &mut |ctx| {
        theme.apply(ctx);
        metrics = giochi_editor::editor::layout::draw(ctx, &mut state, &theme, None);
    });
    drop(output);
    backend.paint(engine.render_mut(), output_after);
    backend.release(engine.render_mut());

    let (_, _, pixels) = engine.render().read_pixels_rgba().expect("read");
    let viewport_color = (theme.viewport.r(), theme.viewport.g(), theme.viewport.b());
    let non_clear = pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|px| (px[0], px[1], px[2]) != viewport_color)
        .count();
    assert!(
        non_clear > 10_000,
        "the composed frame has only {non_clear} non-clear pixels"
    );
}
