//! Axis-aligned rectangle.

use crate::math::vec2::Vec2;

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

    /// Returns the intersection of two rectangles, or an empty rectangle when
    /// they do not overlap.
    pub fn intersection(&self, other: Rect) -> Rect {
        let left = self.x.max(other.x);
        let top = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());

        if right <= left || bottom <= top {
            Rect::ZERO
        } else {
            Rect::from_corners(left, top, right, bottom)
        }
    }

    /// Returns `true` when `point` lies inside the rectangle.
    ///
    /// The top and left edges are inclusive, the bottom and right edges are not,
    /// which keeps adjacent rectangles from both claiming a shared boundary.
    pub fn contains(&self, point: Vec2) -> bool {
        !self.is_empty()
            && point.x >= self.x
            && point.x < self.right()
            && point.y >= self.y
            && point.y < self.bottom()
    }

    /// Centre of the rectangle.
    pub fn center(&self) -> Vec2 {
        Vec2::new(self.x + self.w * 0.5, self.y + self.h * 0.5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_zeroes_the_reserved_tail() {
        assert_eq!(Rect::new(1.0, 2.0, 3.0, 4.0)._reserved, [0; 4]);
    }

    #[test]
    fn from_corners_computes_size() {
        assert_eq!(
            Rect::from_corners(10.0, 20.0, 40.0, 60.0),
            Rect::new(10.0, 20.0, 30.0, 40.0)
        );
    }

    #[test]
    fn empty_rects_are_detected() {
        assert!(Rect::ZERO.is_empty());
        assert!(Rect::new(0.0, 0.0, -1.0, 5.0).is_empty());
        assert!(!Rect::new(0.0, 0.0, 1.0, 1.0).is_empty());
    }

    #[test]
    fn contains_uses_half_open_edges() {
        let rect = Rect::new(10.0, 10.0, 100.0, 50.0);

        assert!(rect.contains(Vec2::new(10.0, 10.0)));
        assert!(rect.contains(Vec2::new(109.9, 59.9)));
        assert!(!rect.contains(Vec2::new(110.0, 30.0)));
        assert!(!rect.contains(Vec2::new(30.0, 60.0)));
        assert!(!rect.contains(Vec2::new(9.9, 30.0)));
        assert!(!Rect::ZERO.contains(Vec2::ZERO));
    }

    #[test]
    fn center_is_the_midpoint() {
        assert_eq!(
            Rect::new(0.0, 0.0, 800.0, 600.0).center(),
            Vec2::new(400.0, 300.0)
        );
    }

    #[test]
    fn intersection_of_overlapping_rects() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(5.0, 5.0, 10.0, 10.0);

        assert_eq!(a.intersection(b), Rect::new(5.0, 5.0, 5.0, 5.0));
    }

    #[test]
    fn intersection_of_disjoint_rects_is_empty() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(20.0, 20.0, 10.0, 10.0);

        assert!(a.intersection(b).is_empty());
    }

    #[test]
    fn touching_rects_do_not_intersect() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(10.0, 0.0, 10.0, 10.0);

        assert!(a.intersection(b).is_empty());
    }
}
