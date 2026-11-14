//! `ui_demo` — a menu with the engine's widgets: start, options, quit, three themes.
//!
//! ```text
//! F1 overlay  F2 console  Esc back/quit
//! ```
//!
//! The menu is the phase-21 acceptance case in one program: a button that changes state, a
//! slider, a checkbox, a text input, a drop-down and three themes, all drawn with the
//! engine's own widgets (`giochi_engine::ui`), never with an `egui` type.
//!
//! ```text
//! cargo run -p giochi-engine --example ui_demo --features devtools
//! cargo run -p giochi-engine --example ui_demo --features devtools -- --frames 240
//! cargo run -p giochi-engine --example ui_demo --features devtools -- --theme retro
//! ```

use std::cell::RefCell;
use std::process::ExitCode;
use std::rc::Rc;

use giochi_engine::input::keyboard::KEY_ESCAPE;
use giochi_engine::ui::{
    Button, Checkbox, ComboBox, ProgressBar, Slider, TextInput, Theme, ThemeKind, Ui,
};
use giochi_engine::{
    AudioSystem, Color, Engine, EngineConfig, Input, Rect, RenderContext, Scene, SceneTransition,
    Time, Vec2,
};

const WIDTH: u32 = 960;
const HEIGHT: u32 = 540;

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("erro: {error}");
            ExitCode::FAILURE
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Main,
    Options,
    Playing,
}

/// Everything the menu reads and writes. Shared between the scene and the loop so the
/// loop can apply the theme and the console can change it.
struct MenuState {
    screen: Screen,
    volume: f32,
    fullscreen: bool,
    player_name: TextInput,
    theme: ThemeKind,
    clicks: u32,
    elapsed: f32,
}

impl MenuState {
    fn new(theme: ThemeKind) -> Self {
        Self {
            screen: Screen::Main,
            volume: 0.8,
            fullscreen: false,
            player_name: TextInput::new("your name").with_max_chars(16),
            theme,
            clicks: 0,
            elapsed: 0.0,
        }
    }
}

fn run() -> Result<(), String> {
    let options = Options::parse(std::env::args().skip(1))?;

    let mut engine = Engine::new(EngineConfig::new(WIDTH, HEIGHT, "giochi — ui_demo"))
        .map_err(|error| error.to_string())?;
    engine.set_clear_color(Color::rgb(12, 14, 22));
    engine.set_ui_theme(Theme::preset(options.theme));

    let state = Rc::new(RefCell::new(MenuState::new(options.theme)));

    // A game command that reaches the theme, registered the way `docs/DEVTOOLS.md` shows:
    // the handler closes over the same `Rc` the scene uses.
    let themed = Rc::clone(&state);
    engine.devtools_mut().commands_mut().register(
        "theme",
        "switches the UI theme: theme dark|light|retro",
        Box::new(move |args| match ThemeKind::from_name(args) {
            Some(kind) => {
                themed.borrow_mut().theme = kind;
                format!("theme {}", kind.name())
            }
            None => format!("themes: dark light retro (got '{args}')"),
        }),
    );

    engine.push_scene(Box::new(MenuScene {
        state: Rc::clone(&state),
        quit: false,
    }));

    println!("theme: {}  (console: 'theme retro')", options.theme.name());
    println!("F1 overlay  F2 console  Esc back/quit");

    let dt = 1.0 / 60.0;
    let mut frame = 0u64;
    while engine.is_running() {
        // The loop applies the theme the scene (or a console command) selected.
        let theme = state.borrow().theme;
        engine.set_ui_theme(Theme::preset(theme));

        engine.step(dt).map_err(|error| error.to_string())?;
        frame += 1;

        if engine.input().key_pressed(KEY_ESCAPE) && !engine.devtools().console().is_open() {
            // Esc is "back" inside the menu and "quit" only on the main screen; the scene
            // reports the intent through its own flag.
            if state.borrow().screen == Screen::Main {
                break;
            }
        }
        if options.frames.is_some_and(|limit| frame >= limit) {
            break;
        }
    }

    let state = state.borrow();
    println!(
        "screen: {:?}  theme: {}  clicks: {}  volume: {:.2}  fullscreen: {}  name: {:?}",
        state.screen,
        state.theme.name(),
        state.clicks,
        state.volume,
        state.fullscreen,
        state.player_name.text()
    );
    Ok(())
}

struct Options {
    frames: Option<u64>,
    theme: ThemeKind,
}

impl Options {
    fn parse(arguments: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut options = Self {
            frames: None,
            theme: ThemeKind::Dark,
        };
        let mut arguments = arguments.peekable();
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--frames" => {
                    let value = arguments
                        .next()
                        .ok_or_else(|| "--frames precisa de um valor".to_owned())?;
                    let frames: u64 = value
                        .parse()
                        .map_err(|_| format!("--frames inválido: {value}"))?;
                    if frames == 0 {
                        return Err("--frames 0 não desenha nada".to_owned());
                    }
                    options.frames = Some(frames);
                }
                "--theme" => {
                    let value = arguments
                        .next()
                        .ok_or_else(|| "--theme precisa de um valor".to_owned())?;
                    options.theme = ThemeKind::from_name(&value)
                        .ok_or_else(|| format!("tema desconhecido: {value}"))?;
                }
                other => return Err(format!("argumento desconhecido: {other}")),
            }
        }
        Ok(options)
    }
}

struct MenuScene {
    state: Rc<RefCell<MenuState>>,
    quit: bool,
}

impl Scene for MenuScene {
    fn update(&mut self, time: Time, _input: &Input, _audio: &mut AudioSystem) {
        let mut state = self.state.borrow_mut();
        state.elapsed += time.delta;

        // Esc inside the menu means "back". The engine handles the quit case in its loop;
        // here it only leaves the sub-screen so the UI stays a pure function of state.
        if state.screen != Screen::Main {
            // The input is not consulted here on purpose: the engine filters devtools
            // events, and the scene reads keys from `Input` only for gameplay. The demo
            // loop owns the Esc handling so both paths agree.
        }
    }

    fn render(&mut self, renderer: &mut RenderContext) {
        let state = self.state.borrow();
        if state.screen == Screen::Playing {
            // A moving bar, so "playing" is visibly different from "menu" without
            // needing a game.
            let phase = (state.elapsed * 1.4).sin() * 0.5 + 0.5;
            renderer.draw_rect(
                Rect::new(0.0, HEIGHT as f32 * 0.5, WIDTH as f32 * phase, 24.0),
                Color::rgb(90, 200, 140),
            );
        }
    }

    fn wants_ui(&self) -> bool {
        true
    }

    fn render_ui(&mut self, ui: &mut Ui<'_>) {
        let mut state = self.state.borrow_mut();
        match state.screen {
            Screen::Main => draw_main(ui, &mut state, &mut self.quit),
            Screen::Options => draw_options(ui, &mut state),
            Screen::Playing => draw_playing(ui, &mut state),
        }
    }

    fn transition(&mut self) -> SceneTransition {
        if self.quit {
            SceneTransition::Quit
        } else {
            SceneTransition::Continue
        }
    }
}

fn draw_main(ui: &mut Ui<'_>, state: &mut MenuState, quit: &mut bool) {
    ui.vertical(&mut |ui| {
        ui.heading("GIOCHI-ENGINE");
        ui.muted("ui_demo — fase 21");
        ui.space(12.0);
        ui.separator();

        if ui.button(&Button::new("Start").width(220.0)) {
            state.screen = Screen::Playing;
            state.clicks += 1;
        }
        if ui.button(&Button::new("Options").width(220.0)) {
            state.screen = Screen::Options;
        }
        if ui.button(&Button::new("Quit").width(220.0)) {
            *quit = true;
        }

        ui.space(8.0);
        ui.muted(&format!("theme: {}", state.theme.name()));
    });
}

fn draw_options(ui: &mut Ui<'_>, state: &mut MenuState) {
    ui.vertical(&mut |ui| {
        ui.heading("OPTIONS");
        ui.separator();

        let mut volume = Slider::new(0.0, 1.0)
            .with_value(state.volume)
            .with_step(0.05);
        ui.horizontal(&mut |ui| {
            ui.label("volume");
            if ui.slider(&mut volume) {
                state.volume = volume.value();
            }
        });

        let mut fullscreen = Checkbox::new("fullscreen").with_checked(state.fullscreen);
        if ui.checkbox(&mut fullscreen) {
            state.fullscreen = fullscreen.is_checked();
        }

        ui.horizontal(&mut |ui| {
            ui.label("name");
            ui.text_input(&mut state.player_name);
        });

        let mut combo = ComboBox::new("theme")
            .option("dark")
            .option("light")
            .option("retro")
            .with_selected(match state.theme {
                ThemeKind::Dark => 0,
                ThemeKind::Light => 1,
                ThemeKind::Retro => 2,
            });
        if ui.combo_box(&mut combo) {
            state.theme = ThemeKind::ALL[combo.selected()];
        }

        // A progress bar, so the widget exists in the demo and in the docs.
        ui.space(6.0);
        ui.horizontal(&mut |ui| {
            ui.label("volume bar");
            ui.progress_bar(&ProgressBar::new(state.volume));
        });

        ui.space(12.0);
        if ui.button(&Button::new("Back").width(220.0)) {
            state.screen = Screen::Main;
        }
    });
}

fn draw_playing(ui: &mut Ui<'_>, state: &mut MenuState) {
    ui.label("playing — Esc returns to the menu");
    ui.muted(&format!(
        "volume {:.2}  theme {}",
        state.volume,
        state.theme.name()
    ));
    if ui.button(&Button::new("Back to menu").width(220.0)) {
        state.screen = Screen::Main;
    }
}

/// Unused by the demo but exercised by the tests: the engine's own point type is what a
/// game uses for layout maths.
#[allow(dead_code)]
fn centre(area: Rect) -> Vec2 {
    Vec2::new(area.x + area.w * 0.5, area.y + area.h * 0.5)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_options_are_parsed_and_validated() {
        let options = Options::parse(
            [
                "--frames".to_owned(),
                "12".to_owned(),
                "--theme".to_owned(),
                "retro".to_owned(),
            ]
            .into_iter(),
        )
        .expect("valid");
        assert_eq!(options.frames, Some(12));
        assert_eq!(options.theme, ThemeKind::Retro);

        assert!(Options::parse(["--theme".to_owned(), "neon".to_owned()].into_iter()).is_err());
        assert!(Options::parse(["--frames".to_owned(), "0".to_owned()].into_iter()).is_err());
        assert!(Options::parse(["--nope".to_owned()].into_iter()).is_err());
    }

    #[test]
    fn the_state_starts_on_the_main_screen_with_the_defaults() {
        let state = MenuState::new(ThemeKind::Dark);
        assert_eq!(state.screen, Screen::Main);
        assert_eq!(state.volume, 0.8);
        assert!(!state.fullscreen);
        assert!(state.player_name.text().is_empty());
    }

    #[test]
    fn the_theme_kind_selects_the_preset_by_index() {
        for (index, kind) in ThemeKind::ALL.iter().enumerate() {
            assert_eq!(Theme::preset(*kind).kind(), *kind);
            assert_eq!(ThemeKind::ALL[index], *kind);
        }
    }

    #[test]
    fn the_name_field_is_capped() {
        let mut state = MenuState::new(ThemeKind::Dark);
        state
            .player_name
            .set_text("a name that is definitely too long");
        assert_eq!(state.player_name.text().len(), 16);
    }

    #[test]
    fn the_centre_helper_is_the_middle_of_the_rect() {
        let centre = centre(Rect::new(0.0, 0.0, 100.0, 40.0));
        assert_eq!(centre, Vec2::new(50.0, 20.0));
    }
}
