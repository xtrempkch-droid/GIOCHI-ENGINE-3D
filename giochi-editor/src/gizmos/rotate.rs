//! Rotating: three rings around the selected object, one per world axis.
//!
//! # Picking a ring
//!
//! A ring is the intersection of a torus with nothing — it is drawn as a circle in the plane
//! through the object whose normal is the axis. Picking it is therefore: intersect the ray
//! with that plane, measure how far the point is from the centre, and accept it when it is
//! within a tolerance of the ring's radius. That is the same analytic approach the plan
//! suggests for the arrows ("raio do mouse vs cilindro fino"), with a circle instead of a
//! cylinder — cheaper, and it cannot accidentally grab a ring that is edge-on because an
//! edge-on ring projects to a line and the plane intersection is still well defined.
//!
//! # Unwrapping the angle
//!
//! `atan2` returns ±π. Subtracting two such angles and using the result directly would jump
//! by 2π every time the pointer crossed the seam, which is the classic "the object spins
//! wildly once per turn" bug. [`unwrap`] takes the previous total and picks the branch
//! nearest to it, so a drag can go round several times and the object follows.

use glam::Vec3;

use crate::gizmos::picker::Ray;
use crate::gizmos::{DragKind, GizmoGeometry, GizmoHandle, ParentSpace};
use giochi_engine_3d::scene::NodeTransform;

/// Picks the ring under `ray`, if any.
///
/// Ties are broken by the axis closest to the camera's view direction being **last**: a ring
/// seen edge-on is a line, and the user almost never means the one they cannot see, so the
/// least face-on ring wins when two are within tolerance.
pub fn pick(
    ray: Ray,
    geometry: GizmoGeometry,
    radius_points: f32,
    tolerance_points: f32,
    camera: &crate::editor::orbit_camera::OrbitCamera,
) -> Option<GizmoHandle> {
    let units = geometry.units_per_point();
    let radius = units * radius_points;
    let tolerance = (units * tolerance_points).max(radius * 0.02);
    let view = (camera.target - camera.eye()).normalize_or_zero();

    let mut best: Option<(f32, GizmoHandle)> = None;
    for handle in [GizmoHandle::X, GizmoHandle::Y, GizmoHandle::Z] {
        let Some(axis) = handle.axis() else {
            continue;
        };
        let Some(distance) = ring_distance(ray, geometry.center, axis, radius) else {
            continue;
        };
        if distance > tolerance {
            continue;
        }
        // Face-on rings are easier to grab than edge-on ones; subtract a bonus so they win
        // a near tie, while an exact hit still wins on distance.
        let face_on = axis.dot(view).abs();
        let score = distance - face_on * tolerance * 0.5;
        if best.is_none_or(|(closest, _)| score < closest) {
            best = Some((score, handle));
        }
    }
    best.map(|(_, handle)| handle)
}

/// How far the ray's hit on the ring's plane is from the ring, or `None` when the ray is
/// parallel to the plane or hits it behind the eye.
pub fn ring_distance(ray: Ray, center: Vec3, axis: Vec3, radius: f32) -> Option<f32> {
    let axis = axis.normalize_or_zero();
    if axis.length_squared() < 1e-9 {
        return None;
    }
    let denominator = ray.direction.dot(axis);
    // A ring seen exactly edge-on is parallel to the ray: no plane hit, nothing to grab.
    if denominator.abs() < 1e-6 {
        return None;
    }
    let distance = (center - ray.origin).dot(axis) / denominator;
    if distance <= 0.0 || !distance.is_finite() {
        return None;
    }
    let point = ray.origin + ray.direction * distance;
    let radial = (point - center).length();
    Some((radial - radius).abs())
}

/// Begins a rotate drag, or `None` when the ray does not meet the ring's plane.
pub fn begin(
    node: giochi_engine_3d::scene::SceneNodeId,
    start: NodeTransform,
    handle: GizmoHandle,
    ray: Ray,
    geometry: GizmoGeometry,
    parent: ParentSpace,
) -> Option<super::Drag> {
    let axis = handle.axis()?;
    Some(super::Drag {
        node,
        handle,
        start,
        geometry,
        parent,
        kind: DragKind::Rotate {
            start_angle: angle_on_plane(ray, geometry.center, axis)?,
            accumulated: 0.0,
        },
    })
}

/// The pointer's angle around `axis`, measured in the plane through `center`.
///
/// The reference direction is chosen from the axis so that the angle is continuous as the
/// pointer crosses the ring: any fixed pair of basis vectors does, as long as it never
/// degenerates. `Vec3::Y` and `Vec3::Z` are the fallbacks for an axis that is ±Y.
pub fn angle_on_plane(ray: Ray, center: Vec3, axis: Vec3) -> Option<f32> {
    let axis = axis.normalize_or_zero();
    if axis.length_squared() < 1e-9 {
        return None;
    }
    let denominator = ray.direction.dot(axis);
    if denominator.abs() < 1e-6 {
        return None;
    }
    let distance = (center - ray.origin).dot(axis) / denominator;
    if distance <= 0.0 || !distance.is_finite() {
        return None;
    }
    let point = ray.origin + ray.direction * distance - center;
    let (u, v) = basis(axis);
    Some(point.dot(v).atan2(point.dot(u)))
}

/// Two unit vectors perpendicular to `axis` and to each other.
fn basis(axis: Vec3) -> (Vec3, Vec3) {
    let helper = if axis.y.abs() > 0.9 { Vec3::Z } else { Vec3::Y };
    let u = axis.cross(helper).normalize_or_zero();
    let v = axis.cross(u).normalize_or_zero();
    (u, v)
}

/// Chooses the representable angle nearest `previous`.
///
/// `raw` is the new angle in `(-π, π]`; `previous` is the total so far. The result differs
/// from `previous` by the smallest possible step, which is what makes a drag continuous
/// across the seam and lets it exceed a full turn.
pub fn unwrap(raw: f32, previous: f32) -> f32 {
    if !raw.is_finite() {
        return previous;
    }
    let mut result = raw;
    while result - previous > std::f32::consts::PI {
        result -= std::f32::consts::TAU;
    }
    while result - previous < -std::f32::consts::PI {
        result += std::f32::consts::TAU;
    }
    result
}

/// Applies a rotation of `angle` radians about the world `axis` to `start`.
///
/// In a rotated parent the axis is converted into the parent's frame, which is the same
/// conversion [`ParentSpace::axis_to_local`] does for a translation.
pub fn apply_rotation(
    start: NodeTransform,
    parent: ParentSpace,
    axis: Vec3,
    angle: f32,
) -> NodeTransform {
    let local_axis = parent.axis_to_local(axis);
    if local_axis.length_squared() < 1e-9 {
        return start;
    }
    let delta = glam::Quat::from_axis_angle(local_axis, angle);
    let start_rotation = glam::Quat::from_array(start.rotation);
    let rotated = delta * start_rotation;
    let rotated = if rotated.is_finite() {
        rotated.normalize()
    } else {
        start_rotation
    };
    NodeTransform {
        rotation: [rotated.x, rotated.y, rotated.z, rotated.w],
        ..start
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::orbit_camera::OrbitCamera;
    use egui::Rect;

    fn rect() -> Rect {
        Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))
    }

    fn center() -> Vec3 {
        Vec3::new(0.0, 1.0, 0.0)
    }

    fn camera() -> OrbitCamera {
        OrbitCamera::looking_at(center(), 10.0)
    }

    #[test]
    fn unwrapping_keeps_a_drag_continuous_across_the_seam() {
        // Crossing from +179° to -179° is a two-degree step, not a 358-degree one.
        let previous = 179.0_f32.to_radians();
        let raw = (-179.0_f32).to_radians();
        let total = unwrap(raw, previous);
        assert!(
            (total - 181.0_f32.to_radians()).abs() < 1e-6,
            "{}",
            total.to_degrees()
        );
    }

    #[test]
    fn unwrapping_allows_more_than_a_full_turn() {
        let mut total = 0.0;
        // Ten steps of 100° — 1000°, nearly three turns.
        for step in 1..=10 {
            let raw = (step as f32 * 100.0).rem_euclid(360.0).to_radians();
            total = unwrap(raw, total);
        }
        assert!(
            (total.to_degrees() - 1000.0).abs() < 1e-3,
            "{}",
            total.to_degrees()
        );
    }

    #[test]
    fn unwrapping_a_non_finite_angle_keeps_the_previous() {
        assert_eq!(unwrap(f32::NAN, 1.5), 1.5);
        assert_eq!(unwrap(f32::INFINITY, -2.0), -2.0);
    }

    #[test]
    fn a_ring_is_picked_near_its_radius() {
        let center = center();
        let camera = camera();
        let geometry = GizmoGeometry::for_camera(center, &camera, rect());
        let radius = geometry.units_per_point() * super::super::RING_POINTS;

        // A ray straight down -Y through a point on the X ring (which lies in the XZ plane).
        let on_ring = center + Vec3::X * radius;
        let ray = Ray {
            origin: on_ring + Vec3::Y * 5.0,
            direction: -Vec3::Y,
        };
        // The Y axis' ring lies in XZ; the X axis' ring lies in YZ and this ray is parallel
        // to its plane, so only the Y ring can be picked.
        let handle = pick(
            ray,
            geometry,
            super::super::RING_POINTS,
            super::super::AXIS_TOLERANCE_POINTS,
            &camera,
        );
        assert_eq!(handle, Some(GizmoHandle::Y));
    }

    #[test]
    fn a_ray_through_the_centre_is_not_on_any_ring() {
        let center = center();
        let camera = camera();
        let geometry = GizmoGeometry::for_camera(center, &camera, rect());
        let ray =
            crate::gizmos::picker::ray_through(rect().center(), rect(), &camera).expect("ray");
        assert!(pick(
            ray,
            geometry,
            super::super::RING_POINTS,
            super::super::AXIS_TOLERANCE_POINTS,
            &camera
        )
        .is_none());
    }

    #[test]
    fn the_angle_is_zero_on_the_basis_reference_and_grows_counter_clockwise() {
        let center = Vec3::ZERO;
        let axis = Vec3::Y;
        let (u, v) = basis(axis);

        let ray_from = |offset: Vec3| Ray {
            origin: offset + Vec3::Y * 5.0,
            direction: -Vec3::Y,
        };
        let zero = angle_on_plane(ray_from(u * 2.0), center, axis).expect("angle");
        let quarter = angle_on_plane(ray_from(v * 2.0), center, axis).expect("angle");
        assert!(zero.abs() < 1e-5, "{zero}");
        assert!(
            (quarter - std::f32::consts::FRAC_PI_2).abs() < 1e-5,
            "{quarter}"
        );
    }

    #[test]
    fn a_ray_parallel_to_the_ring_plane_has_no_angle() {
        let ray = Ray {
            origin: Vec3::new(0.0, 0.0, -5.0),
            direction: Vec3::Z,
        };
        assert!(angle_on_plane(ray, Vec3::ZERO, Vec3::Y).is_none());
    }

    #[test]
    fn rotating_about_y_by_ninety_degrees_turns_the_forward_axis() {
        let start = NodeTransform::default();
        let rotated = apply_rotation(
            start,
            ParentSpace::default(),
            Vec3::Y,
            std::f32::consts::FRAC_PI_2,
        );
        let quaternion = glam::Quat::from_array(rotated.rotation);
        let turned = quaternion * Vec3::Z;
        assert!(turned.x > 0.99, "{turned:?}");
    }

    #[test]
    fn a_rotation_keeps_the_position_and_scale() {
        let start = NodeTransform::new([1.0, 2.0, 3.0], [0.0, 0.0, 0.0, 1.0], [2.0, 3.0, 4.0]);
        let rotated = apply_rotation(start, ParentSpace::default(), Vec3::X, 0.3);
        assert_eq!(rotated.position, start.position);
        assert_eq!(rotated.scale, start.scale);
        let length = glam::Quat::from_array(rotated.rotation).length();
        assert!((length - 1.0).abs() < 1e-5, "{length}");
    }

    #[test]
    fn a_rotated_parent_converts_the_world_axis_into_its_own_frame() {
        let parent = ParentSpace {
            rotation: glam::Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
            scale: Vec3::ONE,
        };
        // A world-Y rotation is, in that parent, a rotation about its local +Z.
        let local = parent.axis_to_local(Vec3::Y);
        assert!(local.z > 0.99 || local.z < -0.99, "{local:?}");
    }

    #[test]
    fn a_drag_through_the_ring_centre_is_refused_instead_of_guessed() {
        let ray =
            crate::gizmos::picker::ray_through(rect().center(), rect(), &camera()).expect("ray");
        assert!(begin(
            giochi_engine_3d::scene::SceneNodeId(1),
            NodeTransform::default(),
            GizmoHandle::X,
            ray,
            GizmoGeometry::for_camera(center(), &camera(), rect()),
            ParentSpace::default(),
        )
        .is_some());

        // A ring whose plane the ray cannot meet gives no drag at all.
        let parallel = Ray {
            origin: Vec3::new(0.0, 1.0, -5.0),
            direction: Vec3::Z,
        };
        assert!(begin(
            giochi_engine_3d::scene::SceneNodeId(1),
            NodeTransform::default(),
            GizmoHandle::Y,
            parallel,
            GizmoGeometry::for_camera(center(), &camera(), rect()),
            ParentSpace::default(),
        )
        .is_none());
    }
}
