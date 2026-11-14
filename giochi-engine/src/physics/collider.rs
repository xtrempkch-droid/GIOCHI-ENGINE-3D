//! Collision shapes.

/// A shape a body can be touched by.
///
/// The 2D shapes are the ones Rapier 2D supports directly. Extents are **half sizes**
/// (the distance from the centre to the edge), which is what Rapier takes and what
/// keeps a `1x1` box expressed as `0.5, 0.5` instead of a `width/height` pair that every
/// caller halves anyway.
///
/// ```no_run
/// use giochi_engine::physics::ColliderShape;
///
/// assert!(ColliderShape::circle(0.5).is_valid());
/// assert!(!ColliderShape::circle(0.0).is_valid());
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColliderShape {
    /// An axis-aligned box with the given half extents.
    Box {
        /// Half width.
        half_width: f32,
        /// Half height.
        half_height: f32,
    },
    /// A circle.
    Circle {
        /// Radius.
        radius: f32,
    },
    /// A capsule: a rectangle with rounded ends, standing upright.
    Capsule {
        /// Half height of the straight section, not counting the caps.
        half_height: f32,
        /// Radius of the caps.
        radius: f32,
    },
}

impl ColliderShape {
    /// A box from its half extents.
    pub fn boxed(half_width: f32, half_height: f32) -> ColliderShape {
        ColliderShape::Box {
            half_width,
            half_height,
        }
    }

    /// A circle from its radius.
    pub fn circle(radius: f32) -> ColliderShape {
        ColliderShape::Circle { radius }
    }

    /// A capsule from its straight half height and cap radius.
    pub fn capsule(half_height: f32, radius: f32) -> ColliderShape {
        ColliderShape::Capsule {
            half_height,
            radius,
        }
    }

    /// Whether every extent is finite and positive.
    ///
    /// Zero is rejected, not clamped: a zero-extent collider is a shape Rapier will
    /// happily accept and then never collide with, which reads as "physics is broken"
    /// much later.
    pub fn is_valid(&self) -> bool {
        match *self {
            ColliderShape::Box {
                half_width,
                half_height,
            } => {
                half_width.is_finite()
                    && half_height.is_finite()
                    && half_width > 0.0
                    && half_height > 0.0
            }
            ColliderShape::Circle { radius } => radius.is_finite() && radius > 0.0,
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
        assert!(ColliderShape::boxed(1.0, 2.0).is_valid());
        assert!(!ColliderShape::boxed(0.0, 2.0).is_valid());
        assert!(!ColliderShape::boxed(1.0, -0.5).is_valid());
        assert!(!ColliderShape::boxed(f32::NAN, 1.0).is_valid());
        assert!(!ColliderShape::circle(0.0).is_valid());
        assert!(!ColliderShape::circle(f32::INFINITY).is_valid());
        assert!(ColliderShape::capsule(0.5, 0.25).is_valid());
        assert!(!ColliderShape::capsule(0.0, 0.25).is_valid());
        assert!(!ColliderShape::capsule(0.5, 0.0).is_valid());
    }
}
