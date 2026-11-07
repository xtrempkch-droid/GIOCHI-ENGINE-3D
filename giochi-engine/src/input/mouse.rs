//! Mouse state with per-frame edge detection.

use std::collections::HashSet;

use crate::math::Vec2;

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
///
/// Positions and deltas are in window coordinates (physical pixels), i.e. the same
/// space as [`crate::render::RenderContext::draw_rect`].
#[repr(C)]
#[derive(Debug, Default)]
pub struct MouseState {
    position: Vec2,
    delta: Vec2,
    buttons: HashSet<MouseButton>,
    pressed: HashSet<MouseButton>,
    released: HashSet<MouseButton>,
    wheel: f32,
    _reserved: [u32; 4],
}

impl MouseState {
    /// Creates a state with the cursor at the origin, no buttons down and no
    /// wheel movement.
    pub fn new() -> Self {
        Self::default()
    }

    /// Clears the one-frame data (delta, wheel and the button edges).
    ///
    /// The absolute position and the held buttons are preserved: they describe
    /// state, not events.
    pub fn begin_frame(&mut self) {
        self.delta = Vec2::ZERO;
        self.wheel = 0.0;
        self.pressed.clear();
        self.released.clear();
    }

    /// Sets the absolute cursor position.
    ///
    /// The delta is **not** derived from consecutive positions: SDL already reports
    /// the motion of each event (`xrel`/`yrel`), and accumulating both would
    /// double-count. Feed motion through [`MouseState::add_delta`].
    pub fn set_position(&mut self, x: f32, y: f32) {
        self.position = Vec2::new(sanitize(x), sanitize(y));
    }

    /// Adds to the movement accumulated for this frame.
    pub fn add_delta(&mut self, dx: f32, dy: f32) {
        if dx.is_finite() && dy.is_finite() {
            self.delta = self.delta + Vec2::new(dx, dy);
        }
    }

    /// Marks `button` as held, registering a "pressed" edge on the transition.
    pub fn press(&mut self, button: MouseButton) {
        if self.buttons.insert(button) {
            self.released.remove(&button);
            self.pressed.insert(button);
        }
    }

    /// Marks `button` as no longer held, registering a "released" edge.
    pub fn release(&mut self, button: MouseButton) {
        if self.buttons.remove(&button) {
            self.pressed.remove(&button);
            self.released.insert(button);
        }
    }

    /// Adds `amount` to the vertical wheel delta for this frame.
    ///
    /// Positive values mean the wheel was scrolled up (away from the user).
    pub fn add_wheel(&mut self, amount: f32) {
        if amount.is_finite() {
            self.wheel += amount;
        }
    }

    /// Forgets buttons, position and every edge.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Returns the cursor position in window coordinates as `(x, y)`.
    pub fn position(&self) -> (f32, f32) {
        (self.position.x, self.position.y)
    }

    /// Returns the cursor position as a [`Vec2`].
    pub fn position_vec(&self) -> Vec2 {
        self.position
    }

    /// Movement accumulated during the current frame, in pixels.
    pub fn delta(&self) -> Vec2 {
        self.delta
    }

    /// Returns `true` while `button` is held down.
    pub fn is_down(&self, button: MouseButton) -> bool {
        self.buttons.contains(&button)
    }

    /// Returns `true` only during the frame in which `button` went down.
    pub fn is_pressed_this_frame(&self, button: MouseButton) -> bool {
        self.pressed.contains(&button)
    }

    /// Returns `true` only during the frame in which `button` went up.
    pub fn is_released_this_frame(&self, button: MouseButton) -> bool {
        self.released.contains(&button)
    }

    /// Vertical wheel delta accumulated for the current frame.
    ///
    /// Positive values mean the wheel was scrolled up.
    pub fn wheel_delta(&self) -> f32 {
        self.wheel
    }
}

/// Replaces non-finite coordinates with `0.0`.
fn sanitize(value: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_state_is_at_the_origin_with_no_buttons() {
        let state = MouseState::new();

        assert_eq!(state.position(), (0.0, 0.0));
        assert_eq!(state.delta(), Vec2::ZERO);
        assert_eq!(state.wheel_delta(), 0.0);
        assert!(!state.is_down(MOUSE_LEFT));
    }

    #[test]
    fn setting_the_position_does_not_invent_a_delta() {
        let mut state = MouseState::new();

        state.set_position(10.0, 20.0);
        state.set_position(15.0, 18.0);

        assert_eq!(state.position(), (15.0, 18.0));
        assert_eq!(state.delta(), Vec2::ZERO, "SDL reports the delta itself");
    }

    #[test]
    fn delta_resets_every_frame_but_the_position_does_not() {
        let mut state = MouseState::new();
        state.set_position(100.0, 100.0);

        state.begin_frame();

        assert_eq!(state.position(), (100.0, 100.0));
        assert_eq!(state.delta(), Vec2::ZERO);
    }

    #[test]
    fn delta_comes_only_from_add_delta() {
        let mut state = MouseState::new();

        state.set_position(10.0, 10.0);
        state.add_delta(3.0, -4.0);
        state.add_delta(1.0, 1.0);

        assert_eq!(state.position(), (10.0, 10.0));
        assert_eq!(state.delta(), Vec2::new(4.0, -3.0));
    }

    #[test]
    fn buttons_report_down_and_single_frame_edges() {
        let mut state = MouseState::new();

        state.press(MOUSE_RIGHT);
        assert!(state.is_down(MOUSE_RIGHT));
        assert!(state.is_pressed_this_frame(MOUSE_RIGHT));

        state.begin_frame();
        assert!(state.is_down(MOUSE_RIGHT));
        assert!(!state.is_pressed_this_frame(MOUSE_RIGHT));

        state.release(MOUSE_RIGHT);
        assert!(!state.is_down(MOUSE_RIGHT));
        assert!(state.is_released_this_frame(MOUSE_RIGHT));

        state.begin_frame();
        assert!(!state.is_released_this_frame(MOUSE_RIGHT));
    }

    #[test]
    fn buttons_are_independent() {
        let mut state = MouseState::new();
        state.press(MOUSE_LEFT);
        state.press(MOUSE_MIDDLE);
        state.release(MOUSE_LEFT);

        assert!(!state.is_down(MOUSE_LEFT));
        assert!(state.is_down(MOUSE_MIDDLE));
        assert!(!state.is_down(MOUSE_X2));
    }

    #[test]
    fn wheel_delta_accumulates_and_resets_per_frame() {
        let mut state = MouseState::new();

        state.add_wheel(1.0);
        state.add_wheel(0.5);
        assert_eq!(state.wheel_delta(), 1.5);

        state.begin_frame();
        assert_eq!(state.wheel_delta(), 0.0);
    }

    #[test]
    fn non_finite_input_does_not_poison_the_state() {
        let mut state = MouseState::new();

        state.set_position(f32::NAN, f32::INFINITY);
        state.add_delta(f32::NAN, 1.0);
        state.add_wheel(f32::NAN);

        assert_eq!(state.position(), (0.0, 0.0));
        assert_eq!(state.wheel_delta(), 0.0);
        assert!(state.delta().is_finite());
    }
}
