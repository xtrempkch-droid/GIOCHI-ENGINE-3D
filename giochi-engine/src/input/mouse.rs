//! Mouse state.

/// Mouse button identifier, matching SDL2's `SDL_BUTTON_*` values.
pub type MouseButton = u32;

/// Left button.
pub const MOUSE_LEFT: MouseButton = 1;
/// Middle button.
pub const MOUSE_MIDDLE: MouseButton = 2;
/// Right button.
pub const MOUSE_RIGHT: MouseButton = 3;
/// Extra button 1.
pub const MOUSE_X1: MouseButton = 4;
/// Extra button 2.
pub const MOUSE_X2: MouseButton = 5;

/// Mouse state for the current frame.
#[repr(C)]
#[derive(Debug, Default)]
pub struct MouseState {
    _reserved: [u32; 4],
}

impl MouseState {
    /// Creates a state with the cursor at the origin and no buttons down.
    pub const fn new() -> Self {
        Self { _reserved: [0; 4] }
    }

    /// Returns the cursor position in window coordinates as `(x, y)`.
    ///
    /// Implemented in phase 3.
    pub fn position(&self) -> (f32, f32) {
        todo!("fase 3: mouse position")
    }

    /// Returns `true` while `button` is held down.
    ///
    /// Implemented in phase 3.
    pub fn is_down(&self, _button: MouseButton) -> bool {
        todo!("fase 3: mouse is_down")
    }

    /// Vertical wheel delta accumulated for the current frame.
    ///
    /// Positive values mean the wheel was scrolled up.
    ///
    /// Implemented in phase 3.
    pub fn wheel_delta(&self) -> f32 {
        todo!("fase 3: mouse wheel_delta")
    }
}
