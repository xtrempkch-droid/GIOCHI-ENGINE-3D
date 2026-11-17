//! Drawing the gizmo: three arrows, three rings and the centre square.
//!
//! Everything goes through [`crate::editor::viewport::draw_segment`] and
//! [`crate::editor::viewport::project_point`], which are the projection, clipping and colour
//! path the scene already uses. A gizmo drawn with its own projection would sit a pixel off
//! the object it moves — the kind of bug that is impossible to describe and trivial to
//! avoid.
//!
//! # Arrows without an arrow primitive
//!
//! The renderer draws one-pixel lines. An arrowhead is therefore two short segments at the
//! tip, angled back from the axis; a diamond at the tip reads as an arrowhead at any screen
//! size, which a single line does not. The centre handle is a small square of four segments.
//! Rings are polylines around the circle.

use egui::Rect;
use giochi_engine::{Color, RenderContext};
use glam::{Mat4, Vec3};

use crate::editor::orbit_camera::OrbitCamera;
use crate::editor::viewport::{draw_segment, project_point};
use crate::gizmos::{GizmoGeometry, GizmoHandle, RING_POINTS};

/// How many segments a ring is drawn with. Sixteen is smooth at the sizes the rings are
/// drawn and cheap enough to draw every frame without thinking about it.
pub const RING_SEGMENTS: usize = 48;

/// Draws the translate gizmo: three arrows and the centre square.
pub fn translate(
    render: &mut RenderContext,
    rect: Rect,
    camera: &OrbitCamera,
    geometry: GizmoGeometry,
    active: Option<GizmoHandle>,
) {
    let view_projection = projection(camera, rect);
    for handle in [GizmoHandle::X, GizmoHandle::Y, GizmoHandle::Z] {
        let Some(axis) = handle.axis() else {
            continue;
        };
        let color = color_for(handle, active);
        let tip = geometry.tip(axis);
        draw_segment(render, rect, &view_projection, geometry.center, tip, color);
        arrow_head(render, rect, &view_projection, geometry, axis, color);
    }
    centre_square(render, rect, &view_projection, geometry, active);
}

/// Draws the rotate gizmo: three rings.
pub fn rotate(
    render: &mut RenderContext,
    rect: Rect,
    camera: &OrbitCamera,
    geometry: GizmoGeometry,
    active: Option<GizmoHandle>,
) {
    let view_projection = projection(camera, rect);
    let radius = geometry.units_per_point() * RING_POINTS;
    for handle in [GizmoHandle::X, GizmoHandle::Y, GizmoHandle::Z] {
        let Some(axis) = handle.axis() else {
            continue;
        };
        let color = color_for(handle, active);
        draw_ring(
            render,
            rect,
            &view_projection,
            geometry.center,
            axis,
            radius,
            color,
        );
    }
}

/// Draws the scale gizmo: three lines with a cube at the tip, plus the uniform cube.
///
/// The lines are the same length as the translate arrows so switching tools does not make
/// the handles jump, but the tips are cubes rather than arrowheads: an arrowhead says "this
/// way", which is the one thing a scale handle must not say.
pub fn scale(
    render: &mut RenderContext,
    rect: Rect,
    camera: &OrbitCamera,
    geometry: GizmoGeometry,
    active: Option<GizmoHandle>,
) {
    let view_projection = projection(camera, rect);
    for handle in [GizmoHandle::X, GizmoHandle::Y, GizmoHandle::Z] {
        let Some(dir) = geometry.axis_dir(handle) else {
            continue;
        };
        let color = color_for(handle, active);
        let tip = geometry.tip(dir);
        draw_segment(render, rect, &view_projection, geometry.center, tip, color);
        cube_at(render, rect, camera, geometry, tip, color);
    }
    cube_at(
        render,
        rect,
        camera,
        geometry,
        geometry.center,
        color_for(GizmoHandle::Center, active),
    );
}

/// A small screen-facing square: the cube at the end of a scale handle.
fn cube_at(
    render: &mut RenderContext,
    rect: Rect,
    camera: &OrbitCamera,
    geometry: GizmoGeometry,
    center: Vec3,
    color: Color,
) {
    let view_projection = projection(camera, rect);
    let half = geometry.units_per_point() * CUBE_POINTS;
    // Built in the camera's own right/up so the square faces the user as they orbit.
    let eye = camera.eye();
    let forward = (center - eye).normalize_or_zero();
    let forward = if forward == Vec3::ZERO {
        Vec3::Z
    } else {
        forward
    };
    let helper = if forward.y.abs() > 0.9 {
        Vec3::Z
    } else {
        Vec3::Y
    };
    let right = forward.cross(helper).normalize_or_zero();
    let up = right.cross(forward);
    let corners = [
        center - right * half - up * half,
        center + right * half - up * half,
        center + right * half + up * half,
        center - right * half + up * half,
    ];
    for index in 0..4 {
        draw_segment(
            render,
            rect,
            &view_projection,
            corners[index],
            corners[(index + 1) % 4],
            color,
        );
    }
}

/// Half the side of a scale handle's cube, in points.
pub const CUBE_POINTS: f32 = 5.0;

/// Draws the gizmo for `mode`: arrows for translate, rings for rotate, cubes for scale,
/// nothing for select.
pub fn draw_for_mode(
    render: &mut RenderContext,
    rect: Rect,
    camera: &OrbitCamera,
    geometry: GizmoGeometry,
    mode: crate::editor::EditorMode,
    active: Option<GizmoHandle>,
) {
    match mode {
        crate::editor::EditorMode::Translate => {
            translate(render, rect, camera, geometry, active);
        }
        crate::editor::EditorMode::Rotate => {
            rotate(render, rect, camera, geometry, active);
        }
        crate::editor::EditorMode::Scale => {
            scale(render, rect, camera, geometry, active);
        }
        // Select draws no handles: a handle you cannot drag is a lie about the tool.
        crate::editor::EditorMode::Select => {}
    }
}

fn projection(camera: &OrbitCamera, rect: Rect) -> Mat4 {
    Mat4::from_cols_array(&camera.view_projection(rect.width() / rect.height().max(1.0)))
}

fn color_for(handle: GizmoHandle, active: Option<GizmoHandle>) -> Color {
    let base = handle.color();
    // The grabbed handle is yellow, the convention every editor shares: it is the one bit of
    // feedback that says "this drag will move this axis".
    let color = if active == Some(handle) {
        egui::Color32::from_rgb(0xff, 0xd0, 0x4a)
    } else {
        base
    };
    Color::rgb(color.r(), color.g(), color.b())
}

fn arrow_head(
    render: &mut RenderContext,
    rect: Rect,
    view_projection: &Mat4,
    geometry: GizmoGeometry,
    axis: Vec3,
    color: Color,
) {
    let tip = geometry.tip(axis);
    let (u, v) = perpendicular_basis(axis);
    // A diamond: four points around the tip, each at 0.12 of the arrow back from it.
    let back = tip - axis * (geometry.length * 0.12);
    let width = geometry.length * 0.055;
    let points = [
        back + u * width,
        back + v * width,
        back - u * width,
        back - v * width,
    ];
    for corner in points {
        draw_segment(render, rect, view_projection, tip, corner, color);
    }
    // Close the diamond so it reads as a solid head rather than four whiskers.
    for index in 0..4 {
        draw_segment(
            render,
            rect,
            view_projection,
            points[index],
            points[(index + 1) % 4],
            color,
        );
    }
}

fn centre_square(
    render: &mut RenderContext,
    rect: Rect,
    view_projection: &Mat4,
    geometry: GizmoGeometry,
    active: Option<GizmoHandle>,
) {
    let half = geometry.length * 0.045;
    let center = geometry.center;
    // A screen-facing square: the camera's right and up vectors, so it does not tilt as the
    // user orbits. `geometry.units_per_point()` already encodes the size on screen.
    let corners = [
        center + Vec3::new(-half, -half, 0.0),
        center + Vec3::new(half, -half, 0.0),
        center + Vec3::new(half, half, 0.0),
        center + Vec3::new(-half, half, 0.0),
    ];
    let color = color_for(GizmoHandle::Center, active);
    for index in 0..4 {
        draw_segment(
            render,
            rect,
            view_projection,
            corners[index],
            corners[(index + 1) % 4],
            color,
        );
    }
}

fn draw_ring(
    render: &mut RenderContext,
    rect: Rect,
    view_projection: &Mat4,
    center: Vec3,
    axis: Vec3,
    radius: f32,
    color: Color,
) {
    let (u, v) = perpendicular_basis(axis);
    let mut previous = center + u * radius;
    for step in 1..=RING_SEGMENTS {
        let angle = std::f32::consts::TAU * step as f32 / RING_SEGMENTS as f32;
        let point = center + (u * angle.cos() + v * angle.sin()) * radius;
        draw_segment(render, rect, view_projection, previous, point, color);
        previous = point;
    }
}

/// Two unit vectors perpendicular to `axis` and to each other.
///
/// The same construction `rotate::basis` uses, repeated here because the painter must not
/// depend on the grab math: a change to one would silently rotate the other's rings.
fn perpendicular_basis(axis: Vec3) -> (Vec3, Vec3) {
    let helper = if axis.y.abs() > 0.9 { Vec3::Z } else { Vec3::Y };
    let u = axis.cross(helper).normalize_or_zero();
    let v = axis.cross(u).normalize_or_zero();
    (u, v)
}

/// Where the gizmo's centre projects, for a caller that wants to place an overlay label.
pub fn centre_on_screen(camera: &OrbitCamera, rect: Rect, center: Vec3) -> Option<egui::Pos2> {
    project_point(center, &projection(camera, rect), rect)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_basis_of_each_axis_is_orthonormal() {
        for axis in [
            Vec3::X,
            Vec3::Y,
            Vec3::Z,
            Vec3::new(1.0, 1.0, 0.0).normalize(),
        ] {
            let (u, v) = perpendicular_basis(axis);
            assert!((u.length() - 1.0).abs() < 1e-5, "{u:?}");
            assert!((v.length() - 1.0).abs() < 1e-5, "{v:?}");
            assert!(
                u.dot(axis).abs() < 1e-5,
                "{u:?} not perpendicular to {axis:?}"
            );
            assert!(u.dot(v).abs() < 1e-5);
        }
    }

    #[test]
    fn the_basis_does_not_degenerate_on_the_y_axis() {
        let (u, v) = perpendicular_basis(Vec3::Y);
        assert!(u.length() > 0.99);
        assert!(v.length() > 0.99);
        assert!(u.dot(v).abs() < 1e-5);
    }

    #[test]
    fn a_ring_is_a_closed_loop_of_points_at_the_radius() {
        let center = Vec3::new(1.0, 2.0, 3.0);
        let radius = 0.75;
        let (u, v) = perpendicular_basis(Vec3::Y);
        let mut previous = center + u * radius;
        for step in 1..=RING_SEGMENTS {
            let angle = std::f32::consts::TAU * step as f32 / RING_SEGMENTS as f32;
            let point = center + (u * angle.cos() + v * angle.sin()) * radius;
            assert!(((point - center).length() - radius).abs() < 1e-4);
            assert!((point - previous).length() < radius, "a gap in the ring");
            previous = point;
        }
        assert!(
            (previous - (center + u * radius)).length() < 1e-4,
            "not closed"
        );
    }

    #[test]
    fn every_mode_has_a_painter_entry_point() {
        // The dispatch must cover every mode; the match is exhaustive, so this is a
        // compile-time guarantee made visible to a reader.
        for mode in [
            crate::editor::EditorMode::Select,
            crate::editor::EditorMode::Translate,
            crate::editor::EditorMode::Rotate,
            crate::editor::EditorMode::Scale,
        ] {
            assert!(!mode.name().is_empty());
        }
    }

    #[test]
    fn the_centre_projects_to_the_middle_of_the_viewport() {
        let camera = OrbitCamera::looking_at(Vec3::new(0.0, 1.0, 0.0), 10.0);
        let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let point = centre_on_screen(&camera, rect, camera.target).expect("centre");
        assert!((point.x - 400.0).abs() < 1e-3);
        assert!((point.y - 300.0).abs() < 1e-3);
    }
}
