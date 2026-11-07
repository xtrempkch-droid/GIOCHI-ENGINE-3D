//! Perspective camera, `#[repr(C)]`.
//!
//! `aspect == 0.0` means "ask the renderer", which is what a console needs: the panel's
//! real size is authoritative, the same rule ADR-035 established for 2D. A camera that
//! hardcoded 16:9 on a 640x480 panel would render a cropped scene with no error.

use super::{Mat4, Vec3};

/// A perspective camera looking from `position` towards `target`.
///
/// # C ABI
///
/// ```c
/// typedef struct GiochiCamera3D {
///     GiochiVec3 position;
///     GiochiVec3 target;
///     GiochiVec3 up;
///     float      fov_y;                /* radians, vertical */
///     float      near_plane;
///     float      far_plane;
///     float      aspect;               /* width / height; 0 = use the drawable */
///     uint32_t   _reserved[4];
/// } GiochiCamera3D;                  /* size 116, align 4 */
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera3D {
    /// Eye position.
    pub position: Vec3,
    /// Point the camera looks at.
    pub target: Vec3,
    /// Up direction. Must not be parallel to the view direction.
    pub up: Vec3,
    /// Vertical field of view in radians.
    pub fov_y: f32,
    /// Near clip distance. Must be positive.
    pub near_plane: f32,
    /// Far clip distance. Must be greater than `near_plane`.
    pub far_plane: f32,
    /// Width divided by height. `0.0` means "use the renderer's drawable size".
    pub aspect: f32,
    /// Reserved for future ABI-compatible fields. Must be zero.
    pub _reserved: [u32; 4],
}

impl Camera3D {
    /// A sensible default: at the origin, looking down -Z, 60° vertical FOV.
    ///
    /// Down -Z is the conventional "forward" for a right-handed view, and it matches
    /// what `look_at` produces, so a caller that forgets to aim the camera still sees
    /// something in front of it.
    pub const fn new() -> Self {
        Self {
            position: Vec3::ZERO,
            target: Vec3::new(0.0, 0.0, -1.0),
            up: Vec3::UP,
            // 60 degrees = PI/3 exactly. `f32::to_radians` is not `const` on
            // the 1.82 MSRV (it stabilised in 1.85), so the constant is used directly.
            fov_y: std::f32::consts::FRAC_PI_3,
            near_plane: 0.1,
            far_plane: 1000.0,
            aspect: 0.0,
            _reserved: [0; 4],
        }
    }

    /// Builds a camera from position, target and up.
    pub const fn from_parts(position: Vec3, target: Vec3, up: Vec3) -> Self {
        let mut camera = Self::new();
        camera.position = position;
        camera.target = target;
        camera.up = up;
        camera
    }

    /// Returns the same camera with a new field of view, in radians.
    ///
    /// Out-of-range values are ignored here and rejected by
    /// [`Camera3D::projection_matrix`], so a bad C caller gets one clear error rather
    /// than a camera that silently renders nothing.
    pub const fn with_fov_y(mut self, fov_y: f32) -> Self {
        self.fov_y = fov_y;
        self
    }

    /// Returns the same camera with different clip planes.
    pub const fn with_clip_planes(mut self, near: f32, far: f32) -> Self {
        self.near_plane = near;
        self.far_plane = far;
        self
    }

    /// Returns the same camera with an explicit aspect ratio.
    ///
    /// Pass `0.0` to let the renderer fill it from the drawable size.
    pub const fn with_aspect(mut self, aspect: f32) -> Self {
        self.aspect = aspect;
        self
    }

    /// Returns the view matrix.
    ///
    /// # Errors
    ///
    /// When `position == target`, `up` has no direction, or `up` is parallel to the
    /// view direction. These are all cases where no view matrix exists.
    pub fn view_matrix(&self) -> Result<Mat4, &'static str> {
        Mat4::look_at(self.position, self.target, self.up)
    }

    /// Returns the projection matrix.
    ///
    /// # Errors
    ///
    /// When `fov_y`, the clip planes or the aspect ratio are out of range, or when
    /// `aspect == 0.0` (the caller was supposed to resolve it first — a camera that
    /// does not know its vector is a bug, and guessing 16:9 would crop a 4:3 panel).
    pub fn projection_matrix(&self) -> Result<Mat4, &'static str> {
        if self.aspect == 0.0 {
            return Err("aspect is 0; call with_aspect(drawable_width / drawable_height) first");
        }

        Mat4::perspective(self.fov_y, self.aspect, self.near_plane, self.far_plane)
    }

    /// Returns `projection * view`, the matrix a shader wants as `u_view_proj`.
    pub fn view_projection(&self) -> Result<Mat4, &'static str> {
        Ok(self.projection_matrix()? * self.view_matrix()?)
    }

    /// Resolves `aspect` from a drawable size, falling back to the current value when
    /// the size is unusable.
    ///
    /// Callers run this once per frame with `RenderContext3D`'s drawable size, which is
    /// what keeps the projection correct when a console ignores the requested window
    /// size (ADR-035).
    pub fn with_drawable_size(mut self, width: u32, height: u32) -> Self {
        if height > 0 {
            self.aspect = width as f32 / height as f32;
        } else {
            log::warn!("drawable height is 0; keeping aspect {}", self.aspect);
        }
        self
    }

    /// Returns `true` when every field is finite.
    pub fn is_finite(&self) -> bool {
        self.position.is_finite()
            && self.target.is_finite()
            && self.up.is_finite()
            && self.fov_y.is_finite()
            && self.near_plane.is_finite()
            && self.far_plane.is_finite()
            && self.aspect.is_finite()
    }
}

impl Default for Camera3D {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{align_of, size_of};
    use std::f32::consts::FRAC_PI_2;
    use std::f32::consts::FRAC_PI_3;

    fn assert_close(actual: f32, expected: f32, what: &str) {
        assert!(
            (actual - expected).abs() < 1e-5,
            "{what}: got {actual}, expected {expected}"
        );
    }

    #[test]
    fn layout_is_the_documented_116_bytes() {
        // Vec3(28) * 3 + 4 floats(16) + reserved(16).
        assert_eq!(size_of::<Camera3D>(), 116);
        assert_eq!(align_of::<Camera3D>(), 4);
    }

    #[test]
    fn the_default_camera_looks_down_minus_z() {
        let camera = Camera3D::new();

        assert_eq!(camera.position, Vec3::ZERO);
        assert_eq!(camera.target, Vec3::new(0.0, 0.0, -1.0));
        assert_eq!(camera.up, Vec3::UP);
        assert_eq!(camera._reserved, [0; 4]);
    }

    #[test]
    fn the_default_fov_is_sixty_degrees() {
        assert_close(Camera3D::new().fov_y, FRAC_PI_3, "60 degrees in radians");
        // PI/3 is exactly 60 degrees; this pins the claim rather than assuming it.
        assert_close(FRAC_PI_3, 60.0_f32.to_radians(), "PI/3 vs to_radians");
    }

    #[test]
    fn the_target_lands_on_the_view_space_minus_z_axis() {
        let camera = Camera3D::new().with_aspect(1.0);
        let view = camera.view_matrix().expect("valid view");

        let in_view = view.transform_point(camera.target);
        assert_close(in_view.x, 0.0, "x");
        assert_close(in_view.y, 0.0, "y");
        assert_close(in_view.z, -1.0, "z");
    }

    #[test]
    fn a_point_at_the_centre_of_the_near_plane_projects_to_minus_one() {
        let camera = Camera3D::from_parts(Vec3::ZERO, Vec3::new(0.0, 0.0, -1.0), Vec3::UP)
            .with_aspect(1.0)
            .with_fov_y(FRAC_PI_2)
            .with_clip_planes(0.5, 50.0);

        let view_proj = camera.view_projection().expect("valid");
        let near_point = glam::Vec4::new(0.0, 0.0, -0.5, 1.0);
        let clip = view_proj.to_glam() * near_point;

        assert_close(clip.z / clip.w, -1.0, "near plane depth");
    }

    #[test]
    fn a_point_at_the_centre_of_the_far_plane_projects_to_plus_one() {
        let camera = Camera3D::new()
            .with_aspect(1.0)
            .with_fov_y(FRAC_PI_2)
            .with_clip_planes(0.5, 50.0);

        let view_proj = camera.view_projection().expect("valid");
        let far_point = glam::Vec4::new(0.0, 0.0, -50.0, 1.0);
        let clip = view_proj.to_glam() * far_point;

        assert_close(clip.z / clip.w, 1.0, "far plane depth");
    }

    #[test]
    fn a_point_behind_the_camera_has_a_non_positive_w() {
        // This is the test that catches a left-handed projection sneaking in: w stays
        // positive in front of the camera only for a right-handed frustum.
        let camera = Camera3D::new().with_aspect(1.0);
        let view_proj = camera.view_projection().expect("valid");

        let in_front = view_proj.to_glam() * glam::Vec4::new(0.0, 0.0, -10.0, 1.0);
        assert!(in_front.w > 0.0, "in front: w = {}", in_front.w);

        let behind = view_proj.to_glam() * glam::Vec4::new(0.0, 0.0, 10.0, 1.0);
        assert!(behind.w <= 0.0, "behind: w = {}", behind.w);
    }

    #[test]
    fn an_unresolved_aspect_is_an_error_not_a_guess() {
        let camera = Camera3D::new();

        assert_eq!(camera.aspect, 0.0);
        let error = camera
            .projection_matrix()
            .expect_err("aspect is unresolved");
        assert!(error.contains("aspect"), "{error}");
    }

    #[test]
    fn the_drawable_size_resolves_the_aspect() {
        let camera = Camera3D::new().with_drawable_size(640, 480);

        assert_close(camera.aspect, 4.0 / 3.0, "4:3 panel");
        assert!(camera.projection_matrix().is_ok());
    }

    #[test]
    fn a_zero_height_drawable_keeps_the_previous_aspect() {
        // A minimised window reports 0; dividing by it would give infinity and a blank
        // frame. The previous value is the honest fallback.
        let camera = Camera3D::new().with_aspect(1.5).with_drawable_size(0, 0);

        assert_close(camera.aspect, 1.5, "unchanged");
    }

    #[test]
    fn a_degenerate_camera_reports_an_error() {
        let bad_up = Camera3D::from_parts(Vec3::ZERO, Vec3::new(0.0, 0.0, -1.0), Vec3::ZERO);
        assert!(bad_up.view_matrix().is_err(), "up has no direction");

        let same_point = Camera3D::from_parts(Vec3::ZERO, Vec3::ZERO, Vec3::UP);
        assert!(same_point.view_matrix().is_err(), "eye == target");

        let numeric = |fov: f32, near: f32, far: f32| {
            Camera3D::new()
                .with_aspect(1.0)
                .with_fov_y(fov)
                .with_clip_planes(near, far)
                .projection_matrix()
                .is_err()
        };
        assert!(numeric(0.0, 0.1, 100.0), "zero fov");
        assert!(numeric(1.0, 0.0, 100.0), "zero near");
        assert!(numeric(1.0, 100.0, 10.0), "far < near");
        assert!(numeric(f32::NAN, 0.1, 100.0), "NaN fov");
    }

    #[test]
    fn the_view_projection_is_projection_times_view() {
        let camera =
            Camera3D::from_parts(Vec3::new(1.0, 2.0, 3.0), Vec3::new(0.0, 0.0, 0.0), Vec3::UP)
                .with_aspect(1.0);

        let expected =
            camera.projection_matrix().expect("valid") * camera.view_matrix().expect("valid");

        assert_eq!(
            camera.view_projection().expect("valid").to_cols_array(),
            expected.to_cols_array()
        );
    }

    #[test]
    fn finite_checks_every_field() {
        assert!(Camera3D::new().is_finite());
        assert!(!Camera3D::new().with_fov_y(f32::NAN).is_finite());
        assert!(!Camera3D::new()
            .with_clip_planes(0.1, f32::INFINITY)
            .is_finite());
    }

    #[test]
    fn builders_preserve_the_reserved_tail() {
        let camera = Camera3D::new()
            .with_fov_y(1.0)
            .with_aspect(2.0)
            .with_clip_planes(1.0, 2.0)
            .with_drawable_size(800, 600);

        assert_eq!(camera._reserved, [0; 4]);
    }
}
