//! Input state: keyboard, mouse and the first connected game controller.
//!
//! [`Input`] is updated once per frame from the SDL2 event queue *before* the
//! scene's `update` runs, so scenes see a consistent snapshot.

pub mod gamepad;
pub mod keyboard;
pub mod mouse;

pub use gamepad::GamepadState;
pub use keyboard::{KeyCode, KeyboardState};
pub use mouse::{MouseButton, MouseState};

/// Aggregated input state for the current frame.
///
/// # C ABI
///
/// `Input` is never passed across the FFI boundary. C consumers query individual
/// values through `giochi_input_*` functions; timestamps and edge detection stay
/// inside the engine.
#[repr(C)]
#[derive(Debug, Default)]
pub struct Input {
    keyboard: KeyboardState,
    mouse: MouseState,
    gamepad: GamepadState,
    _reserved: [u32; 4],
}

impl Input {
    /// Creates an all-released input state.
    pub const fn new() -> Self {
        Self {
            keyboard: KeyboardState::new(),
            mouse: MouseState::new(),
            gamepad: GamepadState::new(),
            _reserved: [0; 4],
        }
    }

    /// Consumes one frame of SDL2 events and updates the edge-detection state.
    ///
    /// Implemented in phase 3.
    pub fn update(&mut self, _events: &[sdl2::event::Event]) {
        todo!("fase 3: processar eventos SDL2 em Input")
    }

    /// Read-only keyboard state.
    pub fn keyboard(&self) -> &KeyboardState {
        &self.keyboard
    }

    /// Read-only mouse state.
    pub fn mouse(&self) -> &MouseState {
        &self.mouse
    }

    /// Read-only gamepad state.
    pub fn gamepad(&self) -> &GamepadState {
        &self.gamepad
    }
}
