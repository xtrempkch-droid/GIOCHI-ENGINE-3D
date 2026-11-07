//! Position, rotation and scale, `#[repr(C)]`.
//!
//! Applied in the order scale → rotation → translation, the same order every scene
//! graph uses. Documented because getting it wrong looks like a broken pivot rather
//! than a wrong matrix.

use super::{Mat4, Quat, Vec3};

/// A model transform.
///
/// # C ABI
///
/// ```c
/// typedef struct GiochiTransform3D {
///     GiochiVec3 position;
///     GiochiQuat rotation;
///     GiochiVec3 scale;
///     uint32_t   _reserved[4];
/// } GiochiTransform3D;              /* size 104, align 4 */
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform3D {
    /// World position.
    pub position: Vec3,
    /// Rotation about `position`.
    pub rotation: Quat,
    /// Per-axis scale, applied before the rotation.
    pub scale: Vec3,
    /// Reserved for future ABI-compatible fields. Must be zero.
    pub _reserved: [u32; 4],
}

impl Transform3D {
    /// The identity transform: at the origin, unrotated, unscaled.
    pub const IDENTITY: Self = Self::new(Vec3::ZERO, Quat::IDENTITY, Vec3::new(1.0, 1.0, 1.0));

    /// Builds a transform from its three parts.
    pub const fn new(position: Vec3, rotation: Quat, scale: Vec3) -> Self {
        Self {
            position,
            rotation,
            scale,
            _reserved: [0; 4],
        }
    }

    /// A translation only.
    pub const fn from_translation(position: Vec3) -> Self {
        Self::new(position, Quat::IDENTITY, Vec3::new(1.0, 1.0, 1.0))
    }

    /// A rotation only.
    pub const fn from_rotation(rotation: Quat) -> Self {
        Self::new(Vec3::ZERO, rotation, Vec3::new(1.0, 1.0, 1.0))
    }

    /// A uniform scale only.
    pub const fn from_scale(scalar: f32) -> Self {
        Self::new(
            Vec3::ZERO,
            Quat::IDENTITY,
            Vec3::new(scalar, scalar, scalar),
        )
    }

    /// Returns the model matrix: translation · rotation · scale.
    ///
    /// The rotation is normalised first. A quaternion arriving through the C boundary
    /// can be zero-length or NaN, and either would propagate into every vertex of the
    /// mesh; [`Quat::normalized`] falls back to the identity so the model stays visible.
    pub fn matrix(&self) -> Mat4 {
        Mat4::from_translation(self.position)
            * Mat4::from_rotation(self.rotation.normalized())
            * Mat4::from_scale(self.scale)
    }

    /// Returns the same transform with a different position.
    pub const fn with_position(mut self, position: Vec3) -> Self {
        self.position = position;
        self
    }

    /// Returns the same transform with a different rotation.
    pub const fn with_rotation(mut self, rotation: Quat) -> Self {
        self.rotation = rotation;
        self
    }

    /// Returns the same transform with a different scale.
    pub const fn with_scale(mut self, scale: Vec3) -> Self {
        self.scale = scale;
        self
    }

    /// Returns `true` when every component is finite.
    pub fn is_finite(&self) -> bool {
        self.position.is_finite() && self.rotation.is_finite() && self.scale.is_finite()
    }
}

impl Default for Transform3D {
    fn default() -> Self {
        Self::IDENTITY
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{align_of, size_of};
    use std::f32::consts::FRAC_PI_2;

    fn assert_close(actual: f32, expected: f32, what: &str) {
        assert!(
            (actual - expected).abs() < 1e-5,
            "{what}: got {actual}, expected {expected}"
        );
    }

    #[test]
    fn layout_is_the_documented_104_bytes() {
        // Vec3(28) + Quat(32) + Vec3(28) + reserved(16).
        assert_eq!(size_of::<Transform3D>(), 104);
        assert_eq!(align_of::<Transform3D>(), 4);
    }

    #[test]
    fn identity_composes_to_the_identity_matrix() {
        assert_eq!(Transform3D::IDENTITY.matrix(), Mat4::IDENTITY);
    }

    #[test]
    fn the_composition_order_is_scale_then_rotate_then_translate() {
        // Scale by 2 about the origin, rotate 90 degrees about Z, then move to (10, 0, 0).
        // A point at +X should end up at (10, 2, 0):
        //   scale   (1,0,0) -> (2,0,0)
        //   rotate        -> (0,2,0)
        //   translate     -> (10,2,0)
        // Any other order gives a different answer, which is the whole point of the test.
        let transform = Transform3D::new(
            Vec3::new(10.0, 0.0, 0.0),
            Quat::from_axis_angle(Vec3::new(0.0, 0.0, 1.0), FRAC_PI_2),
            Vec3::new(2.0, 2.0, 2.0),
        );

        let result = transform.matrix().transform_point(Vec3::new(1.0, 0.0, 0.0));

        assert_close(result.x, 10.0, "x");
        assert_close(result.y, 2.0, "y");
        assert_close(result.z, 0.0, "z");
    }

    #[test]
    fn an_unscaled_unrotated_transform_is_a_pure_translation() {
        let transform = Transform3D::from_translation(Vec3::new(1.0, 2.0, 3.0));

        assert_eq!(
            transform.matrix().transform_point(Vec3::ZERO),
            Vec3::new(1.0, 2.0, 3.0)
        );
    }

    #[test]
    fn a_uniform_scale_constructor_scales_every_axis() {
        let transform = Transform3D::from_scale(3.0);

        assert_eq!(transform.scale, Vec3::new(3.0, 3.0, 3.0));
        assert_eq!(
            transform.matrix().transform_point(Vec3::new(1.0, 1.0, 1.0)),
            Vec3::new(3.0, 3.0, 3.0)
        );
    }

    #[test]
    fn a_broken_quaternion_does_not_poison_the_matrix() {
        // A zero quaternion arriving from C must not produce a NaN matrix: the model
        // would vanish with no error anywhere.
        let transform = Transform3D::new(
            Vec3::new(1.0, 0.0, 0.0),
            Quat::new(0.0, 0.0, 0.0, 0.0),
            Vec3::new(1.0, 1.0, 1.0),
        );

        let matrix = transform.matrix();
        assert!(matrix.is_finite(), "matrix contains NaN or infinity");
        assert_eq!(
            matrix.transform_point(Vec3::ZERO),
            Vec3::new(1.0, 0.0, 0.0),
            "falls back to no rotation"
        );
    }

    #[test]
    fn builders_do_not_disturb_the_other_parts() {
        let base = Transform3D::from_translation(Vec3::new(1.0, 2.0, 3.0));
        let moved = base.with_position(Vec3::new(4.0, 5.0, 6.0));

        assert_eq!(moved.position, Vec3::new(4.0, 5.0, 6.0));
        assert_eq!(moved.rotation, base.rotation);
        assert_eq!(moved.scale, base.scale);
        assert_eq!(moved._reserved, [0; 4]);
    }

    #[test]
    fn every_constructor_zeroes_the_reserved_tail() {
        assert_eq!(Transform3D::IDENTITY._reserved, [0; 4]);
        assert_eq!(Transform3D::default()._reserved, [0; 4]);
        assert_eq!(Transform3D::from_scale(2.0)._reserved, [0; 4]);
        assert_eq!(Transform3D::from_rotation(Quat::IDENTITY)._reserved, [0; 4]);
    }

    #[test]
    fn finite_checks_every_component() {
        assert!(Transform3D::IDENTITY.is_finite());
        assert!(!Transform3D::new(
            Vec3::new(f32::NAN, 0.0, 0.0),
            Quat::IDENTITY,
            Vec3::new(1.0, 1.0, 1.0)
        )
        .is_finite());
    }
}
