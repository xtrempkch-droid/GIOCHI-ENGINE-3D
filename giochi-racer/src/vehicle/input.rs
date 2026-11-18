//! What the driver is asking the car to do.
//!
//! The gamepad comes first (R5): this is a handheld console, and the plan's control
//! mapping is RT/LT/stick/B/Start. The keyboard is the fallback and needs no
//! configuration — but a *plugged-in* idler does not lock the keyboard out, because a
//! controller left connected to the console while the player uses the keyboard is the
//! normal case on a dev machine. The rule is: whichever source is actually producing
//! input this frame.
//!
//! # Dead zone
//!
//! 10 % on the stick, as the plan asks. A worn analogue stick rests anywhere within a few
//! percent of centre, and without a dead zone the car creeps to one side with nobody
//! touching it.

use giochi_engine::input::gamepad::{
    AXIS_LEFT_X, AXIS_TRIGGER_LEFT, AXIS_TRIGGER_RIGHT, BUTTON_EAST, BUTTON_START,
};
use giochi_engine::input::keyboard::{
    KEY_A, KEY_D, KEY_DOWN, KEY_ESCAPE, KEY_LEFT, KEY_RIGHT, KEY_S, KEY_SPACE, KEY_UP, KEY_W,
};
use giochi_engine::input::Input;

/// Stick values inside this are read as zero.
pub const DEAD_ZONE: f32 = 0.10;

/// One frame of driver input, in the car's own language.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CarInput {
    /// `-1.0` full brake/reverse … `+1.0` full throttle.
    pub throttle: f32,
    /// `-1.0` full left … `+1.0` full right.
    pub steer: f32,
    /// Whether the handbrake is held.
    pub handbrake: bool,
    /// Whether pause was *pressed* this frame.
    pub pause: bool,
}

impl CarInput {
    /// Nothing pressed.
    pub const NEUTRAL: Self = Self {
        throttle: 0.0,
        steer: 0.0,
        handbrake: false,
        pause: false,
    };

    /// Clamps both axes into range, so a caller building input by hand cannot send the
    /// model a value outside `-1..=1`.
    pub fn clamped(self) -> Self {
        Self {
            throttle: if self.throttle.is_finite() {
                self.throttle.clamp(-1.0, 1.0)
            } else {
                0.0
            },
            steer: if self.steer.is_finite() {
                self.steer.clamp(-1.0, 1.0)
            } else {
                0.0
            },
            handbrake: self.handbrake,
            pause: self.pause,
        }
    }

    /// Whether the driver is asking for anything at all.
    pub fn is_neutral(&self) -> bool {
        self.throttle == 0.0 && self.steer == 0.0 && !self.handbrake && !self.pause
    }
}

/// Reads the gamepad if it is producing input, otherwise the keyboard.
pub fn read_input(input: &Input) -> CarInput {
    let gamepad = input.gamepad();
    if gamepad.is_connected() {
        let throttle =
            trigger(gamepad.axis(AXIS_TRIGGER_RIGHT)) - trigger(gamepad.axis(AXIS_TRIGGER_LEFT));
        let steer = apply_dead_zone(gamepad.axis(AXIS_LEFT_X));
        let handbrake = gamepad.is_down(BUTTON_EAST);
        let pause = gamepad.is_down(BUTTON_START);
        if throttle != 0.0 || steer != 0.0 || handbrake || pause {
            return CarInput {
                throttle,
                steer,
                handbrake,
                pause,
            }
            .clamped();
        }
    }
    read_keyboard(input)
}

/// WASD or the arrows, with Space as the handbrake and `Esc` as pause.
pub fn read_keyboard(input: &Input) -> CarInput {
    let forward = input.key_down(KEY_W) || input.key_down(KEY_UP);
    let back = input.key_down(KEY_S) || input.key_down(KEY_DOWN);
    let left = input.key_down(KEY_A) || input.key_down(KEY_LEFT);
    let right = input.key_down(KEY_D) || input.key_down(KEY_RIGHT);
    CarInput {
        throttle: f32::from(forward as u8) - f32::from(back as u8),
        steer: f32::from(right as u8) - f32::from(left as u8),
        handbrake: input.key_down(KEY_SPACE),
        pause: input.key_pressed(KEY_ESCAPE),
    }
}

/// A scripted driver, for `--auto` and for the unattended screenshot in the gates.
///
/// It holds full throttle and sweeps the steering left and right once a second, which is
/// enough to prove the car moves, turns and follows the track without a keyboard.
pub fn scripted_input(elapsed: f32) -> CarInput {
    CarInput {
        throttle: 1.0,
        steer: (elapsed * std::f32::consts::TAU / 4.0).sin() * 0.6,
        handbrake: false,
        pause: false,
    }
}

/// Turns a controller trigger's `-1..=1` resting/pressed range into `0..=1`.
fn trigger(axis: f32) -> f32 {
    ((axis + 1.0) * 0.5).clamp(0.0, 1.0)
}

/// Zeroes a stick axis inside [`DEAD_ZONE`] and rescales the rest back to `0..=1`.
fn apply_dead_zone(value: f32) -> f32 {
    if !value.is_finite() {
        return 0.0;
    }
    let magnitude = value.abs();
    if magnitude <= DEAD_ZONE {
        return 0.0;
    }
    let scaled = (magnitude - DEAD_ZONE) / (1.0 - DEAD_ZONE);
    scaled.clamp(0.0, 1.0) * value.signum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dead_zone_zeroes_a_resting_stick_and_keeps_the_ends() {
        assert_eq!(apply_dead_zone(0.0), 0.0);
        assert_eq!(apply_dead_zone(0.05), 0.0);
        assert_eq!(apply_dead_zone(-0.09), 0.0);
        assert!((apply_dead_zone(0.1) - 0.0).abs() < 1e-6);
        assert!((apply_dead_zone(1.0) - 1.0).abs() < 1e-6);
        assert!((apply_dead_zone(-1.0) + 1.0).abs() < 1e-6);
    }

    #[test]
    fn just_past_the_dead_zone_the_stick_starts_from_zero_not_from_ten_percent() {
        // Without the rescale the first millimetre of travel would jump the car to 10 %
        // of full lock.
        let value = apply_dead_zone(DEAD_ZONE + 0.01);
        assert!(value < 0.05, "{value}");
        assert!(value > 0.0);
    }

    #[test]
    fn a_non_finite_axis_reads_as_centred() {
        assert_eq!(apply_dead_zone(f32::NAN), 0.0);
        assert_eq!(apply_dead_zone(f32::INFINITY), 0.0);
    }

    #[test]
    fn a_resting_trigger_is_zero_and_a_pressed_one_is_one() {
        assert_eq!(trigger(-1.0), 0.0);
        assert_eq!(trigger(1.0), 1.0);
        assert!((trigger(0.0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn neutral_input_is_neutral() {
        assert!(CarInput::NEUTRAL.is_neutral());
        assert_eq!(CarInput::default(), CarInput::NEUTRAL);
        assert!(!CarInput {
            throttle: 0.5,
            ..CarInput::NEUTRAL
        }
        .is_neutral());
    }

    #[test]
    fn clamping_keeps_the_axes_in_range_and_floors_nan() {
        let input = CarInput {
            throttle: 4.0,
            steer: -9.0,
            handbrake: true,
            pause: false,
        }
        .clamped();
        assert_eq!(input.throttle, 1.0);
        assert_eq!(input.steer, -1.0);
        assert!(input.handbrake);

        let nan = CarInput {
            throttle: f32::NAN,
            steer: f32::NAN,
            ..CarInput::NEUTRAL
        }
        .clamped();
        assert_eq!(nan.throttle, 0.0);
        assert_eq!(nan.steer, 0.0);
    }

    #[test]
    fn the_scripted_driver_accelerates_and_sweeps() {
        let early = scripted_input(0.0);
        assert_eq!(early.throttle, 1.0);
        assert_eq!(early.steer, 0.0);
        let quarter = scripted_input(1.0);
        assert!(quarter.steer > 0.5, "{quarter:?}");
        let three_quarters = scripted_input(3.0);
        assert!(three_quarters.steer < -0.5, "{three_quarters:?}");
    }
}
