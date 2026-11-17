//! Picking: turning a click in the viewport into a node.
//!
//! The plan's warning is right that GLES 2.0 has no occlusion query, and that a raycast is
//! the answer. Here it is doubly true: the viewport is a projected wireframe, so there is
//! no depth buffer at all. A ray against the nodes' world AABBs gives the same answer the
//! picture shows — the nearest box the ray enters.
//!
//! The algorithm is the slab method: for each axis, the ray's interval inside the box's
//! slab is intersected with the running interval. A ray parallel to a slab that starts
//! outside it misses; one inside contributes no constraint. `t` is in world units along
//! the ray, so the smallest positive `t` is the nearest hit.

use egui::Rect;
use giochi_engine_3d::scene::{Scene, SceneNodeId};
use glam::{Mat4, Vec3};

use crate::editor::orbit_camera::OrbitCamera;
use crate::editor::viewport;

/// A ray in world space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ray {
    /// Where it starts, the camera's eye.
    pub origin: Vec3,
    /// Unit direction.
    pub direction: Vec3,
}

/// Builds the ray through `position` (viewport points) for `camera`.
///
/// Unprojecting uses the inverse of the same view-projection the viewport draws with, so a
/// click and a line can never disagree about where something is. `None` when the matrix is
/// singular or the position is outside the rectangle.
pub fn ray_through(position: egui::Pos2, rect: Rect, camera: &OrbitCamera) -> Option<Ray> {
    if !rect.contains(position) {
        return None;
    }
    let aspect = rect.width() / rect.height().max(1.0);
    let view_projection = Mat4::from_cols_array(&camera.view_projection(aspect));
    let inverse = view_projection.inverse();
    if !inverse.is_finite() {
        return None;
    }

    // Viewport point → NDC. Y is flipped: screen +Y is down, NDC +Y is up.
    let ndc_x = ((position.x - rect.left()) / rect.width()) * 2.0 - 1.0;
    let ndc_y = 1.0 - ((position.y - rect.top()) / rect.height()) * 2.0;

    let near = inverse * glam::Vec4::new(ndc_x, ndc_y, -1.0, 1.0);
    let far = inverse * glam::Vec4::new(ndc_x, ndc_y, 1.0, 1.0);
    if near.w.abs() < 1e-9 || far.w.abs() < 1e-9 {
        return None;
    }
    let near = near.truncate() / near.w;
    let far = far.truncate() / far.w;
    let direction = (far - near).normalize_or_zero();
    if direction.length_squared() < 1e-6 {
        return None;
    }
    Some(Ray {
        origin: camera.eye(),
        direction,
    })
}

/// The first node whose world box the ray enters, or `None` for empty space.
///
/// Nodes are tested in document order and the nearest hit wins, so a child inside its
/// parent's box is picked rather than the parent when it is in front.
pub fn pick(
    scene: &Scene,
    position: egui::Pos2,
    rect: Rect,
    camera: &OrbitCamera,
) -> Option<SceneNodeId> {
    let ray = ray_through(position, rect, camera)?;
    let mut best: Option<(f32, SceneNodeId)> = None;
    for node in &scene.nodes {
        let (min, max) = viewport::node_world_bounds(scene, node);
        if let Some(distance) = ray_aabb(ray, min, max) {
            if best.is_none_or(|(closest, _)| distance < closest) {
                best = Some((distance, node.id));
            }
        }
    }
    best.map(|(_, id)| id)
}

/// Distance along `ray` to the entry point of `[min, max]`, or `None` when it misses.
///
/// A box behind the ray (`exit < 0`) misses; a ray that starts inside returns `0.0`, which
/// is what makes clicking the inside of a large parent box select it.
pub fn ray_aabb(ray: Ray, min: [f32; 3], max: [f32; 3]) -> Option<f32> {
    let mut enter = f32::NEG_INFINITY;
    let mut exit = f32::INFINITY;
    for axis in 0..3 {
        let origin = ray.origin[axis];
        let direction = ray.direction[axis];
        let low = min[axis];
        let high = max[axis];
        if direction.abs() < 1e-9 {
            if origin < low || origin > high {
                return None;
            }
            continue;
        }
        let mut t0 = (low - origin) / direction;
        let mut t1 = (high - origin) / direction;
        if t0 > t1 {
            std::mem::swap(&mut t0, &mut t1);
        }
        enter = enter.max(t0);
        exit = exit.min(t1);
        if enter > exit {
            return None;
        }
    }
    if exit < 0.0 {
        // The whole interval is behind the eye.
        return None;
    }
    Some(enter.max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use giochi_engine_3d::scene::{MeshSource, Primitive, SceneNode};

    fn rect() -> Rect {
        Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0))
    }

    fn camera() -> OrbitCamera {
        // Looking at the origin from (8.5, 8.5, 8.5)-ish, which is the default's yaw.
        OrbitCamera::looking_at(Vec3::new(0.0, 0.5, 0.0), 9.0)
    }

    fn cube(scene: &mut Scene, name: &str, position: [f32; 3], size: f32) -> SceneNodeId {
        let mut node = scene.add_node(SceneNode::new(SceneNodeId::new(0), name));
        node.transform.position = position;
        node.transform.scale = [size, size, size];
        node.mesh = Some(MeshSource::Primitive(Primitive::Cube { size: 1.0 }));
        let id = node.id;
        scene.replace_node(node);
        id
    }

    #[test]
    fn the_ray_through_the_centre_points_at_the_target() {
        let camera = camera();
        let ray = ray_through(rect().center(), rect(), &camera).expect("ray");
        let to_target = (camera.target - ray.origin).normalize();
        assert!(
            ray.direction.dot(to_target) > 0.999,
            "{} vs {}",
            ray.direction,
            to_target
        );
    }

    #[test]
    fn a_click_outside_the_viewport_has_no_ray() {
        assert!(ray_through(egui::pos2(-1.0, 0.0), rect(), &camera()).is_none());
        assert!(ray_through(egui::pos2(900.0, 300.0), rect(), &camera()).is_none());
    }

    #[test]
    fn clicking_the_centre_of_the_viewport_selects_the_cube_at_the_target() {
        let camera = camera();
        let mut scene = Scene::new("pick");
        let expected = cube(&mut scene, "Cube", [0.0, 0.5, 0.0], 1.0);

        let picked = pick(&scene, rect().center(), rect(), &camera);
        assert_eq!(picked, Some(expected));
    }

    #[test]
    fn clicking_empty_space_selects_nothing() {
        let camera = camera();
        let mut scene = Scene::new("empty-space");
        cube(&mut scene, "Cube", [0.0, 0.5, 0.0], 1.0);
        // Top-left corner of the viewport: the box is in the middle, so nothing is there.
        assert_eq!(pick(&scene, egui::pos2(4.0, 4.0), rect(), &camera), None);
    }

    #[test]
    fn the_nearest_of_two_overlapping_boxes_wins() {
        let camera = camera();
        let mut scene = Scene::new("overlap");
        let to_target = (camera.target - camera.eye()).normalize();
        let near_position = camera.eye() + to_target * (camera.distance - 1.0);
        let far_position = camera.target;
        let near = cube(
            &mut scene,
            "Near",
            [near_position.x, near_position.y, near_position.z],
            1.0,
        );
        cube(
            &mut scene,
            "Far",
            [far_position.x, far_position.y, far_position.z],
            2.0,
        );

        assert_eq!(pick(&scene, rect().center(), rect(), &camera), Some(near));
    }

    #[test]
    fn a_box_behind_the_eye_is_not_picked() {
        let ray = Ray {
            origin: Vec3::ZERO,
            direction: Vec3::Z,
        };
        assert!(ray_aabb(ray, [-1.0, -1.0, -2.0], [1.0, 1.0, -1.0]).is_none());
    }

    #[test]
    fn a_ray_starting_inside_a_box_hits_it_at_zero() {
        let ray = Ray {
            origin: Vec3::ZERO,
            direction: Vec3::Z,
        };
        assert_eq!(
            ray_aabb(ray, [-1.0, -1.0, -1.0], [1.0, 1.0, 1.0]),
            Some(0.0)
        );
    }

    #[test]
    fn a_ray_parallel_to_a_slab_but_outside_it_misses() {
        let ray = Ray {
            origin: Vec3::new(0.0, 10.0, 0.0),
            direction: Vec3::Z,
        };
        assert!(ray_aabb(ray, [-1.0, -1.0, -1.0], [1.0, 1.0, 1.0]).is_none());
    }

    #[test]
    fn a_ray_parallel_to_a_slab_and_inside_it_hits() {
        let ray = Ray {
            origin: Vec3::new(0.0, 0.5, -5.0),
            direction: Vec3::Z,
        };
        assert!((ray_aabb(ray, [-1.0, -1.0, -1.0], [1.0, 1.0, 1.0]).unwrap() - 4.0).abs() < 1e-6);
    }

    #[test]
    fn a_rotated_box_uses_its_world_aabb() {
        let mut scene = Scene::new("rotated");
        let mut node = scene.add_node(SceneNode::new(SceneNodeId::new(0), "Rotated"));
        node.mesh = Some(MeshSource::Primitive(Primitive::Cube { size: 1.0 }));
        // 45° about Y: the AABB grows to sqrt(2) in x and z. The quaternion of half the
        // angle is (0, sin 22.5°, 0, cos 22.5°) — `FRAC_1_SQRT_2` here would be 90°,
        // which maps a cube onto itself and would make the assertion pass for the wrong
        // reason.
        let half_angle = std::f32::consts::FRAC_PI_4 * 0.5;
        node.transform.rotation = [0.0, half_angle.sin(), 0.0, half_angle.cos()];
        node.transform.position = [0.0, 0.5, 0.0];
        scene.replace_node(node);

        let (min, max) = viewport::node_world_bounds(&scene, &scene.nodes[0]);
        assert!(
            (max[0] - min[0] - std::f32::consts::SQRT_2).abs() < 1e-4,
            "{min:?} {max:?}"
        );
    }

    #[test]
    fn the_default_scene_picks_its_cube_from_the_front() {
        let scene = Scene::default_scene();
        let camera = OrbitCamera::looking_at(Vec3::new(0.0, 0.5, 0.0), 9.0);
        let picked = pick(&scene, rect().center(), rect(), &camera);
        assert_eq!(picked, Some(SceneNodeId(1)));
    }
}
