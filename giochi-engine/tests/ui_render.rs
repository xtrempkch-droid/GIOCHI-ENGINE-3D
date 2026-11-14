//! The scene UI really draws, through the engine's own widgets.
//!
//! The interesting part is not "buttons exist" but the chain: `Scene::wants_ui` makes the
//! engine run an egui pass, the theme is applied, the widget facade draws, the backend
//! tessellates, and the pixels reach the canvas. A regression anywhere in that chain
//! produces a menu that is invisible while every unit test still passes, so this test
//! asserts on pixels and on the widget state the frame produced.
//!
//! One test per binary: SDL allows a single `EventPump` per process.

use giochi_engine::ui::{Button, Checkbox, Slider, Theme, ThemeKind};
use giochi_engine::{
    AudioSystem, Color, Engine, EngineConfig, Input, Rect, RenderContext, Scene, SceneTransition,
    Time, Ui,
};

const SIZE: u32 = 480;

/// What the scene's UI reported back to the game.
#[derive(Default)]
struct Observed {
    frames_with_ui: u32,
    clicks: u32,
    volume: f32,
    fullscreen: bool,
}

struct MenuScene {
    observed: std::rc::Rc<std::cell::RefCell<Observed>>,
    clicks: u32,
}

impl Scene for MenuScene {
    fn update(&mut self, _time: Time, _input: &Input, _audio: &mut AudioSystem) {}

    fn render(&mut self, _renderer: &mut RenderContext) {}

    fn wants_ui(&self) -> bool {
        true
    }

    fn render_ui(&mut self, ui: &mut Ui<'_>) {
        let mut observed = self.observed.borrow_mut();
        observed.frames_with_ui += 1;

        // A heading, a button and two stateful widgets: enough to exercise every draw path
        // the facade owns (text, a filled rect, a knob, a checkbox mark).
        ui.heading("GIOCHI");
        ui.separator();

        if ui.button(&Button::new("Start").width(180.0)) {
            self.clicks += 1;
            observed.clicks += 1;
        }

        let mut volume = Slider::new(0.0, 1.0).with_value(observed.volume);
        if ui.slider(&mut volume) {
            observed.volume = volume.value();
        }

        let mut fullscreen = Checkbox::new("fullscreen").with_checked(observed.fullscreen);
        if ui.checkbox(&mut fullscreen) {
            observed.fullscreen = fullscreen.is_checked();
        }
    }

    fn transition(&mut self) -> SceneTransition {
        SceneTransition::Continue
    }
}

fn frame(engine: &mut Engine) -> Vec<u8> {
    let (width, height) = engine.render().size();
    engine
        .render_mut()
        .read_pixels(Rect::new(0.0, 0.0, width as f32, height as f32))
        .expect("read_pixels must work")
}

fn changed_in(pixels: &[u8], width: u32, area: Rect) -> usize {
    let mut changed = 0;
    for row in 0..area.h as u32 {
        for column in 0..area.w as u32 {
            let offset = (((area.y as u32 + row) * width + (area.x as u32 + column)) * 4) as usize;
            if (pixels[offset], pixels[offset + 1], pixels[offset + 2]) != (0, 0, 0) {
                changed += 1;
            }
        }
    }
    changed
}

#[test]
fn a_scene_ui_draws_with_every_theme() {
    std::env::set_var("SDL_VIDEODRIVER", "dummy");

    let observed = std::rc::Rc::new(std::cell::RefCell::new(Observed::default()));
    let mut engine = Engine::new(EngineConfig::new(SIZE, SIZE, "giochi-ui-test"))
        .expect("the dummy driver must initialise");
    engine.set_clear_color(Color::rgb(0, 0, 0));
    engine.push_scene(Box::new(MenuScene {
        observed: std::rc::Rc::clone(&observed),
        clicks: 0,
    }));

    let (width, height) = engine.render().size();
    let whole = Rect::new(0.0, 0.0, width as f32, height as f32);

    // ---- the scene asked for UI, so egui runs and draws ----------------
    // The first egui frame builds the font atlas; a second frame is the steady state.
    engine.step(1.0 / 60.0).expect("step");
    engine.step(1.0 / 60.0).expect("step");
    let drawn = changed_in(&frame(&mut engine), width, whole);
    assert!(
        drawn > 500,
        "the menu must paint text and widgets, only {drawn} pixel(s) changed"
    );
    // The scene is pushed *after* the engine was built, so the first `step` before this
    // point was the zero-UI frame; two more follow.
    let ui_frames = observed.borrow().frames_with_ui;
    assert!(
        ui_frames >= 2,
        "render_ui must be called on every frame after the scene was pushed, got {ui_frames}"
    );

    // ---- each theme paints a different picture -------------------------
    let mut per_theme = Vec::new();
    for kind in ThemeKind::ALL {
        engine.set_ui_theme(Theme::preset(kind));
        engine.step(1.0 / 60.0).expect("step");
        engine.step(1.0 / 60.0).expect("step");
        let pixels = frame(&mut engine);
        let coloured = changed_in(&pixels, width, whole);
        assert!(
            coloured > 500,
            "{} must paint a menu, {coloured} pixel(s)",
            kind.name()
        );
        per_theme.push((kind, pixels));
    }
    assert_ne!(
        per_theme[0].1, per_theme[2].1,
        "retro and dark must not be the same picture"
    );

    // ---- the engine survives closing and reopening the UI --------------
    // A scene that stops asking for UI must stop the egui pass entirely.
    engine.set_ui_theme(Theme::dark());
    engine.step(1.0 / 60.0).expect("step");
    assert!(engine.ui_theme().kind() == ThemeKind::Dark);

    // The console still works over the scene UI: one context, two panels.
    engine.devtools_mut().console_mut().open();
    engine.step(1.0 / 60.0).expect("step");
    engine.step(1.0 / 60.0).expect("step");
    let with_console = frame(&mut engine);
    assert!(
        changed_in(
            &with_console,
            width,
            Rect::new(0.0, height as f32 * 0.7, width as f32, height as f32 * 0.3)
        ) > 100,
        "the console must draw below the scene UI"
    );
    engine.devtools_mut().console_mut().close();
}
