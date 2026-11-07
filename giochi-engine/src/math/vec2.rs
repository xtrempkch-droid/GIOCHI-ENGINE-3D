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
}
