//! The chase camera.
//!
//! The engine ships [`giochi_engine_3d::FollowCamera`], and it is the right shape —
//! exponential smoothing, look-ahead along the velocity — but it places the eye at
//! `target + (0, height, distance)`: behind the target *in world `+Z`*, not behind it
//! along the direction it is facing. That is exactly right for a walker whose movement is
//! world-relative and wrong for a car that can face any way, so the racer has its own
//! heading-aware camera. Everything else is borrowed: [`blend_factor`] is the engine's
//! frame-rate-independent blend, so 30 and 144 fps behave the same.

use giochi_engine_3d::camera::blend_factor;
use giochi_engine_3d::math3d::{Camera3D, Vec3};

/// How the camera trails the car.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChaseConfig {
    /// How far behind the car the eye sits at rest, in metres.
    pub distance: f32,
    /// How far above the car, in metres.
    pub height: f32,
    /// How quickly the eye catches up. Higher is snappier.
    pub stiffness: f32,
    /// How far ahead of the car the camera looks, in metres.
    pub look_ahead: f32,
    /// Extra distance at top speed, as a fraction of [`ChaseConfig::distance`].
    ///
    /// Pulling back as the car speeds up is the oldest trick in the racing-camera book:
    /// it widens the view exactly when more of the road needs to be on screen.
    pub distance_gain: f32,
    /// Vertical field of view, in radians. Widens with speed by the same amount.
    pub fov_y: f32,
}

impl Default for ChaseConfig {
    fn default() -> Self {
        Self {
            distance: 7.5,
            height: 3.0,
            stiffness: 7.0,
            look_ahead: 6.0,
            distance_gain: 0.35,
            fov_y: 65.0_f32.to_radians(),
        }
    }
}

/// A camera that sits behind whatever the car is facing.
#[derive(Debug, Clone, PartialEq)]
pub struct ChaseCamera {
    config: ChaseConfig,
    eye: Vec3,
    look: Vec3,
    started: bool,
}

impl ChaseCamera {
    /// A camera that has not been placed yet; the first update snaps.
    pub fn new(config: ChaseConfig) -> Self {
        Self {
            config,
            eye: Vec3::ZERO,
            look: Vec3::ZERO,
            started: false,
        }
    }

    /// The tuning in use.
    pub fn config(&self) -> ChaseConfig {
        self.config
    }

    /// The current eye.
    pub fn eye(&self) -> Vec3 {
        self.eye
    }

    /// The current look-at point.
    pub fn look_at(&self) -> Vec3 {
        self.look
    }

    /// Whether the camera has been placed.
    pub fn started(&self) -> bool {
        self.started
    }

    /// Where the camera wants to be, for a car at `target` facing `forward`.
    ///
    /// `forward` is flattened and normalised; a car pointing straight up (which cannot
    /// happen with locked rotations, but a caller could still pass it) falls back to
    /// `+Z` instead of producing a NaN.
    pub fn desired(&self, target: Vec3, forward: Vec3, speed_ratio: f32) -> (Vec3, Vec3) {
        let target = target.to_glam();
        let mut forward = forward.to_glam();
        forward.y = 0.0;
        let forward = if forward.length_squared() > 1e-6 {
            forward.normalize()
        } else {
            glam::Vec3::Z
        };
        let ratio = speed_ratio.clamp(0.0, 1.5);
        let distance = self.config.distance * (1.0 + self.config.distance_gain * ratio);
        let height = self.config.height + 0.25 * ratio;
        let look = target + forward * self.config.look_ahead;
        let eye = target - forward * distance + glam::Vec3::Y * height;
        (Vec3::from_glam(eye), Vec3::from_glam(look))
    }

    /// Places the camera exactly where it wants to be.
    ///
    /// What a respawn or a race start needs: a camera that eases in from wherever it was
    /// is a camera the player watches instead of the road.
    pub fn snap(&mut self, target: Vec3, forward: Vec3, speed_ratio: f32) {
        let (eye, look) = self.desired(target, forward, speed_ratio);
        self.eye = eye;
        self.look = look;
        self.started = true;
    }

    /// Advances one frame towards the desired place.
    pub fn update(&mut self, target: Vec3, forward: Vec3, speed_ratio: f32, dt: f32) {
        if !self.started {
            self.snap(target, forward, speed_ratio);
            return;
        }
        let (eye, look) = self.desired(target, forward, speed_ratio);
        let blend = blend_factor(self.config.stiffness, dt);
        self.eye = lerp(self.eye, eye, blend);
        self.look = lerp(self.look, look, blend);
    }

    /// This camera as the renderer's camera, for a `width`×`height` drawable.
    pub fn to_camera3d(&self, width: u32, height: u32, speed_ratio: f32) -> Camera3D {
        let aspect = if height == 0 {
            1.0
        } else {
            width as f32 / height as f32
        };
        let mut camera = Camera3D::from_parts(self.eye, self.look, Vec3::UP);
        camera.fov_y = self.config.fov_y + 0.12 * speed_ratio.clamp(0.0, 1.0);
        camera.aspect = aspect.max(0.1);
        // 300 m of far plane is four times the longest straight in the shipped tracks;
        // a 2 cm near plane keeps the car's own nose out of the clip.
        camera.near_plane = 0.02;
        camera.far_plane = 300.0;
        camera
    }
}

fn lerp(from: Vec3, to: Vec3, t: f32) -> Vec3 {
    let from = from.to_glam();
    let to = to.to_glam();
    Vec3::from_glam(from + (to - from) * t.clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TARGET: Vec3 = Vec3::new(10.0, 0.5, -4.0);

    fn camera() -> ChaseCamera {
        ChaseCamera::new(ChaseConfig::default())
    }

    #[test]
    fn the_eye_sits_behind_the_car_not_behind_it_in_world_z() {
        let camera = camera();
        // Facing +X: the eye has to be at smaller x, not at smaller z.
        let (eye, _) = camera.desired(TARGET, Vec3::new(1.0, 0.0, 0.0), 0.0);
        assert!((eye.to_glam().x - (10.0 - 7.5)).abs() < 1e-4, "{eye:?}");
        assert!((eye.to_glam().z - (-4.0)).abs() < 1e-4, "{eye:?}");
        // 3 m above the *car*, which is at y = 0.5.
        assert!((eye.to_glam().y - (0.5 + 3.0)).abs() < 1e-4, "{eye:?}");
    }

    #[test]
    fn the_eye_is_behind_a_car_facing_plus_z_too() {
        let camera = camera();
        let (eye, _) = camera.desired(TARGET, Vec3::new(0.0, 0.0, 1.0), 0.0);
        assert!((eye.to_glam().z - (-4.0 - 7.5)).abs() < 1e-4, "{eye:?}");
        assert!((eye.to_glam().x - 10.0).abs() < 1e-4, "{eye:?}");
    }

    #[test]
    fn the_look_at_point_leads_the_car() {
        let camera = camera();
        let (_, look) = camera.desired(TARGET, Vec3::new(0.0, 0.0, 1.0), 0.0);
        assert!((look.to_glam().z - (-4.0 + 6.0)).abs() < 1e-4, "{look:?}");
    }

    #[test]
    fn speed_pulls_the_camera_back_and_raises_it() {
        let camera = camera();
        let (slow, _) = camera.desired(TARGET, Vec3::new(0.0, 0.0, 1.0), 0.0);
        let (fast, _) = camera.desired(TARGET, Vec3::new(0.0, 0.0, 1.0), 1.0);
        let slow_distance = (slow.to_glam() - TARGET.to_glam()).length();
        let fast_distance = (fast.to_glam() - TARGET.to_glam()).length();
        assert!(fast_distance > slow_distance);
        assert!(fast.to_glam().y > slow.to_glam().y);
    }

    #[test]
    fn the_first_update_snaps_instead_of_flying_in_from_the_origin() {
        let mut camera = camera();
        assert!(!camera.started());
        camera.update(TARGET, Vec3::new(0.0, 0.0, 1.0), 0.0, 1.0 / 60.0);
        assert!(camera.started());
        let (eye, _) = camera.desired(TARGET, Vec3::new(0.0, 0.0, 1.0), 0.0);
        let distance = (camera.eye().to_glam() - eye.to_glam()).length();
        assert!(distance < 1e-4, "snapped exactly: {distance}");
    }

    #[test]
    fn the_camera_converges_without_overshooting() {
        let mut camera = camera();
        camera.snap(Vec3::ZERO, Vec3::new(0.0, 0.0, 1.0), 0.0);
        let far_target = Vec3::new(100.0, 0.5, 0.0);
        let mut previous = (camera.eye().to_glam() - far_target.to_glam()).length();
        for _ in 0..180 {
            camera.update(far_target, Vec3::new(1.0, 0.0, 0.0), 0.0, 1.0 / 60.0);
            let (eye, _) = camera.desired(far_target, Vec3::new(1.0, 0.0, 0.0), 0.0);
            let distance = (camera.eye().to_glam() - eye.to_glam()).length();
            assert!(distance <= previous + 1e-3, "it went backwards");
            previous = distance;
        }
        assert!(previous < 0.1, "it arrived: {previous}");
    }

    #[test]
    fn the_smoothing_is_frame_rate_independent() {
        let mut thirty = camera();
        let mut one_forty_four = camera();
        thirty.snap(Vec3::ZERO, Vec3::new(0.0, 0.0, 1.0), 0.0);
        one_forty_four.snap(Vec3::ZERO, Vec3::new(0.0, 0.0, 1.0), 0.0);
        let target = Vec3::new(50.0, 0.5, 0.0);
        for _ in 0..30 {
            thirty.update(target, Vec3::new(1.0, 0.0, 0.0), 0.0, 1.0 / 30.0);
        }
        for _ in 0..144 {
            one_forty_four.update(target, Vec3::new(1.0, 0.0, 0.0), 0.0, 1.0 / 144.0);
        }
        let difference = (thirty.eye().to_glam() - one_forty_four.eye().to_glam()).length();
        assert!(difference < 5.0, "30 vs 144 fps differ by {difference} m");
    }

    #[test]
    fn a_car_pointing_straight_up_falls_back_to_plus_z_instead_of_nan() {
        let camera = camera();
        let (eye, look) = camera.desired(TARGET, Vec3::new(0.0, 1.0, 0.0), 0.0);
        assert!(eye.is_finite(), "{eye:?}");
        assert!(look.is_finite(), "{look:?}");
        assert!((eye.to_glam().z - (-4.0 - 7.5)).abs() < 1e-4);
    }

    #[test]
    fn the_renderer_camera_has_the_aspect_and_the_projection_it_needs() {
        let mut camera = camera();
        camera.snap(TARGET, Vec3::new(0.0, 0.0, 1.0), 0.0);
        let camera3d = camera.to_camera3d(1600, 900, 0.0);
        assert!((camera3d.aspect - 1600.0 / 900.0).abs() < 1e-4);
        assert!(camera3d.near_plane > 0.0);
        assert!(camera3d.far_plane > camera3d.near_plane);
        assert!(camera3d.fov_y > 0.5 && camera3d.fov_y < 1.5);
    }

    #[test]
    fn the_field_of_view_widens_a_little_with_speed() {
        let mut camera = camera();
        camera.snap(TARGET, Vec3::new(0.0, 0.0, 1.0), 0.0);
        let slow = camera.to_camera3d(800, 600, 0.0);
        let fast = camera.to_camera3d(800, 600, 1.0);
        assert!(fast.fov_y > slow.fov_y);
        assert!(
            fast.fov_y - slow.fov_y < 0.3,
            "a subtle change, not a fisheye"
        );
    }

    #[test]
    fn a_zero_height_drawable_does_not_divide_by_zero() {
        let mut camera = camera();
        camera.snap(TARGET, Vec3::new(0.0, 0.0, 1.0), 0.0);
        let camera3d = camera.to_camera3d(800, 0, 0.0);
        assert!(camera3d.aspect.is_finite() && camera3d.aspect > 0.0);
    }

    #[test]
    fn a_negative_delta_does_not_move_the_camera() {
        let mut camera = camera();
        camera.snap(Vec3::ZERO, Vec3::new(0.0, 0.0, 1.0), 0.0);
        let before = camera.eye();
        camera.update(
            Vec3::new(50.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            0.0,
            -1.0,
        );
        assert_eq!(camera.eye(), before);
    }
}
