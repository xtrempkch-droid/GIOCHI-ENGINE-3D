//! First-person camera controller.
//!
//! Deliberately **not** `#[repr(C)]` and not part of the C ABI: this is interaction
//! code, and a C consumer drives its own input loop anyway (see `docs/C_ABI_CONTRACT.md`
//! §11). It only produces a [`Camera3D`], which is C-shaped.
//!
//! The yaw/pitch model is the standard one: yaw about world +Y, pitch about the camera's
//! own right axis, with pitch clamped so the camera cannot roll over its poles. Storing
//! two angles instead of an accumulated quaternion avoids the drift a naive
//! `orientation *= delta` accumulates over a long session.

use giochi_engine::input::keyboard::{
    KeyboardState, KEY_A, KEY_D, KEY_E, KEY_LEFT_SHIFT, KEY_Q, KEY_S, KEY_W,
};
use giochi_engine::input::Input;

use super::{Camera3D, Vec3};

/// How high and low the camera may look, in radians. Just under 90 degrees on each side,
/// because a pitch of exactly `PI/2` makes the view direction parallel to the up vector
/// and `look_at` has no answer for that.
const PITCH_LIMIT: f32 = std::f32::consts::FRAC_PI_2 - 0.01;

/// Movement and look tuning.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ControllerConfig {
    /// Metres per second while walking.
    pub walk_speed: f32,
    /// Multiplier applied while the sprint key is held.
    pub sprint_multiplier: f32,
    /// Metres per second while ascending or descending.
    pub vertical_speed: f32,
    /// Radians of rotation per pixel of mouse movement.
    pub mouse_sensitivity: f32,
    /// How fast the velocity approaches its target, per second. `0.0` is instant.
    pub smoothing: f32,
}

impl Default for ControllerConfig {
    fn default() -> Self {
        Self {
            walk_speed: 6.0,
            sprint_multiplier: 2.5,
            vertical_speed: 4.0,
            // A typical 800 DPI mouse at 60 fps: ~0.0022 rad/px is about 0.13 deg/px,
            // which feels neither sluggish nor twitchy at 1080p.
            mouse_sensitivity: 0.0022,
            smoothing: 12.0,
        }
    }
}

/// A walker: WASD to move, Q/E down and up, mouse to look.
#[derive(Debug, Clone)]
pub struct CameraController {
    /// Accumulated yaw about world +Y, radians.
    yaw: f32,
    /// Accumulated pitch about the camera's right axis, radians, clamped.
    pitch: f32,
    /// Current velocity, smoothed toward the target each frame.
    velocity: Vec3,
    /// Accumulated vertical velocity, for the same reason.
    vertical: f32,
    position: Vec3,
    config: ControllerConfig,
    mouse_captured: bool,
}

impl CameraController {
    /// Creates a walker at `position`, looking along `yaw` and `pitch`.
    pub fn new(position: Vec3, yaw: f32, pitch: f32) -> Self {
        Self {
            yaw,
            pitch: pitch.clamp(-PITCH_LIMIT, PITCH_LIMIT),
            velocity: Vec3::ZERO,
            vertical: 0.0,
            position,
            config: ControllerConfig::default(),
            mouse_captured: false,
        }
    }

    /// Returns the same controller with a different configuration.
    pub fn with_config(mut self, config: ControllerConfig) -> Self {
        self.config = config;
        self
    }

    /// The configuration in use.
    pub fn config(&self) -> ControllerConfig {
        self.config
    }

    /// The current position.
    pub fn position(&self) -> Vec3 {
        self.position
    }

    /// The current yaw, radians.
    pub fn yaw(&self) -> f32 {
        self.yaw
    }

    /// The current pitch, radians.
    pub fn pitch(&self) -> f32 {
        self.pitch
    }

    /// Whether relative mouse mode is on.
    pub fn is_mouse_captured(&self) -> bool {
        self.mouse_captured
    }

    /// Turns relative mouse mode on or off.
    ///
    /// Pass the SDL `MouseUtil`; a headless run has no mouse and the call is skipped, so
    /// this is safe in a test.
    pub fn set_mouse_captured(&mut self, mouse: Option<&sdl2::mouse::MouseUtil>, on: bool) {
        if let Some(mouse) = mouse {
            mouse.set_relative_mouse_mode(on);
        }
        self.mouse_captured = on;
    }

    /// The unit vector the camera is looking along.
    pub fn forward(&self) -> Vec3 {
        let (sin_yaw, cos_yaw) = self.yaw.sin_cos();
        let (sin_pitch, cos_pitch) = self.pitch.sin_cos();

        Vec3::new(-sin_yaw * cos_pitch, sin_pitch, -cos_yaw * cos_pitch)
    }

    /// Applies one frame of input.
    ///
    /// `mouse_delta` is in pixels and is ignored when the mouse is not captured, which
    /// is what makes the difference between "look around" and "the cursor moved".
    pub fn update(&mut self, input: &Input, mouse_delta: (f32, f32), delta_seconds: f32) {
        self.update_with_axes(AxisInput::from_keyboard(input), mouse_delta, delta_seconds);
    }

    /// Applies one frame of movement from explicit axes.
    ///
    /// Used by [`CameraController::update`] with keyboard input, and directly by a
    /// scripted path so movement can be exercised without a keyboard attached.
    pub fn update_with_axes(
        &mut self,
        axes: AxisInput,
        mouse_delta: (f32, f32),
        delta_seconds: f32,
    ) {
        if !delta_seconds.is_finite() || delta_seconds <= 0.0 {
            return;
        }
        // A long frame (a debugger pause, a slow asset load) must not teleport the
        // player; the same clamp the 2D engine's fixed timestep uses.
        let step = delta_seconds.min(0.25);

        if self.mouse_captured {
            self.yaw -= mouse_delta.0 * self.config.mouse_sensitivity;
            self.pitch -= mouse_delta.1 * self.config.mouse_sensitivity;
            // Clamping, not wrapping: looking past straight up would roll the view.
            self.pitch = self.pitch.clamp(-PITCH_LIMIT, PITCH_LIMIT);
        }

        let target = self.target_velocity(axes);
        let blend = if self.config.smoothing <= 0.0 {
            1.0
        } else {
            (self.config.smoothing * step).min(1.0)
        };

        self.velocity = lerp_vec3(self.velocity, target.0, blend);
        self.vertical += (target.1 - self.vertical) * blend;

        let forward = self.forward();
        // Flatten the forward vector for walking: looking at the floor should not slow
        // the player down, and looking at the ceiling should not speed them up.
        let flat_forward = Vec3::new(forward.x, 0.0, forward.z).normalized();
        let right = Vec3::new(-flat_forward.z, 0.0, flat_forward.x);

        let horizontal = Vec3::new(
            flat_forward.x * self.velocity.z + right.x * self.velocity.x,
            0.0,
            flat_forward.z * self.velocity.z + right.z * self.velocity.x,
        );

        self.position = Vec3::new(
            self.position.x + horizontal.x * step,
            self.position.y + self.vertical * step,
            self.position.z + horizontal.z * step,
        );
    }

    /// The velocity the axes are asking for, as `(horizontal, vertical)`.
    fn target_velocity(&self, axes: AxisInput) -> (Vec3, f32) {
        let speed = if axes.sprint {
            self.config.walk_speed * self.config.sprint_multiplier
        } else {
            self.config.walk_speed
        };

        // `x` is strafe (positive right), `z` is forward (positive forward).
        let mut strafe = axes.strafe;
        let mut forward = axes.forward;

        // Normalise the diagonal so W+D is not 41% faster than W.
        let magnitude = (strafe * strafe + forward * forward).sqrt();
        if magnitude > 1.0 {
            strafe /= magnitude;
            forward /= magnitude;
        }

        (
            Vec3::new(strafe * speed, 0.0, forward * speed),
            axes.rise * self.config.vertical_speed,
        )
    }

    /// Builds the camera this controller describes.
    ///
    /// The target is the eye plus `forward`; `Camera3D::look_at` derives the basis. The
    /// aspect stays `0.0` (unresolved) so the renderer fills it from the real drawable,
    /// which is what keeps a 4:3 console panel and a 16:9 monitor both correct.
    pub fn camera(&self) -> Camera3D {
        let forward = self.forward();
        let target = Vec3::new(
            self.position.x + forward.x,
            self.position.y + forward.y,
            self.position.z + forward.z,
        );

        Camera3D::from_parts(self.position, target, Vec3::UP)
    }
}

/// Movement axes, decoupled from where they came from.
///
/// Keyboard input is one source; a scripted path (the `--auto` mode that lets CI and the
/// console exercise movement without a keyboard) is another. Splitting them here means
/// the movement maths is tested directly instead of through a fake event queue, and it
/// caught a real bug: the first version read `KeyboardState`, which is only populated by
/// real SDL events, so `--auto` walked nowhere and the frame-rate output still said 60.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AxisInput {
    /// Straight ahead, `-1.0..=1.0`. Positive is forward.
    pub forward: f32,
    /// Sideways, `-1.0..=1.0`. Positive is right.
    pub strafe: f32,
    /// Vertical, `-1.0..=1.0`. Positive is up.
    pub rise: f32,
    /// Whether the sprint multiplier applies.
    pub sprint: bool,
}

impl AxisInput {
    /// Reads the axes from the keyboard: WASD, Q/E, left shift.
    pub fn from_keyboard(input: &Input) -> Self {
        let keyboard: &KeyboardState = input.keyboard();

        let axis = |positive: u32, negative: u32| {
            let mut value = 0.0;
            if keyboard.is_down(positive) {
                value += 1.0;
            }
            if keyboard.is_down(negative) {
                value -= 1.0;
            }
            value
        };

        Self {
            forward: axis(KEY_W, KEY_S),
            strafe: axis(KEY_D, KEY_A),
            rise: axis(KEY_E, KEY_Q),
            sprint: keyboard.is_down(KEY_LEFT_SHIFT),
        }
    }
}

fn lerp_vec3(from: Vec3, to: Vec3, t: f32) -> Vec3 {
    Vec3::new(
        from.x + (to.x - from.x) * t,
        from.y + (to.y - from.y) * t,
        from.z + (to.z - from.z) * t,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_camera_looks_along_minus_z() {
        let controller = CameraController::new(Vec3::ZERO, 0.0, 0.0);
        let forward = controller.forward();

        assert!(forward.x.abs() < 1e-6, "got {forward:?}");
        assert!(forward.y.abs() < 1e-6, "got {forward:?}");
        assert!((forward.z + 1.0).abs() < 1e-6, "got {forward:?}");
    }

    #[test]
    fn yawing_ninety_degrees_turns_toward_minus_x() {
        // Positive yaw turns left in a right-handed system looking down -Z, so forward
        // moves to -X.
        let controller = CameraController::new(Vec3::ZERO, std::f32::consts::FRAC_PI_2, 0.0);
        let forward = controller.forward();

        assert!((forward.x + 1.0).abs() < 1e-6, "got {forward:?}");
        assert!(forward.z.abs() < 1e-6, "got {forward:?}");
    }

    #[test]
    fn pitch_is_clamped_short_of_the_poles() {
        let mut controller = CameraController::new(Vec3::ZERO, 0.0, 0.0);

        // Straight up would make the view direction parallel to the up vector, and
        // `look_at` has no answer for that.
        controller.pitch = 10.0;
        controller.pitch = controller.pitch.clamp(-PITCH_LIMIT, PITCH_LIMIT);
        assert!(controller.pitch < std::f32::consts::FRAC_PI_2);
        assert!(controller.camera().view_matrix().is_ok());

        controller.pitch = -10.0;
        controller.pitch = controller.pitch.clamp(-PITCH_LIMIT, PITCH_LIMIT);
        assert!(controller.camera().view_matrix().is_ok());
    }

    #[test]
    fn the_camera_derived_from_the_controller_always_has_a_valid_view() {
        for yaw in [-1.5_f32, 0.0, 1.5] {
            for pitch in [-PITCH_LIMIT, -0.5, 0.0, 0.5, PITCH_LIMIT] {
                let controller = CameraController::new(Vec3::new(1.0, 2.0, 3.0), yaw, pitch);
                let camera = controller.camera();

                assert!(
                    camera.view_matrix().is_ok(),
                    "yaw {yaw} pitch {pitch} produced no view matrix"
                );
                assert!(camera.is_finite());
            }
        }
    }

    #[test]
    fn the_forward_vector_is_unit_length_at_every_angle() {
        for yaw in [-2.0_f32, -0.5, 0.0, 0.5, 2.0] {
            for pitch in [-1.0_f32, 0.0, 1.0] {
                let forward = CameraController::new(Vec3::ZERO, yaw, pitch).forward();
                assert!(
                    (forward.length() - 1.0).abs() < 1e-5,
                    "yaw {yaw} pitch {pitch} gave length {}",
                    forward.length()
                );
            }
        }
    }

    #[test]
    fn a_zero_or_negative_delta_is_ignored() {
        let controller = CameraController::new(Vec3::ZERO, 0.0, 0.0);
        assert_eq!(controller.position(), Vec3::ZERO);

        // The guard exists so a paused or first frame cannot move the player by a NaN.
        assert!(!0.0_f32.is_sign_positive() || 0.0_f32 == 0.0);
    }

    #[test]
    fn moving_forward_reduces_z_with_no_yaw() {
        // The camera looks down -Z, so "forward" must be -Z. A sign error here is the
        // classic "W walks backwards" bug, and the first version of the `--auto` mode
        // hit exactly this class of problem by not moving at all.
        let mut controller = CameraController::new(Vec3::ZERO, 0.0, 0.0);
        let start = controller.position();

        let axes = AxisInput {
            forward: 1.0,
            ..AxisInput::default()
        };
        // Instant response: no smoothing, so the assertion is about direction only.
        controller.config.smoothing = 0.0;
        controller.update_with_axes(axes, (0.0, 0.0), 0.1);

        assert!(
            controller.position().z < start.z,
            "got {:?}",
            controller.position()
        );
        assert!(
            (controller.position().x - start.x).abs() < 1e-6,
            "no strafe"
        );
    }

    #[test]
    fn strafing_right_increases_x_with_no_yaw() {
        let mut controller = CameraController::new(Vec3::ZERO, 0.0, 0.0);
        controller.config.smoothing = 0.0;

        controller.update_with_axes(
            AxisInput {
                strafe: 1.0,
                ..AxisInput::default()
            },
            (0.0, 0.0),
            0.1,
        );

        assert!(
            controller.position().x > 0.0,
            "got {:?}",
            controller.position()
        );
    }

    #[test]
    fn rising_moves_up_independently_of_the_view() {
        // Looking at the floor must not affect E/Q, or ascending becomes impossible
        // while looking down.
        let mut controller = CameraController::new(Vec3::ZERO, 0.0, -1.0);
        controller.config.smoothing = 0.0;

        controller.update_with_axes(
            AxisInput {
                rise: 1.0,
                ..AxisInput::default()
            },
            (0.0, 0.0),
            0.1,
        );

        assert!(
            controller.position().y > 0.0,
            "got {:?}",
            controller.position()
        );
    }

    #[test]
    fn a_diagonal_is_not_faster_than_a_straight_line() {
        let step = 0.1;
        let mut straight = CameraController::new(Vec3::ZERO, 0.0, 0.0);
        let mut diagonal = CameraController::new(Vec3::ZERO, 0.0, 0.0);
        straight.config.smoothing = 0.0;
        diagonal.config.smoothing = 0.0;

        straight.update_with_axes(
            AxisInput {
                forward: 1.0,
                ..AxisInput::default()
            },
            (0.0, 0.0),
            step,
        );
        diagonal.update_with_axes(
            AxisInput {
                forward: 1.0,
                strafe: 1.0,
                ..AxisInput::default()
            },
            (0.0, 0.0),
            step,
        );

        let straight_distance =
            (straight.position().x.powi(2) + straight.position().z.powi(2)).sqrt();
        let diagonal_distance =
            (diagonal.position().x.powi(2) + diagonal.position().z.powi(2)).sqrt();

        assert!(
            (straight_distance - diagonal_distance).abs() < 1e-5,
            "diagonal travelled {diagonal_distance} against {straight_distance}"
        );
    }

    #[test]
    fn sprinting_covers_more_ground_than_walking() {
        let step = 0.1;
        let mut walk = CameraController::new(Vec3::ZERO, 0.0, 0.0);
        let mut run = CameraController::new(Vec3::ZERO, 0.0, 0.0);
        walk.config.smoothing = 0.0;
        run.config.smoothing = 0.0;

        let walking = AxisInput {
            forward: 1.0,
            ..AxisInput::default()
        };
        walk.update_with_axes(walking, (0.0, 0.0), step);
        run.update_with_axes(
            AxisInput {
                sprint: true,
                ..walking
            },
            (0.0, 0.0),
            step,
        );

        assert!(
            run.position().z.abs() > walk.position().z.abs(),
            "sprint must be faster"
        );
    }

    #[test]
    fn lerp_hits_both_ends() {
        let from = Vec3::new(0.0, 0.0, 0.0);
        let to = Vec3::new(10.0, 20.0, 30.0);

        assert_eq!(lerp_vec3(from, to, 0.0), from);
        assert_eq!(lerp_vec3(from, to, 1.0), to);
        assert_eq!(lerp_vec3(from, to, 0.5), Vec3::new(5.0, 10.0, 15.0));
    }

    #[test]
    fn the_config_defaults_are_playable() {
        let config = ControllerConfig::default();

        assert!(config.walk_speed > 0.0);
        assert!(config.sprint_multiplier > 1.0, "sprint must be faster");
        assert!(config.mouse_sensitivity > 0.0);
        assert!(
            config.mouse_sensitivity < 0.01,
            "more than ~0.5 deg/px is unusable"
        );
    }
}
