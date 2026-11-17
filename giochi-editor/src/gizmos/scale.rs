//! The scale gizmo: three axis handles and a uniform centre cube.
//!
//! # What the drag measures
//!
//! Scale is a ratio, so any drag has to answer "ratio of what". The pointer's distance from
//! the object is the only quantity that survives a change of camera, of zoom, or of which
//! pixel of the handle was grabbed — so the factor is
//!
//! ```text
//! factor = 1 + (current_distance - start_distance) / handle_length
//! ```
//!
//! measured along the handle. Grabbing the tip of a 96-point arrow and dragging another 96
//! points out doubles the scale, and it does so whether the grab was at the tip or halfway
//! along. An earlier draft divided the two distances instead; that is the same answer only
//! when the grab was exactly at the tip, and explodes when the pointer is grabbed near the
//! centre.
//!
//! # Which axes
//!
//! [`GizmoGeometry::axis_dir`] is not a world axis for the scale gizmo: the handles are
//! turned by the node's world rotation, because scale is a property of the node's own axes.
//! Dragging the "X" of a node rotated 45° changes `scale.x`, and the arrow points along the
//! edge it changes.
//!
//! # Why no mirroring
//!
//! The factor is clamped to [`MIN_FACTOR`], so a drag through zero collapses the object
//! rather than inverting it. A negative scale component is legal in the scene format and
//! reads as a mirrored mesh to anyone who writes it by hand, but a drag that flips an object
//! inside out with no warning is a trap, and nothing in the plan asks for mirroring.

use giochi_engine_3d::scene::{NodeTransform, SceneNodeId};

use super::picker::Ray;
use super::translate::{closest_parameter_on_line, distance_from_ray, distance_ray_to_segment};
use super::{Drag, DragKind, GizmoGeometry, GizmoHandle, ParentSpace};

/// How far, in points, the pointer must travel to double the scale.
pub const SCALE_POINTS: f32 = 96.0;
/// The smallest factor a drag may produce. Zero would make the object unrecoverable without
/// undo, which phase 37 has not shipped yet.
pub const MIN_FACTOR: f32 = 0.01;

/// The handle under the pointer, if any.
///
/// The centre wins ties, exactly like the translate gizmo: it is drawn on top of where the
/// three axes meet, and it is the smallest target on screen.
pub fn pick(ray: Ray, geometry: GizmoGeometry, tolerance_points: f32) -> Option<GizmoHandle> {
    let tolerance = geometry.units_per_point() * tolerance_points;
    if distance_from_ray(ray, geometry.center) <= geometry.units_per_point() * super::CENTER_POINTS
    {
        return Some(GizmoHandle::Center);
    }
    let mut best: Option<(f32, GizmoHandle)> = None;
    for handle in [GizmoHandle::X, GizmoHandle::Y, GizmoHandle::Z] {
        let Some(dir) = geometry.axis_dir(handle) else {
            continue;
        };
        let distance = distance_ray_to_segment(ray, geometry.center, geometry.tip(dir));
        if distance <= tolerance && best.is_none_or(|(previous, _)| distance < previous) {
            best = Some((distance, handle));
        }
    }
    best.map(|(_, handle)| handle)
}

/// Starts a drag on `handle`.
pub fn begin(
    node: SceneNodeId,
    start: NodeTransform,
    handle: GizmoHandle,
    ray: Ray,
    geometry: GizmoGeometry,
    parent: ParentSpace,
) -> Option<Drag> {
    let kind = match handle {
        GizmoHandle::Center => {
            let start_offset = screen_offset(ray, geometry.center);
            // The centre cube is grabbed *at* the centre, where the offset is float noise
            // and its direction is whatever the round-off decided. Dragging right is then
            // the only honest meaning, so that is the direction taken.
            let noise = geometry.units_per_point() * super::CENTER_POINTS;
            let direction = if start_offset.length() <= noise {
                Vec2::X
            } else {
                start_offset.normalize_or_zero()
            };
            DragKind::ScaleUniform {
                start_offset,
                direction,
            }
        }
        _ => {
            let dir = geometry.axis_dir(handle)?;
            DragKind::ScaleAxis {
                start_parameter: closest_parameter_on_line(ray, geometry.center, dir)?,
            }
        }
    };
    Some(Drag {
        node,
        handle,
        start,
        geometry,
        parent,
        kind,
    })
}

/// The scale factor an axis drag asks for, or `None` when the ray is parallel to the handle.
pub fn axis_factor(
    start_parameter: f32,
    geometry: GizmoGeometry,
    ray: Ray,
    dir: Vec3,
) -> Option<f32> {
    let current = closest_parameter_on_line(ray, geometry.center, dir)?;
    let length = geometry.length.max(1e-6);
    Some((1.0 + (current - start_parameter) / length).max(MIN_FACTOR))
}

/// The scale factor a uniform drag asks for.
pub fn uniform_factor(
    start_offset: Vec2,
    direction: Vec2,
    geometry: GizmoGeometry,
    ray: Ray,
) -> f32 {
    let current = screen_offset(ray, geometry.center);
    let units = geometry.units_per_point() * SCALE_POINTS;
    // Signed along the direction the pointer started in, not by distance from the centre:
    // dragging back through the centre must shrink past 1× instead of growing again.
    let travelled = (current - start_offset).dot(direction);
    (1.0 + travelled / units.max(1e-6)).max(MIN_FACTOR)
}

/// `start` with one or all of its scale components multiplied by `factor`.
pub fn apply_scale(start: NodeTransform, handle: GizmoHandle, factor: f32) -> NodeTransform {
    let mut scale = start.scale;
    match handle {
        GizmoHandle::X => scale[0] *= factor,
        GizmoHandle::Y => scale[1] *= factor,
        GizmoHandle::Z => scale[2] *= factor,
        // Uniform scale multiplies all three, including a negative one the user typed into
        // the inspector: mirroring is preserved, it is only the *drag* that never creates it.
        GizmoHandle::Center => {
            scale = [scale[0] * factor, scale[1] * factor, scale[2] * factor];
        }
    }
    NodeTransform { scale, ..start }
}

/// The pointer's offset from `center`, in a basis facing the camera.
///
/// The camera's right and up are rebuilt from the ray itself: a ray leaves the eye, so
/// `center - origin` is the view direction for that pixel. That keeps this function pure —
/// it needs no camera, which is what lets [`super::Drag::transform_for`] stay testable with a
/// hand-built ray.
pub fn screen_offset(ray: Ray, center: Vec3) -> Vec2 {
    let forward = (center - ray.origin).normalize_or_zero();
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
    // The point of the ray closest to the centre is the pixel the user is pointing at, as
    // seen from the camera.
    let to_center = center - ray.origin;
    let along = to_center.dot(ray.direction);
    let point = ray.origin + ray.direction * along;
    let offset = point - center;
    Vec2::new(offset.dot(right), offset.dot(up))
}

/// The factor a finished drag produced, for the status line.
///
/// Read back from the transform rather than stored during the drag: the scene is the source
/// of truth, and a stored factor would be one more thing that can disagree with it.
pub fn factor_from(start: NodeTransform, end: NodeTransform, handle: GizmoHandle) -> f32 {
    let component = match handle {
        GizmoHandle::X => 0,
        GizmoHandle::Y => 1,
        GizmoHandle::Z => 2,
        // Uniform drags multiply all three equally, so any component answers.
        GizmoHandle::Center => 0,
    };
    if start.scale[component].abs() < 1e-6 {
        return 1.0;
    }
    end.scale[component] / start.scale[component]
}

use glam::Vec2;
use glam::Vec3;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::OrbitCamera;
    use crate::gizmos::picker::ray_through;
    use egui::Rect;
    use glam::Vec3;

    fn rect() -> Rect {
        Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))
    }

    fn scene_geometry(rotation: glam::Quat) -> GizmoGeometry {
        let camera = OrbitCamera::looking_at(Vec3::ZERO, 10.0);
        GizmoGeometry::for_camera(Vec3::ZERO, &camera, rect()).with_rotation(rotation)
    }

    #[test]
    fn the_factor_grows_linearly_with_the_distance_dragged() {
        let geometry = GizmoGeometry::frozen(Vec3::ZERO, 1.0);
        let start = 1.0;
        // The ray crosses the X axis at x = 3: two lengths further out than the grab, so the
        // factor is 1 + 2.
        let ray = Ray {
            origin: Vec3::new(3.0, 0.0, 5.0),
            direction: Vec3::new(0.0, 0.0, -1.0),
        };
        let factor = axis_factor(start, geometry, ray, Vec3::X).expect("not parallel");
        assert!((factor - 3.0).abs() < 1e-4, "{factor}");

        // And the same pointer at x = 0.5 shrinks by half.
        let inner = Ray {
            origin: Vec3::new(0.5, 0.0, 5.0),
            direction: Vec3::new(0.0, 0.0, -1.0),
        };
        let factor = axis_factor(start, geometry, inner, Vec3::X).expect("not parallel");
        assert!((factor - 0.5).abs() < 1e-4, "{factor}");
    }

    #[test]
    fn a_factor_never_reaches_zero() {
        let geometry = GizmoGeometry::frozen(Vec3::ZERO, 1.0);
        let ray = Ray {
            origin: Vec3::new(0.0, 0.0, 5.0),
            direction: Vec3::new(-40.0, 0.0, -5.0).normalize(),
        };
        let factor = axis_factor(1.0, geometry, ray, Vec3::X).expect("not parallel");
        assert_eq!(factor, MIN_FACTOR);
    }

    #[test]
    fn an_axis_scale_touches_only_its_own_component() {
        let start = NodeTransform {
            scale: [2.0, 3.0, 4.0],
            ..NodeTransform::default()
        };
        assert_eq!(
            apply_scale(start, GizmoHandle::X, 2.0).scale,
            [4.0, 3.0, 4.0]
        );
        assert_eq!(
            apply_scale(start, GizmoHandle::Y, 0.5).scale,
            [2.0, 1.5, 4.0]
        );
        assert_eq!(
            apply_scale(start, GizmoHandle::Z, 3.0).scale,
            [2.0, 3.0, 12.0]
        );
        assert_eq!(
            apply_scale(start, GizmoHandle::Center, 2.0).scale,
            [4.0, 6.0, 8.0]
        );
    }

    #[test]
    fn a_uniform_drag_keeps_a_negative_component_negative() {
        // A mirrored object typed into the inspector stays mirrored: the drag multiplies.
        let start = NodeTransform {
            scale: [-1.0, 1.0, 1.0],
            ..NodeTransform::default()
        };
        assert_eq!(
            apply_scale(start, GizmoHandle::Center, 2.0).scale,
            [-2.0, 2.0, 2.0]
        );
    }

    #[test]
    fn a_rotated_geometry_points_its_handles_along_the_nodes_axes() {
        let rotation = glam::Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        let geometry = scene_geometry(rotation);
        let x = geometry.axis_dir(GizmoHandle::X).expect("x");
        assert!((x - (rotation * Vec3::X)).length() < 1e-5, "{x:?}");
        assert!(
            (x.z + 1.0).abs() < 1e-4,
            "a +90° Y rotation sends X to -Z: {x:?}"
        );
        assert!(geometry.axis_dir(GizmoHandle::Center).is_none());
    }

    #[test]
    fn picking_the_centre_beats_the_axes() {
        let camera = OrbitCamera::looking_at(Vec3::ZERO, 10.0);
        let geometry = GizmoGeometry::for_camera(Vec3::ZERO, &camera, rect());
        let center =
            crate::gizmos::paint::centre_on_screen(&camera, rect(), Vec3::ZERO).expect("on screen");
        let ray = ray_through(center, rect(), &camera).expect("ray");
        assert_eq!(
            pick(ray, geometry, super::super::AXIS_TOLERANCE_POINTS),
            Some(GizmoHandle::Center)
        );
    }

    #[test]
    fn dragging_the_x_handle_changes_the_x_scale_and_nothing_else() {
        let camera = OrbitCamera::looking_at(Vec3::ZERO, 10.0);
        let geometry = GizmoGeometry::for_camera(Vec3::ZERO, &camera, rect());
        let start = NodeTransform::default();
        let tip = crate::editor::viewport::project_point(
            geometry.tip(Vec3::X),
            &glam::Mat4::from_cols_array(&camera.view_projection(rect().width() / rect().height())),
            rect(),
        )
        .expect("on screen");
        let grab = ray_through(tip, rect(), &camera).expect("ray");
        let mut drag = begin(
            SceneNodeId(1),
            start,
            GizmoHandle::X,
            grab,
            geometry,
            ParentSpace::default(),
        )
        .expect("drag");

        // Pull the pointer a quarter of the arrow length further out along X, in screen
        // space, and let the drag recompute from `start`.
        let out = tip + egui::vec2(30.0, 0.0);
        let ray = ray_through(out, rect(), &camera).expect("ray");
        drag.advance(ray);
        let moved = drag.transform_for(ray);
        assert!(moved.scale[0] > 1.0, "{:?}", moved.scale);
        assert_eq!(moved.scale[1], start.scale[1]);
        assert_eq!(moved.scale[2], start.scale[2]);
        assert_eq!(moved.position, start.position, "scale moves nothing");
    }

    #[test]
    fn a_uniform_drag_outwards_enlarges_and_backwards_shrinks() {
        let camera = OrbitCamera::looking_at(Vec3::ZERO, 10.0);
        let geometry = GizmoGeometry::for_camera(Vec3::ZERO, &camera, rect());
        let center =
            crate::gizmos::paint::centre_on_screen(&camera, rect(), Vec3::ZERO).expect("on screen");
        let grab = ray_through(center + egui::vec2(6.0, 0.0), rect(), &camera).expect("ray");
        let drag = begin(
            SceneNodeId(1),
            NodeTransform::default(),
            GizmoHandle::Center,
            grab,
            geometry,
            ParentSpace::default(),
        )
        .expect("drag");

        let out = ray_through(center + egui::vec2(60.0, 0.0), rect(), &camera).expect("ray");
        assert!(drag.transform_for(out).scale[0] > 1.0);
        let back = ray_through(center - egui::vec2(60.0, 0.0), rect(), &camera).expect("ray");
        assert!(drag.transform_for(back).scale[0] < 1.0);
    }

    #[test]
    fn the_description_names_the_scale() {
        let camera = OrbitCamera::looking_at(Vec3::ZERO, 10.0);
        let geometry = GizmoGeometry::for_camera(Vec3::ZERO, &camera, rect());
        let grab = ray_through(rect().center(), rect(), &camera).expect("ray");
        let drag = begin(
            SceneNodeId(1),
            NodeTransform::default(),
            GizmoHandle::Center,
            grab,
            geometry,
            ParentSpace::default(),
        )
        .expect("drag");
        assert_eq!(drag.description(), "Scale uniform");
    }
}
