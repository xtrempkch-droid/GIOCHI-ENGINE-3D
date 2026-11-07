//! C-shaped quaternion.
//!
//! Field order is `(x, y, z, w)` — **not** `(w, x, y, z)`. Getting this wrong produces
//! a camera that is subtly wrong rather than visibly broken, which is why the order is
//! stated in `docs/C_ABI_CONTRACT.md` §9.2 as well as here.

use glam::Quat as GlamQuat;

use super::Vec3;

/// A rotation quaternion, `#[repr(C)]`.
///
/// # C ABI
///
/// ```c
/// typedef struct GiochiQuat {
///     float    x, y, z, w;
///     uint32_t _reserved[4];
/// } GiochiQuat;                     /* size 32, align 4 */
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quat {
    /// X component (imaginary).
    pub x: f32,
    /// Y component (imaginary).
    pub y: f32,
    /// Z component (imaginary).
    pub z: f32,
    /// W component (real part).
    pub w: f32,
    /// Reserved for future ABI-compatible fields. Must be zero.
    pub _reserved: [u32; 4],
}

impl Quat {
    /// The identity rotation: no rotation at all.
    pub const IDENTITY: Self = Self::new(0.0, 0.0, 0.0, 1.0);

    /// Creates a quaternion in `(x, y, z, w)` order.
    pub const fn new(x: f32, y: f32, z: f32, w: f32) -> Self {
        Self {
            x,
            y,
            z,
            w,
            _reserved: [0; 4],
        }
    }

    /// Builds a rotation of `angle` radians about `axis`.
    ///
    /// A zero-length axis has no direction; the result is the identity rather than
    /// NaN, matching [`Vec3::normalized`].
    pub fn from_axis_angle(axis: Vec3, angle: f32) -> Self {
        let axis = axis.to_glam();
        if axis.length_squared() == 0.0 {
            return Self::IDENTITY;
        }
        Self::from_glam(GlamQuat::from_axis_angle(axis.normalize(), angle))
    }

    /// Builds a rotation from Euler angles in radians, applied Z then Y then X.
    pub fn from_euler_angles(x: f32, y: f32, z: f32) -> Self {
        Self::from_glam(GlamQuat::from_euler(glam::EulerRot::XYZ, x, y, z))
    }

    /// Converts to `glam` for arithmetic.
    ///
    /// Not `const`: `glam`'s `from_xyzw` derefs internally, which a const fn may not do.
    pub fn to_glam(self) -> GlamQuat {
        GlamQuat::from_xyzw(self.x, self.y, self.z, self.w)
    }

    /// Builds from `glam`.
    pub fn from_glam(value: GlamQuat) -> Self {
        Self::new(value.x, value.y, value.z, value.w)
    }

    /// Rotates `point` by this quaternion.
    pub fn rotate_vec3(self, point: Vec3) -> Vec3 {
        Vec3::from_glam(self.to_glam() * point.to_glam())
    }

    /// Returns the unit quaternion, or the identity when it cannot be normalised.
    ///
    /// Callers pass quaternions across the C boundary, where nothing prevents a
    /// zero-length or NaN value. Normalising keeps the model matrix orthonormal;
    /// falling back to the identity keeps the scene visible instead of blank.
    pub fn normalized(self) -> Self {
        let glam = self.to_glam();
        let length_squared = glam.length_squared();
        if length_squared == 0.0 || !length_squared.is_finite() {
            return Self::IDENTITY;
        }
        Self::from_glam(glam.normalize())
    }

    /// Returns `true` when every component is finite.
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite() && self.w.is_finite()
    }
}

impl Default for Quat {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl From<GlamQuat> for Quat {
    fn from(value: GlamQuat) -> Self {
        Self::from_glam(value)
    }
}

impl From<Quat> for GlamQuat {
    fn from(value: Quat) -> Self {
        value.to_glam()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{align_of, size_of};
    use std::f32::consts::PI;

    #[test]
    fn layout_is_the_documented_32_bytes() {
        assert_eq!(size_of::<Quat>(), 32);
        assert_eq!(align_of::<Quat>(), 4);
    }

    #[test]
    fn default_is_the_identity() {
        assert_eq!(Quat::default(), Quat::IDENTITY);
        assert_eq!(Quat::IDENTITY._reserved, [0; 4]);
        assert_eq!(
            (Quat::IDENTITY.x, Quat::IDENTITY.y, Quat::IDENTITY.z),
            (0.0, 0.0, 0.0)
        );
        assert_eq!(Quat::IDENTITY.w, 1.0);
    }

    #[test]
    fn field_order_is_xyzw() {
        // A 90 degree turn about +Z is (0, 0, sin(45), cos(45)).
        let half_turn = Quat::from_axis_angle(Vec3::new(0.0, 0.0, 1.0), PI / 2.0);
        let expected = (PI / 4.0).sin();

        assert!(
            (half_turn.z - expected).abs() < 1e-6,
            "z is the axis component"
        );
        assert!((half_turn.w - expected).abs() < 1e-6, "w is the real part");
        assert!(half_turn.x.abs() < 1e-6);
        assert!(half_turn.y.abs() < 1e-6);
    }

    #[test]
    fn a_zero_axis_yields_the_identity_not_nan() {
        let rotation = Quat::from_axis_angle(Vec3::ZERO, 1.0);

        assert_eq!(rotation, Quat::IDENTITY);
        assert!(rotation.is_finite());
    }

    #[test]
    fn normalizing_a_broken_quaternion_falls_back_to_the_identity() {
        assert_eq!(Quat::new(0.0, 0.0, 0.0, 0.0).normalized(), Quat::IDENTITY);
        assert_eq!(
            Quat::new(f32::NAN, 0.0, 0.0, 1.0).normalized(),
            Quat::IDENTITY
        );
    }

    #[test]
    fn rotating_the_unit_x_axis_ninety_degrees_about_z_gives_plus_y() {
        let rotation = Quat::from_axis_angle(Vec3::new(0.0, 0.0, 1.0), PI / 2.0);
        let rotated = rotation.rotate_vec3(Vec3::new(1.0, 0.0, 0.0));

        assert!(rotated.x.abs() < 1e-6, "got {rotated:?}");
        assert!((rotated.y - 1.0).abs() < 1e-6, "got {rotated:?}");
    }

    #[test]
    fn identity_leaves_a_vector_untouched() {
        let point = Vec3::new(1.5, -2.5, 7.0);
        assert_eq!(Quat::IDENTITY.rotate_vec3(point), point);
    }

    #[test]
    fn round_trips_through_glam() {
        let original = Quat::from_euler_angles(0.1, 0.2, 0.3);
        let back = Quat::from_glam(original.to_glam());

        assert_eq!(back, original);
        assert_eq!(back._reserved, [0; 4]);
    }
}
