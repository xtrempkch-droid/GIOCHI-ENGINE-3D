//! Immediate-mode debug geometry: lines, boxes and grids drawn on top of the frame.
//!
//! # Why a buffer instead of immediate draws
//!
//! A physics debug view submits hundreds of segments from inside the physics step, which
//! is not where drawing can happen (the renderer is borrowed by the frame). The calls
//! therefore append to a buffer, [`DebugDraw::render`] flushes it once per frame, and
//! [`DebugDraw::clear`] empties it at the end — "immediate mode" from the caller's point
//! of view, "one pass" from the renderer's.
//!
//! # The 3D calls
//!
//! `draw_aabb_3d`, `draw_ray_3d` and `draw_grid_3d` take a **column-major view-projection
//! matrix** as `[f32; 16]`, the same layout `giochi_engine_3d::math3d::Mat4::to_cols_array`
//! produces. They project, then emit 2D segments, which is why they work in the 2D
//! renderer and over a 3D scene's overlay alike. A dedicated GL_LINES pass belongs to the
//! 3D crate (phase 22/24); see `docs/DEVTOOLS.md` §6 for why the projection lives here.
//!
//! A point behind the camera (clip `w <= 0`) is **dropped** rather than wrapped around the
//! screen: a wireframe box with one vertex at infinity is worse than no box.

use crate::math::{Color, Rect, Vec2};
use crate::render::RenderContext;

/// One buffered segment.
#[derive(Debug, Clone, Copy)]
struct Segment {
    from: Vec2,
    to: Vec2,
    color: Color,
}

/// One buffered filled rectangle.
#[derive(Debug, Clone, Copy)]
struct Box2D {
    rect: Rect,
    color: Color,
}

/// Collects debug geometry for one frame and draws it in one pass.
///
/// ```no_run
/// use giochi_engine::devtools::DebugDraw;
/// use giochi_engine::{Color, Rect, Vec2};
///
/// let mut debug = DebugDraw::new();
/// debug.set_visible(true);
/// debug.draw_line_2d(Vec2::ZERO, Vec2::new(100.0, 100.0), Color::rgb(0, 255, 0));
/// debug.draw_rect_2d(Rect::new(10.0, 10.0, 20.0, 20.0), Color::rgb(255, 0, 0));
/// // once per frame, after the scene has drawn:
/// // debug.render(&mut render);
/// // debug.clear();
/// ```
///
/// # C ABI
///
/// Not exposed.
#[repr(C)]
pub struct DebugDraw {
    visible: bool,
    segments: Vec<Segment>,
    rectangles: Vec<Box2D>,
    /// Drawable size, needed to turn NDC into pixels for the 3D calls.
    viewport: (f32, f32),
    _reserved: [u32; 4],
}

impl std::fmt::Debug for DebugDraw {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DebugDraw")
            .field("visible", &self.visible)
            .field("segments", &self.segments.len())
            .field("rectangles", &self.rectangles.len())
            .finish_non_exhaustive()
    }
}

impl Default for DebugDraw {
    fn default() -> Self {
        Self::new()
    }
}

impl DebugDraw {
    /// Creates a hidden buffer.
    pub fn new() -> Self {
        Self {
            visible: false,
            segments: Vec::new(),
            rectangles: Vec::new(),
            viewport: (1.0, 1.0),
            _reserved: [0; 4],
        }
    }

    /// Whether [`Self::render`] draws anything.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Shows or hides the geometry. `F3` toggles this.
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    /// Flips visibility and returns the new state.
    pub fn toggle(&mut self) -> bool {
        self.visible = !self.visible;
        self.visible
    }

    /// How many segments are buffered.
    pub fn segment_count(&self) -> usize {
        self.segments.len()
    }

    /// How many filled rectangles are buffered.
    pub fn rect_count(&self) -> usize {
        self.rectangles.len()
    }

    /// Empties the buffer. Call it at the end of the frame.
    pub fn clear(&mut self) {
        self.segments.clear();
        self.rectangles.clear();
    }

    /// Sets the drawable size the 3D calls project into.
    ///
    /// [`Self::render`] refreshes it from the render context, so a caller only needs this
    /// when it projects *before* having one.
    pub fn set_viewport(&mut self, width: f32, height: f32) {
        if width > 0.0 && height > 0.0 {
            self.viewport = (width, height);
        }
    }

    /// A segment in screen space.
    pub fn draw_line_2d(&mut self, from: Vec2, to: Vec2, color: Color) {
        self.segments.push(Segment { from, to, color });
    }

    /// An outlined rectangle in screen space.
    pub fn draw_rect_2d(&mut self, rect: Rect, color: Color) {
        self.rectangles.push(Box2D { rect, color });
    }

    /// A grid of `spacing` pixels inside `area`.
    ///
    /// Skips a spacing of zero or less: a grid with an infinite number of lines is a
    /// hang, not a debugging aid.
    pub fn draw_grid_2d(&mut self, area: Rect, spacing: f32, color: Color) {
        if spacing <= 0.0 || !spacing.is_finite() {
            return;
        }
        let right = area.x + area.w;
        let bottom = area.y + area.h;

        let mut x = area.x;
        while x <= right {
            self.draw_line_2d(Vec2::new(x, area.y), Vec2::new(x, bottom), color);
            x += spacing;
        }
        let mut y = area.y;
        while y <= bottom {
            self.draw_line_2d(Vec2::new(area.x, y), Vec2::new(right, y), color);
            y += spacing;
        }
    }

    /// The twelve edges of an axis-aligned box, projected.
    pub fn draw_aabb_3d(
        &mut self,
        min: [f32; 3],
        max: [f32; 3],
        view_projection: &[f32; 16],
        color: Color,
    ) {
        let corners = [
            [min[0], min[1], min[2]],
            [max[0], min[1], min[2]],
            [max[0], max[1], min[2]],
            [min[0], max[1], min[2]],
            [min[0], min[1], max[2]],
            [max[0], min[1], max[2]],
            [max[0], max[1], max[2]],
            [min[0], max[1], max[2]],
        ];
        const EDGES: [(usize, usize); 12] = [
            (0, 1),
            (1, 2),
            (2, 3),
            (3, 0),
            (4, 5),
            (5, 6),
            (6, 7),
            (7, 4),
            (0, 4),
            (1, 5),
            (2, 6),
            (3, 7),
        ];

        let projected: Vec<Option<Vec2>> = corners
            .iter()
            .map(|corner| project(*corner, view_projection, self.viewport))
            .collect();

        for (a, b) in EDGES {
            if let (Some(from), Some(to)) = (projected[a], projected[b]) {
                self.draw_line_2d(from, to, color);
            }
        }
    }

    /// A ray from `origin` along `direction`, `length` units long, projected.
    pub fn draw_ray_3d(
        &mut self,
        origin: [f32; 3],
        direction: [f32; 3],
        length: f32,
        view_projection: &[f32; 16],
        color: Color,
    ) {
        let end = [
            origin[0] + direction[0] * length,
            origin[1] + direction[1] * length,
            origin[2] + direction[2] * length,
        ];
        if let (Some(from), Some(to)) = (
            project(origin, view_projection, self.viewport),
            project(end, view_projection, self.viewport),
        ) {
            self.draw_line_2d(from, to, color);
        }
    }

    /// A floor grid on the XZ plane at `y`, from `-extent` to `+extent`.
    ///
    /// `divisions` is the number of cells per side; a value below 1 draws nothing.
    pub fn draw_grid_3d(
        &mut self,
        y: f32,
        extent: f32,
        divisions: u32,
        view_projection: &[f32; 16],
        color: Color,
    ) {
        if divisions == 0 || !extent.is_finite() || extent <= 0.0 {
            return;
        }
        let step = (extent * 2.0) / divisions as f32;

        for index in 0..=divisions {
            let offset = -extent + step * index as f32;
            self.draw_projected_segment(
                [offset, y, -extent],
                [offset, y, extent],
                view_projection,
                color,
            );
            self.draw_projected_segment(
                [-extent, y, offset],
                [extent, y, offset],
                view_projection,
                color,
            );
        }
    }

    /// Projects and emits one segment, dropping it if either end is behind the camera.
    fn draw_projected_segment(
        &mut self,
        from: [f32; 3],
        to: [f32; 3],
        view_projection: &[f32; 16],
        color: Color,
    ) {
        if let (Some(from), Some(to)) = (
            project(from, view_projection, self.viewport),
            project(to, view_projection, self.viewport),
        ) {
            self.draw_line_2d(from, to, color);
        }
    }

    /// Draws everything buffered and then empties it.
    ///
    /// Clearing here rather than in the engine means a caller that forgets cannot
    /// accumulate a frame's geometry forever — the failure mode of a debug tool is
    /// "slower and slower", which is precisely what it is supposed to detect.
    pub fn render(&mut self, render: &mut RenderContext) {
        if !self.visible {
            self.clear();
            return;
        }

        let (width, height) = render.size();
        self.set_viewport(width as f32, height as f32);

        for rectangle in &self.rectangles {
            render.stroke_rect(rectangle.rect, rectangle.color);
        }
        for segment in &self.segments {
            render.draw_line(segment.from, segment.to, segment.color);
        }

        self.clear();
    }
}

/// Projects a world point to screen pixels, or `None` when it is behind the camera.
///
/// Column-major `m`: `clip = m * vec4(point, 1)` is
/// `clip.x = m[0]x + m[4]y + m[8]z + m[12]`, and so on.
fn project(point: [f32; 3], m: &[f32; 16], viewport: (f32, f32)) -> Option<Vec2> {
    let [x, y, z] = point;
    let clip_x = m[0] * x + m[4] * y + m[8] * z + m[12];
    let clip_y = m[1] * x + m[5] * y + m[9] * z + m[13];
    let clip_w = m[3] * x + m[7] * y + m[11] * z + m[15];

    // `w <= 0` is at or behind the eye plane: dividing would mirror the point through the
    // centre of the screen, which draws a convincing lie.
    if !clip_w.is_finite() || clip_w <= f32::EPSILON {
        return None;
    }

    let ndc_x = clip_x / clip_w;
    let ndc_y = clip_y / clip_w;
    if !ndc_x.is_finite() || !ndc_y.is_finite() {
        return None;
    }

    Some(Vec2::new(
        (ndc_x * 0.5 + 0.5) * viewport.0,
        // NDC is +Y up, screen coordinates are +Y down.
        (0.5 - ndc_y * 0.5) * viewport.1,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> [f32; 16] {
        [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ]
    }

    #[test]
    fn a_new_buffer_is_hidden_and_empty() {
        let debug = DebugDraw::new();
        assert!(!debug.is_visible());
        assert_eq!(debug.segment_count(), 0);
        assert_eq!(debug.rect_count(), 0);
    }

    #[test]
    fn toggling_reports_the_new_state() {
        let mut debug = DebugDraw::new();
        assert!(debug.toggle());
        assert!(debug.is_visible());
        assert!(!debug.toggle());
        assert!(!debug.is_visible());
    }

    #[test]
    fn lines_and_rectangles_accumulate_until_cleared() {
        let mut debug = DebugDraw::new();
        debug.draw_line_2d(Vec2::ZERO, Vec2::new(1.0, 1.0), Color::rgb(1, 2, 3));
        debug.draw_rect_2d(Rect::new(0.0, 0.0, 4.0, 4.0), Color::rgb(1, 2, 3));
        assert_eq!(debug.segment_count(), 1);
        assert_eq!(debug.rect_count(), 1);
        debug.clear();
        assert_eq!(debug.segment_count(), 0);
        assert_eq!(debug.rect_count(), 0);
    }

    #[test]
    fn a_two_by_two_grid_is_six_segments() {
        let mut debug = DebugDraw::new();
        debug.draw_grid_2d(Rect::new(0.0, 0.0, 10.0, 10.0), 5.0, Color::rgb(1, 2, 3));
        // Three vertical lines (0, 5, 10) and three horizontal ones.
        assert_eq!(debug.segment_count(), 6);
    }

    #[test]
    fn a_grid_with_no_spacing_is_ignored_rather_than_hanging() {
        let mut debug = DebugDraw::new();
        debug.draw_grid_2d(Rect::new(0.0, 0.0, 10.0, 10.0), 0.0, Color::rgb(1, 2, 3));
        debug.draw_grid_2d(
            Rect::new(0.0, 0.0, 10.0, 10.0),
            f32::NAN,
            Color::rgb(1, 2, 3),
        );
        assert_eq!(debug.segment_count(), 0);
    }

    #[test]
    fn the_centre_of_an_identity_projection_is_the_centre_of_the_screen() {
        let point = project([0.0, 0.0, 0.0], &identity(), (200.0, 100.0)).expect("visible");
        assert_eq!(point, Vec2::new(100.0, 50.0));
    }

    #[test]
    fn y_is_flipped_between_ndc_and_screen_coordinates() {
        let up = project([0.0, 1.0, 0.0], &identity(), (100.0, 100.0)).expect("visible");
        assert!(up.y < 50.0, "+Y in NDC is above the centre, got {up:?}");
        let down = project([0.0, -1.0, 0.0], &identity(), (100.0, 100.0)).expect("visible");
        assert!(down.y > 50.0, "-Y is below the centre, got {down:?}");
    }

    #[test]
    fn a_point_behind_the_eye_is_dropped() {
        // Column-major: `m[11]` is row 3, column 2 — the coefficient of `z` in `w`. With
        // `m[11] = 1`, w = z + 1, which is what a camera at the origin looking down -Z
        // produces: z = -1 is the eye plane and z < -1 is behind it.
        let mut m = identity();
        m[11] = 1.0;
        assert_eq!(project([0.0, 0.0, -2.0], &m, (10.0, 10.0)), None);
        assert!(project([0.0, 0.0, 1.0], &m, (10.0, 10.0)).is_some());
    }

    #[test]
    fn an_aabb_behind_the_camera_draws_nothing() {
        let mut debug = DebugDraw::new();
        debug.set_viewport(64.0, 64.0);
        let mut m = identity();
        m[11] = 1.0;
        // Every corner has w = z + 1 < 0.
        debug.draw_aabb_3d(
            [-1.0, -1.0, -4.0],
            [1.0, 1.0, -3.0],
            &m,
            Color::rgb(1, 2, 3),
        );
        assert_eq!(debug.segment_count(), 0);
    }

    #[test]
    fn an_aabb_in_front_of_the_camera_draws_twelve_edges() {
        let mut debug = DebugDraw::new();
        debug.set_viewport(64.0, 64.0);
        // `m[14] = 1` is a translation in z and leaves w = 1, so nothing is clipped and the
        // only thing under test is that a visible box emits its twelve edges.
        let mut m = identity();
        m[14] = 1.0;
        debug.draw_aabb_3d([-1.0; 3], [1.0; 3], &m, Color::rgb(1, 2, 3));
        assert_eq!(debug.segment_count(), 12);
    }

    #[test]
    fn a_ray_projects_two_points() {
        let mut debug = DebugDraw::new();
        debug.set_viewport(64.0, 64.0);
        let mut m = identity();
        m[11] = 1.0; // w = 1 + z
        debug.draw_ray_3d(
            [0.0, 0.0, 1.0],
            [0.0, 0.0, 1.0],
            2.0,
            &m,
            Color::rgb(1, 2, 3),
        );
        assert_eq!(debug.segment_count(), 1);
    }

    #[test]
    fn a_zero_division_grid_3d_draws_nothing() {
        let mut debug = DebugDraw::new();
        debug.set_viewport(64.0, 64.0);
        debug.draw_grid_3d(0.0, 10.0, 0, &identity(), Color::rgb(1, 2, 3));
        assert_eq!(debug.segment_count(), 0);
    }

    #[test]
    fn a_grid_3d_emits_two_segments_per_division() {
        let mut debug = DebugDraw::new();
        debug.set_viewport(64.0, 64.0);
        let mut m = identity();
        m[14] = 1.0;
        debug.draw_grid_3d(0.0, 10.0, 2, &m, Color::rgb(1, 2, 3));
        // 3 lines each way.
        assert_eq!(debug.segment_count(), 6);
    }
}
