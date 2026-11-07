//! Game controller state for the first connected controller.
//!
//! The engine follows the "player 1 console" model: the first controller opened
//! by SDL2 is player one, and its state is mirrored here.

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

/// Bottom face button (`A` on Xbox, `Cross` on PlayStation).
pub const BUTTON_SOUTH: GamepadButton = 0;
/// Right face button (`B` on Xbox, `Circle` on PlayStation).
pub const BUTTON_EAST: GamepadButton = 1;

/// State of the first connected controller.
#[repr(C)]
#[derive(Debug, Default)]
pub struct GamepadState {
    _reserved: [u32; 4],
}

impl GamepadState {
    /// Creates a state with no controller connected.
    pub const fn new() -> Self {
        Self { _reserved: [0; 4] }
    }

    /// Returns `true` if a controller is currently open.
    ///
    /// Implemented in phase 3.
    pub fn is_connected(&self) -> bool {
        todo!("fase 3: gamepad is_connected")
    }

    /// Returns the normalized position of `axis` in `[-1.0, 1.0]`.
    ///
    /// Sticks rest at `0.0`; triggers rest at `-1.0`, matching SDL2.
    ///
    /// Implemented in phase 3.
    pub fn axis(&self, _axis: GamepadAxis) -> f32 {
        todo!("fase 3: gamepad axis")
    }

    /// Returns `true` while `button` is held down.
    ///
    /// Implemented in phase 3.
    pub fn is_down(&self, _button: GamepadButton) -> bool {
        todo!("fase 3: gamepad is_down")
    }
}
