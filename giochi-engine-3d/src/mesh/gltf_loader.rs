//! glTF 2.0 and GLB loading.
//!
//! # Why the `gltf` crate without its `import` feature
//!
//! `gltf`'s `import` feature is what pulls in `image`, whose MSRV is 1.88 — above the 1.86
//! this crate promises. This loader needs geometry and material factors, not textures, so it
//! parses the file with `gltf::Gltf::from_slice` (JSON **and** the GLB container) and loads
//! the buffers itself: the GLB binary chunk, a `data:` URI, or a file next to the `.gltf`.
//! The crate's `utils` feature gives the accessor reader, which is the fiddly part nobody
//! should rewrite. ADR-073 records the trade.
//!
//! # What is supported, and what is refused by name
//!
//! Supported: meshes made of triangles, indexed or not, with positions, optional normals and
//! one UV set; the node hierarchy (world transforms are **baked** into the vertices);
//! per-primitive materials (base colour, emissive, metallic, roughness, alpha, double-sided).
//!
//! Refused with a message that names the primitive: lines, points, triangle strips and fans
//! (the renderer's pipeline is indexed triangles), and Draco/meshopt compression (a different
//! decoder, not a bug). A file that half-loads is worse than one that says what it needs.
//!
//! # Why the transforms are baked
//!
//! The renderer draws one mesh with one model matrix per call. A glTF scene is a tree of nodes
//! that each carry a transform, and a car modelled with four wheel nodes has its wheels in
//! *their* node's space. Baking the world matrix into the vertices is what makes a downloaded
//! model look like the preview it came with, and it costs one pass at load time. The
//! alternative — an instance per node — needs a scene graph in the renderer, which is the
//! game's job (phase 41), not the loader's.

use std::path::{Path, PathBuf};

use base64::Engine as _;
use glam::{Mat4, Vec3};

use super::{prepare_normals, Vertex};
use crate::error::{Error3D, Result3D};

/// A material as the engine understands it.
///
/// Deliberately the small set of factors a GLES 2.0 shader can use: the metallic-roughness
/// *textures* are not read (see the module docs), so only the scalar factors survive.
#[derive(Debug, Clone, PartialEq)]
pub struct GltfMaterial {
    /// Linear base colour, RGBA.
    pub base_color: [f32; 4],
    /// Linear emissive colour, RGB. Added after lighting.
    pub emissive: [f32; 3],
    /// Metallic factor, `0.0..=1.0`.
    pub metallic: f32,
    /// Roughness factor, `0.0..=1.0`. `0` is a mirror, `1` is fully diffuse.
    pub roughness: f32,
    /// Whether back faces should be drawn.
    pub double_sided: bool,
    /// `true` when the alpha mode is not `Opaque`.
    pub transparent: bool,
}

impl Default for GltfMaterial {
    fn default() -> Self {
        // The glTF defaults, from the spec's own table: white, fully metallic-rough at
        // 1/1 per the *default* material — but a primitive that names no material must look
        // like something, and the spec's default material is metallic 1 / roughness 1, which
        // on a Phong shader reads as black. White and non-metal is the useful default here.
        Self {
            base_color: [1.0, 1.0, 1.0, 1.0],
            emissive: [0.0; 3],
            metallic: 0.0,
            roughness: 0.5,
            double_sided: false,
            transparent: false,
        }
    }
}

impl GltfMaterial {
    /// The material of a `gltf` primitive.
    pub fn from_gltf(material: &gltf::Material<'_>) -> Self {
        let pbr = material.pbr_metallic_roughness();
        let emissive = material.emissive_factor();
        Self {
            base_color: pbr.base_color_factor(),
            emissive,
            metallic: pbr.metallic_factor(),
            roughness: pbr.roughness_factor(),
            double_sided: material.double_sided(),
            transparent: material.alpha_mode() != gltf::material::AlphaMode::Opaque,
        }
    }

    /// This material as the renderer takes it.
    ///
    /// The glTF factors map one to one: base colour, emissive, metallic, roughness. Double
    /// sidedness has no per-draw switch in the GLES 2.0 path yet, so it is carried on the
    /// material and ignored by the draw call rather than silently dropped here.
    pub fn to_material(&self) -> crate::render3d::Material {
        crate::render3d::Material {
            base_color: crate::math3d::Color3D::new(
                self.base_color[0],
                self.base_color[1],
                self.base_color[2],
                self.effective_alpha(),
            ),
            emissive: crate::math3d::Color3D::new(
                self.emissive[0],
                self.emissive[1],
                self.emissive[2],
                1.0,
            ),
            metallic: self.metallic.clamp(0.0, 1.0),
            roughness: self.roughness.clamp(0.0, 1.0),
            double_sided: self.double_sided,
            ..crate::render3d::Material::default()
        }
    }

    /// The same material with the base colour's alpha folded in from `alpha_mode: BLEND`.
    ///
    /// glTF's `BLEND` mode reads alpha from the base colour, which the caller already has;
    /// this exists so the check is written down once instead of at every call site.
    pub fn effective_alpha(&self) -> f32 {
        if self.transparent {
            self.base_color[3]
        } else {
            1.0
        }
    }
}

/// One drawable piece of a glTF file: a primitive, with its node transform already applied.
#[derive(Debug, Clone, PartialEq)]
pub struct GltfPrimitive {
    /// The mesh's name, or `"mesh N"` when the file has none (the crate's `names` feature is
    /// off, so node/mesh names are not parsed — see ADR-073).
    pub name: String,
    /// Vertices, in world space.
    pub vertices: Vec<Vertex>,
    /// Triangle indices.
    pub indices: Vec<u32>,
    /// The material, if the primitive named one.
    pub material: Option<GltfMaterial>,
}

impl GltfPrimitive {
    /// How many triangles this primitive draws.
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }
}

/// Everything a glTF file contains, flattened to primitives.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GltfModel {
    /// One entry per primitive, in scene order.
    pub primitives: Vec<GltfPrimitive>,
    /// World-space bounds, `(min, max)`, when the file has geometry.
    pub bounds: Option<([f32; 3], [f32; 3])>,
}

impl GltfModel {
    /// Total vertices across every primitive.
    pub fn vertex_count(&self) -> usize {
        self.primitives.iter().map(|p| p.vertices.len()).sum()
    }

    /// Total triangles across every primitive.
    pub fn triangle_count(&self) -> usize {
        self.primitives
            .iter()
            .map(GltfPrimitive::triangle_count)
            .sum()
    }

    /// Every primitive merged into one vertex/index pair, for a caller that draws one mesh.
    ///
    /// The material of the first primitive is returned with it: a merged mesh has one colour,
    /// which is exactly why the per-primitive form exists. This is for a model that turned out
    /// to be one material split over several primitives.
    pub fn merged(&self) -> (Vec<Vertex>, Vec<u32>, Option<GltfMaterial>) {
        let mut vertices = Vec::with_capacity(self.vertex_count());
        let mut indices = Vec::with_capacity(self.triangle_count() * 3);
        for primitive in &self.primitives {
            let offset = vertices.len() as u32;
            vertices.extend_from_slice(&primitive.vertices);
            indices.extend(primitive.indices.iter().map(|index| index + offset));
        }
        let material = self
            .primitives
            .first()
            .and_then(|primitive| primitive.material.clone());
        (vertices, indices, material)
    }
}

/// Loads `path`, which may be a `.gltf` (JSON) or a `.glb` (binary container).
///
/// Relative buffer URIs are resolved against the file's own directory, which is what the
/// specification requires and what makes a model folder portable.
///
/// # Errors
///
/// [`Error3D::Asset`] with the path and the parser's own message: an unreadable file, JSON
/// that is not glTF, a buffer that is missing or short, or a primitive whose mode this
/// pipeline cannot draw.
pub fn load_gltf(path: &Path) -> Result3D<GltfModel> {
    let bytes = std::fs::read(path).map_err(|error| {
        Error3D::Asset(format!("cannot read glTF '{}': {error}", path.display()))
    })?;
    let base = path.parent().unwrap_or_else(|| Path::new("."));
    load_gltf_from_slice(&bytes, base)
}

/// Loads glTF from memory. `base` resolves relative buffer URIs.
pub fn load_gltf_from_slice(bytes: &[u8], base: &Path) -> Result3D<GltfModel> {
    let gltf = gltf::Gltf::from_slice(bytes)
        .map_err(|error| Error3D::Asset(format!("not a glTF file: {error}")))?;
    let document = gltf.document;
    let buffers = load_buffers(&document, gltf.blob.as_deref(), base)?;

    let mut model = GltfModel::default();
    let roots: Vec<gltf::Node<'_>> = match document.default_scene() {
        // `default_scene` honours `scene`, and falls back to the first scene when the file
        // does not name one — which is what every viewer does.
        Some(scene) => scene.nodes().collect(),
        // A file with no scene at all: every node is a root, except the ones some other node
        // already lists as a child — walking those as well would draw them twice.
        None => {
            let children: Vec<usize> = document
                .nodes()
                .flat_map(|node| {
                    node.children()
                        .map(|child| child.index())
                        .collect::<Vec<_>>()
                })
                .collect();
            document
                .nodes()
                .filter(|node| !children.contains(&node.index()))
                .collect()
        }
    };
    for node in roots {
        visit_node(&node, Mat4::IDENTITY, &buffers, &mut model)?;
    }

    // A file with no triangles draws nothing, and a scene node pointing at it is a bug the
    // caller can only report if it hears about it here. Lights, cameras and skins are legal
    // glTF and are not geometry; this pipeline has nothing to do with them.
    if model.primitives.is_empty() {
        return Err(Error3D::Asset(
            "the glTF file contains no triangle geometry".to_owned(),
        ));
    }

    model.bounds = bounds_of(&model.primitives);
    Ok(model)
}

/// The buffer data, in `buffer.index()` order.
fn load_buffers(
    document: &gltf::Document,
    blob: Option<&[u8]>,
    base: &Path,
) -> Result3D<Vec<Vec<u8>>> {
    let mut out = Vec::new();
    for buffer in document.buffers() {
        let data = match buffer.source() {
            gltf::buffer::Source::Bin => blob
                .map(<[u8]>::to_vec)
                .ok_or_else(|| Error3D::Asset("GLB has no binary chunk".to_owned()))?,
            gltf::buffer::Source::Uri(uri) => read_uri(uri, base)?,
        };
        if data.len() < buffer.length() {
            return Err(Error3D::Asset(format!(
                "buffer {} is {} bytes but the file declares {}",
                buffer.index(),
                data.len(),
                buffer.length()
            )));
        }
        out.push(data);
    }
    Ok(out)
}

/// Reads a buffer URI: a `data:` URI or a path relative to `base`.
fn read_uri(uri: &str, base: &Path) -> Result3D<Vec<u8>> {
    if let Some(rest) = uri.strip_prefix("data:") {
        let Some((meta, payload)) = rest.split_once(',') else {
            return Err(Error3D::Asset(format!(
                "malformed data URI (no comma): {}",
                shorten(uri)
            )));
        };
        if meta.ends_with(";base64") {
            return base64::engine::general_purpose::STANDARD
                .decode(payload.trim())
                .map_err(|error| Error3D::Asset(format!("data URI is not valid base64: {error}")));
        }
        // Percent-encoded text payload: rare, but the spec allows it, and treating it as raw
        // bytes is correct for the ASCII-safe case.
        return Ok(percent_decode(payload));
    }
    let path = resolve_path(base, uri);
    let mut data = std::fs::read(&path).map_err(|error| {
        Error3D::Asset(format!("cannot read buffer '{}': {error}", path.display()))
    })?;
    Ok(std::mem::take(&mut data))
}

/// `base/uri`, with the URI's own `%XX` escapes decoded.
fn resolve_path(base: &Path, uri: &str) -> PathBuf {
    let decoded = String::from_utf8_lossy(&percent_decode(uri)).into_owned();
    base.join(decoded)
}

/// Decodes `%XX` escapes; anything else is copied through.
fn percent_decode(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok();
            if let Some(byte) = hex.and_then(|hex| u8::from_str_radix(hex, 16).ok()) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    out
}

/// Walks one node and its children, baking `parent * node` into the vertices.
fn visit_node(
    node: &gltf::Node<'_>,
    parent: Mat4,
    buffers: &[Vec<u8>],
    model: &mut GltfModel,
) -> Result3D<()> {
    let local = Mat4::from_cols_array_2d(&node.transform().matrix());
    let world = parent * local;

    if let Some(mesh) = node.mesh() {
        for (index, primitive) in mesh.primitives().enumerate() {
            model
                .primitives
                .push(read_primitive(index, &primitive, world, buffers)?);
        }
    }

    for child in node.children() {
        visit_node(&child, world, buffers, model)?;
    }
    Ok(())
}

/// Reads one primitive and transforms it into world space.
fn read_primitive(
    index: usize,
    primitive: &gltf::Primitive<'_>,
    world: Mat4,
    buffers: &[Vec<u8>],
) -> Result3D<GltfPrimitive> {
    let name = format!("primitive {index}");
    match primitive.mode() {
        gltf::mesh::Mode::Triangles => {}
        other => {
            return Err(Error3D::Asset(format!(
                "glTF {name} uses {other:?}, and this pipeline draws triangles only"
            )));
        }
    }

    // Indexed decode of the four attributes this vertex layout has room for.
    let reader = primitive.reader(|buffer| buffers.get(buffer.index()).map(Vec::as_slice));
    let Some(positions) = reader.read_positions() else {
        return Err(Error3D::Asset(format!(
            "glTF {name} has no POSITION attribute"
        )));
    };
    let positions: Vec<[f32; 3]> = positions.collect();
    if positions.is_empty() {
        return Err(Error3D::Asset(format!(
            "glTF {name} has an empty POSITION attribute"
        )));
    }
    let normals: Vec<[f32; 3]> = reader
        .read_normals()
        .map(|normals| normals.collect())
        .unwrap_or_default();
    let uvs: Vec<[f32; 2]> = reader
        .read_tex_coords(0)
        .map(|uvs| uvs.into_f32().collect())
        .unwrap_or_default();

    let indices: Vec<u32> = match reader.read_indices() {
        Some(indices) => indices.into_u32().collect(),
        None => (0..positions.len() as u32).collect(),
    };
    if indices.len() % 3 != 0 {
        return Err(Error3D::Asset(format!(
            "glTF {name} has {} indices, which is not a whole number of triangles",
            indices.len()
        )));
    }

    // The normal matrix is the inverse transpose of the upper 3×3: correct the normals for
    // non-uniform scale, which is where baking a transform visibly goes wrong otherwise.
    let normal_matrix = Mat4::from_mat3(glam::Mat3::from_mat4(world).inverse().transpose());
    let mut vertices = Vec::with_capacity(positions.len());
    for (vertex, position) in positions.iter().enumerate() {
        let position = world.transform_point3(Vec3::from_array(*position));
        let normal = match normals.get(vertex) {
            Some(normal) => {
                let normal = normal_matrix.transform_vector3(Vec3::from_array(*normal));
                if normal.length_squared() > 1e-12 {
                    normal.normalize().to_array()
                } else {
                    [0.0, 0.0, 0.0]
                }
            }
            // A zero normal is the signal `prepare_normals` looks for to compute a face
            // normal; leaving it zero is how missing normals are handled for OBJ too.
            None => [0.0, 0.0, 0.0],
        };
        let uv = uvs.get(vertex).copied().unwrap_or([0.0, 0.0]);
        vertices.push(Vertex::new(position.to_array(), normal, uv));
    }
    if normals.is_empty() {
        prepare_normals(&mut vertices, &indices);
    } else {
        // A normal of zero slipped through (a degenerate triangle in the file): the same
        // repair pass fills those without touching the good ones.
        prepare_normals(&mut vertices, &indices);
    }

    let material = primitive.material();
    // A primitive with no `material` gets `Material::default()`, which glTF defines as
    // metallic 1 / roughness 1; `GltfMaterial::default` is the friendlier white.
    let material = if material.index().is_some() {
        Some(GltfMaterial::from_gltf(&material))
    } else {
        None
    };

    Ok(GltfPrimitive {
        name,
        vertices,
        indices,
        material,
    })
}

/// Computes bounds over every primitive.
fn bounds_of(primitives: &[GltfPrimitive]) -> Option<([f32; 3], [f32; 3])> {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for primitive in primitives {
        for vertex in &primitive.vertices {
            for axis in 0..3 {
                min[axis] = min[axis].min(vertex.position[axis]);
                max[axis] = max[axis].max(vertex.position[axis]);
            }
        }
    }
    min[0].is_finite().then_some((min, max))
}

/// A URI shortened for an error message: a data URI can be megabytes.
fn shorten(uri: &str) -> String {
    if uri.chars().count() <= 60 {
        return uri.to_owned();
    }
    let head: String = uri.chars().take(60).collect();
    format!("{head}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal triangle: one primitive, positions only, no indices.
    fn triangle_gltf() -> String {
        r#"{
            "asset": {"version": "2.0"},
            "scene": 0,
            "scenes": [{"nodes": [0]}],
            "nodes": [{"mesh": 0}],
            "meshes": [{"primitives": [{"attributes": {"POSITION": 0}}]}],
            "accessors": [{
                "bufferView": 0,
                "componentType": 5126,
                "count": 3,
                "type": "VEC3",
                "min": [0.0, 0.0, 0.0],
                "max": [1.0, 1.0, 0.0]
            }],
            "bufferViews": [{
                "buffer": 0,
                "byteOffset": 0,
                "byteLength": 36
            }],
            "buffers": [{
                "byteLength": 36,
                "uri": "BUFFER_URI"
            }]
        }"#
        .to_owned()
    }

    fn triangle_bytes() -> Vec<u8> {
        let mut bytes = Vec::new();
        for value in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    fn data_uri(bytes: &[u8]) -> String {
        format!(
            "data:application/octet-stream;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        )
    }

    #[test]
    fn a_minimal_triangle_loads_with_computed_normals() {
        let json = triangle_gltf().replace("BUFFER_URI", &data_uri(&triangle_bytes()));
        let model = load_gltf_from_slice(json.as_bytes(), Path::new(".")).expect("loaded");
        assert_eq!(model.primitives.len(), 1);
        let primitive = &model.primitives[0];
        assert_eq!(primitive.vertices.len(), 3);
        assert_eq!(
            primitive.indices,
            vec![0, 1, 2],
            "no indices means sequential"
        );
        assert_eq!(primitive.triangle_count(), 1);
        // The file has no NORMAL, so the face normal is computed: +Z for a CCW triangle in
        // the XY plane.
        for vertex in &primitive.vertices {
            assert!((vertex.normal[2] - 1.0).abs() < 1e-5, "{:?}", vertex.normal);
        }
        assert_eq!(model.bounds, Some(([0.0, 0.0, 0.0], [1.0, 1.0, 0.0])));
        assert!(primitive.material.is_none());
    }

    #[test]
    fn a_node_transform_is_baked_into_the_vertices() {
        // The node is translated and scaled; the accessor still says 0..1, so any vertex
        // outside that box proves the matrix was applied.
        let json = triangle_gltf()
            .replace("BUFFER_URI", &data_uri(&triangle_bytes()))
            .replace(
                r#"{"mesh": 0}"#,
                r#"{"mesh": 0, "translation": [10, 0, 0], "scale": [2, 2, 2]}"#,
            );
        let model = load_gltf_from_slice(json.as_bytes(), Path::new(".")).expect("loaded");
        let vertices = &model.primitives[0].vertices;
        assert!(
            (vertices[1].position[0] - 12.0).abs() < 1e-5,
            "{vertices:?}"
        );
        let bounds = model.bounds.expect("bounds");
        assert!((bounds.0[0] - 10.0).abs() < 1e-5);
        assert!((bounds.1[0] - 12.0).abs() < 1e-5);
    }

    #[test]
    fn a_child_node_inherits_its_parents_transform() {
        let json = triangle_gltf()
            .replace("BUFFER_URI", &data_uri(&triangle_bytes()))
            .replace(
                r#""nodes": [{"mesh": 0}],"#,
                r#""nodes": [{"translation": [5, 0, 0], "children": [1]}, {"mesh": 0}],"#,
            );
        let model = load_gltf_from_slice(json.as_bytes(), Path::new(".")).expect("loaded");
        let vertices = &model.primitives[0].vertices;
        assert!(vertices[0].position[0] >= 4.99, "{vertices:?}");
    }

    #[test]
    fn indexed_geometry_uses_the_index_list() {
        let bytes = triangle_bytes();
        let json = r#"{
            "asset": {"version": "2.0"},
            "scene": 0,
            "scenes": [{"nodes": [0]}],
            "nodes": [{"mesh": 0}],
            "meshes": [{"primitives": [{
                "attributes": {"POSITION": 0},
                "indices": 1
            }]}],
            "accessors": [
                {"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
                 "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0]},
                {"bufferView": 1, "componentType": 5123, "count": 3, "type": "SCALAR"}
            ],
            "bufferViews": [
                {"buffer": 0, "byteOffset": 0, "byteLength": 36},
                {"buffer": 0, "byteOffset": 36, "byteLength": 6}
            ],
            "buffers": [{"byteLength": 42, "uri": "URI"}]
        }"#
        .replace(
            "URI",
            &data_uri(&{
                let mut data = bytes;
                data.extend_from_slice(&[2u8, 0, 1, 0, 0, 0]);
                data
            }),
        );
        let model = load_gltf_from_slice(json.as_bytes(), Path::new(".")).expect("loaded");
        assert_eq!(model.primitives[0].indices, vec![2, 1, 0]);
    }

    #[test]
    fn a_material_carries_colour_emissive_and_roughness() {
        let json = triangle_gltf()
            .replace("BUFFER_URI", &data_uri(&triangle_bytes()))
            .replace(
                r#""primitives": [{"attributes": {"POSITION": 0}}]"#,
                r#""primitives": [{"attributes": {"POSITION": 0}, "material": 0}]"#,
            )
            .replace(
                r#""meshes": ["#,
                r#""materials": [{
                    "pbrMetallicRoughness": {
                        "baseColorFactor": [0.8, 0.2, 0.1, 1.0],
                        "metallicFactor": 0.1,
                        "roughnessFactor": 0.25
                    },
                    "emissiveFactor": [0.5, 0.0, 0.0],
                    "doubleSided": true
                }],
                "meshes": ["#,
            );
        let model = load_gltf_from_slice(json.as_bytes(), Path::new(".")).expect("loaded");
        let material = model.primitives[0].material.as_ref().expect("material");
        assert_eq!(material.base_color, [0.8, 0.2, 0.1, 1.0]);
        assert_eq!(material.emissive, [0.5, 0.0, 0.0]);
        assert!((material.metallic - 0.1).abs() < 1e-6);
        assert!((material.roughness - 0.25).abs() < 1e-6);
        assert!(material.double_sided);
        assert!(!material.transparent);
    }

    #[test]
    fn the_default_material_is_white_not_black() {
        let material = GltfMaterial::default();
        assert_eq!(material.base_color, [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(material.emissive, [0.0; 3]);
        assert_eq!(material.effective_alpha(), 1.0);
    }

    #[test]
    fn an_unsupported_mode_is_refused_by_name() {
        let json = triangle_gltf()
            .replace("BUFFER_URI", &data_uri(&triangle_bytes()))
            .replace(
                r#""attributes": {"POSITION": 0}"#,
                r#""attributes": {"POSITION": 0}, "mode": 1"#,
            );
        let error = load_gltf_from_slice(json.as_bytes(), Path::new(".")).expect_err("refused");
        let text = format!("{error}");
        assert!(text.contains("Lines") || text.contains("mode"), "{text}");
    }

    #[test]
    fn a_missing_buffer_says_which_path() {
        let json = triangle_gltf().replace("BUFFER_URI", "missing.bin");
        let error = load_gltf_from_slice(json.as_bytes(), Path::new("/tmp/nothing-here"))
            .expect_err("refused");
        let text = format!("{error}");
        assert!(text.contains("missing.bin"), "{text}");
    }

    #[test]
    fn a_short_buffer_is_refused_instead_of_read_out_of_bounds() {
        let json = triangle_gltf().replace("BUFFER_URI", &data_uri(&[0u8; 12]));
        let error = load_gltf_from_slice(json.as_bytes(), Path::new(".")).expect_err("refused");
        let text = format!("{error}");
        assert!(text.contains("12 bytes"), "{text}");
    }

    #[test]
    fn not_json_is_an_error_and_not_a_panic() {
        let error = load_gltf_from_slice(b"this is not gltf", Path::new(".")).expect_err("refused");
        assert!(format!("{error}").contains("not a glTF"));
    }

    #[test]
    fn merging_offsets_the_indices() {
        let json = triangle_gltf()
            .replace("BUFFER_URI", &data_uri(&triangle_bytes()))
            .replace(
                r#""primitives": [{"attributes": {"POSITION": 0}}]"#,
                r#""primitives": [
                    {"attributes": {"POSITION": 0}},
                    {"attributes": {"POSITION": 0}}
                ]"#,
            );
        let model = load_gltf_from_slice(json.as_bytes(), Path::new(".")).expect("loaded");
        assert_eq!(model.primitives.len(), 2);
        let (vertices, indices, _) = model.merged();
        assert_eq!(vertices.len(), 6);
        assert_eq!(indices, vec![0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn percent_escapes_decode() {
        assert_eq!(percent_decode("a%20b"), b"a b");
        assert_eq!(percent_decode("100%"), b"100%");
        assert_eq!(percent_decode("%zz"), b"%zz");
    }

    #[test]
    fn a_glb_container_is_read() {
        // Build a GLB by hand: a 12-byte header, a JSON chunk and a BIN chunk, both padded
        // to four bytes (spaces for JSON, zeros for BIN). The JSON describes a buffer with
        // **no** `uri`, which is how a GLB says "the binary chunk".
        let json = r#"{
            "asset": {"version": "2.0"},
            "scene": 0,
            "scenes": [{"nodes": [0]}],
            "nodes": [{"mesh": 0}],
            "meshes": [{"primitives": [{"attributes": {"POSITION": 0}}]}],
            "accessors": [{
                "bufferView": 0,
                "componentType": 5126,
                "count": 3,
                "type": "VEC3",
                "min": [0.0, 0.0, 0.0],
                "max": [1.0, 1.0, 0.0]
            }],
            "bufferViews": [{"buffer": 0, "byteOffset": 0, "byteLength": 36}],
            "buffers": [{"byteLength": 36}]
        }"#;
        let mut json = json.as_bytes().to_vec();
        while json.len() % 4 != 0 {
            json.push(b' ');
        }
        let mut bin = triangle_bytes();
        while bin.len() % 4 != 0 {
            bin.push(0);
        }

        let total = 12 + 8 + json.len() + 8 + bin.len();
        let mut glb = Vec::new();
        glb.extend_from_slice(b"glTF");
        glb.extend_from_slice(&2u32.to_le_bytes());
        glb.extend_from_slice(&(total as u32).to_le_bytes());
        glb.extend_from_slice(&(json.len() as u32).to_le_bytes());
        glb.extend_from_slice(b"JSON");
        glb.extend_from_slice(&json);
        glb.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        glb.extend_from_slice(b"BIN\0");
        glb.extend_from_slice(&bin);

        let model = load_gltf_from_slice(&glb, Path::new(".")).expect("GLB loads");
        assert_eq!(model.primitives.len(), 1);
        assert_eq!(model.primitives[0].vertices.len(), 3);
        assert_eq!(model.primitives[0].indices, vec![0, 1, 2]);
    }
}
