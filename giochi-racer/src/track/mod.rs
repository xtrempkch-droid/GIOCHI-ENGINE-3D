//! The circuit: what the editor saved, plus the gates a lap is counted on.
//!
//! A track is two things at once and they are built from the same scene graph:
//!
//! * **what you see** — every node with a mesh becomes a [`TrackInstance`], deduplicated
//!   by shape so a wall made of 200 identical cubes is one GPU buffer and 200 matrices;
//! * **what you hit** — the primitives also become static colliders, so the car cannot
//!   drive through the barriers the editor drew.
//!
//! Markers are the third thing, and they are *not* geometry: a node named `Start` places
//! the car, and `Checkpoint 1`…`Checkpoint 4` order the lap. Names rather than a new
//! component because the scene format belongs to the engine and the editor (R2), and a
//! name is something the editor already lets you edit. The convention is written down in
//! `docs/TRACK_SPECS.md`.
//!
//! # Two stages, on purpose
//!
//! [`loading::parse_track`] turns a [`giochi_engine_3d::scene::Scene`] into descriptors
//! with no GL involved, and
//! [`Track::upload`] turns those descriptors into GPU handles. That split is what makes
//! the loader testable on a machine with no display — which is what CI is.

pub mod lap;
pub mod loading;

use giochi_engine_3d::math3d::{Mat4, Quat, Transform3D, Vec3};
use giochi_engine_3d::physics::ColliderShape;
use giochi_engine_3d::render3d::{DirectionalLight, Material, MeshHandle, RenderContext3D};
use giochi_engine_3d::scene::{MaterialDesc, Primitive};
use giochi_engine_3d::Result3D;

pub use lap::{LapEvent, LapTracker};

/// Where the car is placed and which way it faces.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StartPosition {
    /// Centre of the car, in world units.
    pub position: Vec3,
    /// Heading in radians about `+Y`, in the convention of
    /// [`crate::vehicle::car::ArcadeModel`]: `0` faces `+Z`, `+π/2` faces `+X`.
    pub yaw: f32,
}

impl Default for StartPosition {
    fn default() -> Self {
        Self {
            position: Vec3::new(0.0, 0.5, 0.0),
            yaw: 0.0,
        }
    }
}

/// One gate on the lap.
///
/// The `direction` is **derived** from the neighbouring gates rather than read from the
/// node's rotation: an author placing four gates around an oval has already said which
/// way the lap goes, and asking them to also rotate each gate correctly is a second way
/// to get it wrong.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Checkpoint {
    /// Position in the ordered lap, `0` for the start/finish line.
    pub index: usize,
    /// Centre of the gate.
    pub position: Vec3,
    /// How close the car has to be, horizontally, for the gate to trigger.
    pub radius: f32,
    /// Unit horizontal vector the car must be travelling along to score the gate.
    pub direction: Vec3,
}

impl Checkpoint {
    /// Whether `point` is inside the gate, horizontally, and not a storey above or below it.
    ///
    /// The vertical test is what keeps a bridge over the start line from counting as
    /// crossing it.
    pub fn is_inside(&self, point: Vec3) -> bool {
        let (dx, dz) = (
            point.to_glam().x - self.position.to_glam().x,
            point.to_glam().z - self.position.to_glam().z,
        );
        let vertical = (point.to_glam().y - self.position.to_glam().y).abs();
        dx * dx + dz * dz <= self.radius * self.radius && vertical <= Self::MAX_HEIGHT
    }

    /// How far above or below a gate still counts, in metres.
    pub const MAX_HEIGHT: f32 = 4.0;
}

/// Where a track's geometry comes from.
#[derive(Debug, Clone, PartialEq)]
pub enum DrawShape {
    /// A mesh the engine generates from its parameters.
    Primitive(Primitive),
    /// An `.obj` on disk, relative to the asset root.
    Obj(String),
    /// A `.gltf`/`.glb` on disk, relative to the asset root.
    Gltf(String),
}

/// One drawable node.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackInstance {
    /// What to draw.
    pub shape: DrawShape,
    /// Where, in world space.
    pub transform: Transform3D,
    /// How to shade it.
    pub material: MaterialDesc,
}

/// One collider, already placed in the world.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StaticCollider {
    /// The shape, in the node's own units.
    pub shape: ColliderShape,
    /// Centre of the shape.
    pub position: Vec3,
    /// Orientation of the shape.
    pub rotation: Quat,
}

impl StaticCollider {
    /// The collider's transform, for a caller that wants to draw its outline.
    pub fn transform(&self) -> Transform3D {
        Transform3D::new(self.position, self.rotation, Vec3::new(1.0, 1.0, 1.0))
    }
}

/// A parsed circuit, ready to upload.
#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    /// The scene's name, shown in the HUD.
    pub name: String,
    /// Where the car starts.
    pub start: StartPosition,
    /// The gates in lap order; `[0]` is the start/finish line.
    pub checkpoints: Vec<Checkpoint>,
    /// Everything that gets drawn.
    pub instances: Vec<TrackInstance>,
    /// Everything that gets hit.
    pub colliders: Vec<StaticCollider>,
    /// The sun, if the scene has one.
    pub light: Option<DirectionalLight>,
}

impl Track {
    /// How many gates a complete lap passes, including the finish line.
    pub fn gate_count(&self) -> usize {
        self.checkpoints.len()
    }

    /// How many checkpoints a lap has, not counting the start/finish line.
    pub fn checkpoint_count(&self) -> usize {
        self.checkpoints.len().saturating_sub(1)
    }

    /// The track's world-space bounds, for placing a debug camera or framing a screenshot.
    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        let mut min = glam::Vec3::splat(f32::INFINITY);
        let mut max = glam::Vec3::splat(f32::NEG_INFINITY);
        let mut any = false;
        for instance in &self.instances {
            let (half_x, half_y, half_z) = primitive_half_extents(&instance.shape);
            let scale = instance.transform.scale.to_glam().abs();
            let half = glam::Vec3::new(half_x, half_y, half_z) * scale;
            // The AABB of a rotated box: rotate the half-extents and take the absolute.
            let matrix = glam::Mat3::from_quat(instance.transform.rotation.to_glam());
            let world_half = (matrix * half).abs();
            let centre = instance.transform.position.to_glam();
            min = min.min(centre - world_half);
            max = max.max(centre + world_half);
            any = true;
        }
        any.then(|| (Vec3::from_glam(min), Vec3::from_glam(max)))
    }

    /// Uploads the meshes and returns what the render loop draws.
    ///
    /// A shape that fails to load is reported once and skipped, so one bad file does not
    /// take the whole track with it. A glTF yields one draw per primitive, each with the
    /// material the file gave it; every other shape yields one draw with the node's.
    pub fn upload(
        &self,
        render: &mut RenderContext3D,
        asset_root: &std::path::Path,
    ) -> Result3D<UploadedTrack> {
        let mut cache: ShapeCache = Vec::new();
        let mut draws = Vec::new();

        for instance in &self.instances {
            let parts = match cache.iter().position(|(shape, _)| *shape == instance.shape) {
                Some(index) => Some(cache[index].1.clone()),
                None => match upload_shape(render, &instance.shape, asset_root) {
                    Ok(parts) => {
                        cache.push((instance.shape.clone(), parts.clone()));
                        Some(parts)
                    }
                    Err(error) => {
                        log::warn!(
                            "track '{}': {} not drawn: {error}",
                            self.name,
                            describe(&instance.shape)
                        );
                        None
                    }
                },
            };
            let Some(parts) = parts else { continue };
            for (mesh, material) in parts {
                draws.push(DrawCall {
                    mesh,
                    transform: instance.transform,
                    material: material.unwrap_or_else(|| instance.material.to_material()),
                });
            }
        }

        let handles = cache
            .into_iter()
            .flat_map(|(_, parts)| parts.into_iter().map(|(handle, _)| handle))
            .collect();

        Ok(UploadedTrack {
            draws,
            handles,
            light: self.light,
        })
    }
}

/// A track whose meshes are on the GPU.
#[derive(Debug)]
pub struct UploadedTrack {
    /// One entry per instance, in scene order.
    pub draws: Vec<DrawCall>,
    /// Every mesh handle, so the caller can free them.
    pub handles: Vec<MeshHandle>,
    /// The sun the scene asked for.
    pub light: Option<DirectionalLight>,
}

/// One uploaded shape: the meshes it produced and, for a glTF, the material of each.
type UploadedShape = Vec<(MeshHandle, Option<Material>)>;

/// One shape between its `DrawShape` key and what it uploaded.
type ShapeCache = Vec<(DrawShape, UploadedShape)>;

/// One instance, with its mesh already resolved.
#[derive(Debug, Clone, PartialEq)]
pub struct DrawCall {
    /// The mesh to draw.
    pub mesh: MeshHandle,
    /// Where.
    pub transform: Transform3D,
    /// How.
    pub material: Material,
}

impl UploadedTrack {
    /// How many draw calls a frame costs. The plan's Phase 45 budget starts here.
    pub fn draw_calls(&self) -> usize {
        self.draws.len()
    }

    /// The model matrix of `call`, for the renderer.
    pub fn model_matrix(call: &DrawCall) -> Mat4 {
        call.transform.matrix()
    }
}

/// Half-extents of a shape's own geometry, before the node's scale.
///
/// `Plane` reports zero height: it is a surface, not a slab, and
/// [`crate::track::loading`] is where it grows the thickness a collider needs.
pub(crate) fn primitive_half_extents(shape: &DrawShape) -> (f32, f32, f32) {
    match shape {
        DrawShape::Primitive(Primitive::Cube { size }) => {
            let half = size.abs() * 0.5;
            (half, half, half)
        }
        DrawShape::Primitive(Primitive::Sphere { radius, .. }) => {
            let radius = radius.abs();
            (radius, radius, radius)
        }
        DrawShape::Primitive(Primitive::Plane { size, .. }) => {
            (size.abs() * 0.5, 0.0, size.abs() * 0.5)
        }
        // An imported mesh's bounds are not known before it is loaded, and a collider
        // guessed from the file name would be worse than none. `docs/TRACK_SPECS.md`
        // says so: an imported model is scenery, not a wall.
        DrawShape::Obj(_) | DrawShape::Gltf(_) => (0.0, 0.0, 0.0),
    }
}

/// Uploads one shape, returning one `(mesh, material)` pair per drawable primitive.
fn upload_shape(
    render: &mut RenderContext3D,
    shape: &DrawShape,
    asset_root: &std::path::Path,
) -> Result3D<Vec<(MeshHandle, Option<Material>)>> {
    let handle = match shape {
        DrawShape::Primitive(Primitive::Cube { size }) => render.create_cube_mesh(size.abs())?,
        DrawShape::Primitive(Primitive::Sphere {
            radius,
            rings,
            segments,
        }) => render.create_sphere_mesh(radius.abs(), *rings, *segments)?,
        DrawShape::Primitive(Primitive::Plane { size, subdivisions }) => {
            render.create_plane_mesh(size.abs(), *subdivisions)?
        }
        DrawShape::Obj(path) => render.load_obj_mesh(&asset_root.join(path))?,
        DrawShape::Gltf(path) => {
            // Every primitive, with the material the file gave it (ADR-073). A node's
            // `material` is the fallback for a primitive that names none.
            let loaded = render.load_gltf_meshes(&asset_root.join(path))?;
            if loaded.mesh_count() == 0 {
                return Err(giochi_engine_3d::Error3D::Asset(format!(
                    "{path} has no primitives"
                )));
            }
            return Ok(loaded
                .primitives
                .iter()
                .map(|(handle, material)| (*handle, material.as_ref().map(|m| m.to_material())))
                .collect());
        }
    };
    Ok(vec![(handle, None)])
}

fn describe(shape: &DrawShape) -> String {
    match shape {
        DrawShape::Primitive(primitive) => format!("{primitive:?}"),
        DrawShape::Obj(path) => path.clone(),
        DrawShape::Gltf(path) => path.clone(),
    }
}

/// Turns a node's rotation into a heading, for the convention the car uses.
///
/// Only the Y component is read: a car placed with a roll is placed with a heading and a
/// mistake, and the heading is what the race needs.
pub(crate) fn yaw_from(rotation: Quat) -> f32 {
    let quat = rotation.normalized().to_glam();
    // For a pure Y rotation `q = (0, sin(θ/2), 0, cos(θ/2))`, so `2·atan2(y, w)` is θ.
    2.0 * quat.y.atan2(quat.w)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate(index: usize, x: f32, z: f32, radius: f32) -> Checkpoint {
        Checkpoint {
            index,
            position: Vec3::new(x, 0.5, z),
            radius,
            direction: Vec3::new(0.0, 0.0, 1.0),
        }
    }

    #[test]
    fn a_point_at_the_gate_centre_is_inside() {
        assert!(gate(0, 10.0, 2.0, 5.0).is_inside(Vec3::new(10.0, 0.7, 2.0)));
    }

    #[test]
    fn the_radius_is_measured_horizontally() {
        let checkpoint = gate(0, 0.0, 0.0, 5.0);
        assert!(checkpoint.is_inside(Vec3::new(4.9, 0.0, 0.0)));
        assert!(!checkpoint.is_inside(Vec3::new(5.1, 0.0, 0.0)));
        // A point on the circle is inside; the test is inclusive at the edge.
        assert!(checkpoint.is_inside(Vec3::new(3.0, 0.0, 4.0)));
    }

    #[test]
    fn a_car_a_storey_above_the_gate_does_not_trigger_it() {
        let checkpoint = gate(0, 0.0, 0.0, 10.0);
        let above = checkpoint.position.to_glam().y;
        assert!(checkpoint.is_inside(Vec3::new(0.0, above + Checkpoint::MAX_HEIGHT, 0.0)));
        assert!(!checkpoint.is_inside(Vec3::new(0.0, above + Checkpoint::MAX_HEIGHT + 0.1, 0.0)));
        assert!(!checkpoint.is_inside(Vec3::new(0.0, above - Checkpoint::MAX_HEIGHT - 0.1, 0.0)));
    }

    #[test]
    fn a_gate_counts_the_car_below_its_surface_too() {
        // A checkpoint placed 1 m above the road still sees a car sitting on it.
        let checkpoint = gate(0, 0.0, 0.0, 5.0);
        assert!(checkpoint.is_inside(Vec3::new(0.0, -1.0, 0.0)));
    }

    #[test]
    fn yaw_of_the_identity_is_zero_and_a_quarter_turn_is_half_pi() {
        assert_eq!(yaw_from(Quat::IDENTITY), 0.0);
        let quarter = Quat::from_axis_angle(Vec3::UP, std::f32::consts::FRAC_PI_2);
        assert!((yaw_from(quarter) - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
        let half = Quat::from_axis_angle(Vec3::UP, std::f32::consts::PI);
        assert!((yaw_from(half) - std::f32::consts::PI).abs() < 1e-5);
    }

    #[test]
    fn the_bounds_of_two_boxes_cover_both() {
        let track = Track {
            name: "test".to_owned(),
            start: StartPosition::default(),
            checkpoints: Vec::new(),
            instances: vec![
                TrackInstance {
                    shape: DrawShape::Primitive(Primitive::Cube { size: 2.0 }),
                    transform: Transform3D::from_translation(Vec3::new(0.0, 0.0, 0.0)),
                    material: MaterialDesc::default(),
                },
                TrackInstance {
                    shape: DrawShape::Primitive(Primitive::Cube { size: 2.0 }),
                    transform: Transform3D::from_translation(Vec3::new(10.0, 0.0, 0.0)),
                    material: MaterialDesc::default(),
                },
            ],
            colliders: Vec::new(),
            light: None,
        };
        let (min, max) = track.bounds().expect("bounds");
        assert_eq!(min.to_glam(), glam::Vec3::new(-1.0, -1.0, -1.0));
        assert_eq!(max.to_glam(), glam::Vec3::new(11.0, 1.0, 1.0));
    }

    #[test]
    fn an_empty_track_has_no_bounds() {
        let track = Track {
            name: "empty".to_owned(),
            start: StartPosition::default(),
            checkpoints: Vec::new(),
            instances: Vec::new(),
            colliders: Vec::new(),
            light: None,
        };
        assert!(track.bounds().is_none());
    }

    #[test]
    fn a_rotated_box_reports_the_bounds_of_the_rotated_corners() {
        // A 2×2×10 box turned 90° about Y is 10 wide and 2 deep.
        let track = Track {
            name: "rotated".to_owned(),
            start: StartPosition::default(),
            checkpoints: Vec::new(),
            instances: vec![TrackInstance {
                shape: DrawShape::Primitive(Primitive::Cube { size: 1.0 }),
                transform: Transform3D::new(
                    Vec3::ZERO,
                    Quat::from_axis_angle(Vec3::UP, std::f32::consts::FRAC_PI_2),
                    Vec3::new(2.0, 2.0, 10.0),
                ),
                material: MaterialDesc::default(),
            }],
            colliders: Vec::new(),
            light: None,
        };
        let (min, max) = track.bounds().expect("bounds");
        let (min, max) = (min.to_glam(), max.to_glam());
        assert!((min.x + 5.0).abs() < 1e-4, "{min:?}");
        assert!((max.x - 5.0).abs() < 1e-4, "{max:?}");
        assert!((min.z + 1.0).abs() < 1e-4, "{min:?}");
        assert!((max.z - 1.0).abs() < 1e-4, "{max:?}");
    }
}
