//! Game controller state for the first connected controller.
//!
//! The engine follows the "player 1 console" model: the first controller SDL2
//! reports as a game controller is player one, and its state is read live from SDL
//! on demand (SDL already applies the axis/button dead zones from the controller
//! mapping). Only device add/remove needs event handling, because that changes
//! which controller the engine holds open.

use sdl2::controller::{Axis, Button, GameController};
use sdl2::GameControllerSubsystem;

/// Controller axis identifier, matching SDL2's `SDL_CONTROLLER_AXIS_*` values.
pub type GamepadAxis = u32;

/// Controller button identifier, matching SDL2's `SDL_CONTROLLER_BUTTON_*`
/// values.
pub type GamepadButton = u32;

/// Left stick, horizontal.
pub const AXIS_LEFT_X: GamepadAxis = 0;
/// Left stick, vertical.
pub const AXIS_LEFT_Y: GamepadAxis = 1;
/// Right stick, horizontal.
pub const AXIS_RIGHT_X: GamepadAxis = 2;
/// Right stick, vertical.
pub const AXIS_RIGHT_Y: GamepadAxis = 3;
/// Left trigger.
pub const AXIS_TRIGGER_LEFT: GamepadAxis = 4;
/// Right trigger.
pub const AXIS_TRIGGER_RIGHT: GamepadAxis = 5;

/// Bottom face button (`A` on Xbox, `Cross` on PlayStation).
pub const BUTTON_SOUTH: GamepadButton = 0;
/// Right face button (`B` on Xbox, `Circle` on PlayStation).
pub const BUTTON_EAST: GamepadButton = 1;
/// Left face button (`X` on Xbox, `Square` on PlayStation).
pub const BUTTON_WEST: GamepadButton = 2;
/// Top face button (`Y` on Xbox, `Triangle` on PlayStation).
pub const BUTTON_NORTH: GamepadButton = 3;
/// Back / select.
pub const BUTTON_BACK: GamepadButton = 4;
/// Guide / home.
pub const BUTTON_GUIDE: GamepadButton = 5;
/// Start / options.
pub const BUTTON_START: GamepadButton = 6;
/// Left stick click.
pub const BUTTON_LEFT_STICK: GamepadButton = 7;
/// Right stick click.
pub const BUTTON_RIGHT_STICK: GamepadButton = 8;
/// Left shoulder / bumper.
pub const BUTTON_LEFT_SHOULDER: GamepadButton = 9;
/// Right shoulder / bumper.
pub const BUTTON_RIGHT_SHOULDER: GamepadButton = 10;
/// D-pad up.
pub const BUTTON_DPAD_UP: GamepadButton = 11;
/// D-pad down.
pub const BUTTON_DPAD_DOWN: GamepadButton = 12;
/// D-pad left.
pub const BUTTON_DPAD_LEFT: GamepadButton = 13;
/// D-pad right.
pub const BUTTON_DPAD_RIGHT: GamepadButton = 14;

/// Highest joystick index probed when looking for a controller to open.
///
/// Cheap and bounded: SDL's `is_game_controller` is a table lookup, and probing
/// only happens while no controller is open.
const MAX_PROBED_INDEX: u32 = 8;

/// State of the first connected controller.
///
/// Holds at most one open device. `0`/invalid axis or button identifiers are
/// reported as "neutral" (`0.0` / `false`) rather than as an error, because input
/// queries happen in draw loops.
#[repr(C)]
pub struct GamepadState {
    controller: Option<GameController>,
    name: Option<String>,
    _reserved: [u32; 4],
}

impl std::fmt::Debug for GamepadState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GamepadState")
            .field("connected", &self.is_connected())
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl Default for GamepadState {
    fn default() -> Self {
        Self::new()
    }
}

impl GamepadState {
    /// Creates a state with no controller connected.
    pub fn new() -> Self {
        Self {
            controller: None,
            name: None,
            _reserved: [0; 4],
        }
    }

    /// Opens the lowest-indexed device SDL reports as a game controller.
    ///
    /// Returns `true` when a controller is open afterwards. Already-connected
    /// controllers are picked up because their state is queried live; this method
    /// only decides *which device* the engine holds.
    pub fn open_first(&mut self, subsystem: &GameControllerSubsystem) -> bool {
        if self.controller.is_some() {
            return true;
        }

        for index in 0..MAX_PROBED_INDEX {
            if let Some(controller) = self.try_open(subsystem, index) {
                self.name = Some(controller.name());
                log::info!(
                    "gamepad conectado: '{}' (joypad {index})",
                    self.name.as_deref().unwrap_or("?")
                );
                self.controller = Some(controller);
                return true;
            }
        }

        false
    }

    /// Opens a specific joystick index, if SDL maps it to a game controller.
    pub fn open_index(&mut self, subsystem: &GameControllerSubsystem, index: u32) -> bool {
        if self.controller.is_some() {
            return true;
        }

        match self.try_open(subsystem, index) {
            Some(controller) => {
                self.name = Some(controller.name());
                log::info!(
                    "gamepad conectado: '{}' (joypad {index})",
                    self.name.as_deref().unwrap_or("?")
                );
                self.controller = Some(controller);
                true
            }
            None => false,
        }
    }

    /// Closes the open controller, if any.
    pub fn close(&mut self) {
        if let Some(controller) = self.controller.take() {
            log::info!("gamepad desconectado: '{}'", controller.name());
        }
        self.name = None;
    }

    /// Closes the controller only if it is the device with `instance_id`.
    ///
    /// Used to ignore the removal of a different device while player one stays
    /// connected.
    pub fn close_if_instance(&mut self, instance_id: u32) -> bool {
        match self.controller.as_ref() {
            Some(controller) if controller.instance_id() == instance_id => {
                self.close();
                true
            }
            _ => false,
        }
    }

    /// Returns `true` if a controller is currently open.
    pub fn is_connected(&self) -> bool {
        self.controller.is_some()
    }

    /// Name reported by SDL for the open controller.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// SDL instance id of the open controller, used to correlate removal events.
    pub fn instance_id(&self) -> Option<u32> {
        self.controller.as_ref().map(GameController::instance_id)
    }

    /// Returns the normalized position of `axis` in `[-1.0, 1.0]`.
    ///
    /// Sticks rest at `0.0`; triggers rest at `-1.0`, matching SDL2. Unknown axis
    /// identifiers and a disconnected controller return `0.0`.
    pub fn axis(&self, axis: GamepadAxis) -> f32 {
        let Some(controller) = self.controller.as_ref() else {
            return 0.0;
        };
        let Some(axis) = to_sdl_axis(axis) else {
            return 0.0;
        };

        normalize(controller.axis(axis))
    }

    /// Returns `true` while `button` is held down.
    ///
    /// Unknown button identifiers and a disconnected controller return `false`.
    pub fn is_down(&self, button: GamepadButton) -> bool {
        let Some(controller) = self.controller.as_ref() else {
            return false;
        };
        let Some(button) = to_sdl_button(button) else {
            return false;
        };

        controller.button(button)
    }

    fn try_open(&self, subsystem: &GameControllerSubsystem, index: u32) -> Option<GameController> {
        if !subsystem.is_game_controller(index) {
            return None;
        }

        match subsystem.open(index) {
            Ok(controller) => Some(controller),
            Err(err) => {
                log::warn!("falha ao abrir o gamepad {index}: {err}");
                None
            }
        }
    }
}

/// Converts an engine axis id into SDL's enum.
fn to_sdl_axis(axis: GamepadAxis) -> Option<Axis> {
    Some(match axis {
        AXIS_LEFT_X => Axis::LeftX,
        AXIS_LEFT_Y => Axis::LeftY,
        AXIS_RIGHT_X => Axis::RightX,
        AXIS_RIGHT_Y => Axis::RightY,
        AXIS_TRIGGER_LEFT => Axis::TriggerLeft,
        AXIS_TRIGGER_RIGHT => Axis::TriggerRight,
        _ => return None,
    })
}

/// Converts an engine button id into SDL's enum.
fn to_sdl_button(button: GamepadButton) -> Option<Button> {
    Some(match button {
        BUTTON_SOUTH => Button::A,
        BUTTON_EAST => Button::B,
        BUTTON_WEST => Button::X,
        BUTTON_NORTH => Button::Y,
        BUTTON_BACK => Button::Back,
        BUTTON_GUIDE => Button::Guide,
        BUTTON_START => Button::Start,
        BUTTON_LEFT_STICK => Button::LeftStick,
        BUTTON_RIGHT_STICK => Button::RightStick,
        BUTTON_LEFT_SHOULDER => Button::LeftShoulder,
        BUTTON_RIGHT_SHOULDER => Button::RightShoulder,
        BUTTON_DPAD_UP => Button::DPadUp,
        BUTTON_DPAD_DOWN => Button::DPadDown,
        BUTTON_DPAD_LEFT => Button::DPadLeft,
        BUTTON_DPAD_RIGHT => Button::DPadRight,
        _ => return None,
    })
}

/// Maps SDL's `i16` axis range onto `[-1.0, 1.0]`.
///
/// `i16::MIN` divided by `i16::MAX` is slightly below `-1.0`, so the result is
/// clamped: consumers may rely on the documented range.
fn normalize(value: i16) -> f32 {
    (f32::from(value) / f32::from(i16::MAX)).clamp(-1.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_state_is_disconnected_and_neutral() {
        let state = GamepadState::new();

        assert!(!state.is_connected());
        assert_eq!(state.name(), None);
        assert_eq!(state.instance_id(), None);
        assert_eq!(state.axis(AXIS_LEFT_X), 0.0);
        assert!(!state.is_down(BUTTON_SOUTH));
        assert_eq!(state._reserved, [0; 4]);
    }

    #[test]
    fn axis_normalisation_maps_the_full_range() {
        assert_eq!(normalize(0), 0.0);
        assert_eq!(normalize(i16::MAX), 1.0);
        assert_eq!(normalize(i16::MIN), -1.0);
        assert!((normalize(16_384) - 0.5).abs() < 1e-4);
    }

    #[test]
    fn known_axes_and_buttons_map_to_sdl() {
        assert_eq!(to_sdl_axis(AXIS_LEFT_X), Some(Axis::LeftX));
        assert_eq!(to_sdl_axis(AXIS_RIGHT_Y), Some(Axis::RightY));
        assert_eq!(to_sdl_axis(AXIS_TRIGGER_RIGHT), Some(Axis::TriggerRight));
        assert_eq!(to_sdl_axis(99), None);

        assert_eq!(to_sdl_button(BUTTON_SOUTH), Some(Button::A));
        assert_eq!(to_sdl_button(BUTTON_DPAD_RIGHT), Some(Button::DPadRight));
        assert_eq!(
            to_sdl_button(BUTTON_RIGHT_SHOULDER),
            Some(Button::RightShoulder)
        );
        assert_eq!(to_sdl_button(99), None);
    }

    #[test]
    fn closing_a_disconnected_state_is_a_no_op() {
        let mut state = GamepadState::new();

        state.close();

        assert!(!state.is_connected());
        assert!(!state.close_if_instance(0));
    }
}
