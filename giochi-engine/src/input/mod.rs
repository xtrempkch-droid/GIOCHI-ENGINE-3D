//! Input state: keyboard, mouse and the first connected game controller.
//!
//! [`Input`] is updated once per frame from the SDL2 event queue *before* the
//! scene's `update` runs, so scenes see one consistent snapshot for the whole
//! frame:
//!
//! ```no_run
//! # use giochi_engine::{Input, Platform};
//! # let platform = Platform::new().unwrap();
//! let mut input = Input::new(&platform);
//!
//! loop {
//!     let events = platform.poll_events();
//!     input.update(&events); // clears one-frame edges, then applies events
//!
//!     if input.key_down(giochi_engine::input::keyboard::KEY_W) {
//!         // move the player
//!     }
//!     # break;
//! }
//! ```

use sdl2::event::Event;
use sdl2::GameControllerSubsystem;

use crate::platform::Platform;

pub mod gamepad;
pub mod keyboard;
pub mod mouse;

pub use gamepad::{GamepadAxis, GamepadButton, GamepadState};
pub use keyboard::KeyboardState;
pub use mouse::MouseState;

use keyboard::KeyCode;
use mouse::MouseButton;

/// Aggregated input state for the current frame.
///
/// Holds no public growable state: every collection is private and reachable only
/// through query methods, which keeps the type C-shaped (ADR-006) and lets the C
/// ABI expose `giochi_input_*` free functions without a context pointer (ADR-012).
///
/// # C ABI
///
/// `Input` is never passed across the FFI boundary. C consumers query individual
/// values through `giochi_input_*` functions.
#[repr(C)]
#[derive(Debug)]
pub struct Input {
    keyboard: KeyboardState,
    mouse: MouseState,
    gamepad: GamepadState,
    /// `None` when the engine runs without the controller subsystem (tests).
    controllers: Option<GameControllerSubsystem>,
    _reserved: [u32; 4],
}

impl Input {
    /// Creates an input state bound to `platform`'s controller subsystem.
    ///
    /// The controller subsystem is initialised by [`Platform::new`]; if it is
    /// unavailable, input still works for keyboard and mouse and
    /// [`Input::gamepad`] simply never reports a connected device.
    pub fn new(platform: &Platform) -> Self {
        let controllers = match platform.game_controller_subsystem() {
            Ok(subsystem) => Some(subsystem.clone()),
            Err(error) => {
                log::warn!("gamepad input indisponível: {error}");
                None
            }
        };

        Self {
            keyboard: KeyboardState::new(),
            mouse: MouseState::new(),
            gamepad: GamepadState::new(),
            controllers,
            _reserved: [0; 4],
        }
    }

    /// Creates an input state with no controller subsystem.
    ///
    /// Keyboard and mouse work normally, no gamepad is ever reported, and no SDL
    /// subsystem is required. Intended for tests, headless replay and any consumer
    /// that injects events itself.
    pub fn without_gamepad() -> Self {
        Self {
            keyboard: KeyboardState::new(),
            mouse: MouseState::new(),
            gamepad: GamepadState::new(),
            controllers: None,
            _reserved: [0; 4],
        }
    }

    /// Consumes one frame of SDL2 events and updates the edge-detection state.
    ///
    /// Call this once per frame, **before** the scene's `update`. It:
    ///
    /// 1. clears the one-frame data (pressed/released edges, mouse delta, wheel);
    /// 2. applies every event in order;
    /// 3. opens the first available controller when none is open.
    ///
    /// Held state (keys down, buttons down, cursor position) survives the frame.
    pub fn update(&mut self, events: &[Event]) {
        self.begin_frame();

        for event in events {
            self.apply_event(event);
        }

        if !self.gamepad.is_connected() {
            if let Some(controllers) = self.controllers.as_ref() {
                self.gamepad.open_first(controllers);
            }
        }
    }

    /// Clears the per-frame data without applying any event.
    ///
    /// [`Input::update`] calls this itself; it is public so a consumer that injects
    /// input manually (replays, tests, scripted demos) can drive the same
    /// lifecycle.
    pub fn begin_frame(&mut self) {
        self.keyboard.begin_frame();
        self.mouse.begin_frame();
    }

    /// Applies a single event.
    ///
    /// Exposed so a consumer that does not use [`Platform::poll_events`] can feed
    /// events one by one; [`Input::update`] is the normal entry point.
    pub fn apply_event(&mut self, event: &Event) {
        match event {
            Event::KeyDown {
                keycode, repeat, ..
            } => {
                // SDL sets `repeat` on auto-repeat: the key is still down, but this
                // is not a fresh press.
                if let (Some(keycode), false) = (keycode, *repeat) {
                    self.keyboard.press(keycode_to_code(*keycode));
                }
            }
            Event::KeyUp {
                keycode: Some(keycode),
                ..
            } => {
                self.keyboard.release(keycode_to_code(*keycode));
            }
            Event::MouseMotion {
                x, y, xrel, yrel, ..
            } => {
                self.mouse.set_position(*x as f32, *y as f32);
                self.mouse.add_delta(*xrel as f32, *yrel as f32);
            }
            Event::MouseButtonDown {
                mouse_btn, x, y, ..
            } => {
                self.mouse.set_position(*x as f32, *y as f32);
                self.mouse.press(mouse_button_to_code(*mouse_btn));
            }
            Event::MouseButtonUp {
                mouse_btn, x, y, ..
            } => {
                self.mouse.set_position(*x as f32, *y as f32);
                self.mouse.release(mouse_button_to_code(*mouse_btn));
            }
            Event::MouseWheel {
                precise_y,
                y,
                mouse_x,
                mouse_y,
                ..
            } => {
                self.mouse.set_position(*mouse_x as f32, *mouse_y as f32);
                // `precise_y` needs SDL >= 2.0.18; the integer field keeps older
                // recordings and platforms working.
                let amount = if *precise_y != 0.0 {
                    *precise_y
                } else {
                    *y as f32
                };
                self.mouse.add_wheel(amount);
            }
            Event::ControllerDeviceAdded { which, .. } => {
                if let Some(controllers) = self.controllers.as_ref() {
                    self.gamepad.open_index(controllers, *which);
                }
            }
            Event::ControllerDeviceRemoved { which, .. } => {
                self.gamepad.close_if_instance(*which);
            }
            // A window that loses focus never receives the `KeyUp`/`MouseButtonUp` for
            // whatever was held at that moment: the OS takes the keyboard and the mouse
            // away, and SDL has nothing to report. Without this the held set keeps the
            // keys it had when focus was lost, so a player who alt-tabs while walking
            // keeps walking — for the rest of the process's life.
            //
            // The `reset` deliberately does **not** synthesise "released" edges. A game
            // that binds "release to jump" would fire it on an alt-tab, and the engine
            // cannot tell the two apart. Dropping the state silently is the honest
            // option: the key is simply no longer held.
            Event::Window {
                win_event: sdl2::event::WindowEvent::FocusLost,
                ..
            } => {
                log::debug!("window lost focus: releasing every held key and button");
                self.keyboard.reset();
                self.mouse.reset();
            }
            _ => {}
        }
    }

    /// Read-only keyboard state.
    pub fn keyboard(&self) -> &KeyboardState {
        &self.keyboard
    }

    /// Mutable keyboard state, for consumers that inject input.
    pub fn keyboard_mut(&mut self) -> &mut KeyboardState {
        &mut self.keyboard
    }

    /// Read-only mouse state.
    pub fn mouse(&self) -> &MouseState {
        &self.mouse
    }

    /// Mutable mouse state, for consumers that inject input.
    pub fn mouse_mut(&mut self) -> &mut MouseState {
        &mut self.mouse
    }

    /// Read-only gamepad state.
    pub fn gamepad(&self) -> &GamepadState {
        &self.gamepad
    }

    /// Convenience: `true` while `key` is held. Same as
    /// [`KeyboardState::is_down`].
    pub fn key_down(&self, key: KeyCode) -> bool {
        self.keyboard.is_down(key)
    }

    /// Convenience: `true` only on the frame `key` went down.
    pub fn key_pressed(&self, key: KeyCode) -> bool {
        self.keyboard.is_pressed_this_frame(key)
    }

    /// Convenience: `true` only on the frame `key` went up.
    pub fn key_released(&self, key: KeyCode) -> bool {
        self.keyboard.is_released_this_frame(key)
    }

    /// Convenience: `true` while `button` is held.
    pub fn mouse_down(&self, button: MouseButton) -> bool {
        self.mouse.is_down(button)
    }
}

/// Widens an SDL keycode into the engine's `u32` representation.
fn keycode_to_code(keycode: sdl2::keyboard::Keycode) -> KeyCode {
    u32::from_ne_bytes(i32::from(keycode).to_ne_bytes())
}

/// Widens an SDL mouse button into the engine's `u32` representation.
fn mouse_button_to_code(button: sdl2::mouse::MouseButton) -> MouseButton {
    button as u8 as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::keyboard::{KEY_A, KEY_D, KEY_ESCAPE, KEY_S, KEY_W};
    use crate::input::mouse::MOUSE_LEFT;
    use crate::input::mouse::MOUSE_RIGHT;
    use crate::math::Vec2;
    use sdl2::keyboard::{Keycode, Mod};
    use sdl2::mouse::MouseButton as SdlMouseButton;

    fn key_down(keycode: Keycode, repeat: bool) -> Event {
        Event::KeyDown {
            timestamp: 0,
            window_id: 0,
            keycode: Some(keycode),
            scancode: None,
            keymod: Mod::NOMOD,
            repeat,
        }
    }

    fn key_up(keycode: Keycode) -> Event {
        Event::KeyUp {
            timestamp: 0,
            window_id: 0,
            keycode: Some(keycode),
            scancode: None,
            keymod: Mod::NOMOD,
            repeat: false,
        }
    }

    fn motion(x: i32, y: i32, xrel: i32, yrel: i32) -> Event {
        Event::MouseMotion {
            timestamp: 0,
            window_id: 0,
            which: 0,
            mousestate: sdl2::mouse::MouseState::from_sdl_state(0),
            x,
            y,
            xrel,
            yrel,
        }
    }

    fn button_down(button: SdlMouseButton, x: i32, y: i32) -> Event {
        Event::MouseButtonDown {
            timestamp: 0,
            window_id: 0,
            which: 0,
            mouse_btn: button,
            clicks: 1,
            x,
            y,
        }
    }

    fn focus_lost() -> Event {
        Event::Window {
            timestamp: 0,
            window_id: 0,
            win_event: sdl2::event::WindowEvent::FocusLost,
        }
    }

    fn wheel(precise_y: f32, y: i32) -> Event {
        Event::MouseWheel {
            timestamp: 0,
            window_id: 0,
            which: 0,
            x: 0,
            y,
            direction: sdl2::mouse::MouseWheelDirection::Normal,
            precise_x: 0.0,
            precise_y,
            mouse_x: 0,
            mouse_y: 0,
        }
    }

    #[test]
    fn a_fresh_input_reports_nothing() {
        let input = Input::without_gamepad();

        assert!(!input.key_down(KEY_W));
        assert!(!input.key_pressed(KEY_W));
        assert!(!input.key_released(KEY_W));
        assert!(!input.mouse_down(MOUSE_RIGHT));
        assert_eq!(input.mouse().position(), (0.0, 0.0));
        assert!(!input.gamepad().is_connected());
        assert_eq!(input._reserved, [0; 4]);
    }

    #[test]
    fn a_key_press_is_down_for_the_whole_frame_and_pressed_once() {
        let mut input = Input::without_gamepad();

        input.update(&[key_down(Keycode::W, false)]);
        assert!(input.key_down(KEY_W));
        assert!(input.key_pressed(KEY_W));

        input.update(&[]);
        assert!(input.key_down(KEY_W), "the key is still held");
        assert!(!input.key_pressed(KEY_W), "the edge must clear");
    }

    #[test]
    fn key_repeat_does_not_retrigger_a_press() {
        let mut input = Input::without_gamepad();
        input.update(&[key_down(Keycode::W, false)]);

        input.update(&[key_down(Keycode::W, true)]);

        assert!(input.key_down(KEY_W));
        assert!(!input.key_pressed(KEY_W));
    }

    #[test]
    fn release_produces_a_single_frame_edge() {
        let mut input = Input::without_gamepad();
        input.update(&[key_down(Keycode::A, false)]);

        input.update(&[key_up(Keycode::A)]);
        assert!(!input.key_down(KEY_A));
        assert!(input.key_released(KEY_A));

        input.update(&[]);
        assert!(!input.key_released(KEY_A));
    }

    #[test]
    fn escape_maps_to_the_documented_constant() {
        let mut input = Input::without_gamepad();
        input.update(&[key_down(Keycode::Escape, false)]);

        assert!(input.key_down(KEY_ESCAPE));
    }

    #[test]
    fn mouse_motion_updates_position_and_delta() {
        let mut input = Input::without_gamepad();

        input.update(&[motion(120, 80, 20, -5)]);

        assert_eq!(input.mouse().position(), (120.0, 80.0));
        assert_eq!(input.mouse().delta(), Vec2::new(20.0, -5.0));

        input.update(&[]);
        assert_eq!(input.mouse().position(), (120.0, 80.0));
        assert_eq!(input.mouse().delta(), Vec2::ZERO);
    }

    #[test]
    fn mouse_button_events_carry_the_position() {
        let mut input = Input::without_gamepad();

        input.update(&[button_down(SdlMouseButton::Right, 300, 200)]);

        assert!(input.mouse_down(MOUSE_RIGHT));
        assert_eq!(input.mouse().position(), (300.0, 200.0));
        assert!(input.mouse().is_pressed_this_frame(MOUSE_RIGHT));
    }

    #[test]
    fn wheel_delta_accumulates_within_a_frame_and_resets_after() {
        let mut input = Input::without_gamepad();

        input.update(&[wheel(1.0, 1), wheel(0.5, 0)]);
        assert_eq!(input.mouse().wheel_delta(), 1.5);

        input.update(&[]);
        assert_eq!(input.mouse().wheel_delta(), 0.0);
    }

    #[test]
    fn the_integer_wheel_field_is_used_when_the_precise_one_is_zero() {
        let mut input = Input::without_gamepad();

        input.update(&[wheel(0.0, -2)]);

        assert_eq!(input.mouse().wheel_delta(), -2.0);
    }

    #[test]
    fn begin_frame_clears_edges_but_keeps_held_state() {
        let mut input = Input::without_gamepad();
        input.update(&[
            key_down(Keycode::S, false),
            motion(10, 10, 1, 1),
            button_down(SdlMouseButton::Left, 10, 10),
        ]);

        input.begin_frame();

        assert!(input.key_down(KEY_S));
        assert!(input.mouse_down(MOUSE_LEFT));
        assert!(!input.key_pressed(KEY_S));
        assert_eq!(input.mouse().delta(), Vec2::ZERO);
    }

    #[test]
    fn events_are_applied_in_order() {
        let mut input = Input::without_gamepad();

        input.update(&[key_down(Keycode::D, false), key_up(Keycode::D)]);

        assert!(!input.key_down(KEY_D));
        assert!(input.key_pressed(KEY_D));
        assert!(input.key_released(KEY_D));
    }

    #[test]
    fn events_from_other_windows_and_unknown_variants_are_ignored() {
        let mut input = Input::without_gamepad();

        input.update(&[Event::Quit { timestamp: 0 }]);

        assert!(!input.keyboard().any_down());
        assert!(!input.gamepad().is_connected());
    }

    #[test]
    fn losing_focus_releases_every_held_key_and_button() {
        // SDL delivers no `KeyUp` for a key held at the moment the window loses focus:
        // the OS has taken the keyboard. Without the `FocusLost` arm the held set kept
        // those keys forever, so a player who alt-tabbed mid-stride kept walking with
        // nothing pressed. The same applied to a held mouse button, which a drag-based
        // input path would keep reading as "still dragging".
        let mut input = Input::without_gamepad();
        input.update(&[
            key_down(Keycode::W, false),
            key_down(Keycode::LCtrl, false),
            button_down(SdlMouseButton::Left, 10, 10),
        ]);
        assert!(input.key_down(KEY_W));
        assert!(input.mouse_down(MOUSE_LEFT));

        input.update(&[focus_lost()]);

        assert!(
            !input.keyboard().any_down(),
            "no key may stay held after focus is lost"
        );
        assert!(
            !input.mouse().is_down(MOUSE_LEFT),
            "no button may stay held after focus is lost"
        );
        assert_eq!(input.mouse().delta(), Vec2::ZERO);

        // A phantom "released" edge would fire a game's release binding on an alt-tab,
        // and the engine cannot tell an alt-tab from a real release.
        assert!(
            !input.key_released(KEY_W),
            "focus loss must not synthesise a release edge"
        );
        assert!(!input.mouse().is_released_this_frame(MOUSE_LEFT));
    }

    #[test]
    fn focus_loss_leaves_the_rest_of_the_frame_working() {
        // The reset must not deafen the input for the frame: a key pressed in the same
        // batch, after the focus event, is still reported.
        let mut input = Input::without_gamepad();
        input.update(&[key_down(Keycode::A, false)]);

        input.update(&[focus_lost(), key_down(Keycode::D, false)]);

        assert!(!input.key_down(KEY_A), "the pre-focus key is gone");
        assert!(input.key_down(KEY_D), "the post-focus key is live");
        assert!(input.key_pressed(KEY_D));
    }
}
