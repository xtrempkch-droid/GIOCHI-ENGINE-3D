//! 4×4 matrix, column-major, `#[repr(C)]`.
//!
//! Deliberately **not** part of the C ABI (`docs/C_ABI_CONTRACT.md` §11): callers pass
//! a [`Transform3D`](super::Transform3D) and a [`Camera3D`](super::Camera3D) and the
//! engine composes the matrices. Exposing them would let callers depend on the
//! multiplication order, which is the single most common source of "my model is
//! inside-out" bugs.
//!
//! Storage is column-major to match GL's uniform layout, so `as_ptr()` can be handed to
//! `glUniformMatrix4fv` without a transpose.

use glam::{camera, Mat3 as GlamMat3, Mat4 as GlamMat4};

use super::{Quat, Vec3};

/// A 4×4 matrix in column-major order.
///
/// Layout: `data[column * 4 + row]`. `data[12..15]` is the translation column.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mat4 {
    /// The 16 elements, column-major.
    pub data: [f32; 16],
    /// Reserved for future ABI-compatible fields. Must be zero.
    pub _reserved: [u32; 4],
}

impl Mat4 {
    /// The identity matrix.
    pub const IDENTITY: Self = Self::from_cols_array(&[
        1.0, 0.0, 0.0, 0.0, //
        0.0, 1.0, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, //
        0.0, 0.0, 0.0, 1.0,
    ]);

    /// Builds a matrix from 16 column-major elements.
    pub const fn from_cols_array(data: &[f32; 16]) -> Self {
        Self {
            data: *data,
            _reserved: [0; 4],
        }
    }

    /// Returns the 16 column-major elements.
    pub const fn to_cols_array(self) -> [f32; 16] {
        self.data
    }

    /// Returns a pointer suitable for `glUniformMatrix4fv` with `transpose = false`.
    pub fn as_ptr(&self) -> *const f32 {
        self.data.as_ptr()
    }

    /// Converts to `glam` for composition.
    pub const fn to_glam(self) -> GlamMat4 {
        GlamMat4::from_cols_array(&self.data)
    }

    /// Builds from `glam`.
    pub const fn from_glam(value: GlamMat4) -> Self {
        Self::from_cols_array(&value.to_cols_array())
    }

    /// A pure translation.
    pub fn from_translation(translation: Vec3) -> Self {
        Self::from_glam(GlamMat4::from_translation(translation.to_glam()))
    }

    /// A pure rotation.
    pub fn from_rotation(rotation: Quat) -> Self {
        Self::from_glam(GlamMat4::from_quat(rotation.to_glam()))
    }

    /// A pure scale.
    pub fn from_scale(scale: Vec3) -> Self {
        Self::from_glam(GlamMat4::from_scale(scale.to_glam()))
    }

    /// The standard right-handed perspective projection, depth in `-1.0..=1.0`.
    ///
    /// `fov_y` is the **vertical** field of view in radians, which is what
    /// `GiochiCamera3D::fov_y` documents. A horizontal FOV would make the same camera
    /// frame differently on a 4:3 panel and a 16:9 monitor.
    ///
    /// Returns `Err` for a degenerate frustum: non-finite or non-positive `fov_y`,
    /// `aspect`, `near`, or `far <= near`. Such a matrix produces a blank screen with
    /// no GL error, so failing loudly here is the only way to find it.
    pub fn perspective(fov_y: f32, aspect: f32, near: f32, far: f32) -> Result<Self, &'static str> {
        if !fov_y.is_finite() || fov_y <= 0.0 {
            return Err("vertical field of view must be finite and positive");
        }
        if fov_y >= std::f32::consts::PI {
            return Err("vertical field of view must be less than PI radians");
        }
        if !aspect.is_finite() || aspect <= 0.0 {
            return Err("aspect ratio must be finite and positive");
        }
        if !near.is_finite() || near <= 0.0 {
            return Err("near plane must be finite and positive");
        }
        if !far.is_finite() || far <= near {
            return Err("far plane must be finite and greater than near");
        }

        // `camera::rh::proj::opengl`, not `camera::rh::proj::vulkan`/`directx`: os
        // dois últimos mapeiam a profundidade para 0..1, enquanto o OpenGL ES usa
        // -1..1. Com a convenção errada a cena parece plausível até a geometria se
        // aproximar do plano próximo — por isso os testes abaixo prendem o -1..1.
        //
        // glam 0.34 tirou `Mat4::perspective_rh_gl` e moveu as projeções para o
        // módulo `camera`. O submódulo `opengl` é justamente o que carrega a
        // convenção de profundidade do OpenGL.
        Ok(Self::from_glam(camera::rh::proj::opengl::perspective(
            fov_y, aspect, near, far,
        )))
    }

    /// An orthographic projection, used for UI and debug overlays.
    ///
    /// Returns `Err` when any bound is non-finite or the box has no volume.
    pub fn orthographic(
        left: f32,
        right: f32,
        bottom: f32,
        top: f32,
        near: f32,
        far: f32,
    ) -> Result<Self, &'static str> {
        let finite = [left, right, bottom, top, near, far]
            .iter()
            .all(|value| value.is_finite());
        if !finite {
            return Err("orthographic bounds must be finite");
        }
        if left == right || bottom == top || near == far {
            return Err("orthographic bounds enclose no volume");
        }

        Ok(Self::from_glam(camera::rh::proj::opengl::orthographic(
            left, right, bottom, top, near, far,
        )))
    }

    /// A right-handed view matrix looking from `eye` towards `center`.
    ///
    /// Returns `Err` when the up vector is parallel to the view direction, because
    /// there is then no way to pick a "right" — the usual cause is a camera looking
    /// straight up with an unmodified up vector.
    pub fn look_at(eye: Vec3, center: Vec3, up: Vec3) -> Result<Self, &'static str> {
        let direction = center.to_glam() - eye.to_glam();
        let up = up.to_glam();
        // A NaN camera would otherwise produce a NaN matrix (or trip glam's
        // `glam_assert` in debug builds) instead of the documented error.
        if !direction.is_finite() || !up.is_finite() {
            return Err("eye, center and up must be finite");
        }
        if direction.length_squared() == 0.0 {
            return Err("eye and center are the same point");
        }
        if up.length_squared() == 0.0 {
            return Err("up vector has no direction");
        }
        // The cross product vanishes when the two are parallel.
        let up = up.normalize();
        if direction.normalize().cross(up).length_squared() < 1e-12 {
            return Err("up vector is parallel to the view direction");
        }

        // glam 0.34 requires a unit `up`: with `glam_assert` it panics, and without
        // it the unnormalised vector scales the view basis.
        Ok(Self::from_glam(camera::rh::view::look_at_mat4(
            eye.to_glam(),
            center.to_glam(),
            up,
        )))
    }

    /// Multiplies two matrices without the operator, for call sites where `*` reads
    /// badly (a long chain). Prefer `a * b`.
    ///
    /// Named `multiply` rather than `mul` so it cannot be confused with
    /// `std::ops::Mul::mul`, which is the trait that makes `*` work below.
    pub fn multiply(self, rhs: Mat4) -> Mat4 {
        Self::from_glam(self.to_glam() * rhs.to_glam())
    }

    /// Returns the inverse, or the identity when the matrix is singular.
    ///
    /// A singular matrix has no inverse; returning the identity keeps a frame
    /// rendering instead of filling it with NaN. Callers that care can compare the
    /// determinant through `is_invertible`.
    pub fn inverse(self) -> Mat4 {
        let determinant = self.to_glam().determinant();
        if determinant == 0.0 || !determinant.is_finite() {
            log::warn!("inverting a singular matrix; returning the identity");
            return Self::IDENTITY;
        }
        Self::from_glam(self.to_glam().inverse())
    }

    /// Whether the matrix can be inverted.
    pub fn is_invertible(self) -> bool {
        let determinant = self.to_glam().determinant();
        determinant != 0.0 && determinant.is_finite()
    }

    /// The matrix that carries **normals** into the same space as [`Self::transform_point`].
    ///
    /// A normal is a direction, so it takes the model's rotation — but not a
    /// non-uniform scale. Squashing a sphere on X doubles the length of the normals on
    /// the sides and leaves the top and bottom alone, which tips every normal towards
    /// the squashed axis: a lit sphere ends up shaded as though it were a different
    /// shape. The inverse-transpose of the upper 3×3 cancels exactly that, and for a
    /// rotation (or a uniform scale, which only changes length and is re-normalised in
    /// the fragment shader) it is the rotation itself.
    ///
    /// The result is a [`Mat4`] rather than a 3×3 so it can be uploaded with
    /// `glUniformMatrix4fv`; the shader reads its upper-left `mat3`. The translation
    /// column is zeroed, since moving a direction is a bug that looks like lighting
    /// sliding when an object is dragged.
    ///
    /// A singular matrix — a scale of zero on some axis, which a collapsing animation
    /// produces for one frame — has no inverse. The identity is returned and a warning
    /// logged, matching [`Self::inverse`].
    pub fn normal_matrix(self) -> Mat4 {
        let upper = GlamMat3::from_mat4(self.to_glam());
        let determinant = upper.determinant();
        if determinant == 0.0 || !determinant.is_finite() {
            log::warn!("normal matrix of a singular model matrix; returning the identity");
            return Self::IDENTITY;
        }

        let normal = upper.inverse().transpose();
        // `to_cols_array` on a `Mat3` in `glam` is column-major, and the 4×4 we build
        // from it leaves the fourth row and column as the identity's, which is what a
        // `mat3(u_normal_matrix)` read in GLSL wants.
        let m = normal.to_cols_array();
        Self::from_cols_array(&[
            m[0], m[1], m[2], 0.0, //
            m[3], m[4], m[5], 0.0, //
            m[6], m[7], m[8], 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ])
    }

    /// Transforms a point (including the translation column).
    ///
    /// **This is only correct for an affine matrix** — a model or a view matrix. It does
    /// not divide by `w`, which `glam`'s `transform_point3` also does not (it asserts that
    /// the bottom row is `[0, 0, 0, 1]`). Feeding it a projection matrix returns *clip*
    /// coordinates, which look like plausible but wrong numbers rather than failing; use
    /// [`Self::project_point`] for that.
    pub fn transform_point(self, point: Vec3) -> Vec3 {
        Vec3::from_glam(self.to_glam().transform_point3(point.to_glam()))
    }

    /// Projects a point through this matrix **with** the perspective divide.
    ///
    /// Returns normalised device coordinates: `x` and `y` in `-1..=1` inside the frame,
    /// and `z` in `-1..=1` between the near and far planes (OpenGL's convention, which is
    /// what [`Camera3D::projection_matrix`](super::Camera3D::projection_matrix) produces —
    /// see ADR-038 on why the OpenGL depth range and not the 0..1 one. Em glam 0.34 isso
    /// é `camera::rh::proj::opengl::perspective`; as versões `directx`/`vulkan` do mesmo
    /// módulo mapeiam a profundidade para 0..1).
    ///
    /// Returns `None` when `w <= 0`, which is a point at or behind the eye. There the
    /// divide flips the sign, so a point behind the camera would otherwise come back as a
    /// perfectly ordinary-looking coordinate in front of it — the bug that makes an
    /// off-screen object appear to be on screen.
    pub fn project_point(self, point: Vec3) -> Option<Vec3> {
        let d = self.data;
        let (x, y, z) = (point.x, point.y, point.z);

        let w = d[3] * x + d[7] * y + d[11] * z + d[15];
        // `w <= 0.0` is false for NaN, which would then be divided by, so the finiteness
        // check is not redundant.
        if !w.is_finite() || w <= 0.0 {
            return None;
        }
        let inverse_w = 1.0 / w;

        Some(Vec3::new(
            (d[0] * x + d[4] * y + d[8] * z + d[12]) * inverse_w,
            (d[1] * x + d[5] * y + d[9] * z + d[13]) * inverse_w,
            (d[2] * x + d[6] * y + d[10] * z + d[14]) * inverse_w,
        ))
    }

    /// Returns `true` when every element is finite.
    pub fn is_finite(self) -> bool {
        self.data.iter().all(|value| value.is_finite())
    }
}

impl Default for Mat4 {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl std::ops::Mul for Mat4 {
    type Output = Mat4;

    fn mul(self, rhs: Mat4) -> Mat4 {
        self.multiply(rhs)
    }
}

#[cfg(test)]
mod tests {
    use super::super::Camera3D;
    use super::*;
    use core::mem::{align_of, size_of};
    use std::f32::consts::{FRAC_PI_2, PI};

    /// Tolerance for float comparisons: a perspective matrix involves a `tan`.
    const EPSILON: f32 = 1e-5;

    fn assert_close(actual: f32, expected: f32, what: &str) {
        assert!(
            (actual - expected).abs() < EPSILON,
            "{what}: got {actual}, expected {expected}"
        );
    }

    #[test]
    fn layout_is_64_bytes_of_data_plus_reserved() {
        assert_eq!(size_of::<Mat4>(), 64 + 16);
        assert_eq!(align_of::<Mat4>(), 4);
    }

    #[test]
    fn identity_is_the_multiplicative_unit() {
        let point = Vec3::new(1.0, -2.0, 3.0);

        assert_eq!(Mat4::IDENTITY.transform_point(point), point);
        assert_eq!(Mat4::IDENTITY * Mat4::IDENTITY, Mat4::IDENTITY);
    }

    #[test]
    fn identity_has_the_expected_diagonal() {
        let data = Mat4::IDENTITY.to_cols_array();

        for i in 0..4 {
            assert_close(data[i * 4 + i], 1.0, "diagonal");
        }
        // Everything off the diagonal is zero.
        let off_diagonal = data
            .iter()
            .enumerate()
            .filter(|(i, _)| i % 5 != 0)
            .filter(|(_, value)| **value != 0.0)
            .count();
        assert_eq!(off_diagonal, 0);
    }

    #[test]
    fn translation_matrix_moves_a_point() {
        let matrix = Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0));

        assert_eq!(matrix.transform_point(Vec3::ZERO), Vec3::new(1.0, 2.0, 3.0));
    }

    #[test]
    fn scale_matrix_scales_a_point() {
        let matrix = Mat4::from_scale(Vec3::new(2.0, 3.0, 4.0));

        assert_eq!(
            matrix.transform_point(Vec3::new(1.0, 1.0, 1.0)),
            Vec3::new(2.0, 3.0, 4.0)
        );
    }

    #[test]
    fn rotation_matrix_turns_x_into_y() {
        let matrix =
            Mat4::from_rotation(Quat::from_axis_angle(Vec3::new(0.0, 0.0, 1.0), FRAC_PI_2));
        let rotated = matrix.transform_point(Vec3::new(1.0, 0.0, 0.0));

        assert_close(rotated.x, 0.0, "x");
        assert_close(rotated.y, 1.0, "y");
    }

    #[test]
    fn perspective_projects_the_near_plane_to_minus_one_and_far_to_plus_one() {
        let (near, far) = (0.1_f32, 100.0_f32);
        let matrix = Mat4::perspective(FRAC_PI_2, 16.0 / 9.0, near, far).expect("valid frustum");
        let glam = matrix.to_glam();

        // A point on the near plane in front of the camera.
        let on_near = glam * glam::Vec4::new(0.0, 0.0, -near, 1.0);
        assert_close(on_near.z / on_near.w, -1.0, "near plane depth");

        let on_far = glam * glam::Vec4::new(0.0, 0.0, -far, 1.0);
        assert_close(on_far.z / on_far.w, 1.0, "far plane depth");
    }

    #[test]
    fn perspective_puts_a_point_at_the_centre_on_the_axis() {
        let matrix = Mat4::perspective(FRAC_PI_2, 1.0, 0.1, 100.0).expect("valid frustum");
        let projected = matrix.to_glam() * glam::Vec4::new(0.0, 0.0, -10.0, 1.0);

        assert_close(projected.x, 0.0, "x on the axis");
        assert_close(projected.y, 0.0, "y on the axis");
        assert!(
            projected.w > 0.0,
            "w must stay positive in front of the camera"
        );
    }

    #[test]
    fn perspective_is_vertical_fov_so_widening_the_aspect_widens_the_frame() {
        // The same vertical FOV on a wider aspect must show more horizontally: the
        // x term of the matrix is scaled by 1/aspect.
        let narrow = Mat4::perspective(FRAC_PI_2, 1.0, 0.1, 100.0).expect("valid");
        let wide = Mat4::perspective(FRAC_PI_2, 2.0, 0.1, 100.0).expect("valid");

        // data[0] is m00 = 1 / (aspect * tan(fov/2)).
        assert!(wide.data[0] < narrow.data[0], "a wider aspect sees more");
        // data[5] (m11) does not depend on aspect: the vertical extent is unchanged.
        assert_close(
            wide.data[5],
            narrow.data[5],
            "vertical extent is aspect-free",
        );
    }

    #[test]
    fn perspective_rejects_a_degenerate_frustum() {
        assert!(Mat4::perspective(0.0, 1.0, 0.1, 100.0).is_err(), "zero fov");
        assert!(
            Mat4::perspective(-1.0, 1.0, 0.1, 100.0).is_err(),
            "negative fov"
        );
        assert!(
            Mat4::perspective(PI, 1.0, 0.1, 100.0).is_err(),
            "fov of 180 degrees"
        );
        assert!(
            Mat4::perspective(1.0, 0.0, 0.1, 100.0).is_err(),
            "zero aspect"
        );
        assert!(
            Mat4::perspective(1.0, 1.0, 0.0, 100.0).is_err(),
            "zero near"
        );
        assert!(
            Mat4::perspective(1.0, 1.0, 100.0, 100.0).is_err(),
            "far == near"
        );
        assert!(
            Mat4::perspective(1.0, 1.0, 100.0, 10.0).is_err(),
            "far < near"
        );
        assert!(
            Mat4::perspective(f32::NAN, 1.0, 0.1, 100.0).is_err(),
            "NaN fov"
        );
    }

    #[test]
    fn look_at_from_the_origin_towards_minus_z_places_the_target_at_minus_z() {
        let view =
            Mat4::look_at(Vec3::ZERO, Vec3::new(0.0, 0.0, -10.0), Vec3::UP).expect("valid view");

        // The camera looks down its own -Z, so the target lands on -Z in view space.
        let in_view = view.transform_point(Vec3::new(0.0, 0.0, -10.0));
        assert_close(in_view.x, 0.0, "x");
        assert_close(in_view.y, 0.0, "y");
        assert_close(in_view.z, -10.0, "z");

        // And the eye is at the view-space origin.
        let eye = view.transform_point(Vec3::ZERO);
        assert_close(eye.x, 0.0, "eye x");
        assert_close(eye.y, 0.0, "eye y");
        assert_close(eye.z, 0.0, "eye z");
    }

    #[test]
    fn look_at_accepts_a_non_unit_up_vector() {
        // glam 0.34 requires a normalised `up`; passing a scaled one through would
        // panic with `glam_assert` or silently scale the view basis.
        let unit =
            Mat4::look_at(Vec3::ZERO, Vec3::new(0.0, 0.0, -10.0), Vec3::UP).expect("valid view");
        let scaled = Mat4::look_at(
            Vec3::ZERO,
            Vec3::new(0.0, 0.0, -10.0),
            Vec3::new(0.0, 7.0, 0.0),
        )
        .expect("a scaled up vector still has a direction");

        for (a, b) in unit.to_cols_array().iter().zip(scaled.to_cols_array()) {
            assert_close(*a, b, "scaled up gives the same view");
        }
    }

    #[test]
    fn look_at_rejects_a_degenerate_configuration() {
        assert!(
            Mat4::look_at(Vec3::ZERO, Vec3::ZERO, Vec3::UP).is_err(),
            "eye == center"
        );
        assert!(
            Mat4::look_at(Vec3::ZERO, Vec3::new(0.0, 0.0, -1.0), Vec3::ZERO).is_err(),
            "no up vector"
        );
        // Looking straight up with UP as the up vector: the cross product vanishes.
        assert!(
            Mat4::look_at(Vec3::ZERO, Vec3::new(0.0, 5.0, 0.0), Vec3::UP).is_err(),
            "up parallel to the view direction"
        );
        assert!(
            Mat4::look_at(
                Vec3::new(f32::NAN, 0.0, 0.0),
                Vec3::new(0.0, 0.0, -1.0),
                Vec3::UP
            )
            .is_err(),
            "NaN eye"
        );
        assert!(
            Mat4::look_at(
                Vec3::ZERO,
                Vec3::new(0.0, 0.0, -1.0),
                Vec3::new(f32::INFINITY, 0.0, 0.0)
            )
            .is_err(),
            "infinite up"
        );
    }

    #[test]
    fn orthographic_maps_the_box_to_the_unit_cube() {
        let matrix = Mat4::orthographic(-10.0, 10.0, -5.0, 5.0, 0.1, 100.0).expect("valid ortho");
        let glam = matrix.to_glam();

        let left_bottom = glam * glam::Vec4::new(-10.0, -5.0, -0.1, 1.0);
        assert_close(left_bottom.x / left_bottom.w, -1.0, "left edge");
        assert_close(left_bottom.y / left_bottom.w, -1.0, "bottom edge");

        let right_top = glam * glam::Vec4::new(10.0, 5.0, -100.0, 1.0);
        assert_close(right_top.x / right_top.w, 1.0, "right edge");
        assert_close(right_top.y / right_top.w, 1.0, "top edge");
    }

    #[test]
    fn orthographic_rejects_an_empty_box() {
        assert!(Mat4::orthographic(0.0, 0.0, -1.0, 1.0, 0.1, 10.0).is_err());
        assert!(Mat4::orthographic(-1.0, 1.0, 0.0, 0.0, 0.1, 10.0).is_err());
        assert!(Mat4::orthographic(-1.0, 1.0, -1.0, 1.0, 5.0, 5.0).is_err());
        assert!(Mat4::orthographic(f32::NAN, 1.0, -1.0, 1.0, 0.1, 10.0).is_err());
    }

    #[test]
    fn multiplication_composes_in_the_expected_order() {
        // translate * scale applies the scale first, then the translation.
        let translate = Mat4::from_translation(Vec3::new(10.0, 0.0, 0.0));
        let scale = Mat4::from_scale(Vec3::new(2.0, 2.0, 2.0));
        let combined = translate * scale;

        assert_eq!(
            combined.transform_point(Vec3::new(1.0, 0.0, 0.0)),
            Vec3::new(12.0, 0.0, 0.0),
            "scale then translate"
        );
    }

    #[test]
    fn inverting_a_transform_undoes_it() {
        let matrix = Mat4::from_translation(Vec3::new(3.0, -4.0, 5.0))
            * Mat4::from_rotation(Quat::from_axis_angle(Vec3::UP, FRAC_PI_2));
        let point = Vec3::new(1.0, 2.0, 3.0);

        let round_trip = matrix
            .inverse()
            .transform_point(matrix.transform_point(point));

        assert_close(round_trip.x, point.x, "x");
        assert_close(round_trip.y, point.y, "y");
        assert_close(round_trip.z, point.z, "z");
    }

    #[test]
    fn inverting_a_singular_matrix_returns_the_identity_instead_of_nan() {
        let singular = Mat4::from_scale(Vec3::new(1.0, 0.0, 1.0));

        assert!(!singular.is_invertible());
        let inverted = singular.inverse();
        assert_eq!(inverted, Mat4::IDENTITY);
        assert!(inverted.is_finite(), "NaN here would blank the frame");
    }

    #[test]
    fn as_ptr_points_at_the_column_major_data() {
        let matrix = Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0));
        let pointer = matrix.as_ptr();

        // SAFETY: the pointer belongs to `matrix`, which outlives this block.
        let first = unsafe { *pointer };
        assert_eq!(first, matrix.data[0]);

        // The translation lives in the fourth column, elements 12..15.
        assert_eq!(matrix.data[12], 1.0);
        assert_eq!(matrix.data[13], 2.0);
        assert_eq!(matrix.data[14], 3.0);
    }

    #[test]
    fn reserved_stays_zero_through_every_constructor() {
        assert_eq!(Mat4::IDENTITY._reserved, [0; 4]);
        assert_eq!(Mat4::from_translation(Vec3::ZERO)._reserved, [0; 4]);
        assert_eq!(Mat4::from_rotation(Quat::IDENTITY)._reserved, [0; 4]);
        assert_eq!(Mat4::from_scale(Vec3::new(1.0, 1.0, 1.0))._reserved, [0; 4]);
        assert_eq!(
            Mat4::perspective(1.0, 1.0, 0.1, 10.0)
                .expect("valid")
                ._reserved,
            [0; 4]
        );
        assert_eq!(Mat4::default()._reserved, [0; 4]);
        assert_eq!(
            Mat4::from_scale(Vec3::new(2.0, 1.0, 1.0))
                .normal_matrix()
                ._reserved,
            [0; 4]
        );
    }

    /// Applies the upper 3×3 of `matrix` to `direction`, ignoring translation.
    fn rotate_direction(matrix: Mat4, direction: Vec3) -> Vec3 {
        let d = matrix.data;
        Vec3::new(
            d[0] * direction.x + d[4] * direction.y + d[8] * direction.z,
            d[1] * direction.x + d[5] * direction.y + d[9] * direction.z,
            d[2] * direction.x + d[6] * direction.y + d[10] * direction.z,
        )
    }

    #[test]
    fn the_normal_matrix_of_a_rotation_is_the_rotation() {
        let rotation =
            Mat4::from_rotation(Quat::from_axis_angle(Vec3::new(0.0, 1.0, 0.0), FRAC_PI_2));
        let normal = rotation.normal_matrix();

        // A pure rotation is orthogonal, so inverse-transpose is itself. The normal on
        // +X must end up where the geometry's +X face ends up, or the lit face and the
        // visible face disagree.
        let rotated = rotate_direction(normal, Vec3::new(1.0, 0.0, 0.0));
        let expected = rotate_direction(rotation, Vec3::new(1.0, 0.0, 0.0));

        assert_close(rotated.x, expected.x, "x");
        assert_close(rotated.y, expected.y, "y");
        assert_close(rotated.z, expected.z, "z");
    }

    #[test]
    fn the_normal_matrix_undoes_a_non_uniform_scale() {
        // This is the whole point of the inverse-transpose. Squash on X only: the
        // geometry's +X face comes closer to the centre but still faces +X, so its
        // normal must stay a unit +X. Multiplying by the model matrix instead would
        // give 0.5 on X, and normalising that is still +X here -- the visible error is
        // on the diagonal, which is the next assertion.
        let squashed = Mat4::from_scale(Vec3::new(0.5, 1.0, 1.0));
        let normal = squashed.normal_matrix();

        let side = rotate_direction(normal, Vec3::new(1.0, 0.0, 0.0)).normalized();
        assert_close(side.x, 1.0, "the +X normal direction");

        // A diagonal normal of the ellipsoid. The model matrix would shrink its X
        // component; the normal matrix must stretch it instead, tilting the normal
        // away from Y and towards X, which is what a lit squashed sphere shows.
        let diagonal = Vec3::new(1.0, 1.0, 0.0).normalized();
        let naive = rotate_direction(squashed, diagonal);
        let correct = rotate_direction(normal, diagonal).normalized();

        assert!(
            correct.x > naive.normalized().x,
            "the normal matrix must tilt the diagonal normal towards the squashed axis: \
             naive x={}, correct x={}",
            naive.normalized().x,
            correct.x
        );
    }

    #[test]
    fn the_normal_matrix_has_no_translation() {
        // A direction that picks up a translation rotates the lighting as the object
        // moves, which is the classic "the sun follows my model" bug.
        let moved = Mat4::from_translation(Vec3::new(10.0, -4.0, 7.0));
        let normal = moved.normal_matrix();

        for index in [12, 13, 14] {
            assert_close(normal.data[index], 0.0, "translation column");
        }
        assert_close(normal.data[15], 1.0, "w");
    }

    #[test]
    fn project_point_divides_by_w() {
        // An affine matrix has w = 1 everywhere, so projecting through it must agree with
        // `transform_point`. This is the case a caller is most likely to rely on.
        let model = Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0));
        let point = Vec3::new(-1.0, 0.5, 2.0);
        let projected = model.project_point(point).expect("in front");

        assert_close(projected.x, 0.0, "x");
        assert_close(projected.y, 2.5, "y");
        assert_close(projected.z, 5.0, "z");
        assert_eq!(projected, model.transform_point(point));
    }

    #[test]
    fn project_point_puts_the_centre_of_the_view_at_the_centre_of_the_frame() {
        let camera = Camera3D::from_parts(Vec3::ZERO, Vec3::new(0.0, 0.0, -1.0), Vec3::UP)
            .with_fov_y(FRAC_PI_2)
            .with_aspect(1.0);
        let view_proj = camera.view_projection().expect("valid");

        let on_axis = view_proj
            .project_point(Vec3::new(0.0, 0.0, -5.0))
            .expect("in front");
        assert_close(on_axis.x, 0.0, "x at the centre");
        assert_close(on_axis.y, 0.0, "y at the centre");

        // At 90 degrees of vertical FOV and an aspect of 1, a point 5 units away and 5
        // units up is exactly on the top edge of the frame: ndc.y == 1.
        let on_edge = view_proj
            .project_point(Vec3::new(0.0, 5.0, -5.0))
            .expect("in front");
        assert_close(on_edge.y, 1.0, "y on the top edge");
    }

    #[test]
    fn project_point_refuses_a_point_behind_the_eye() {
        // Without the `w > 0` guard the divide flips the sign and a point behind the camera
        // comes back looking like a point in front of it, which is how an object that
        // should be culled ends up drawn at the wrong place.
        let camera = Camera3D::from_parts(Vec3::ZERO, Vec3::new(0.0, 0.0, -1.0), Vec3::UP)
            .with_fov_y(FRAC_PI_2)
            .with_aspect(1.0);
        let view_proj = camera.view_projection().expect("valid");

        assert!(view_proj.project_point(Vec3::new(0.0, 0.0, 5.0)).is_none());
        assert!(view_proj.project_point(Vec3::new(0.0, 0.0, 0.0)).is_none());
    }

    #[test]
    fn a_singular_model_gets_the_identity_normal_matrix() {
        // A collapsing animation passes through a zero scale; the frame must render
        // rather than fill with NaN.
        let collapsed = Mat4::from_scale(Vec3::new(1.0, 0.0, 1.0));
        assert_eq!(collapsed.normal_matrix(), Mat4::IDENTITY);
    }
}
