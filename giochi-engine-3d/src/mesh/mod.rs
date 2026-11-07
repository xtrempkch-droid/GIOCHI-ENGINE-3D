//! Geometry: vertices, GPU meshes, OBJ loading and procedural primitives.
//!
//! One interleaved buffer per mesh (`position / normal / uv`), indexed with `u32`. The
//! attribute slots are fixed by [`crate::shader::Program`] before linking, which is what
//! lets a single optional vertex array object serve every program.

mod gpu_mesh;
mod obj_loader;
mod primitives;
mod vertex;

pub use gpu_mesh::{Mesh, FORCE_NO_VAO_ENV};
pub use obj_loader::load_obj;
pub use primitives::{cube, plane};
pub use vertex::Vertex;
