//! The selection's bounding box, dashed.
//!
//! The viewport already draws a solid box around every mesh ([`crate::editor::viewport`]).
//! That is the *world*, and it has to stay readable when ten objects are on screen. What
//! this adds is the one box that matters while a tool is armed: the extent of the selected
//! object, drawn dashed so it reads as an annotation rather than as geometry.
//!
//! Dashes are produced by walking each edge in fixed steps and drawing alternate ones. The
//! engine's renderer has no line style, and a shader is not available for a 2D line in this
//! path, so the dash *is* the primitive: fourteen segments per edge, seven of them drawn.
//! The step count is per edge rather than per world unit so a large box and a small box dash
//! the same number of times — a 1 cm object would otherwise be a solid line.
//!
//! Every edge goes through [`crate::editor::viewport::draw_segment`], so the same near-plane
//! clipping that protects the scene protects the outline: a box with a corner behind the
//! camera loses that corner instead of wrapping around the screen.

use egui::Rect;
use giochi_engine::{Color, RenderContext};
use glam::{Mat4, Vec3};

use crate::editor::viewport::{draw_segment, project_point};
use crate::editor::OrbitCamera;

/// Segments per edge.
pub const DASHES_PER_EDGE: usize = 14;

/// Draws `min..max` as a dashed wireframe box.
pub fn draw_dashed_aabb(
    render: &mut RenderContext,
    rect: Rect,
    camera: &OrbitCamera,
    min: [f32; 3],
    max: [f32; 3],
    color: Color,
) {
    let view_projection = projection(camera, rect);
    let min = Vec3::from(min);
    let max = Vec3::from(max);

    // The eight corners, indexed so edge `i` joins `CORNERS[i]` to `CORNERS[i+1]` below.
    let corners = [
        Vec3::new(min.x, min.y, min.z),
        Vec3::new(max.x, min.y, min.z),
        Vec3::new(max.x, max.y, min.z),
        Vec3::new(min.x, max.y, min.z),
        Vec3::new(min.x, min.y, max.z),
        Vec3::new(max.x, min.y, max.z),
        Vec3::new(max.x, max.y, max.z),
        Vec3::new(min.x, max.y, max.z),
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

    for (from, to) in EDGES {
        dashed_edge(
            render,
            rect,
            &view_projection,
            corners[from],
            corners[to],
            color,
        );
    }
}

/// One edge, every other step drawn.
fn dashed_edge(
    render: &mut RenderContext,
    rect: Rect,
    view_projection: &Mat4,
    from: Vec3,
    to: Vec3,
    color: Color,
) {
    for step in (0..DASHES_PER_EDGE).step_by(2) {
        let t0 = step as f32 / DASHES_PER_EDGE as f32;
        let t1 = (step + 1) as f32 / DASHES_PER_EDGE as f32;
        draw_segment(
            render,
            rect,
            view_projection,
            from.lerp(to, t0),
            from.lerp(to, t1),
            color,
        );
    }
}

/// A colour the outline uses: the theme's accent, so it matches the selection outline
/// without being mistaken for it.
pub const fn default_color() -> Color {
    Color::rgb(0xff, 0xd0, 0x4a)
}

fn projection(camera: &OrbitCamera, rect: Rect) -> Mat4 {
    Mat4::from_cols_array(&camera.view_projection(rect.width() / rect.height().max(1.0)))
}

/// Whether a box is worth drawing at all: an empty or degenerate one is a single point.
pub fn is_visible(min: [f32; 3], max: [f32; 3]) -> bool {
    (max[0] - min[0]) + (max[1] - min[1]) + (max[2] - min[2]) > 1e-5
}

/// Where a corner projects, for tests and for a caller that wants a label anchor.
pub fn corner_on_screen(camera: &OrbitCamera, rect: Rect, point: [f32; 3]) -> Option<egui::Pos2> {
    project_point(Vec3::from(point), &projection(camera, rect), rect)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_degenerate_box_is_not_visible() {
        assert!(!is_visible([0.0; 3], [0.0; 3]));
        assert!(is_visible([-0.5; 3], [0.5; 3]));
    }

    #[test]
    fn a_dash_pattern_leaves_gaps() {
        // Half the steps are drawn; if that ratio drifts the box reads as solid or as dots.
        let drawn = (0..DASHES_PER_EDGE).step_by(2).count();
        assert_eq!(drawn, DASHES_PER_EDGE / 2);
    }
}
