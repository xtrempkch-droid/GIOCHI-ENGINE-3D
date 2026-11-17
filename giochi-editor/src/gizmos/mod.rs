//! Gizmos: the manipulators drawn on the selection.
//!
//! # Screen-constant size
//!
//! A gizmo that is 1.5 world units long is unusable: invisible at ten metres, filling the
//! screen at one. Every tool solves this by making the handle a fixed number of *points*
//! and converting that to world units at the object's depth — that is
//! [`GizmoGeometry::for_camera`]. The conversion is `2 · distance · tan(fov/2) / height`,
//! which is the same `units_per_point` the orbit camera's pan uses; the two must agree or
//! dragging the gizmo and dragging the view would feel different.
//!
//! # A drag is one operation
//!
//! R6 of the plan is explicit: "1 comando por drag inteiro (começou-drag → terminou-drag),
//! não 100 movezinhos". [`Drag`] therefore stores the transform the node had when the drag
//! **began** and recomputes the result from the pointer every frame. The result never
//! accumulates float error, and when phase 37 adds the undo stack it has exactly one
//! `before`/`after` pair to push: `drag.start` and the node's final transform.
//!
//! # World axes, local storage
//!
//! The handles are world axes (X red, Y green, Z blue), which is what `W` in Unity and Godot
//! gives and what a user expects. The scene stores *local* transforms, so a child of a
//! rotated parent needs the world delta converted back — [`ParentSpace`] does that, and it
//! is the reason a rotated parent does not make its children slide sideways.

pub mod paint;
pub mod picker;
pub mod rotate;
pub mod translate;

use egui::Rect;
use giochi_engine_3d::scene::{NodeTransform, SceneNodeId};
use glam::Vec3;

use crate::editor::orbit_camera::OrbitCamera;

/// Which handle of the gizmo the pointer is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GizmoHandle {
    /// The X arrow, or the X ring.
    X,
    /// The Y arrow, or the Y ring.
    Y,
    /// The Z arrow, or the Z ring.
    Z,
    /// The square at the centre: translate in the camera plane.
    Center,
}

impl GizmoHandle {
    /// The world axis this handle moves along, or `None` for the centre.
    pub const fn axis(self) -> Option<Vec3> {
        match self {
            Self::X => Some(Vec3::X),
            Self::Y => Some(Vec3::Y),
            Self::Z => Some(Vec3::Z),
            Self::Center => None,
        }
    }

    /// The colour every editor uses: X red, Y green, Z blue, centre white.
    pub const fn color(self) -> egui::Color32 {
        match self {
            Self::X => egui::Color32::from_rgb(0xe0, 0x50, 0x50),
            Self::Y => egui::Color32::from_rgb(0x60, 0xc8, 0x60),
            Self::Z => egui::Color32::from_rgb(0x55, 0x88, 0xe8),
            Self::Center => egui::Color32::from_rgb(0xe8, 0xe8, 0xe8),
        }
    }

    /// Every handle, in the order the painter draws them.
    pub const ALL: [GizmoHandle; 4] = [Self::X, Self::Y, Self::Z, Self::Center];
}

/// Where and how big the gizmo is this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GizmoGeometry {
    /// The selected node's world position: the arrows start here.
    pub center: Vec3,
    /// Arrow length in world units, chosen so it is [`ARROW_POINTS`] long on screen.
    pub length: f32,
}

/// How long the arrows should look, in points.
pub const ARROW_POINTS: f32 = 96.0;
/// Radius of the rotate rings, in points.
pub const RING_POINTS: f32 = 88.0;
/// How close (in points) the pointer must be to a drawn line to grab it.
pub const AXIS_TOLERANCE_POINTS: f32 = 7.0;
/// Radius of the centre handle, in points.
pub const CENTER_POINTS: f32 = 8.0;

/// World units per point at `distance`, for a camera with `fov_y` and a `height`-point
/// viewport.
pub fn units_per_point(distance: f32, fov_y: f32, height: f32) -> f32 {
    2.0 * distance.max(1e-4) * (fov_y * 0.5).tan() / height.max(1.0)
}

impl GizmoGeometry {
    /// The gizmo for `world_position`, sized for `camera` and `rect`.
    pub fn for_camera(world_position: Vec3, camera: &OrbitCamera, rect: Rect) -> Self {
        let distance = (camera.eye() - world_position).length();
        Self {
            center: world_position,
            length: units_per_point(distance, camera.fov_y, rect.height()) * ARROW_POINTS,
        }
    }

    /// The same geometry, but with the length a drag was started with.
    ///
    /// A drag must not change scale when the camera moves: the handle the user grabbed has
    /// to stay where it was, so the world length is frozen at `begin`.
    pub const fn frozen(center: Vec3, length: f32) -> Self {
        Self { center, length }
    }

    /// The world-space tip of `axis`.
    pub fn tip(&self, axis: Vec3) -> Vec3 {
        self.center + axis * self.length
    }

    /// World units per point for this geometry, derived from the arrow length.
    pub fn units_per_point(&self) -> f32 {
        self.length / ARROW_POINTS
    }
}

/// The parent's contribution, which turns a world delta into a local one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParentSpace {
    /// The parent's world rotation; identity at the root.
    pub rotation: glam::Quat,
    /// The parent's world scale; ones at the root.
    pub scale: Vec3,
}

impl Default for ParentSpace {
    fn default() -> Self {
        Self {
            rotation: glam::Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }
}

impl ParentSpace {
    /// A world-space direction in the space the node's transform is stored in.
    pub fn direction_to_local(self, world: Vec3) -> Vec3 {
        let rotated = self.rotation.inverse() * world;
        Vec3::new(
            rotated.x / non_zero(self.scale.x),
            rotated.y / non_zero(self.scale.y),
            rotated.z / non_zero(self.scale.z),
        )
    }

    /// A world-space rotation axis in the space the node's transform is stored in.
    pub fn axis_to_local(self, world: Vec3) -> Vec3 {
        (self.rotation.inverse() * world).normalize_or_zero()
    }

    /// A world-space position offset in local units.
    pub fn offset_to_local(self, world: Vec3) -> Vec3 {
        self.direction_to_local(world)
    }
}

fn non_zero(value: f32) -> f32 {
    if value.abs() < 1e-6 {
        1.0
    } else {
        value
    }
}

/// How a drag converts pointer motion into a transform.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DragKind {
    /// Slide along one world axis.
    TranslateAxis {
        /// Distance along the axis at the moment the drag began.
        start_parameter: f32,
    },
    /// Slide in the horizontal plane through the object, from where the drag began.
    TranslatePlane {
        /// The world point where the pointer met the plane when the drag began.
        start_point: Vec3,
    },
    /// Turn around an axis.
    Rotate {
        /// Angle, in radians, at the moment the drag began.
        start_angle: f32,
        /// Total turn so far, unwrapped across ±π. Accumulating rather than subtracting
        /// two wrapped angles is what lets a drag go round more than half a turn.
        accumulated: f32,
    },
}

/// An in-progress drag.
#[derive(Debug, Clone, PartialEq)]
pub struct Drag {
    /// Which node is being moved.
    pub node: SceneNodeId,
    /// The handle the pointer grabbed.
    pub handle: GizmoHandle,
    /// The node's local transform when the drag began. One per drag, by design.
    pub start: NodeTransform,
    /// The gizmo as it was when the drag began; frozen so the handle cannot slide away.
    pub geometry: GizmoGeometry,
    /// The parent's world rotation and scale, for converting a world delta to local space.
    pub parent: ParentSpace,
    /// What the drag has done so far.
    pub kind: DragKind,
}

impl Drag {
    /// The node's transform for the current pointer state, derived from `start`.
    ///
    /// Pure: the caller decides when to write it into the scene, so the drag can be tested
    /// without a scene and the scene can be written once per frame.
    pub fn transform_for(&self, ray: picker::Ray) -> NodeTransform {
        match self.kind {
            DragKind::TranslateAxis { start_parameter } => {
                let Some(axis) = self.handle.axis() else {
                    return self.start;
                };
                let Some(current) =
                    translate::closest_parameter_on_line(ray, self.geometry.center, axis)
                else {
                    return self.start;
                };
                let world = axis * (current - start_parameter);
                translate::apply_offset(self.start, self.parent, world)
            }
            DragKind::TranslatePlane { start_point } => {
                let Some(current) = translate::plane_point(ray, self.geometry.center) else {
                    return self.start;
                };
                // Only the horizontal component moves: dragging "in the plane" must not
                // lift the object off the plane it started on.
                let offset = current - start_point;
                let world = Vec3::new(offset.x, 0.0, offset.z);
                translate::apply_offset(self.start, self.parent, world)
            }
            DragKind::Rotate {
                start_angle,
                accumulated,
            } => {
                let Some(axis) = self.handle.axis() else {
                    return self.start;
                };
                let Some(angle) = rotate::angle_on_plane(ray, self.geometry.center, axis) else {
                    return self.start;
                };
                let total = rotate::unwrap(angle - start_angle, accumulated);
                rotate::apply_rotation(self.start, self.parent, axis, total)
            }
        }
    }

    /// Whether this drag is a rotation, for the caller that has to report it.
    pub const fn is_rotation(&self) -> bool {
        matches!(self.kind, DragKind::Rotate { .. })
    }

    /// A one-line description, for the status bar and phase 37's undo label.
    pub fn description(&self) -> String {
        match self.kind {
            DragKind::TranslateAxis { .. } | DragKind::TranslatePlane { .. } => {
                format!("Move {}", self.handle_name())
            }
            DragKind::Rotate { .. } => format!("Rotate {}", self.handle_name()),
        }
    }

    fn handle_name(&self) -> &'static str {
        match self.handle {
            GizmoHandle::X => "X",
            GizmoHandle::Y => "Y",
            GizmoHandle::Z => "Z",
            GizmoHandle::Center => "plane",
        }
    }

    /// Updates the accumulated rotation after a pointer move.
    ///
    /// Explicit rather than implicit in [`Drag::transform_for`] so a caller that redraws
    /// without moving the pointer does not advance the turn.
    pub fn advance(&mut self, ray: picker::Ray) {
        if let DragKind::Rotate {
            start_angle,
            accumulated,
        } = &mut self.kind
        {
            let Some(axis) = self.handle.axis() else {
                return;
            };
            if let Some(angle) = rotate::angle_on_plane(ray, self.geometry.center, axis) {
                *accumulated = rotate::unwrap(angle - *start_angle, *accumulated);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gizmo_keeps_a_constant_screen_size_as_the_camera_pulls_back() {
        let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let mut camera = OrbitCamera::looking_at(Vec3::new(0.0, 0.5, 0.0), 4.0);
        let near = GizmoGeometry::for_camera(Vec3::new(0.0, 0.5, 0.0), &camera, rect);

        for distance in [8.0, 20.0, 60.0] {
            camera.distance = distance;
            let far = GizmoGeometry::for_camera(Vec3::new(0.0, 0.5, 0.0), &camera, rect);
            let expected = near.length * (distance / 4.0);
            assert!(
                (far.length - expected).abs() / expected < 0.02,
                "at {distance}: {} vs {expected}",
                far.length
            );
        }
    }

    #[test]
    fn units_per_point_matches_the_orbit_cameras_pan() {
        // The pan uses `2 * distance * tan(fov/2) / height`; if this drifts, dragging a
        // gizmo and dragging the view stop feeling the same.
        let rect_height = 720.0;
        let distance = 12.0;
        let fov = std::f32::consts::FRAC_PI_3;
        let units = units_per_point(distance, fov, rect_height);

        let mut camera = OrbitCamera::looking_at(Vec3::ZERO, distance);
        let before = camera.target;
        // One point of pan should move the target by `units` world units.
        camera.pan(1.0, 0.0, rect_height);
        let moved = (camera.target - before).length();
        assert!((moved - units).abs() / units < 1e-3, "{moved} vs {units}");
    }

    #[test]
    fn the_tip_is_one_length_along_the_axis() {
        let geometry = GizmoGeometry {
            center: Vec3::new(1.0, 2.0, 3.0),
            length: 2.0,
        };
        assert_eq!(geometry.tip(Vec3::Y), Vec3::new(1.0, 4.0, 3.0));
    }

    #[test]
    fn a_parent_space_converts_a_world_direction_into_local_units() {
        let parent = ParentSpace {
            rotation: glam::Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
            scale: Vec3::splat(2.0),
        };
        // A parent rotated +90° about Y sends its local +X to world -Z, so a world +X move
        // is the parent's local +Z; the scale then halves it.
        let local = parent.direction_to_local(Vec3::X);
        assert!(local.z > 0.49 && local.z < 0.51, "{local:?}");
        assert!((local.x).abs() < 1e-5);
        assert!((local.y).abs() < 1e-5);
    }

    #[test]
    fn a_parent_space_never_divides_by_zero() {
        let parent = ParentSpace {
            rotation: glam::Quat::IDENTITY,
            scale: Vec3::ZERO,
        };
        let local = parent.direction_to_local(Vec3::X);
        assert!(local.is_finite());
        assert_eq!(local, Vec3::X);
    }

    #[test]
    fn every_handle_has_a_colour_and_the_centre_has_no_axis() {
        for handle in GizmoHandle::ALL {
            let _ = handle.color();
        }
        assert!(GizmoHandle::Center.axis().is_none());
        assert_eq!(GizmoHandle::X.axis(), Some(Vec3::X));
        assert_eq!(GizmoHandle::Y.axis(), Some(Vec3::Y));
        assert_eq!(GizmoHandle::Z.axis(), Some(Vec3::Z));
    }

    #[test]
    fn a_frozen_geometry_does_not_follow_the_camera() {
        let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let camera = OrbitCamera::default();
        let live = GizmoGeometry::for_camera(Vec3::ZERO, &camera, rect);
        let frozen = GizmoGeometry::frozen(Vec3::ZERO, live.length);
        assert_eq!(live, frozen);
    }
}
