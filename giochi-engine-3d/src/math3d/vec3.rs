//! C-shaped 3D vector.
//!
//! `glam::Vec3` cannot appear in a `#[repr(C)]` struct field, so the public type is
//! three floats plus a reserved tail. Conversions to and from `glam` are free (no
//! allocation, no branches) and live here so no other module has to know the layout.

use glam::Vec3 as GlamVec3;

/// A 3-component vector, `#[repr(C)]`.
///
/// # C ABI
///
/// ```c
/// typedef struct GiochiVec3 {
///     float    x, y, z;
///     uint32_t _reserved[4];
/// } GiochiVec3;                     /* size 28, align 4 */
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec3 {
    /// X component.
    pub x: f32,
    /// Y component.
    pub y: f32,
    /// Z component.
    pub z: f32,
    /// Reserved for future ABI-compatible fields. Must be zero.
    pub _reserved: [u32; 4],
}

impl Vec3 {
    /// The origin.
    pub const ZERO: Self = Self::new(0.0, 0.0, 0.0);
    /// Unit +Y, the conventional "up" for a 3D world.
    pub const UP: Self = Self::new(0.0, 1.0, 0.0);

    /// Creates a vector.
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self {
            x,
            y,
            z,
            _reserved: [0; 4],
        }
    }

    /// Returns the vector as `[x, y, z]`.
    pub const fn to_array(self) -> [f32; 3] {
        [self.x, self.y, self.z]
    }

    /// Converts to the `glam` type used for the actual arithmetic.
    pub const fn to_glam(self) -> GlamVec3 {
        GlamVec3::new(self.x, self.y, self.z)
    }

    /// Builds from the `glam` type.
    pub const fn from_glam(value: GlamVec3) -> Self {
        Self::new(value.x, value.y, value.z)
    }

    /// Euclidean length.
    pub fn length(self) -> f32 {
        self.to_glam().length()
    }

    /// Returns the unit vector, or `ZERO` when the length is zero.
    ///
    /// A zero vector has no direction; returning NaN would poison every matrix built
    /// from it, so the caller gets zero and can test for it.
    pub fn normalized(self) -> Self {
        let glam = self.to_glam();
        if glam.length_squared() == 0.0 || !glam.length_squared().is_finite() {
            return Self::ZERO;
        }
        Self::from_glam(glam.normalize())
    }

    /// Returns `true` when every component is finite.
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
}

impl From<GlamVec3> for Vec3 {
    fn from(value: GlamVec3) -> Self {
        Self::from_glam(value)
    }
}

impl From<Vec3> for GlamVec3 {
    fn from(value: Vec3) -> Self {
        value.to_glam()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{align_of, size_of};

    #[test]
    fn layout_is_the_documented_28_bytes() {
        assert_eq!(size_of::<Vec3>(), 28);
        assert_eq!(align_of::<Vec3>(), 4);
    }

    #[test]
    fn constructors_zero_the_reserved_tail() {
        assert_eq!(Vec3::new(1.0, 2.0, 3.0)._reserved, [0; 4]);
        assert_eq!(Vec3::ZERO._reserved, [0; 4]);
        assert_eq!(Vec3::UP._reserved, [0; 4]);
        assert_eq!(Vec3::default()._reserved, [0; 4]);
    }

    #[test]
    fn round_trips_through_glam() {
        let original = Vec3::new(1.5, -2.25, 3.75);
        let back = Vec3::from_glam(original.to_glam());

        assert_eq!(back, original);
        assert_eq!(back._reserved, [0; 4]);
    }

    #[test]
    fn normalizing_a_zero_vector_yields_zero_not_nan() {
        let unit = Vec3::ZERO.normalized();

        assert_eq!(unit, Vec3::ZERO);
        assert!(unit.is_finite(), "NaN here would poison every matrix");
    }

    #[test]
    fn normalizing_produces_unit_length() {
        let unit = Vec3::new(3.0, 0.0, 4.0).normalized();

        assert!((unit.length() - 1.0).abs() < 1e-6);
        assert!((unit.x - 0.6).abs() < 1e-6);
        assert!((unit.z - 0.8).abs() < 1e-6);
    }

    #[test]
    fn a_nan_vector_is_not_finite() {
        assert!(!Vec3::new(f32::NAN, 0.0, 0.0).is_finite());
        assert!(Vec3::new(1.0, 2.0, 3.0).is_finite());
    }
}
