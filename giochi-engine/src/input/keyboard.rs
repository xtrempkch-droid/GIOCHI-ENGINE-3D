//! Keyboard state with per-frame edge detection.

use std::collections::HashSet;

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
/// `M`.
pub const KEY_M: KeyCode = 109;
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
///
/// Three views of the same keys are tracked:
///
/// * **down** — held right now;
/// * **pressed** — went down during the frame that just ended;
/// * **released** — went up during the frame that just ended.
///
/// A key that is pressed *and* released between two frames reports **both** edges
/// for that frame. Dropping the press would make a fast tap invisible to a menu or
/// a jump trigger.
///
/// [`KeyboardState::begin_frame`] must be called once per frame, before the SDL
/// events are applied; [`crate::input::Input::update`] does that.
///
/// The sets are private: no `HashSet` is visible to users, only the query methods.
#[repr(C)]
#[derive(Debug, Default)]
pub struct KeyboardState {
    down: HashSet<KeyCode>,
    pressed: HashSet<KeyCode>,
    released: HashSet<KeyCode>,
    _reserved: [u32; 4],
}

impl KeyboardState {
    /// Creates a state in which no key is down.
    pub fn new() -> Self {
        Self::default()
    }

    /// Clears the one-frame edge sets. Call once per frame before applying events.
    pub fn begin_frame(&mut self) {
        self.pressed.clear();
        self.released.clear();
    }

    /// Marks `key` as held. Registers a "pressed" edge only on the transition.
    ///
    /// Repeated calls while the key is already down (SDL key repeat) are no-ops
    /// for edge detection, so a held key never re-triggers `is_pressed_this_frame`.
    pub fn press(&mut self, key: KeyCode) {
        if self.down.insert(key) {
            self.released.remove(&key);
            self.pressed.insert(key);
        }
    }

    /// Marks `key` as no longer held. Registers a "released" edge only on the
    /// transition; releasing a key that was not down does nothing.
    ///
    /// The "pressed" edge is deliberately **not** cleared: a press and a release
    /// inside the same frame must both be observable.
    pub fn release(&mut self, key: KeyCode) {
        if self.down.remove(&key) {
            self.released.insert(key);
        }
    }

    /// Forgets every key and every edge.
    pub fn reset(&mut self) {
        self.down.clear();
        self.begin_frame();
    }

    /// Returns `true` while `key` is held down.
    pub fn is_down(&self, key: KeyCode) -> bool {
        self.down.contains(&key)
    }

    /// Returns `true` only during the frame in which `key` went down.
    pub fn is_pressed_this_frame(&self, key: KeyCode) -> bool {
        self.pressed.contains(&key)
    }

    /// Returns `true` only during the frame in which `key` went up.
    pub fn is_released_this_frame(&self, key: KeyCode) -> bool {
        self.released.contains(&key)
    }

    /// Number of keys currently held.
    pub fn down_count(&self) -> usize {
        self.down.len()
    }

    /// Returns `true` when any key is held.
    pub fn any_down(&self) -> bool {
        !self.down.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_state_has_everything_released() {
        let state = KeyboardState::new();

        assert!(!state.is_down(KEY_W));
        assert!(!state.is_pressed_this_frame(KEY_W));
        assert!(!state.is_released_this_frame(KEY_W));
        assert!(!state.any_down());
        assert_eq!(state.down_count(), 0);
    }

    #[test]
    fn press_sets_down_and_pressed() {
        let mut state = KeyboardState::new();

        state.press(KEY_W);

        assert!(state.is_down(KEY_W));
        assert!(state.is_pressed_this_frame(KEY_W));
        assert!(!state.is_released_this_frame(KEY_W));
        assert_eq!(state.down_count(), 1);
    }

    #[test]
    fn pressed_edge_lasts_exactly_one_frame() {
        let mut state = KeyboardState::new();
        state.press(KEY_W);

        state.begin_frame();

        assert!(state.is_down(KEY_W), "the key is still held");
        assert!(
            !state.is_pressed_this_frame(KEY_W),
            "the pressed edge must be cleared"
        );
    }

    #[test]
    fn key_repeat_does_not_retrigger_the_pressed_edge() {
        let mut state = KeyboardState::new();
        state.press(KEY_W);
        state.begin_frame();

        state.press(KEY_W);

        assert!(state.is_down(KEY_W));
        assert!(!state.is_pressed_this_frame(KEY_W));
    }

    #[test]
    fn release_sets_released_and_clears_down() {
        let mut state = KeyboardState::new();
        state.press(KEY_SPACE);
        state.begin_frame();

        state.release(KEY_SPACE);

        assert!(!state.is_down(KEY_SPACE));
        assert!(state.is_released_this_frame(KEY_SPACE));
        assert!(!state.is_pressed_this_frame(KEY_SPACE));
    }

    #[test]
    fn released_edge_lasts_exactly_one_frame() {
        let mut state = KeyboardState::new();
        state.press(KEY_SPACE);
        state.release(KEY_SPACE);

        state.begin_frame();

        assert!(!state.is_released_this_frame(KEY_SPACE));
        assert!(!state.is_down(KEY_SPACE));
    }

    #[test]
    fn releasing_a_key_that_is_not_down_is_a_no_op() {
        let mut state = KeyboardState::new();

        state.release(KEY_A);

        assert!(!state.is_released_this_frame(KEY_A));
        assert!(!state.any_down());
    }

    #[test]
    fn multiple_keys_are_tracked_independently() {
        let mut state = KeyboardState::new();
        state.press(KEY_W);
        state.press(KEY_D);

        state.begin_frame();
        state.release(KEY_W);

        assert!(!state.is_down(KEY_W));
        assert!(state.is_down(KEY_D));
        assert!(state.is_released_this_frame(KEY_W));
        assert_eq!(state.down_count(), 1);
    }

    #[test]
    fn press_then_release_within_one_frame_reports_both_edges() {
        let mut state = KeyboardState::new();

        state.press(KEY_SPACE);
        state.release(KEY_SPACE);

        // A press and a release in the same frame are both observable, which is
        // what a "tap" detector needs.
        assert!(state.is_pressed_this_frame(KEY_SPACE));
        assert!(state.is_released_this_frame(KEY_SPACE));
        assert!(!state.is_down(KEY_SPACE));
    }

    #[test]
    fn reset_forgets_everything() {
        let mut state = KeyboardState::new();
        state.press(KEY_W);
        state.press(KEY_A);

        state.reset();

        assert!(!state.any_down());
        assert!(!state.is_pressed_this_frame(KEY_W));
        assert_eq!(state.down_count(), 0);
    }
}
