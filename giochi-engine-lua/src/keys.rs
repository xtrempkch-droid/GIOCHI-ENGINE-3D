//! Named key, mouse, axis and button codes.
//!
//! A script should never type `1073741904` to mean "left arrow". The literals come
//! from the engine's own constants where they exist
//! (`giochi_engine::input::keyboard`/`mouse`/`gamepad`), so the binding cannot drift
//! from the Rust API.
//!
//! ```lua
//! if input:key_down(giochi.keys.d) then ... end
//! if input:key_pressed(giochi.keys.space) then ... end
//! if input:mouse_down(giochi.mouse.left) then ... end
//! local steer = input:axis(giochi.axes.left_x)
//! ```

use giochi_engine::input::gamepad as pad;
use giochi_engine::input::keyboard as kb;
use giochi_engine::input::mouse as m;

/// Every named key code, in the order it is inserted into the Lua table.
///
/// Letters and digits are generated, because SDL keycodes for them are their ASCII
/// values; the rest are the engine's own constants plus the few SDL names a menu
/// needs.
pub(crate) fn keys() -> Vec<(&'static str, u32)> {
    let mut table = Vec::with_capacity(64);

    for letter in b'a'..=b'z' {
        table.push((LETTERS[(letter - b'a') as usize], u32::from(letter)));
    }
    for digit in b'0'..=b'9' {
        table.push((DIGITS[(digit - b'0') as usize], u32::from(digit)));
    }

    table.extend_from_slice(&[
        ("escape", kb::KEY_ESCAPE),
        ("space", kb::KEY_SPACE),
        ("enter", 13),
        ("return", 13),
        ("tab", 9),
        ("backspace", 8),
        ("left", kb::KEY_LEFT),
        ("right", kb::KEY_RIGHT),
        ("up", kb::KEY_UP),
        ("down", kb::KEY_DOWN),
    ]);
    table
}

const LETTERS: [&str; 26] = [
    "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s",
    "t", "u", "v", "w", "x", "y", "z",
];

const DIGITS: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];

/// Mouse button codes.
pub(crate) fn mouse_buttons() -> Vec<(&'static str, u32)> {
    vec![
        ("left", m::MOUSE_LEFT),
        ("middle", m::MOUSE_MIDDLE),
        ("right", m::MOUSE_RIGHT),
        ("x1", m::MOUSE_X1),
        ("x2", m::MOUSE_X2),
    ]
}

/// Gamepad axes, `[-1.0, 1.0]` with triggers resting at `-1.0` (SDL's convention).
pub(crate) fn axes() -> Vec<(&'static str, u32)> {
    vec![
        ("left_x", pad::AXIS_LEFT_X),
        ("left_y", pad::AXIS_LEFT_Y),
        ("right_x", pad::AXIS_RIGHT_X),
        ("right_y", pad::AXIS_RIGHT_Y),
        ("trigger_left", pad::AXIS_TRIGGER_LEFT),
        ("trigger_right", pad::AXIS_TRIGGER_RIGHT),
    ]
}

/// Gamepad buttons, named by position rather than by the letter printed on one
/// vendor's controller.
pub(crate) fn buttons() -> Vec<(&'static str, u32)> {
    vec![
        ("south", pad::BUTTON_SOUTH),
        ("east", pad::BUTTON_EAST),
        ("west", pad::BUTTON_WEST),
        ("north", pad::BUTTON_NORTH),
        ("back", pad::BUTTON_BACK),
        ("guide", pad::BUTTON_GUIDE),
        ("start", pad::BUTTON_START),
        ("left_stick", pad::BUTTON_LEFT_STICK),
        ("right_stick", pad::BUTTON_RIGHT_STICK),
        ("left_shoulder", pad::BUTTON_LEFT_SHOULDER),
        ("right_shoulder", pad::BUTTON_RIGHT_SHOULDER),
        ("dpad_up", pad::BUTTON_DPAD_UP),
        ("dpad_down", pad::BUTTON_DPAD_DOWN),
        ("dpad_left", pad::BUTTON_DPAD_LEFT),
        ("dpad_right", pad::BUTTON_DPAD_RIGHT),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_table_is_free_of_duplicate_names() {
        for table in [keys(), mouse_buttons(), axes(), buttons()] {
            let mut names: Vec<&str> = table.iter().map(|(name, _)| *name).collect();
            names.sort_unstable();
            let count = names.len();
            names.dedup();
            assert_eq!(names.len(), count, "duplicate name in a helper table");
        }
    }

    #[test]
    fn letters_and_digits_match_their_sdl_keycodes() {
        for (name, code) in keys() {
            if name.len() == 1 {
                let byte = name.as_bytes()[0];
                assert_eq!(code, u32::from(byte), "'{name}' must be its ASCII keycode");
            }
        }
    }

    #[test]
    fn the_named_keys_come_from_the_engine() {
        let table = keys();
        let find = |name: &str| table.iter().find(|(n, _)| *n == name).map(|(_, c)| *c);
        assert_eq!(find("space"), Some(kb::KEY_SPACE));
        assert_eq!(find("escape"), Some(kb::KEY_ESCAPE));
        assert_eq!(find("left"), Some(kb::KEY_LEFT));
        // The letters the engine names explicitly resolve to the same code.
        assert_eq!(find("w"), Some(kb::KEY_W));
        assert_eq!(find("m"), Some(kb::KEY_M));
    }
}
