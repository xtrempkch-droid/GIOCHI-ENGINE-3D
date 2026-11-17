//! The scene format: a hierarchy of nodes, saved as JSON.
//!
//! # Why this lives in the 3D crate and not in the editor
//!
//! The plan's phase 27 asked for a serialisable level format, and the editor was
//! specified to consume it rather than invent one. The consumer that matters is not the
//! editor: `giochi-racer` has to load a `.scene` at boot **without** the editor being
//! installed. Putting the format in the crate that owns the 3D types is what makes that
//! possible, and it keeps the editor's own metadata (viewport camera, selection) out of
//! the file the game reads — phase 38 adds that sidecar.
//!
//! # The file
//!
//! ```json
//! {
//!   "format": "giochi-scene",
//!   "version": 1,
//!   "name": "default",
//!   "next_id": 4,
//!   "nodes": [
//!     { "id": 1, "name": "Cube", "parent": null,
//!       "transform": { "position": [0, 0.5, 0], "rotation": [0, 0, 0, 1], "scale": [1, 1, 1] },
//!       "mesh": { "primitive": { "kind": "cube", "size": 1.0 } },
//!       "material": { "base_color": [0.8, 0.8, 0.8, 1.0], "texture": null,
//!                     "specular": 0.35, "shininess": 32.0 } },
//!     { "id": 2, "name": "Sun", "parent": null,
//!       "light": { "direction": [0, -1, 0], "color": [1, 1, 1, 1],
//!                  "ambient": 0.25, "specular": 0.4, "shininess": 48.0 } },
//!     { "id": 3, "name": "Camera", "parent": null,
//!       "camera": { "position": [4, 3, 6], "target": [0, 0.5, 0], "up": [0, 1, 0],
//!                   "fov_y": 1.0472, "near": 0.1, "far": 500.0 } }
//!   ]
//! }
//! ```
//!
//! Numbers are plain `f32` arrays rather than the `#[repr(C)]` math types, so the format
//! does not inherit `_reserved` tails and does not need `serde` on `glam`. A node stores
//! a **parent by id**, never a path, so renaming or moving a subtree cannot break the
//! file.
//!
//! # Stability
//!
//! [`SceneNodeId`] is an opaque `u64` that is never reused within a file: `next_id` is
//! persisted and only grows. An undo stack (phase 37) that stores ids therefore cannot
//! undo an operation on the wrong node after a delete.
//!
//! [`SCENE_FORMAT_VERSION`] is the file's version. [`Scene::from_json`] refuses a newer
//! file than it understands instead of guessing, and [`migrate`] holds the per-version
//! steps (none yet — version 1 is the first).

use std::fmt;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::math3d::{Camera3D, Color3D, Mat4, Quat, Vec3};

/// The `format` field every scene file carries.
pub const SCENE_FORMAT: &str = "giochi-scene";

/// The newest file version this crate reads and writes.
pub const SCENE_FORMAT_VERSION: u32 = 1;

/// Anything that can go wrong loading or saving a scene.
#[derive(Debug, thiserror::Error)]
pub enum SceneError {
    /// The file could not be read or written.
    #[error("scene i/o error at {path}: {source}")]
    Io {
        /// The path involved.
        path: String,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },
    /// The file is not valid JSON, or does not match the schema.
    #[error("scene {path} is not valid JSON for this format: {source}")]
    Json {
        /// The path involved.
        path: String,
        /// The underlying serde error.
        #[source]
        source: serde_json::Error,
    },
    /// The file says it is something else.
    #[error("scene {path} is not a giochi scene (format = {found:?})")]
    NotAScene {
        /// The path involved.
        path: String,
        /// What the `format` field said.
        found: String,
    },
    /// The file is newer than this build understands.
    #[error(
        "scene {path} is version {found}; this build reads up to {supported} \
         (upgrade the engine, do not edit the file)"
    )]
    UnsupportedVersion {
        /// The path involved.
        path: String,
        /// The version in the file.
        found: u32,
        /// [`SCENE_FORMAT_VERSION`].
        supported: u32,
    },
    /// The hierarchy is internally inconsistent: duplicate ids, a parent that does not
    /// exist, or a cycle.
    #[error("scene {path} is invalid: {reason}")]
    Invalid {
        /// The path involved.
        path: String,
        /// What is wrong.
        reason: String,
    },
}

/// Identifies one node in a scene.
///
/// A value, not an index: indices shift when a node is deleted or reparented, and both
/// the undo stack (phase 37) and the editor's selection (phase 32) outlive a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SceneNodeId(pub u64);

impl SceneNodeId {
    /// Wraps a raw identifier.
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// The raw identifier, for a file or a command.
    pub const fn raw(self) -> u64 {
        self.0
    }
}

impl fmt::Display for SceneNodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// A position, rotation and scale in the file's own layout.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeTransform {
    /// Translation.
    #[serde(default)]
    pub position: [f32; 3],
    /// Rotation as a quaternion `[x, y, z, w]`. The zero quaternion is read as identity.
    #[serde(default = "identity_rotation")]
    pub rotation: [f32; 4],
    /// Per-axis scale.
    #[serde(default = "unit_scale")]
    pub scale: [f32; 3],
}

fn identity_rotation() -> [f32; 4] {
    [0.0, 0.0, 0.0, 1.0]
}

fn unit_scale() -> [f32; 3] {
    [1.0, 1.0, 1.0]
}

impl Default for NodeTransform {
    fn default() -> Self {
        Self {
            position: [0.0; 3],
            rotation: identity_rotation(),
            scale: unit_scale(),
        }
    }
}

impl NodeTransform {
    /// Builds one from its three parts.
    pub const fn new(position: [f32; 3], rotation: [f32; 4], scale: [f32; 3]) -> Self {
        Self {
            position,
            rotation,
            scale,
        }
    }

    /// The engine type, for the renderer.
    pub fn to_transform3d(self) -> crate::math3d::Transform3D {
        let rotation = Quat::new(
            self.rotation[0],
            self.rotation[1],
            self.rotation[2],
            self.rotation[3],
        );
        // A zero quaternion has no rotation to apply and would normalise to NaN; a file
        // written by hand with `[0,0,0,0]` must not produce a corrupt matrix.
        // `Quat::normalized` already falls back to the identity for that case.
        let rotation = rotation.normalized();
        crate::math3d::Transform3D::new(
            Vec3::new(self.position[0], self.position[1], self.position[2]),
            rotation,
            Vec3::new(self.scale[0], self.scale[1], self.scale[2]),
        )
    }

    /// The file layout for an engine transform.
    pub fn from_transform3d(transform: &crate::math3d::Transform3D) -> Self {
        Self {
            position: [
                transform.position.x,
                transform.position.y,
                transform.position.z,
            ],
            rotation: [
                transform.rotation.x,
                transform.rotation.y,
                transform.rotation.z,
                transform.rotation.w,
            ],
            scale: [transform.scale.x, transform.scale.y, transform.scale.z],
        }
    }

    /// The model matrix: scale, then rotation, then translation.
    pub fn matrix(self) -> Mat4 {
        self.to_transform3d().matrix()
    }

    /// The axis-aligned bounds of the unit cube transformed by this node.
    ///
    /// Used by the editor's viewport to draw a node's box without a GPU mesh. A rotated
    /// box is not axis-aligned, so this is the bound of the *rotated* corners — which is
    /// what a wireframe selection box wants.
    pub fn aabb(self) -> ([f32; 3], [f32; 3]) {
        let matrix = self.matrix().to_cols_array();
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for corner in 0..8 {
            let x = if corner & 1 == 0 { -0.5 } else { 0.5 };
            let y = if corner & 2 == 0 { -0.5 } else { 0.5 };
            let z = if corner & 4 == 0 { -0.5 } else { 0.5 };
            // Column-major: m[0..4] is the first column.
            let wx = matrix[0] * x + matrix[4] * y + matrix[8] * z + matrix[12];
            let wy = matrix[1] * x + matrix[5] * y + matrix[9] * z + matrix[13];
            let wz = matrix[2] * x + matrix[6] * y + matrix[10] * z + matrix[14];
            min[0] = min[0].min(wx);
            min[1] = min[1].min(wy);
            min[2] = min[2].min(wz);
            max[0] = max[0].max(wx);
            max[1] = max[1].max(wy);
            max[2] = max[2].max(wz);
        }
        (min, max)
    }
}

/// A built-in mesh, described by its parameters rather than by a handle.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Primitive {
    /// A cube with the given edge length.
    Cube {
        /// Edge length.
        #[serde(default = "unit_size")]
        size: f32,
    },
    /// A UV sphere.
    Sphere {
        /// Radius.
        #[serde(default = "half_unit")]
        radius: f32,
        /// Horizontal bands.
        #[serde(default = "default_rings")]
        rings: u32,
        /// Vertical segments.
        #[serde(default = "default_segments")]
        segments: u32,
    },
    /// A flat plane on the XZ plane.
    Plane {
        /// Side length.
        #[serde(default = "default_plane_size")]
        size: f32,
        /// Subdivisions per side.
        #[serde(default = "default_subdivisions")]
        subdivisions: u32,
    },
}

fn unit_size() -> f32 {
    1.0
}
fn half_unit() -> f32 {
    0.5
}
fn default_rings() -> u32 {
    16
}
fn default_segments() -> u32 {
    24
}
fn default_plane_size() -> f32 {
    10.0
}
fn default_subdivisions() -> u32 {
    1
}

impl Default for Primitive {
    fn default() -> Self {
        Self::Cube { size: 1.0 }
    }
}

/// Where a node's geometry comes from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeshSource {
    /// A mesh this crate can generate.
    Primitive(Primitive),
    /// An `.obj` file on disk, relative to the scene's asset root.
    Obj(String),
    /// A `.gltf` or `.glb` file, relative to the scene's asset root.
    ///
    /// A separate variant rather than a second extension on `Obj`: the two are different
    /// formats with different loaders, a glTF file holds several primitives with their own
    /// materials, and a scene that says which is which can be read by a tool that only knows
    /// one of them.
    Gltf(String),
}

/// How a node's surface is shaded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialDesc {
    /// Linear RGBA.
    #[serde(default = "default_base_color")]
    pub base_color: [f32; 4],
    /// Texture path, relative to the scene's asset root.
    #[serde(default)]
    pub texture: Option<String>,
    /// Specular strength, `0.0` is matte.
    #[serde(default)]
    pub specular: f32,
    /// Specular exponent.
    #[serde(default = "default_shininess")]
    pub shininess: f32,
    /// Linear emissive RGB, added after lighting. `[0, 0, 0]` is off.
    ///
    /// The one channel a GLES 2.0 shader can afford and an artist notices: a tail light, a
    /// neon strip, an instrument panel that reads as lit instead of merely bright.
    #[serde(default)]
    pub emissive: [f32; 3],
    /// `0.0` is a mirror, `1.0` is fully diffuse.
    ///
    /// Kept alongside `shininess` rather than replacing it: `shininess` is the number the
    /// shader takes, `roughness` is the number a glTF file and a human both speak. The
    /// conversion lives in [`MaterialDesc::to_material`] and is the only place that knows
    /// both (ADR-074).
    #[serde(default = "default_roughness")]
    pub roughness: f32,
    /// Metalness, `0.0..=1.0`. Tints the highlight and removes the diffuse term, which is
    /// what makes chrome read as chrome and not as white plastic.
    #[serde(default)]
    pub metallic: f32,
}

fn default_base_color() -> [f32; 4] {
    [0.8, 0.8, 0.8, 1.0]
}
fn default_shininess() -> f32 {
    32.0
}

impl Default for MaterialDesc {
    fn default() -> Self {
        Self {
            base_color: default_base_color(),
            texture: None,
            specular: 0.0,
            shininess: default_shininess(),
            emissive: [0.0; 3],
            roughness: default_roughness(),
            metallic: 0.0,
        }
    }
}

fn default_roughness() -> f32 {
    0.5
}

impl MaterialDesc {
    /// This surface as the renderer takes it.
    ///
    /// `roughness` wins over `shininess` when a file sets it: a glTF import fills roughness
    /// and leaves shininess at its default, and the reverse is true of a hand-written scene.
    /// The `shininess` field is kept as the escape hatch for a caller that wants the exact
    /// exponent, so a roughness of the default `0.5` still respects a hand-set `shininess`.
    pub fn to_material(&self) -> crate::render3d::Material {
        let mut material = crate::render3d::Material {
            base_color: crate::math3d::Color3D::new(
                self.base_color[0],
                self.base_color[1],
                self.base_color[2],
                self.base_color[3],
            ),
            specular: self.specular,
            metallic: self.metallic.clamp(0.0, 1.0),
            roughness: self.roughness.clamp(0.0, 1.0),
            ..crate::render3d::Material::default()
        };
        material.emissive = crate::math3d::Color3D::new(
            self.emissive[0].max(0.0),
            self.emissive[1].max(0.0),
            self.emissive[2].max(0.0),
            1.0,
        );
        // `shininess` is honoured when it was set to something other than the default, so a
        // scene written before roughness existed keeps its highlight.
        if (self.roughness - default_roughness()).abs() > f32::EPSILON {
            material.shininess = material.shininess_from_roughness();
        } else {
            material.shininess = self.shininess;
        }
        material
    }
}

/// A directional light, in file form.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LightDesc {
    /// The direction the light travels, normalised on load.
    #[serde(default = "default_light_direction")]
    pub direction: [f32; 3],
    /// Linear RGBA; values above 1.0 are legal. A 3-component array is read as RGB with
    /// alpha 1.
    #[serde(default = "default_white", deserialize_with = "de_rgba")]
    pub color: [f32; 4],
    /// Ambient fraction.
    #[serde(default = "default_ambient")]
    pub ambient: f32,
    /// Specular strength.
    #[serde(default = "default_specular")]
    pub specular: f32,
    /// Specular exponent.
    #[serde(default = "default_light_shininess")]
    pub shininess: f32,
}

fn default_light_direction() -> [f32; 3] {
    [0.0, -1.0, 0.0]
}
fn default_white() -> [f32; 4] {
    [1.0, 1.0, 1.0, 1.0]
}
fn default_ambient() -> f32 {
    0.25
}
fn default_specular() -> f32 {
    0.4
}
fn default_light_shininess() -> f32 {
    48.0
}

/// Accepts a 3-component colour as RGB with full alpha, or 4 components.
///
/// Written scenes always contain 4; accepting 3 is what makes a hand-written file
/// (`"color": [1, 0.9, 0.8]`) work instead of failing with a type error.
fn de_rgba<'de, D>(deserializer: D) -> Result<[f32; 4], D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Rgba {
        Four([f32; 4]),
        Three([f32; 3]),
    }
    Ok(match Rgba::deserialize(deserializer)? {
        Rgba::Four(rgba) => rgba,
        Rgba::Three([r, g, b]) => [r, g, b, 1.0],
    })
}

impl Default for LightDesc {
    fn default() -> Self {
        Self {
            direction: default_light_direction(),
            color: default_white(),
            ambient: default_ambient(),
            specular: default_specular(),
            shininess: default_light_shininess(),
        }
    }
}

impl LightDesc {
    /// The engine type.
    pub fn to_light(self) -> crate::render3d::DirectionalLight {
        let length = (self.direction[0] * self.direction[0]
            + self.direction[1] * self.direction[1]
            + self.direction[2] * self.direction[2])
            .sqrt();
        let direction = if length > 1e-6 {
            [
                self.direction[0] / length,
                self.direction[1] / length,
                self.direction[2] / length,
            ]
        } else {
            default_light_direction()
        };
        crate::render3d::DirectionalLight {
            direction: Vec3::new(direction[0], direction[1], direction[2]),
            color: Color3D::new(self.color[0], self.color[1], self.color[2], self.color[3]),
            ambient: self.ambient.clamp(0.0, 1.0),
            specular: self.specular.max(0.0),
            shininess: self.shininess.max(1.0),
            _reserved: [0; 4],
        }
    }
}

/// A perspective camera, in file form.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraDesc {
    /// Eye position.
    #[serde(default = "default_camera_position")]
    pub position: [f32; 3],
    /// Look-at point.
    #[serde(default = "default_camera_target")]
    pub target: [f32; 3],
    /// Up vector.
    #[serde(default = "default_up")]
    pub up: [f32; 3],
    /// Vertical field of view, radians.
    #[serde(default = "default_fov")]
    pub fov_y: f32,
    /// Near plane.
    #[serde(default = "default_near")]
    pub near: f32,
    /// Far plane.
    #[serde(default = "default_far")]
    pub far: f32,
}

fn default_camera_position() -> [f32; 3] {
    [4.0, 3.0, 6.0]
}
fn default_camera_target() -> [f32; 3] {
    [0.0, 0.5, 0.0]
}
fn default_up() -> [f32; 3] {
    [0.0, 1.0, 0.0]
}
fn default_fov() -> f32 {
    std::f32::consts::FRAC_PI_3
}
fn default_near() -> f32 {
    0.1
}
fn default_far() -> f32 {
    500.0
}

impl Default for CameraDesc {
    fn default() -> Self {
        Self {
            position: default_camera_position(),
            target: default_camera_target(),
            up: default_up(),
            fov_y: default_fov(),
            near: default_near(),
            far: default_far(),
        }
    }
}

impl CameraDesc {
    /// The engine type, with `aspect` left at `0.0` ("use the drawable size").
    pub fn to_camera3d(self, aspect: f32) -> Camera3D {
        Camera3D {
            position: Vec3::new(self.position[0], self.position[1], self.position[2]),
            target: Vec3::new(self.target[0], self.target[1], self.target[2]),
            up: Vec3::new(self.up[0], self.up[1], self.up[2]),
            fov_y: self.fov_y,
            near_plane: self.near,
            far_plane: self.far,
            aspect,
            _reserved: [0; 4],
        }
    }
}

/// One object in a scene.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneNode {
    /// Stable identifier, unique within the file.
    pub id: SceneNodeId,
    /// Display name. Not required to be unique, but the editor warns when it is not.
    pub name: String,
    /// The containing node, if any.
    #[serde(default)]
    pub parent: Option<SceneNodeId>,
    /// Local transform, relative to the parent.
    #[serde(default)]
    pub transform: NodeTransform,
    /// Geometry, if this node draws something.
    #[serde(default)]
    pub mesh: Option<MeshSource>,
    /// Surface, if this node draws something.
    #[serde(default)]
    pub material: MaterialDesc,
    /// A light, if this node is one.
    #[serde(default)]
    pub light: Option<LightDesc>,
    /// A camera, if this node is one.
    #[serde(default)]
    pub camera: Option<CameraDesc>,
}

impl SceneNode {
    /// A named empty at the origin.
    pub fn new(id: SceneNodeId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            parent: None,
            transform: NodeTransform::default(),
            mesh: None,
            material: MaterialDesc::default(),
            light: None,
            camera: None,
        }
    }

    /// Whether this node draws geometry.
    pub const fn is_mesh(&self) -> bool {
        self.mesh.is_some()
    }

    /// Whether this node is a light.
    pub const fn is_light(&self) -> bool {
        self.light.is_some()
    }

    /// Whether this node is a camera.
    pub const fn is_camera(&self) -> bool {
        self.camera.is_some()
    }

    /// A one-word kind, for the hierarchy panel's icon and the console.
    pub fn kind(&self) -> &'static str {
        if self.is_camera() {
            "camera"
        } else if self.is_light() {
            "light"
        } else if self.is_mesh() {
            "mesh"
        } else {
            "group"
        }
    }
}

/// A whole scene: metadata plus a flat list of nodes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scene {
    /// Always [`SCENE_FORMAT`] for a file this crate wrote.
    #[serde(default = "default_format")]
    pub format: String,
    /// [`SCENE_FORMAT_VERSION`] for a file this crate wrote.
    #[serde(default = "default_version")]
    pub version: u32,
    /// The scene's name.
    #[serde(default)]
    pub name: String,
    /// The id the next [`Scene::add_node`] will use. Persisted so ids are never reused.
    #[serde(default = "default_next_id")]
    pub next_id: u64,
    /// Every node, in document order.
    #[serde(default)]
    pub nodes: Vec<SceneNode>,
}

fn default_format() -> String {
    SCENE_FORMAT.to_owned()
}
fn default_version() -> u32 {
    SCENE_FORMAT_VERSION
}
fn default_next_id() -> u64 {
    1
}

impl Default for Scene {
    fn default() -> Self {
        Self::new("untitled")
    }
}

impl Scene {
    /// An empty scene.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            format: default_format(),
            version: SCENE_FORMAT_VERSION,
            name: name.into(),
            next_id: 1,
            nodes: Vec::new(),
        }
    }

    /// The scene `File > New Scene` creates: a cube on the floor, a sun and a camera.
    ///
    /// The same three objects the plan asks for, built through the file format rather
    /// than in code, so `New Scene` and `Open Scene` exercise one path.
    pub fn default_scene() -> Self {
        let mut scene = Self::new("untitled");

        let mut cube = scene.add_node(SceneNode::new(SceneNodeId::new(0), "Cube"));
        cube.transform.position = [0.0, 0.5, 0.0];
        cube.mesh = Some(MeshSource::Primitive(Primitive::Cube { size: 1.0 }));
        cube.material.base_color = [0.75, 0.78, 0.85, 1.0];
        cube.material.specular = 0.35;
        scene.replace_node(cube);

        let mut sun = scene.add_node(SceneNode::new(SceneNodeId::new(0), "Sun"));
        sun.transform.position = [0.0, 4.0, 0.0];
        sun.light = Some(LightDesc::default());
        scene.replace_node(sun);

        let mut camera = scene.add_node(SceneNode::new(SceneNodeId::new(0), "Camera"));
        camera.transform.position = [4.0, 3.0, 6.0];
        camera.camera = Some(CameraDesc::default());
        scene.replace_node(camera);

        scene
    }

    /// Adds `node`, assigning it the next id if it has none, and returns the stored node.
    ///
    /// A caller that already knows the id can use [`Scene::insert_node`]; this is the
    /// path an editor's "create" command takes.
    pub fn add_node(&mut self, mut node: SceneNode) -> SceneNode {
        if node.id.raw() == 0 || self.node(node.id).is_some() {
            node.id = SceneNodeId::new(self.next_id);
        }
        self.next_id = self.next_id.max(node.id.raw() + 1);
        self.nodes.push(node.clone());
        node
    }

    /// Adds a fully-specified node, keeping its id.
    ///
    /// # Errors
    ///
    /// Fails if the id is zero or already present, or if the parent does not exist.
    pub fn insert_node(&mut self, node: SceneNode) -> Result<(), String> {
        if node.id.raw() == 0 {
            return Err("node id 0 is reserved".to_owned());
        }
        if self.node(node.id).is_some() {
            return Err(format!("node {} already exists", node.id));
        }
        if let Some(parent) = node.parent {
            if self.node(parent).is_none() {
                return Err(format!("parent {parent} does not exist"));
            }
        }
        self.next_id = self.next_id.max(node.id.raw() + 1);
        self.nodes.push(node);
        Ok(())
    }

    /// Replaces a node that is already in the scene, keeping its position in the list.
    ///
    /// Returns `false` if the node is not present.
    pub fn replace_node(&mut self, node: SceneNode) -> bool {
        match self
            .nodes
            .iter_mut()
            .find(|existing| existing.id == node.id)
        {
            Some(slot) => {
                *slot = node;
                true
            }
            None => false,
        }
    }

    /// The node with `id`.
    pub fn node(&self, id: SceneNodeId) -> Option<&SceneNode> {
        self.nodes.iter().find(|node| node.id == id)
    }

    /// The node with `id`, mutably.
    pub fn node_mut(&mut self, id: SceneNodeId) -> Option<&mut SceneNode> {
        self.nodes.iter_mut().find(|node| node.id == id)
    }

    /// Direct children of `parent`, in document order.
    pub fn children_of(&self, parent: Option<SceneNodeId>) -> impl Iterator<Item = &SceneNode> {
        self.nodes.iter().filter(move |node| node.parent == parent)
    }

    /// Every node that has no parent.
    pub fn roots(&self) -> impl Iterator<Item = &SceneNode> {
        self.children_of(None)
    }

    /// `node` and all its descendants, parents before children.
    pub fn subtree(&self, id: SceneNodeId) -> Vec<SceneNodeId> {
        let mut out = Vec::new();
        if self.node(id).is_none() {
            return out;
        }
        out.push(id);
        let mut index = 0;
        while index < out.len() {
            let current = out[index];
            for child in self.nodes.iter().filter(|n| n.parent == Some(current)) {
                if !out.contains(&child.id) {
                    out.push(child.id);
                }
            }
            index += 1;
        }
        out
    }

    /// Removes `id` and its descendants. Returns how many nodes went away.
    pub fn remove_node(&mut self, id: SceneNodeId) -> usize {
        let victims = self.subtree(id);
        let before = self.nodes.len();
        self.nodes.retain(|node| !victims.contains(&node.id));
        before - self.nodes.len()
    }

    /// The first light in the scene, if any.
    pub fn first_light(&self) -> Option<LightDesc> {
        self.nodes.iter().find_map(|node| node.light)
    }

    /// The first camera in the scene, if any.
    pub fn first_camera(&self) -> Option<CameraDesc> {
        self.nodes.iter().find_map(|node| node.camera)
    }

    /// The world-space AABB of every mesh node, or `None` for an empty scene.
    ///
    /// The editor's viewport frames a newly opened scene with this instead of guessing a
    /// camera distance.
    pub fn mesh_bounds(&self) -> Option<([f32; 3], [f32; 3])> {
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        let mut any = false;
        for node in self.nodes.iter().filter(|node| node.is_mesh()) {
            let (node_min, node_max) = node.transform.aabb();
            for axis in 0..3 {
                min[axis] = min[axis].min(node_min[axis]);
                max[axis] = max[axis].max(node_max[axis]);
            }
            any = true;
        }
        any.then_some((min, max))
    }

    /// Checks the invariants the rest of the editor relies on.
    ///
    /// Duplicate ids, a missing parent and a cycle are errors. Duplicate names are only a
    /// warning in a UI, so they are returned separately instead of failing the load.
    pub fn validate(&self) -> Result<Vec<String>, String> {
        let mut warnings = Vec::new();

        let mut seen = std::collections::HashSet::new();
        for node in &self.nodes {
            if node.id.raw() == 0 {
                return Err("node id 0 is reserved".to_owned());
            }
            if !seen.insert(node.id) {
                return Err(format!("duplicate node id {}", node.id));
            }
        }
        for node in &self.nodes {
            if let Some(parent) = node.parent {
                if self.node(parent).is_none() {
                    return Err(format!("{} has missing parent {parent}", node.id));
                }
                if parent == node.id {
                    return Err(format!("{} is its own parent", node.id));
                }
            }
        }
        // Every walk must terminate: follow the parent chain with a budget of one visit
        // per node.
        for node in &self.nodes {
            let mut current = node.parent;
            let mut budget = self.nodes.len();
            while let Some(parent) = current {
                if budget == 0 {
                    return Err(format!("cycle through {}", node.id));
                }
                budget -= 1;
                current = self.node(parent).and_then(|n| n.parent);
            }
        }

        let mut names = std::collections::HashSet::new();
        for node in &self.nodes {
            if !names.insert(node.name.as_str()) {
                warnings.push(format!("duplicate node name {:?}", node.name));
            }
        }
        if self.next_id <= self.nodes.iter().map(|n| n.id.raw()).max().unwrap_or(0) {
            warnings.push(format!(
                "next_id {} would reuse an existing id; a hand-written file should bump it",
                self.next_id
            ));
        }
        Ok(warnings)
    }

    /// Serialises the scene as pretty JSON without touching the disk.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Parses a scene, applying the version rules.
    ///
    /// # Errors
    ///
    /// [`SceneError::NotAScene`] when `format` is something else, and
    /// [`SceneError::UnsupportedVersion`] when the file is newer than
    /// [`SCENE_FORMAT_VERSION`].
    pub fn from_json(text: &str) -> Result<Self, SceneError> {
        Self::from_json_with_path(text, "<memory>")
    }

    fn from_json_with_path(text: &str, path: &str) -> Result<Self, SceneError> {
        let scene: Scene = serde_json::from_str(text).map_err(|source| SceneError::Json {
            path: path.to_owned(),
            source,
        })?;
        if scene.format != SCENE_FORMAT {
            return Err(SceneError::NotAScene {
                path: path.to_owned(),
                found: scene.format,
            });
        }
        if scene.version > SCENE_FORMAT_VERSION {
            return Err(SceneError::UnsupportedVersion {
                path: path.to_owned(),
                found: scene.version,
                supported: SCENE_FORMAT_VERSION,
            });
        }
        let scene = migrate(scene)?;
        scene.validate().map_err(|reason| SceneError::Invalid {
            path: path.to_owned(),
            reason,
        })?;
        Ok(scene)
    }

    /// Reads a scene file.
    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self, SceneError> {
        let path = path.as_ref();
        let display = path.display().to_string();
        let text = fs::read_to_string(path).map_err(|source| SceneError::Io {
            path: display.clone(),
            source,
        })?;
        Self::from_json_with_path(&text, &display)
    }

    /// Writes the scene, creating the parent directory if needed.
    ///
    /// Written to a temporary file and renamed into place, so a crash halfway through a
    /// save cannot leave a truncated scene where a good one used to be.
    pub fn save_to_path(&self, path: impl AsRef<Path>) -> Result<(), SceneError> {
        let path = path.as_ref();
        let display = path.display().to_string();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|source| SceneError::Io {
                    path: display.clone(),
                    source,
                })?;
            }
        }
        let text = self.to_json().map_err(|source| SceneError::Json {
            path: display.clone(),
            source,
        })?;
        let temporary = path.with_extension("scene.tmp");
        fs::write(&temporary, text).map_err(|source| SceneError::Io {
            path: display.clone(),
            source,
        })?;
        fs::rename(&temporary, path).map_err(|source| SceneError::Io {
            path: display,
            source,
        })
    }
}

/// Brings an older file up to [`SCENE_FORMAT_VERSION`].
///
/// Version 1 is the first format, so there is nothing to do yet; the function exists so
/// that adding version 2 is one match arm here instead of a rewrite of `load`.
fn migrate(scene: Scene) -> Result<Scene, SceneError> {
    // `from_json_with_path` refuses anything newer before reaching this point, so every
    // version that arrives here is readable as-is.
    Ok(scene)
}

#[cfg(test)]
mod tests;
