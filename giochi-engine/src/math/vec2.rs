//! Two-dimensional vector.

/// A 2D vector, in world or screen space.
///
/// # C ABI
///
/// ```c
/// typedef struct GiochiVec2 {
///     float    x, y;
///     uint32_t _reserved[4];
/// } GiochiVec2;
/// ```
///
/// Size: 24 bytes, alignment: 4 bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Vec2 {
    /// Horizontal component.
    pub x: f32,
    /// Vertical component.
    pub y: f32,
    /// Reserved for future ABI-compatible fields. Must be zero.
    pub _reserved: [u32; 4],
}

impl Vec2 {
    /// The zero vector.
    pub const ZERO: Vec2 = Vec2::new(0.0, 0.0);

    /// Creates a vector from its components.
    pub const fn new(x: f32, y: f32) -> Self {
        Self {
            x,
            y,
            _reserved: [0; 4],
        }
    }

    /// Creates a vector with both components set to `value`.
    pub const fn splat(value: f32) -> Self {
        Self::new(value, value)
    }

    /// Euclidean length.
    pub fn length(self) -> f32 {
        self.x.hypot(self.y)
    }

    /// Euclidean distance to `other`.
    pub fn distance(self, other: Vec2) -> f32 {
        (self - other).length()
    }

    /// Returns `true` when both components are finite.
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

impl std::ops::Add for Vec2 {
    type Output = Vec2;

    fn add(self, other: Vec2) -> Vec2 {
        Vec2::new(self.x + other.x, self.y + other.y)
    }
}

impl std::ops::Sub for Vec2 {
    type Output = Vec2;

    fn sub(self, other: Vec2) -> Vec2 {
        Vec2::new(self.x - other.x, self.y - other.y)
    }
}

impl std::ops::Mul<f32> for Vec2 {
    type Output = Vec2;

    fn mul(self, factor: f32) -> Vec2 {
        Vec2::new(self.x * factor, self.y * factor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_zeroes_the_reserved_tail() {
        let vector = Vec2::new(1.5, -2.5);

        assert_eq!((vector.x, vector.y), (1.5, -2.5));
        assert_eq!(vector._reserved, [0; 4]);
    }

    #[test]
    fn splat_sets_both_components() {
        assert_eq!(Vec2::splat(3.0), Vec2::new(3.0, 3.0));
    }

    #[test]
    fn zero_is_the_additive_identity() {
        let vector = Vec2::new(4.0, -7.0);

        assert_eq!(vector + Vec2::ZERO, vector);
        assert_eq!(vector - vector, Vec2::ZERO);
    }

    #[test]
    fn operators_work_component_wise() {
        assert_eq!(
            Vec2::new(1.0, 2.0) + Vec2::new(3.0, 4.0),
            Vec2::new(4.0, 6.0)
        );
        assert_eq!(
            Vec2::new(3.0, 4.0) - Vec2::new(1.0, 1.0),
            Vec2::new(2.0, 3.0)
        );
    }

    #[test]
    fn scalar_multiplication_scales_both_components() {
        assert_eq!(Vec2::new(2.0, -3.0) * 2.5, Vec2::new(5.0, -7.5));
    }

    #[test]
    fn length_and_distance_use_the_euclidean_metric() {
        let vector = Vec2::new(3.0, 4.0);

        assert_eq!(vector.length(), 5.0);
        assert_eq!(Vec2::ZERO.distance(vector), 5.0);
    }

    #[test]
    fn finiteness_is_reported_per_component() {
        assert!(Vec2::ZERO.is_finite());
        assert!(!Vec2::new(f32::NAN, 0.0).is_finite());
        assert!(!Vec2::new(0.0, f32::INFINITY).is_finite());
    }
}
