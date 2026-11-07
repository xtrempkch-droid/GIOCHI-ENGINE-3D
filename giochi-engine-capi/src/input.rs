//! Keyboard, mouse and gamepad queries.
//!
//! Input is a snapshot of the frame that just ended (ADR-019), valid only during
//! `update` and `render`. Inside `update` the engine lends the snapshot to the
//! scene, so these functions read it directly; outside a callback they borrow it
//! from the engine.

use crate::state::{self, guard};

const INPUT_HINT: &str = "input is only available inside an update callback (or outside a run); \
     a scene must not drive simulation from render";

/// `1` while `keycode` is held.
///
/// `keycode` is an SDL `SDLK_*` value.
///
/// # C ABI
///
/// ```c
/// int32_t giochi_input_key_down(uint32_t keycode);
/// ```
#[no_mangle]
pub extern "C" fn giochi_input_key_down(keycode: u32) -> i32 {
    guard(0, || {
        state::with_input(INPUT_HINT, |input| input.key_down(keycode))
            .map(i32::from)
            .unwrap_or(0)
    })
}

/// `1` during the frame in which `keycode` went down.
///
/// # C ABI
///
/// ```c
/// int32_t giochi_input_key_pressed(uint32_t keycode);
/// ```
#[no_mangle]
pub extern "C" fn giochi_input_key_pressed(keycode: u32) -> i32 {
    guard(0, || {
        state::with_input(INPUT_HINT, |input| input.key_pressed(keycode))
            .map(i32::from)
            .unwrap_or(0)
    })
}

/// `1` during the frame in which `keycode` went up.
///
/// # C ABI
///
/// ```c
/// int32_t giochi_input_key_released(uint32_t keycode);
/// ```
#[no_mangle]
pub extern "C" fn giochi_input_key_released(keycode: u32) -> i32 {
    guard(0, || {
        state::with_input(INPUT_HINT, |input| input.key_released(keycode))
            .map(i32::from)
            .unwrap_or(0)
    })
}

/// Mouse X position, in window pixels.
///
/// # C ABI
///
/// ```c
/// float giochi_input_mouse_x(void);
/// ```
#[no_mangle]
pub extern "C" fn giochi_input_mouse_x() -> f32 {
    guard(0.0, || {
        state::with_input(INPUT_HINT, |input| input.mouse().position().0).unwrap_or(0.0)
    })
}

/// Mouse Y position, in window pixels.
///
/// # C ABI
///
/// ```c
/// float giochi_input_mouse_y(void);
/// ```
#[no_mangle]
pub extern "C" fn giochi_input_mouse_y() -> f32 {
    guard(0.0, || {
        state::with_input(INPUT_HINT, |input| input.mouse().position().1).unwrap_or(0.0)
    })
}

/// `1` while `button` is held. Buttons use SDL's numbering (1 = left, 2 = middle,
/// 3 = right).
///
/// # C ABI
///
/// ```c
/// int32_t giochi_input_mouse_down(uint32_t button);
/// ```
#[no_mangle]
pub extern "C" fn giochi_input_mouse_down(button: u32) -> i32 {
    guard(0, || {
        state::with_input(INPUT_HINT, |input| input.mouse_down(button))
            .map(i32::from)
            .unwrap_or(0)
    })
}

/// Vertical wheel delta accumulated during the frame. Positive means scrolled up.
///
/// # C ABI
///
/// ```c
/// float giochi_input_mouse_wheel(void);
/// ```
#[no_mangle]
pub extern "C" fn giochi_input_mouse_wheel() -> f32 {
    guard(0.0, || {
        state::with_input(INPUT_HINT, |input| input.mouse().wheel_delta()).unwrap_or(0.0)
    })
}

/// `1` when a game controller is mapped and open.
///
/// # C ABI
///
/// ```c
/// int32_t giochi_gamepad_connected(void);
/// ```
#[no_mangle]
pub extern "C" fn giochi_gamepad_connected() -> i32 {
    guard(0, || {
        state::with_input(INPUT_HINT, |input| input.gamepad().is_connected())
            .map(i32::from)
            .unwrap_or(0)
    })
}

/// Normalised value of `axis`, clamped to `[-1.0, 1.0]`.
///
/// Sticks rest at `0.0`, triggers at `-1.0` (SDL's convention). An unmapped axis
/// reads `0.0`, never an error.
///
/// # C ABI
///
/// ```c
/// float giochi_gamepad_axis(uint32_t axis);
/// ```
#[no_mangle]
pub extern "C" fn giochi_gamepad_axis(axis: u32) -> f32 {
    guard(0.0, || {
        state::with_input(INPUT_HINT, |input| input.gamepad().axis(axis)).unwrap_or(0.0)
    })
}

/// `1` while `button` is held on the game controller.
///
/// # C ABI
///
/// ```c
/// int32_t giochi_gamepad_button_down(uint32_t button);
/// ```
#[no_mangle]
pub extern "C" fn giochi_gamepad_button_down(button: u32) -> i32 {
    guard(0, || {
        state::with_input(INPUT_HINT, |input| input.gamepad().is_down(button))
            .map(i32::from)
            .unwrap_or(0)
    })
}
