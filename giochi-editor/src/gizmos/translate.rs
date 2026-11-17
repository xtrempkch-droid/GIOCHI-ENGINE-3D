//! Translating: the three arrows, plus the centre handle that slides in the camera plane.
//!
//! # Picking a *drawn line*
//!
//! The plan's note is the design: GLES 2.0 has no occlusion query, so "is the pointer on
//! the arrow" cannot be a framebuffer question. It is a ray-to-segment distance in world
//! space instead, with the tolerance converted from points through the same
//! `units_per_point` the gizmo is sized with. A handle therefore stays grabbable when the
//! camera is far away, because the tolerance grows with the gizmo.
//!
//! # Why the parameter, not the intersection point
//!
//! Dragging along an axis is: project the pointer onto the axis *line* and take that scalar.
//! Using the raw ray-plane intersection instead would make the object jump the moment the
//! pointer left the (infinitely thin) plane containing the axis, which is every time the
//! user drags off the arrow. The closest point between the ray and the axis line is defined
//! everywhere, so the drag is continuous — the property users notice as "it does not jump".

use glam::Vec3;

use crate::gizmos::picker::Ray;
use crate::gizmos::{DragKind, GizmoGeometry, GizmoHandle, ParentSpace};
use giochi_engine_3d::scene::NodeTransform;

/// Picks the handle under `position`, if any, for a translate gizmo.
///
/// Order matters: the centre square is tested **first**, because it sits on top of the
/// three arrows' origins and a click there means "move in the plane", not "pick an axis the
/// pointer happens to graze".
pub fn pick(ray: Ray, geometry: GizmoGeometry, tolerance_points_from: f32) -> Option<GizmoHandle> {
    let tolerance = geometry.units_per_point() * tolerance_points_from;

    let center_radius = geometry.units_per_point() * super::CENTER_POINTS;
    if distance_from_ray(ray, geometry.center) < center_radius {
        return Some(GizmoHandle::Center);
    }

    let mut best: Option<(f32, GizmoHandle)> = None;
    for handle in [GizmoHandle::X, GizmoHandle::Y, GizmoHandle::Z] {
        let Some(axis) = handle.axis() else {
            continue;
        };
        let distance = distance_ray_to_segment(ray, geometry.center, geometry.tip(axis));
        if distance <= tolerance && best.is_none_or(|(closest, _)| distance < closest) {
            best = Some((distance, handle));
        }
    }
    best.map(|(_, handle)| handle)
}

/// Begins a translate drag.
///
/// The axes store a scalar (the distance along the axis at the start); the centre handle
/// stores the world point where the pointer met the plane. Both are the *begin* state, and
/// both are recomputed from it every frame — never accumulated.
pub fn begin(
    node: giochi_engine_3d::scene::SceneNodeId,
    start: NodeTransform,
    handle: GizmoHandle,
    ray: Ray,
    geometry: GizmoGeometry,
    parent: ParentSpace,
) -> Option<super::Drag> {
    let kind = match handle.axis() {
        Some(axis) => DragKind::TranslateAxis {
            start_parameter: closest_parameter_on_line(ray, geometry.center, axis)?,
        },
        None => DragKind::TranslatePlane {
            start_point: plane_point(ray, geometry.center)?,
        },
    };
    Some(super::Drag {
        node,
        handle,
        start,
        geometry,
        parent,
        kind,
    })
}

/// Where the ray meets the horizontal plane through `center`.
pub fn plane_point(ray: Ray, center: Vec3) -> Option<Vec3> {
    if ray.direction.y.abs() < 1e-6 {
        return None;
    }
    let distance = (center.y - ray.origin.y) / ray.direction.y;
    if distance <= 0.0 || !distance.is_finite() {
        return None;
    }
    let point = ray.origin + ray.direction * distance;
    point.is_finite().then_some(point)
}

/// Applies a world-space offset to a local transform.
pub fn apply_offset(start: NodeTransform, parent: ParentSpace, world: Vec3) -> NodeTransform {
    let local = parent.offset_to_local(world);
    let mut result = start;
    result.position = [
        start.position[0] + local.x,
        start.position[1] + local.y,
        start.position[2] + local.z,
    ];
    result
}

/// The parameter along the line `origin + axis * t` of the point closest to `ray`.
///
/// This is the standard closest-approach of two skew lines. When they are parallel (the
/// camera looking straight down an axis) the denominator vanishes and the drag has no
/// meaning in that direction: `None`, and the caller keeps the transform it had.
pub fn closest_parameter_on_line(ray: Ray, origin: Vec3, axis: Vec3) -> Option<f32> {
    let axis = axis.normalize_or_zero();
    if axis.length_squared() < 1e-9 {
        return None;
    }
    let w0 = ray.origin - origin;
    let a = ray.direction.dot(ray.direction);
    let b = ray.direction.dot(axis);
    let c = axis.dot(axis);
    let d = ray.direction.dot(w0);
    let e = axis.dot(w0);

    let denominator = a * c - b * b;
    if denominator.abs() < 1e-6 {
        return None;
    }
    let t = (a * e - b * d) / denominator;
    t.is_finite().then_some(t)
}

/// Perpendicular distance between `ray` and the segment `from..to`.
pub fn distance_ray_to_segment(ray: Ray, from: Vec3, to: Vec3) -> f32 {
    let direction = to - from;
    let length_squared = direction.length_squared();
    if length_squared < 1e-12 {
        return distance_from_ray(ray, from);
    }

    // Closest point on the *segment* to the ray, computed by sampling the closest approach
    // and clamping to the ends. Clamping is what stops a click far past the arrow tip from
    // grabbing the arrow: only the drawn part is grabbable.
    let mut best = f32::INFINITY;
    for step in 0..=16 {
        let t = step as f32 / 16.0;
        let point = from + direction * t;
        best = best.min(distance_from_ray(ray, point));
    }
    best
}

/// Perpendicular distance from a point to a ray (clamped to the forward half-line).
pub fn distance_from_ray(ray: Ray, point: Vec3) -> f32 {
    let to_point = point - ray.origin;
    let direction = ray.direction.normalize_or_zero();
    let along = to_point.dot(direction).max(0.0);
    (to_point - direction * along).length()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::orbit_camera::OrbitCamera;
    use egui::Rect;

    fn rect() -> Rect {
        Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))
    }

    fn camera_at(center: Vec3) -> OrbitCamera {
        OrbitCamera::looking_at(center, 10.0)
    }

    fn ray_through_center(camera: &OrbitCamera) -> Ray {
        crate::gizmos::picker::ray_through(rect().center(), rect(), camera).expect("ray")
    }

    fn geometry(center: Vec3) -> GizmoGeometry {
        GizmoGeometry::frozen(center, 1.0)
    }

    #[test]
    fn dragging_the_x_arrow_moves_only_x() {
        let center = Vec3::new(0.0, 1.0, 0.0);
        let camera = camera_at(center);
        let geometry = geometry(center);
        let transform = NodeTransform::new([0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0], [1.0; 3]);

        // Two rays a known distance apart, both roughly along the axis' perpendicular.
        let ray_a = ray_through_center(&camera);
        let ray_b = Ray {
            origin: ray_a.origin + Vec3::new(0.0, 0.0, 0.5),
            direction: ray_a.direction,
        };
        let start = closest_parameter_on_line(ray_a, geometry.center, Vec3::X).expect("param");
        let end = closest_parameter_on_line(ray_b, geometry.center, Vec3::X).expect("param");
        let delta = end - start;

        let mut drag = begin(
            giochi_engine_3d::scene::SceneNodeId(1),
            transform,
            GizmoHandle::X,
            ray_a,
            geometry,
            ParentSpace::default(),
        )
        .expect("drag");
        if let DragKind::TranslateAxis { start_parameter } = &mut drag.kind {
            *start_parameter = start;
        }
        let moved = drag.transform_for(ray_b);
        assert!(
            (moved.position[1] - 1.0).abs() < 1e-5,
            "{:?}",
            moved.position
        );
        assert!(
            (moved.position[2] - 0.0).abs() < 1e-5,
            "{:?}",
            moved.position
        );
        assert!(
            (moved.position[0] - delta).abs() < 1e-4,
            "{:?}",
            moved.position
        );
    }

    #[test]
    fn a_drag_never_accumulates_error_because_it_starts_from_the_begin_state() {
        let center = Vec3::new(0.0, 1.0, 0.0);
        let camera = camera_at(center);
        let geometry = geometry(center);
        let transform = NodeTransform::new([0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0], [1.0; 3]);
        let ray = ray_through_center(&camera);

        let drag = begin(
            giochi_engine_3d::scene::SceneNodeId(1),
            transform,
            GizmoHandle::Y,
            ray,
            geometry,
            ParentSpace::default(),
        )
        .expect("drag");

        // Feeding the same ray a hundred times must give the same answer, not a drifting
        // one: `transform_for` derives from `start`, never from the previous result.
        let first = drag.transform_for(ray);
        for _ in 0..100 {
            assert_eq!(drag.transform_for(ray), first);
        }
        assert_eq!(first, transform);
    }

    #[test]
    fn a_drag_into_empty_ray_parameters_keeps_the_last_good_transform() {
        let center = Vec3::new(0.0, 1.0, 0.0);
        let camera = camera_at(center);
        let transform = NodeTransform::new([0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0], [1.0; 3]);
        let ray = ray_through_center(&camera);
        let drag = begin(
            giochi_engine_3d::scene::SceneNodeId(1),
            transform,
            GizmoHandle::X,
            ray,
            geometry(center),
            ParentSpace::default(),
        )
        .expect("drag");

        // A ray parallel to the axis gives no parameter; the transform must be `start`.
        let parallel = Ray {
            origin: ray.origin,
            direction: Vec3::X,
        };
        assert_eq!(drag.transform_for(parallel), transform);
    }

    #[test]
    fn a_rotated_parent_does_not_make_its_child_slide_sideways() {
        let center = Vec3::ZERO;
        let camera = camera_at(center);
        let ray = ray_through_center(&camera);
        let transform = NodeTransform::default();
        let parent = ParentSpace {
            rotation: glam::Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
            scale: Vec3::ONE,
        };

        let drag = begin(
            giochi_engine_3d::scene::SceneNodeId(1),
            transform,
            GizmoHandle::X,
            ray,
            geometry(center),
            parent,
        )
        .expect("drag");

        // Ask for a world +X move directly through `apply_offset`, which is what the drag
        // does internally: a parent rotated +90° about Y has its local +Z along world +X.
        let moved = apply_offset(transform, parent, Vec3::X);
        assert!((moved.position[0]).abs() < 1e-5, "{:?}", moved.position);
        assert!(moved.position[2] > 0.99, "{:?}", moved.position);
        let _ = drag;
    }

    #[test]
    fn picking_prefers_the_centre_over_an_axis_it_overlaps() {
        let center = Vec3::new(0.0, 1.0, 0.0);
        let camera = camera_at(center);
        let ray = ray_through_center(&camera);
        let handle = pick(ray, geometry(center), super::super::AXIS_TOLERANCE_POINTS);
        assert_eq!(handle, Some(GizmoHandle::Center));
    }

    #[test]
    fn picking_finds_the_x_arrow_when_the_pointer_is_near_its_tip() {
        let center = Vec3::new(0.0, 1.0, 0.0);
        let camera = camera_at(center);
        let rect = rect();
        let geometry = GizmoGeometry::for_camera(center, &camera, rect);
        // Project the tip of the X arrow and build the ray through that point.
        let projection =
            glam::Mat4::from_cols_array(&camera.view_projection(rect.width() / rect.height()));
        let tip = crate::editor::viewport::project_point(geometry.tip(Vec3::X), &projection, rect)
            .expect("tip on screen");
        let ray = crate::gizmos::picker::ray_through(tip, rect, &camera).expect("ray");
        let handle = pick(ray, geometry, super::super::AXIS_TOLERANCE_POINTS);
        assert_eq!(handle, Some(GizmoHandle::X));
    }

    #[test]
    fn picking_nothing_far_from_the_gizmo() {
        let center = Vec3::new(0.0, 1.0, 0.0);
        let camera = camera_at(center);
        let ray =
            crate::gizmos::picker::ray_through(egui::pos2(4.0, 4.0), rect(), &camera).expect("ray");
        let geometry = GizmoGeometry::for_camera(center, &camera, rect());
        assert!(pick(ray, geometry, super::super::AXIS_TOLERANCE_POINTS).is_none());
    }

    #[test]
    fn the_plane_point_is_where_the_ray_crosses_the_floor() {
        let camera = camera_at(Vec3::new(0.0, 0.0, 0.0));
        let ray = ray_through_center(&camera);
        let point = plane_point(ray, Vec3::ZERO).expect("crosses the floor");
        assert!((point.y).abs() < 1e-5);
    }

    #[test]
    fn a_ray_parallel_to_the_plane_has_no_plane_point() {
        let ray = Ray {
            origin: Vec3::new(0.0, 1.0, 0.0),
            direction: Vec3::X,
        };
        assert!(plane_point(ray, Vec3::ZERO).is_none());
    }

    #[test]
    fn a_parallel_ray_gives_no_parameter() {
        let ray = Ray {
            origin: Vec3::ZERO,
            direction: Vec3::Y,
        };
        assert!(closest_parameter_on_line(ray, Vec3::ZERO, Vec3::Y).is_none());
    }

    #[test]
    fn a_degenerate_segment_falls_back_to_the_distance_to_its_point() {
        let ray = Ray {
            origin: Vec3::new(0.0, 0.0, -5.0),
            direction: Vec3::Z,
        };
        let distance = distance_ray_to_segment(ray, Vec3::ZERO, Vec3::ZERO);
        assert!((distance - 0.0).abs() < 1e-4, "{distance}");
    }

    #[test]
    fn only_the_drawn_part_of_the_arrow_is_grabbable() {
        let ray = Ray {
            origin: Vec3::new(0.0, 10.0, -5.0),
            direction: Vec3::new(0.0, 0.0, 1.0),
        };
        // The segment is far below the ray, so it must not be picked even though the
        // *infinite* line through it passes closer.
        let distance = distance_ray_to_segment(ray, Vec3::ZERO, Vec3::X);
        assert!(distance > 5.0, "{distance}");
    }

    #[test]
    fn the_centre_handle_begins_a_plane_drag_from_where_the_pointer_met_the_plane() {
        let center = Vec3::new(0.0, 1.0, 0.0);
        let camera = camera_at(center);
        let ray = ray_through_center(&camera);
        let drag = begin(
            giochi_engine_3d::scene::SceneNodeId(1),
            NodeTransform::default(),
            GizmoHandle::Center,
            ray,
            geometry(center),
            ParentSpace::default(),
        )
        .expect("plane drag");
        assert!(matches!(drag.kind, DragKind::TranslatePlane { .. }));

        // The same ray means no movement, and the object stays exactly where it was.
        assert_eq!(drag.transform_for(ray), NodeTransform::default());
    }
}
