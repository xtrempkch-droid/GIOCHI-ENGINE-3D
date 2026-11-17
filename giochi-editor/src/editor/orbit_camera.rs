//! The viewport's own camera: orbit, pan and zoom around a target.
//!
//! This camera is deliberately **not** the scene's camera. A scene contains a `Camera`
//! node that a game plays through; the viewport camera is the editor's eye on that
//! scene, and moving it must never change what the game will see. Phase 31's acceptance
//! calls this out explicitly, so the two are separate values with no path from one to
//! the other.
//!
//! The parameterisation is the one every DCC tool uses: a target point, two angles and a
//! distance. Storing `yaw`/`pitch`/`distance` instead of a free position is what keeps
//! the horizon level and the camera outside the geometry no matter how much the user
//! drags.

use giochi_engine_3d::math3d::Mat4 as EngineMat4;
use glam::Vec3;

/// Closest the camera may get to its target.
pub const MIN_DISTANCE: f32 = 0.25;
/// Farthest the camera may get from its target.
pub const MAX_DISTANCE: f32 = 2_000.0;
/// Pitch limits, in radians. Just short of the poles: at exactly ±π/2 the up vector
/// becomes parallel to the view direction and `look_at` degenerates.
pub const PITCH_LIMIT: f32 = std::f32::consts::FRAC_PI_2 - 0.001;

/// The editor's eye on the scene.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OrbitCamera {
    /// Point the camera orbits and looks at.
    pub target: Vec3,
    /// Rotation around the world Y axis, radians.
    pub yaw: f32,
    /// Elevation above the XZ plane, radians, clamped to ±[`PITCH_LIMIT`].
    pub pitch: f32,
    /// Distance from `target`.
    pub distance: f32,
    /// Vertical field of view, radians.
    pub fov_y: f32,
    /// Near clip plane.
    pub near: f32,
    /// Far clip plane.
    pub far: f32,
}

impl Default for OrbitCamera {
    fn default() -> Self {
        // A three-quarter view of the origin, which shows all three axes of a cube
        // instead of collapsing one of them.
        Self {
            target: Vec3::new(0.0, 0.5, 0.0),
            yaw: std::f32::consts::FRAC_PI_4,
            pitch: 0.45,
            distance: 9.0,
            fov_y: std::f32::consts::FRAC_PI_3,
            near: 0.05,
            far: 2_000.0,
        }
    }
}

impl OrbitCamera {
    /// A camera framing `target` from `distance` away, at the default angles.
    pub fn looking_at(target: Vec3, distance: f32) -> Self {
        Self {
            target,
            distance: distance.clamp(MIN_DISTANCE, MAX_DISTANCE),
            ..Self::default()
        }
    }

    /// The eye position implied by the angles and the distance.
    pub fn eye(&self) -> Vec3 {
        let (sin_yaw, cos_yaw) = self.yaw.sin_cos();
        let (sin_pitch, cos_pitch) = self.pitch.sin_cos();
        self.target + Vec3::new(cos_pitch * sin_yaw, sin_pitch, cos_pitch * cos_yaw) * self.distance
    }

    /// The up vector. Always world-up: an orbit camera that rolled with the pitch makes
    /// the horizon tilt, and a level horizon is what makes a viewport readable.
    pub const fn up(&self) -> Vec3 {
        Vec3::Y
    }

    /// Drag with the right button: `dx`/`dy` in points.
    ///
    /// The sensitivity is per point, not per second: a drag that moves the mouse 100
    /// points must move the camera the same amount whether the frame rate is 30 or 300.
    pub fn orbit(&mut self, dx: f32, dy: f32) {
        const RADIANS_PER_POINT: f32 = 0.01;
        self.yaw -= dx * RADIANS_PER_POINT;
        self.pitch = (self.pitch + dy * RADIANS_PER_POINT).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    }

    /// Drag with the middle button: moves `target` in the camera's screen plane.
    ///
    /// `viewport_height` is what turns points into world units, so a drag moves the scene
    /// under the cursor at any zoom level and at any window size.
    pub fn pan(&mut self, dx: f32, dy: f32, viewport_height: f32) {
        let height = viewport_height.max(1.0);
        // World units per point at the target's depth.
        let units_per_point = 2.0 * self.distance * (self.fov_y * 0.5).tan() / height;
        let forward = (self.target - self.eye()).normalize_or_zero();
        let right = forward.cross(self.up()).normalize_or_zero();
        let up = right.cross(forward).normalize_or_zero();
        self.target += (-right * dx + up * dy) * units_per_point;
    }

    /// Scroll: positive `delta` zooms in.
    ///
    /// Multiplicative rather than additive, so one notch changes the view by the same
    /// proportion when the camera is far away and when it is close.
    pub fn zoom(&mut self, delta: f32) {
        if !delta.is_finite() || delta == 0.0 {
            return;
        }
        let factor = (-delta * 0.1).exp();
        self.distance = (self.distance * factor).clamp(MIN_DISTANCE, MAX_DISTANCE);
    }

    /// Frames an axis-aligned box, leaving a margin so the box does not touch the edges.
    pub fn frame_aabb(&mut self, min: [f32; 3], max: [f32; 3]) {
        let center = Vec3::new(
            (min[0] + max[0]) * 0.5,
            (min[1] + max[1]) * 0.5,
            (min[2] + max[2]) * 0.5,
        );
        let radius = Vec3::new(
            (max[0] - min[0]).abs(),
            (max[1] - min[1]).abs(),
            (max[2] - min[2]).abs(),
        )
        .length()
            * 0.5;
        self.target = center;
        // `radius / sin(fov/2)` is the distance at which a sphere of `radius` exactly
        // fills the vertical field of view; the margin makes it comfortable.
        let distance = (radius / (self.fov_y * 0.5).sin().max(1e-3)) * 1.6;
        self.distance = distance.clamp(MIN_DISTANCE, MAX_DISTANCE);
    }

    /// The view matrix.
    ///
    /// Built with the engine's own `Mat4`, not `glam`'s: glam 0.34 no longer ships
    /// `look_at_rh`/`perspective_rh`, and the 3D crate already owns validated versions of
    /// both — the ones its renderer and its `Camera3D` use. A degenerate camera therefore
    /// falls back to the identity instead of producing NaNs.
    pub fn view(&self) -> EngineMat4 {
        let eye = self.eye();
        EngineMat4::look_at(
            giochi_engine_3d::math3d::Vec3::new(eye.x, eye.y, eye.z),
            giochi_engine_3d::math3d::Vec3::new(self.target.x, self.target.y, self.target.z),
            giochi_engine_3d::math3d::Vec3::new(self.up().x, self.up().y, self.up().z),
        )
        .unwrap_or(EngineMat4::IDENTITY)
    }

    /// The projection matrix for an aspect ratio.
    pub fn projection(&self, aspect: f32) -> EngineMat4 {
        EngineMat4::perspective(self.fov_y, aspect.max(1e-3), self.near, self.far)
            .unwrap_or(EngineMat4::IDENTITY)
    }

    /// The combined matrix the viewport projects with, in the column-major layout the
    /// debug draw and the shaders expect.
    pub fn view_projection(&self, aspect: f32) -> [f32; 16] {
        (self.projection(aspect) * self.view()).to_cols_array()
    }

    /// The projection matrix as `glam`, for code that multiplies its own points.
    pub fn projection_glam(&self, aspect: f32) -> glam::Mat4 {
        glam::Mat4::from_cols_array(&self.projection(aspect).to_cols_array())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn the_eye_sits_at_the_distance_from_the_target() {
        for (yaw, pitch) in [(0.0, 0.0), (1.0, 0.3), (-2.0, -1.2), (0.5, PITCH_LIMIT)] {
            let camera = OrbitCamera {
                yaw,
                pitch,
                distance: 7.5,
                target: Vec3::new(1.0, 2.0, 3.0),
                ..OrbitCamera::default()
            };
            let offset = camera.eye() - camera.target;
            assert!(approx(offset.length(), 7.5), "{offset:?}");
        }
    }

    #[test]
    fn the_eye_is_above_the_target_when_pitch_is_positive() {
        let above = OrbitCamera {
            pitch: 0.5,
            ..OrbitCamera::default()
        };
        assert!(above.eye().y > above.target.y);
        let below = OrbitCamera {
            pitch: -0.5,
            ..OrbitCamera::default()
        };
        assert!(below.eye().y < below.target.y);
    }

    #[test]
    fn orbit_turns_and_the_pitch_never_reaches_the_pole() {
        let mut camera = OrbitCamera::default();
        let start = camera.eye();
        camera.orbit(40.0, 0.0);
        assert!((camera.eye() - start).length() > 0.1, "yaw moves the eye");
        assert!(
            camera.distance == OrbitCamera::default().distance,
            "orbiting does not change distance"
        );

        camera.orbit(0.0, 10_000.0);
        assert!(camera.pitch <= PITCH_LIMIT + 1e-6);
        assert!(camera.pitch >= -PITCH_LIMIT - 1e-6);
        camera.orbit(0.0, -20_000.0);
        assert!(camera.pitch >= -PITCH_LIMIT - 1e-6);
    }

    #[test]
    fn orbit_is_proportional_to_the_drag_not_to_time() {
        let mut a = OrbitCamera::default();
        let mut b = OrbitCamera::default();
        a.orbit(50.0, 0.0);
        for _ in 0..10 {
            b.orbit(5.0, 0.0);
        }
        assert!(approx(a.yaw, b.yaw), "{} vs {}", a.yaw, b.yaw);
    }

    #[test]
    fn pan_moves_the_target_in_the_view_plane() {
        let mut camera = OrbitCamera::default();
        let before = camera.target;
        camera.pan(10.0, 0.0, 900.0);
        let moved = camera.target - before;
        assert!(moved.length() > 1e-3, "panning moves the target");
        let view_direction = (camera.target - camera.eye()).normalize();
        assert!(
            moved.dot(view_direction).abs() < 1e-3,
            "pan must stay perpendicular to the view direction"
        );
        assert!(approx(camera.distance, OrbitCamera::default().distance));
    }

    #[test]
    fn pan_is_larger_when_the_camera_is_farther_away() {
        let mut near = OrbitCamera {
            distance: 2.0,
            ..OrbitCamera::default()
        };
        let mut far = OrbitCamera {
            distance: 20.0,
            ..OrbitCamera::default()
        };
        let near_start = near.target;
        let far_start = far.target;
        near.pan(10.0, 0.0, 600.0);
        far.pan(10.0, 0.0, 600.0);
        assert!(
            (far.target - far_start).length() > (near.target - near_start).length() * 5.0,
            "ten times the distance is roughly ten times the pan"
        );
    }

    #[test]
    fn zoom_is_multiplicative_and_clamped() {
        let mut camera = OrbitCamera::default();
        let start = camera.distance;
        camera.zoom(1.0);
        assert!(camera.distance < start, "positive scroll zooms in");
        let once = camera.distance;
        camera.zoom(-1.0);
        assert!(approx(camera.distance, start), "the same notch restores it");
        assert!(approx(once / start, (-0.1f32).exp()));

        camera.zoom(10_000.0);
        assert!(approx(camera.distance, MIN_DISTANCE));
        camera.zoom(-10_000.0);
        assert!(approx(camera.distance, MAX_DISTANCE));
    }

    #[test]
    fn a_non_finite_scroll_is_ignored() {
        let mut camera = OrbitCamera::default();
        let before = camera;
        camera.zoom(f32::NAN);
        camera.zoom(f32::INFINITY);
        camera.zoom(0.0);
        assert_eq!(camera, before);
    }

    #[test]
    fn framing_a_box_centres_it_and_fits_it() {
        let mut camera = OrbitCamera::default();
        camera.frame_aabb([-4.0, 0.0, -2.0], [4.0, 2.0, 2.0]);
        assert!(approx(camera.target.x, 0.0));
        assert!(approx(camera.target.y, 1.0));
        assert!(approx(camera.target.z, 0.0));

        // Every corner must be inside the frustum: project with the camera's own
        // matrices and check the NDC bounds.
        let aspect = 16.0 / 9.0;
        let view_projection = glam::Mat4::from_cols_array(&camera.view_projection(aspect));
        for x in [-4.0, 4.0] {
            for y in [0.0, 2.0] {
                for z in [-2.0, 2.0] {
                    let clip = view_projection * glam::Vec4::new(x, y, z, 1.0);
                    let ndc = clip.truncate() / clip.w;
                    assert!(clip.w > 0.0, "corner behind the camera");
                    assert!(ndc.x.abs() <= 1.0, "x {} outside the frustum", ndc.x);
                    assert!(ndc.y.abs() <= 1.0, "y {} outside the frustum", ndc.y);
                }
            }
        }
    }

    #[test]
    fn the_view_projection_puts_the_target_in_the_centre() {
        let camera = OrbitCamera::default();
        let matrix = glam::Mat4::from_cols_array(&camera.view_projection(1.5));
        let clip = matrix * (camera.target.extend(1.0));
        assert!(clip.w > 0.0);
        let ndc = clip.truncate() / clip.w;
        assert!(ndc.x.abs() < 1e-4, "x = {}", ndc.x);
        assert!(ndc.y.abs() < 1e-4, "y = {}", ndc.y);
    }

    #[test]
    fn looking_at_frames_the_target_at_the_requested_distance() {
        let camera = OrbitCamera::looking_at(Vec3::new(2.0, 0.0, -3.0), 12.0);
        assert_eq!(camera.target, Vec3::new(2.0, 0.0, -3.0));
        assert!(approx(camera.distance, 12.0));
        assert!(approx(
            (camera.eye() - camera.target).length(),
            camera.distance
        ));

        let clamped = OrbitCamera::looking_at(Vec3::ZERO, -5.0);
        assert!(approx(clamped.distance, MIN_DISTANCE));
    }

    #[test]
    fn the_glam_projection_matches_the_engine_one() {
        let camera = OrbitCamera::default();
        let engine = camera.projection(1.6).to_cols_array();
        let glam = camera.projection_glam(1.6).to_cols_array();
        assert_eq!(engine, glam);
    }
}
