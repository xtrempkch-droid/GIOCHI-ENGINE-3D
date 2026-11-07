//! Axis-aligned rectangle.

/// An axis-aligned rectangle.
///
/// `x`/`y` are the top-left corner; `w`/`h` are the extents. A rectangle with a
/// non-positive width or height is considered empty.
///
/// # C ABI
///
/// ```c
/// typedef struct GiochiRect {
///     float    x, y, w, h;
///     uint32_t _reserved[4];
/// } GiochiRect;
/// ```
///
/// Size: 32 bytes, alignment: 4 bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rect {
    /// Horizontal position of the top-left corner.
    pub x: f32,
    /// Vertical position of the top-left corner.
    pub y: f32,
    /// Width (extent on the X axis).
    pub w: f32,
    /// Height (extent on the Y axis).
    pub h: f32,
    /// Reserved for future ABI-compatible fields. Must be zero.
    pub _reserved: [u32; 4],
}

impl Rect {
    /// The empty rectangle at the origin.
    pub const ZERO: Rect = Rect::new(0.0, 0.0, 0.0, 0.0);

    /// Creates a rectangle from position and size.
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self {
            x,
            y,
            w,
            h,
            _reserved: [0; 4],
        }
    }

    /// Creates a rectangle from its top-left and bottom-right corners.
    pub fn from_corners(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self::new(left, top, right - left, bottom - top)
    }

    /// X coordinate of the right edge (`x + w`).
    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    /// Y coordinate of the bottom edge (`y + h`).
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    /// Returns `true` when the rectangle has no area.
    pub fn is_empty(&self) -> bool {
        self.w <= 0.0 || self.h <= 0.0
    }
}
