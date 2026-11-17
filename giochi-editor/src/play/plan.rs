//! Turning a scene into something the 3D renderer can draw.
//!
//! This is the bridge the editor never had: `giochi-engine-3d` draws meshes it has uploaded,
//! and `scene::Scene` describes a world in JSON. Nothing joined the two until now, which is
//! why the viewport draws a wireframe (ADR-064) and why the racer of phases 41–45 could not
//! exist.
//!
//! # Why the conversion is separate from the uploading
//!
//! [`PlayPlan`] is pure data: no GL, no window, no `Platform`. That is what makes the rules
//! testable in a plain `#[test]` on a machine with no GPU — and the rules are the whole risk
//! here: a camera whose target is not transformed by its node's world matrix looks fine in a
//! scene with the camera at the origin and points the wrong way in every other scene.
//!
//! # What is deliberately not converted
//!
//! - **glTF.** There is no loader (ADR-063); the plan lists the file under
//!   [`PlayPlan::unsupported`] so the editor can say which nodes it is not drawing instead of
//!   leaving a hole in the scene.
//! - **Materials beyond colour.** The engine's shaders take a base colour and a texture; the
//!   scene format's `specular`/`shininess` feed [`DirectionalLight`], not the mesh. Emissive
//!   and roughness do not exist in the format yet.
//! - **Groups.** A group has no geometry; its transform is folded into its children's world
//!   matrices by `world_transform`, which is all a group is for.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use giochi_engine_3d::math3d as m3;
use giochi_engine_3d::math3d::Color3D;
use giochi_engine_3d::render3d::DirectionalLight;
use giochi_engine_3d::scene::{MeshSource, Primitive, Scene, SceneNode};
use glam::{Mat4, Quat, Vec3};

/// What the renderer needs to build a mesh.
///
/// A small, comparable description rather than a handle: it is the cache key for the uploaded
/// meshes (ten cubes share one mesh) and it is what the tests compare.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MeshSpec {
    /// A box with the given edge length.
    Cube {
        /// Edge length in millimetres.
        size: i32,
    },
    /// A UV sphere.
    Sphere {
        /// Radius in millimetres.
        radius_mm: i32,
        /// Horizontal bands.
        rings: u32,
        /// Vertical segments.
        segments: u32,
    },
    /// A plane on the XZ plane.
    Plane {
        /// Side length in millimetres.
        size_mm: i32,
        /// Subdivisions per side.
        subdivisions: u32,
    },
    /// An `.obj` file.
    Obj {
        /// The resolved path.
        path: PathBuf,
    },
}

impl MeshSpec {
    /// The spec for a scene primitive.
    ///
    /// Sizes are rounded to the millimetre: two cubes that differ in the fourth decimal are
    /// the same cube on screen, and rounding is what lets them share a mesh. It also keeps
    /// the key `Eq + Hash`, which an `f32` is not.
    pub fn from_primitive(primitive: &Primitive) -> Self {
        match primitive {
            Primitive::Cube { size } => Self::Cube {
                size: millimetres(*size),
            },
            Primitive::Sphere {
                radius,
                rings,
                segments,
            } => Self::Sphere {
                radius_mm: millimetres(*radius),
                rings: *rings,
                segments: *segments,
            },
            Primitive::Plane { size, subdivisions } => Self::Plane {
                size_mm: millimetres(*size),
                subdivisions: *subdivisions,
            },
        }
    }

    /// The spec for a mesh source, resolving an `.obj` against `base`.
    pub fn from_source(source: &MeshSource, base: Option<&Path>) -> Self {
        match source {
            MeshSource::Primitive(primitive) => Self::from_primitive(primitive),
            MeshSource::Obj(path) => {
                let path = PathBuf::from(path);
                Self::Obj {
                    path: match base {
                        Some(base) if path.is_relative() => base.join(path),
                        _ => path,
                    },
                }
            }
        }
    }

    /// The edge length or diameter the spec draws, in world units.
    pub fn extent(&self) -> f32 {
        match self {
            Self::Cube { size } => *size as f32 / 1000.0,
            Self::Sphere { radius_mm, .. } => 2.0 * *radius_mm as f32 / 1000.0,
            Self::Plane { size_mm, .. } => *size_mm as f32 / 1000.0,
            // An `.obj` has no declared size; a caller framing a scene uses this as a guess.
            Self::Obj { .. } => 1.0,
        }
    }

    /// The spec as the millimetre-free values the renderer needs.
    pub fn primitive_values(&self) -> Option<Primitive> {
        match self {
            Self::Cube { size } => Some(Primitive::Cube {
                size: *size as f32 / 1000.0,
            }),
            Self::Sphere {
                radius_mm,
                rings,
                segments,
            } => Some(Primitive::Sphere {
                radius: *radius_mm as f32 / 1000.0,
                rings: *rings,
                segments: *segments,
            }),
            Self::Plane {
                size_mm,
                subdivisions,
            } => Some(Primitive::Plane {
                size: *size_mm as f32 / 1000.0,
                subdivisions: *subdivisions,
            }),
            Self::Obj { .. } => None,
        }
    }
}

fn millimetres(value: f32) -> i32 {
    (value * 1000.0).round() as i32
}

/// One mesh to draw, with its world transform and colour.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayInstance {
    /// The mesh, which the renderer uploads once per distinct spec.
    pub mesh: MeshSpec,
    /// Where it is, in world space.
    pub transform: m3::Transform3D,
    /// Its base colour, linear RGBA.
    pub color: Color3D,
    /// The node it came from, for logs.
    pub node: u64,
}

/// The camera the preview looks through.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayCamera {
    /// Eye.
    pub position: Vec3,
    /// Look-at point.
    pub target: Vec3,
    /// Up.
    pub up: Vec3,
    /// Vertical field of view, radians.
    pub fov_y: f32,
    /// Near plane.
    pub near: f32,
    /// Far plane.
    pub far: f32,
}

impl PlayCamera {
    /// A camera that frames a sphere of `radius` around `centre`.
    pub fn framing(centre: Vec3, radius: f32) -> Self {
        let radius = radius.max(0.5);
        let distance = radius * 3.0;
        Self {
            position: centre + Vec3::new(distance * 0.6, distance * 0.45, distance * 0.8),
            target: centre,
            up: Vec3::Y,
            fov_y: 60f32.to_radians(),
            near: (radius * 0.01).max(0.01),
            far: (radius * 100.0).max(100.0),
        }
    }

    /// This camera as the crate's `Camera3D`, for a drawable of `width`×`height`.
    pub fn to_camera3d(&self, width: u32, height: u32) -> m3::Camera3D {
        // The crate ships its own `#[repr(C)]` math types for the C ABI; the editor works in
        // `glam`. The conversion happens exactly here, at the boundary.
        let mut camera = m3::Camera3D::from_parts(
            m3::Vec3::from_glam(self.position),
            m3::Vec3::from_glam(self.target),
            m3::Vec3::from_glam(self.up),
        );
        camera.fov_y = self.fov_y;
        camera.near_plane = self.near;
        camera.far_plane = self.far;
        camera.with_drawable_size(width.max(1), height.max(1))
    }
}

/// Everything the preview needs, as data.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayPlan {
    /// The meshes to draw, in scene order.
    pub instances: Vec<PlayInstance>,
    /// The light: the scene's first light node, or the engine default.
    pub light: DirectionalLight,
    /// The camera: the scene's first camera node, or one framing the geometry.
    pub camera: PlayCamera,
    /// Nodes that were not converted, with the reason.
    pub unsupported: Vec<(u64, String)>,
    /// Every distinct mesh, so the renderer knows what to upload.
    pub meshes: Vec<MeshSpec>,
}

impl PlayPlan {
    /// Converts `scene`. `base` resolves relative `.obj` paths (the project root).
    pub fn from_scene(scene: &Scene, base: Option<&Path>) -> Self {
        let mut instances = Vec::new();
        let mut unsupported = Vec::new();
        let mut meshes: Vec<MeshSpec> = Vec::new();

        for node in &scene.nodes {
            let Some(source) = node.mesh.as_ref() else {
                continue;
            };
            let spec = MeshSpec::from_source(source, base);
            if let MeshSpec::Obj { path } = &spec {
                if path
                    .extension()
                    .is_some_and(|ext| ext == "gltf" || ext == "glb")
                {
                    // No loader (ADR-063). Saying so is the difference between "the editor
                    // is broken" and "this file needs the Production Pack".
                    unsupported.push((node.id.raw(), format!("no loader for {}", path.display())));
                    continue;
                }
                if !path.exists() {
                    unsupported.push((node.id.raw(), format!("missing {}", path.display())));
                    continue;
                }
            }
            if !meshes.contains(&spec) {
                meshes.push(spec.clone());
            }
            instances.push(instance(scene, node, spec));
        }

        Self {
            light: light_of(scene),
            camera: camera_of(scene, &instances),
            instances,
            unsupported,
            meshes,
        }
    }

    /// How many meshes the renderer has to upload.
    pub fn mesh_count(&self) -> usize {
        self.meshes.len()
    }

    /// The world-space bounds of every instance, for framing a camera.
    pub fn bounds(&self) -> Option<([f32; 3], [f32; 3])> {
        bounds_of(&self.instances)
    }
}

/// The bounds of a set of instances, as `(min, max)`.
fn bounds_of(instances: &[PlayInstance]) -> Option<([f32; 3], [f32; 3])> {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for instance in instances {
        let scale = instance.transform.scale;
        let half =
            instance.mesh.extent() * 0.5 * scale.x.abs().max(scale.y.abs()).max(scale.z.abs());
        let position = instance.transform.position;
        let centre = [position.x, position.y, position.z];
        for axis in 0..3 {
            min[axis] = min[axis].min(centre[axis] - half);
            max[axis] = max[axis].max(centre[axis] + half);
        }
    }
    min[0].is_finite().then_some((min, max))
}

fn instance(scene: &Scene, node: &SceneNode, mesh: MeshSpec) -> PlayInstance {
    let world = crate::editor::viewport::world_transform(scene, node);
    let color = node.material.base_color;
    PlayInstance {
        mesh,
        transform: world.to_transform3d(),
        color: Color3D::new(color[0], color[1], color[2], color[3]).clamped(),
        node: node.id.raw(),
    }
}

/// The light of the scene: the first light node, turned by its world rotation.
fn light_of(scene: &Scene) -> DirectionalLight {
    for node in &scene.nodes {
        let Some(desc) = node.light else {
            continue;
        };
        let mut light = desc.to_light();
        let world = crate::editor::viewport::world_transform(scene, node);
        let rotation = Quat::from_array(world.rotation);
        let direction =
            rotation * Vec3::new(desc.direction[0], desc.direction[1], desc.direction[2]);
        if direction.length_squared() > 1e-8 {
            light.direction = m3::Vec3::from_glam(direction.normalize());
        }
        return light;
    }
    DirectionalLight::default()
}

/// The camera of the scene: the first camera node, with its transform applied to both the
/// eye *and* the look-at point — a camera that is moved but not re-aimed looks at where it
/// used to be.
fn camera_of(scene: &Scene, instances: &[PlayInstance]) -> PlayCamera {
    for node in &scene.nodes {
        let Some(desc) = node.camera else {
            continue;
        };
        let world = crate::editor::viewport::world_transform(scene, node);
        let matrix = world.matrix();
        // The crate's `Mat4::transform_point` takes and returns its own `Vec3`; the glam
        // round trip is the shortest honest way through it.
        let position = matrix
            .transform_point(m3::Vec3::new(
                desc.position[0],
                desc.position[1],
                desc.position[2],
            ))
            .to_glam();
        let target = matrix
            .transform_point(m3::Vec3::new(
                desc.target[0],
                desc.target[1],
                desc.target[2],
            ))
            .to_glam();
        let rotation = Mat4::from_quat(Quat::from_array(world.rotation));
        let up = rotation.transform_vector3(Vec3::new(desc.up[0], desc.up[1], desc.up[2]));
        return PlayCamera {
            position,
            target,
            up: if up.length_squared() > 1e-8 {
                up.normalize()
            } else {
                Vec3::Y
            },
            fov_y: desc.fov_y,
            near: desc.near,
            far: desc.far,
        };
    }
    match bounds_of(instances) {
        Some((min, max)) => {
            let centre = Vec3::new(
                (min[0] + max[0]) * 0.5,
                (min[1] + max[1]) * 0.5,
                (min[2] + max[2]) * 0.5,
            );
            let radius =
                Vec3::new(max[0] - min[0], max[1] - min[1], max[2] - min[2]).length() * 0.5;
            PlayCamera::framing(centre, radius)
        }
        None => PlayCamera::framing(Vec3::ZERO, 2.0),
    }
}

/// "1 mesh(es), 4 instance(s), 0 unsupported", for a log line.
pub fn summary(plan: &PlayPlan) -> String {
    let mut counts: HashMap<&MeshSpec, usize> = HashMap::new();
    for instance in &plan.instances {
        *counts.entry(&instance.mesh).or_default() += 1;
    }
    format!(
        "{} mesh(es), {} instance(s), {} unsupported",
        counts.len(),
        plan.instances.len(),
        plan.unsupported.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use giochi_engine_3d::scene::{CameraDesc, LightDesc, SceneNode, SceneNodeId};

    fn shadowed_scene() -> Scene {
        // A group scaled and moved, with a cube inside it: the instance's world transform
        // must be the composition, not the node's local one.
        let mut scene = Scene::new("play");
        let mut group = SceneNode::new(SceneNodeId::new(0), "Group");
        group.transform.position = [2.0, 0.0, 0.0];
        group.transform.scale = [2.0, 2.0, 2.0];
        let group = scene.add_node(group);

        let mut cube = SceneNode::new(SceneNodeId::new(0), "Cube");
        cube.parent = Some(group.id);
        cube.transform.position = [1.0, 1.0, 0.0];
        cube.mesh = Some(MeshSource::Primitive(Primitive::Cube { size: 1.0 }));
        cube.material.base_color = [0.5, 0.25, 0.125, 1.0];
        scene.add_node(cube);

        scene
    }

    #[test]
    fn an_instance_carries_the_world_transform_not_the_local_one() {
        let scene = shadowed_scene();
        let plan = PlayPlan::from_scene(&scene, None);
        assert_eq!(plan.instances.len(), 1);
        let transform = plan.instances[0].transform;
        // 2 (group) + 1 * 2 (group scale) along X, and 1 * 2 along Y.
        assert!(
            (transform.position.x - 4.0).abs() < 1e-5,
            "{:?}",
            transform.position
        );
        assert!(
            (transform.position.y - 2.0).abs() < 1e-5,
            "{:?}",
            transform.position
        );
        assert!(
            (transform.scale.x - 2.0).abs() < 1e-5,
            "{:?}",
            transform.scale
        );
    }

    #[test]
    fn the_material_colour_reaches_the_instance() {
        let scene = shadowed_scene();
        let plan = PlayPlan::from_scene(&scene, None);
        let color = plan.instances[0].color;
        assert!((color.r - 0.5).abs() < 1e-5);
        assert!((color.g - 0.25).abs() < 1e-5);
        assert!((color.b - 0.125).abs() < 1e-5);
    }

    #[test]
    fn identical_primitives_share_one_mesh() {
        let mut scene = Scene::new("shared");
        for index in 0..4 {
            let mut node = SceneNode::new(SceneNodeId::new(0), format!("Cube {index}"));
            node.mesh = Some(MeshSource::Primitive(Primitive::Cube { size: 1.0 }));
            node.transform.position = [index as f32 * 2.0, 0.0, 0.0];
            scene.add_node(node);
        }
        let plan = PlayPlan::from_scene(&scene, None);
        assert_eq!(plan.mesh_count(), 1, "four cubes, one mesh");
        assert_eq!(plan.instances.len(), 4);
        assert_eq!(summary(&plan), "1 mesh(es), 4 instance(s), 0 unsupported");
    }

    #[test]
    fn a_different_size_is_a_different_mesh() {
        let mut scene = Scene::new("sizes");
        for size in [1.0, 2.0, 1.0] {
            let mut node = SceneNode::new(SceneNodeId::new(0), "Cube");
            node.mesh = Some(MeshSource::Primitive(Primitive::Cube { size }));
            scene.add_node(node);
        }
        assert_eq!(PlayPlan::from_scene(&scene, None).mesh_count(), 2);
    }

    #[test]
    fn groups_lights_and_cameras_are_not_instances() {
        let mut scene = shadowed_scene();
        let mut light = SceneNode::new(SceneNodeId::new(0), "Sun");
        light.light = Some(LightDesc::default());
        scene.add_node(light);
        let mut camera = SceneNode::new(SceneNodeId::new(0), "Camera");
        camera.camera = Some(CameraDesc::default());
        scene.add_node(camera);
        let plan = PlayPlan::from_scene(&scene, None);
        assert_eq!(plan.instances.len(), 1, "only the cube draws");
    }

    #[test]
    fn a_missing_obj_is_reported_instead_of_drawn() {
        let mut scene = Scene::new("obj");
        let mut node = SceneNode::new(SceneNodeId::new(0), "Car");
        node.mesh = Some(MeshSource::Obj("assets/models/nope.obj".to_owned()));
        let id = scene.add_node(node).id;
        let plan = PlayPlan::from_scene(&scene, Some(Path::new("/tmp/nothing-here")));
        assert!(plan.instances.is_empty());
        assert_eq!(plan.unsupported.len(), 1);
        assert_eq!(plan.unsupported[0].0, id.raw());
        assert!(plan.unsupported[0].1.contains("missing"));
    }

    #[test]
    fn a_gltf_is_reported_as_having_no_loader() {
        let mut scene = Scene::new("gltf");
        let mut node = SceneNode::new(SceneNodeId::new(0), "Car");
        node.mesh = Some(MeshSource::Obj("car.gltf".to_owned()));
        scene.add_node(node);
        let plan = PlayPlan::from_scene(&scene, None);
        assert_eq!(plan.unsupported.len(), 1);
        assert!(
            plan.unsupported[0].1.contains("no loader"),
            "{:?}",
            plan.unsupported
        );
    }

    #[test]
    fn a_scene_camera_is_looked_through_with_its_node_transform_applied() {
        let mut scene = Scene::new("camera");
        let mut node = SceneNode::new(SceneNodeId::new(0), "Camera");
        node.camera = Some(CameraDesc {
            position: [0.0, 0.0, 5.0],
            target: [0.0, 0.0, 0.0],
            ..CameraDesc::default()
        });
        scene.add_node(node);
        let plan = PlayPlan::from_scene(&scene, None);
        assert!((plan.camera.position.z - 5.0).abs() < 1e-4);
        assert!(plan.camera.target.z.abs() < 1e-4);

        // Move the node: both the eye and the look-at point move with it, or the camera
        // would keep staring at where it used to be.
        if let Some(node) = scene.nodes.first_mut() {
            node.transform.position = [10.0, 0.0, 0.0];
        }
        let moved = PlayPlan::from_scene(&scene, None);
        assert!((moved.camera.position.x - 10.0).abs() < 1e-4);
        assert!((moved.camera.target.x - 10.0).abs() < 1e-4);
    }

    #[test]
    fn a_scene_without_a_camera_gets_one_that_frames_the_geometry() {
        let mut scene = Scene::new("nocam");
        let mut node = SceneNode::new(SceneNodeId::new(0), "Cube");
        node.transform.position = [0.0, 1.0, 0.0];
        node.mesh = Some(MeshSource::Primitive(Primitive::Cube { size: 2.0 }));
        scene.add_node(node);
        let plan = PlayPlan::from_scene(&scene, None);
        assert!(
            plan.camera.position.length() > 1.0,
            "the eye is outside the box"
        );
        assert!(
            (plan.camera.target.y - 1.0).abs() < 1.0,
            "and looks at the geometry"
        );
        assert!(plan.camera.far > plan.camera.near);
    }

    #[test]
    fn the_light_comes_from_the_first_light_node_and_turns_with_it() {
        let mut scene = Scene::new("light");
        let mut node = SceneNode::new(SceneNodeId::new(0), "Sun");
        node.transform.rotation =
            glam::Quat::from_rotation_z(std::f32::consts::FRAC_PI_2).to_array();
        node.light = Some(LightDesc {
            direction: [0.0, -1.0, 0.0],
            ..LightDesc::default()
        });
        scene.add_node(node);
        let plan = PlayPlan::from_scene(&scene, None);
        // +90° about Z sends -Y to +X.
        assert!(plan.light.direction.x > 0.99, "{:?}", plan.light.direction);
    }

    #[test]
    fn a_scene_with_no_light_gets_the_engine_default() {
        let scene = shadowed_scene();
        let plan = PlayPlan::from_scene(&scene, None);
        assert_eq!(plan.light.direction, DirectionalLight::default().direction);
    }

    #[test]
    fn the_bounds_cover_every_instance() {
        let scene = shadowed_scene();
        let (min, max) = PlayPlan::from_scene(&scene, None).bounds().expect("bounds");
        // The cube is 1×1×1 at local (1,1,0), scaled by 2 and moved by 2 in X.
        assert!((min[0] - 3.0).abs() < 1e-4, "{min:?}");
        assert!((max[0] - 5.0).abs() < 1e-4, "{max:?}");
    }

    #[test]
    fn an_empty_scene_has_no_bounds_and_a_framed_camera() {
        let scene = Scene::new("empty");
        let plan = PlayPlan::from_scene(&scene, None);
        assert!(plan.bounds().is_none());
        assert!(plan.instances.is_empty());
        assert!(plan.camera.position.length() > 0.0);
    }
}
