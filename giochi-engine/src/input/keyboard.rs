//! Keyboard state.

/// Key identifier: the SDL2 keycode value (`SDLK_*`) widened to `u32`.
///
/// Using the SDL keycode keeps the type a plain integer at the C ABI boundary
/// (`uint32_t`), so no enum layout has to be negotiated with other languages
/// (see `docs/DECISIONS.md`, ADR-009).
pub type KeyCode = u32;

/// `Escape`.
pub const KEY_ESCAPE: KeyCode = 27;
/// `Space`.
pub const KEY_SPACE: KeyCode = 32;
/// `A`.
pub const KEY_A: KeyCode = 97;
/// `D`.
pub const KEY_D: KeyCode = 100;
/// `S`.
pub const KEY_S: KeyCode = 115;
/// `W`.
pub const KEY_W: KeyCode = 119;
/// Left arrow.
pub const KEY_LEFT: KeyCode = 1_073_741_904;
/// Right arrow.
pub const KEY_RIGHT: KeyCode = 1_073_741_903;
/// Up arrow.
pub const KEY_UP: KeyCode = 1_073_741_906;
/// Down arrow.
pub const KEY_DOWN: KeyCode = 1_073_741_905;

/// Keyboard state for the current frame.
#[repr(C)]
#[derive(Debug, Default)]
pub struct KeyboardState {
    _reserved: [u32; 4],
}

impl KeyboardState {
    /// Creates a state in which no key is down.
    pub const fn new() -> Self {
        Self { _reserved: [0; 4] }
    }

    /// Returns `true` while `key` is held down.
    ///
    /// Implemented in phase 3.
    pub fn is_down(&self, _key: KeyCode) -> bool {
        todo!("fase 3: keyboard is_down")
    }

    /// Returns `true` only on the frame `key` went down.
    ///
    /// Implemented in phase 3.
    pub fn is_pressed_this_frame(&self, _key: KeyCode) -> bool {
        todo!("fase 3: keyboard is_pressed_this_frame")
    }

    /// Returns `true` only on the frame `key` went up.
    ///
    /// Implemented in phase 3.
    pub fn is_released_this_frame(&self, _key: KeyCode) -> bool {
        todo!("fase 3: keyboard is_released_this_frame")
    }
}
