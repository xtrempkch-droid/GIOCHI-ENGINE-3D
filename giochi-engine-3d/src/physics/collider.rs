//! Collision shapes.

/// A shape a body can be touched by.
///
/// Extents are **half sizes**: the distance from the centre to the face, which is what
/// Rapier takes and what makes a `1x1x1` cube `cuboid(0.5, 0.5, 0.5)`.
///
/// ```no_run
/// use giochi_engine_3d::physics::ColliderShape;
///
/// assert!(ColliderShape::ball(0.5).is_valid());
/// assert!(!ColliderShape::ball(0.0).is_valid());
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColliderShape {
    /// An axis-aligned box with the given half extents.
    Cuboid {
        /// Half width (X).
        half_width: f32,
        /// Half height (Y).
        half_height: f32,
        /// Half depth (Z).
        half_depth: f32,
    },
    /// A sphere.
    Ball {
        /// Radius.
        radius: f32,
    },
    /// A capsule: a cylinder with rounded ends, standing upright.
    Capsule {
        /// Half height of the straight section, not counting the caps.
        half_height: f32,
        /// Radius of the caps.
        radius: f32,
    },
}

impl ColliderShape {
    /// A box from its half extents.
    pub fn cuboid(half_width: f32, half_height: f32, half_depth: f32) -> ColliderShape {
        ColliderShape::Cuboid {
            half_width,
            half_height,
            half_depth,
        }
    }

    /// A sphere from its radius.
    pub fn ball(radius: f32) -> ColliderShape {
        ColliderShape::Ball { radius }
    }

    /// A capsule from its straight half height and cap radius.
    pub fn capsule(half_height: f32, radius: f32) -> ColliderShape {
        ColliderShape::Capsule {
            half_height,
            radius,
        }
    }

    /// Whether every extent is finite and positive.
    pub fn is_valid(&self) -> bool {
        match *self {
            ColliderShape::Cuboid {
                half_width,
                half_height,
                half_depth,
            } => {
                half_width.is_finite()
                    && half_height.is_finite()
                    && half_depth.is_finite()
                    && half_width > 0.0
                    && half_height > 0.0
                    && half_depth > 0.0
            }
            ColliderShape::Ball { radius } => radius.is_finite() && radius > 0.0,
            ColliderShape::Capsule {
                half_height,
                radius,
            } => half_height.is_finite() && half_height > 0.0 && radius.is_finite() && radius > 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_extent_must_be_finite_and_positive() {
        assert!(ColliderShape::cuboid(1.0, 1.0, 1.0).is_valid());
        assert!(!ColliderShape::cuboid(1.0, 0.0, 1.0).is_valid());
        assert!(!ColliderShape::cuboid(1.0, 1.0, -1.0).is_valid());
        assert!(!ColliderShape::cuboid(f32::NAN, 1.0, 1.0).is_valid());
        assert!(ColliderShape::ball(0.5).is_valid());
        assert!(!ColliderShape::ball(0.0).is_valid());
        assert!(!ColliderShape::capsule(0.5, 0.0).is_valid());
    }
}
