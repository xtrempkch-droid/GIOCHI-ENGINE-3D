//! World-to-screen camera.
//!
//! The camera defines which part of world space is visible. The transform is
//! deliberately simple (translate + uniform zoom around the viewport centre) so
//! it maps directly onto SDL2's destination-rectangle drawing:
//!
//! ```text
//! screen = viewport.center + (world - camera.position) * zoom
//! ```
//!
//! A `zoom` of `1.0` leaves sizes untouched; `2.0` magnifies twice. Screen Y
//! grows downwards, so the transform is applied identically to both axes.

use crate::math::{Rect, Vec2};

/// A 2D camera.
///
/// # C ABI
///
/// ```c
/// typedef struct GiochiCamera {
///     float       x, y, zoom;
///     GiochiRect  viewport;
///     uint32_t    _reserved[4];
/// } GiochiCamera;
/// ```
///
/// Size: 60 bytes, alignment: 4 bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// World X coordinate at the centre of the viewport.
    pub x: f32,
    /// World Y coordinate at the centre of the viewport.
    pub y: f32,
    /// Uniform magnification factor; `1.0` is one world unit per pixel.
    ///
    /// Non-finite and non-positive values are treated as `1.0` by every method,
    /// so a bad value degrades to "no zoom" instead of producing invisible or
    /// mirrored geometry.
    pub zoom: f32,
    /// Screen-space rectangle the camera draws into.
    pub viewport: Rect,
    /// Reserved for future ABI-compatible fields. Must be zero.
    pub _reserved: [u32; 4],
}

impl Camera {
    /// Creates a camera centred on the world origin with `zoom = 1.0`.
    pub const fn new(viewport: Rect) -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            zoom: 1.0,
            viewport,
            _reserved: [0; 4],
        }
    }

    /// Creates a camera centred on `(x, y)` that fills `viewport`.
    pub const fn with_position(viewport: Rect, x: f32, y: f32) -> Self {
        Self {
            x,
            y,
            zoom: 1.0,
            viewport,
            _reserved: [0; 4],
        }
    }

    /// Returns the same camera with a different zoom factor.
    ///
    /// The value is stored as given; sanitisation happens in the transform
    /// methods so that callers can observe exactly what they set.
    pub const fn zoomed(mut self, zoom: f32) -> Self {
        self.zoom = zoom;
        self
    }

    /// The zoom factor actually used by the transform, in sanitised form.
    pub fn effective_zoom(&self) -> f32 {
        if self.zoom.is_finite() && self.zoom > 0.0 {
            self.zoom
        } else {
            1.0
        }
    }

    /// Converts a world-space point to screen space.
    pub fn world_to_screen(&self, world: Vec2) -> Vec2 {
        let zoom = self.effective_zoom();
        let center = self.viewport_center();

        Vec2::new(
            center.x + (sanitize(world.x) - sanitize(self.x)) * zoom,
            center.y + (sanitize(world.y) - sanitize(self.y)) * zoom,
        )
    }

    /// Converts a screen-space point to world space.
    ///
    /// The exact inverse of [`Camera::world_to_screen`].
    pub fn screen_to_world(&self, screen: Vec2) -> Vec2 {
        let zoom = self.effective_zoom();
        let center = self.viewport_center();

        Vec2::new(
            (sanitize(screen.x) - center.x) / zoom + sanitize(self.x),
            (sanitize(screen.y) - center.y) / zoom + sanitize(self.y),
        )
    }

    /// Converts a world-space rectangle to a screen-space rectangle.
    ///
    /// Both the position and the size are scaled by the zoom factor.
    pub fn world_rect_to_screen(&self, world: Rect) -> Rect {
        let zoom = self.effective_zoom();
        let origin = self.world_to_screen(Vec2::new(sanitize(world.x), sanitize(world.y)));

        Rect::new(
            origin.x,
            origin.y,
            sanitize(world.w) * zoom,
            sanitize(world.h) * zoom,
        )
    }

    /// Centre of the viewport, in screen space.
    pub fn viewport_center(&self) -> Vec2 {
        Vec2::new(
            self.viewport.x + self.viewport.w * 0.5,
            self.viewport.y + self.viewport.h * 0.5,
        )
    }
}

/// Replaces `NaN`/infinities with `0.0` so transforms stay finite.
fn sanitize(value: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn viewport() -> Rect {
        Rect::new(0.0, 0.0, 800.0, 600.0)
    }

    #[test]
    fn new_centres_on_the_world_origin() {
        let camera = Camera::new(viewport());

        assert_eq!(camera.x, 0.0);
        assert_eq!(camera.y, 0.0);
        assert_eq!(camera.zoom, 1.0);
        assert_eq!(camera._reserved, [0; 4]);
        assert_eq!(camera.viewport_center(), Vec2::new(400.0, 300.0));
    }

    #[test]
    fn world_origin_maps_to_the_viewport_centre() {
        let camera = Camera::new(viewport());
        let screen = camera.world_to_screen(Vec2::ZERO);

        assert_eq!(screen.x, 400.0);
        assert_eq!(screen.y, 300.0);
    }

    #[test]
    fn transform_translates_by_the_camera_position() {
        let camera = Camera::with_position(viewport(), 100.0, 50.0);

        // The camera position itself is the viewport centre.
        let centre = camera.world_to_screen(Vec2::new(100.0, 50.0));
        assert_eq!(centre, Vec2::new(400.0, 300.0));

        // A point to the right and below the camera moves right and down.
        let point = camera.world_to_screen(Vec2::new(110.0, 60.0));
        assert_eq!(point, Vec2::new(410.0, 310.0));
    }

    #[test]
    fn transform_scales_positions_and_sizes_by_the_zoom() {
        let camera = Camera::new(viewport()).zoomed(2.0);

        let screen = camera.world_to_screen(Vec2::new(10.0, 20.0));
        assert_eq!(screen, Vec2::new(420.0, 340.0));

        let rect = camera.world_rect_to_screen(Rect::new(10.0, 20.0, 5.0, 5.0));
        assert_eq!(rect, Rect::new(420.0, 340.0, 10.0, 10.0));
    }

    #[test]
    fn screen_to_world_inverts_world_to_screen() {
        let camera =
            Camera::with_position(Rect::new(16.0, 8.0, 1024.0, 768.0), -40.0, 120.0).zoomed(1.75);

        for point in [
            Vec2::new(0.0, 0.0),
            Vec2::new(-250.5, 333.25),
            Vec2::new(1000.0, -1000.0),
        ] {
            let round_trip = camera.screen_to_world(camera.world_to_screen(point));
            assert!(
                (round_trip.x - point.x).abs() < 1e-3 && (round_trip.y - point.y).abs() < 1e-3,
                "{point:?} round-tripped to {round_trip:?}"
            );
        }
    }

    #[test]
    fn a_viewport_with_an_offset_keeps_screen_coordinates_inside_it() {
        let camera = Camera::new(Rect::new(100.0, 50.0, 200.0, 100.0));

        assert_eq!(camera.viewport_center(), Vec2::new(200.0, 100.0));
        assert_eq!(camera.world_to_screen(Vec2::ZERO), Vec2::new(200.0, 100.0));
    }

    #[test]
    fn degenerate_zoom_falls_back_to_one() {
        for bad in [0.0, -2.0, f32::NAN, f32::INFINITY] {
            let camera = Camera::new(viewport()).zoomed(bad);

            assert_eq!(camera.effective_zoom(), 1.0, "zoom {bad} must be sanitised");
            assert!(
                camera.zoom == bad || (camera.zoom.is_nan() && bad.is_nan()),
                "the stored value must be untouched, got {}",
                camera.zoom
            );
            assert_eq!(
                camera.world_to_screen(Vec2::new(5.0, 5.0)),
                Vec2::new(405.0, 305.0)
            );
        }
    }

    #[test]
    fn non_finite_inputs_do_not_poison_the_transform() {
        let camera = Camera::new(viewport());

        let screen = camera.world_to_screen(Vec2::new(f32::NAN, f32::INFINITY));

        assert!(screen.x.is_finite() && screen.y.is_finite());
        assert_eq!(screen, Vec2::new(400.0, 300.0));
    }

    #[test]
    fn world_rect_to_screen_preserves_order_of_edges() {
        let camera = Camera::with_position(viewport(), 0.0, 0.0).zoomed(0.5);
        let rect = camera.world_rect_to_screen(Rect::new(-100.0, -100.0, 200.0, 200.0));

        assert!(rect.x < rect.right());
        assert!(rect.y < rect.bottom());
        assert_eq!((rect.w, rect.h), (100.0, 100.0));
    }
}
