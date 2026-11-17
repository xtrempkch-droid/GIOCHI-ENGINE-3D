//! Geometry: vertices, GPU meshes, OBJ and glTF loading and procedural primitives.
//!
//! One interleaved buffer per mesh (`position / normal / uv`), indexed with `u32`. The
//! attribute slots are fixed by [`crate::shader::Program`] before linking, which is what
//! lets a single optional vertex array object serve every program.

mod gltf_loader;
mod gpu_mesh;
mod obj_loader;
mod primitives;
mod static_batch;
mod vertex;

pub use gltf_loader::{load_gltf, load_gltf_from_slice, GltfMaterial, GltfModel, GltfPrimitive};
pub use gpu_mesh::{Mesh, FORCE_NO_VAO_ENV};
pub use obj_loader::load_obj;
pub use primitives::{cube, plane, sphere};
pub use static_batch::{supports_u32_indices, StaticBatch, StaticBatchBuilder};
pub use vertex::{needs_normal_repair, prepare_normals, Vertex};

/// A glTF file that has been uploaded: its primitives as GPU handles, plus the model itself.
///
/// The model is kept because the caller almost always wants something from it — the bounds
/// for framing a camera, the triangle count for a log line, the material factors for a scene
/// node — and re-parsing the file to get them would be a second read of the same bytes.
#[derive(Debug, Clone)]
pub struct GltfLoaded {
    /// The parsed model, with node transforms baked in.
    pub model: GltfModel,
    /// One entry per primitive, in the same order as `model.primitives`.
    pub primitives: Vec<(u64, Option<GltfMaterial>)>,
}

impl GltfLoaded {
    /// How many GPU meshes were uploaded.
    pub fn mesh_count(&self) -> usize {
        self.primitives.len()
    }

    /// The size of the model, `(min, max)`, for framing a camera.
    pub fn bounds(&self) -> Option<([f32; 3], [f32; 3])> {
        self.model.bounds
    }
}
