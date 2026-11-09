//! Combining static meshes into one buffer.
//!
//! A scene is mostly things that never move: the floor, the walls, the crates, the
//! scenery. Each of those is a draw call, and 50 of them cost 50 times the CPU work of
//! one for a picture that could have been uploaded once at load time.
//!
//! [`StaticBatch`] takes several `(mesh, transform)` pairs that share a shader,
//! transforms their vertices on the CPU and concatenates them into one vertex and index
//! buffer. The result is drawn with the **ordinary** `draw_mesh`, so nothing downstream
//! needs to know a batch exists.
//!
//! # What it cannot do
//!
//! - **Move.** The transform is baked in. An object that moves is not static, and
//!   putting it in a batch means it stays where it was when the batch was built — the
//!   picture is wrong, not slow. Callers choose what is static; there is no runtime
//!   guard, because "is this object ever going to move" is not a question the GPU can
//!   answer.
//! - **Change shader.** One batch, one program: the material has to agree. A scene with
//!   two materials is two batches, which is still a large win.
//! - **Be rebuilt cheaply.** Building allocates and uploads; the intended lifecycle is
//!   load-time, once.
//!
//! # Why the transform is baked rather than instanced
//!
//! Instancing ([`crate::RenderContext3D::draw_instanced`]) draws *identical* geometry N
//! times; a batch draws *different* geometry once. A wall of the same panel is an
//! instancing problem, a room with a door and two windows is a batch problem. Both
//! reduce draw calls and neither replaces the other, which is why phase 17 has both.

use super::{Mesh, Vertex};
use crate::context::GlesVersion;
use crate::error::{Error3D, Result3D};
use crate::math3d::{Mat4, Vec3};

/// Accumulates transformed geometry, then uploads it as one mesh.
///
/// Dropping a builder without calling [`StaticBatchBuilder::build`] costs nothing but
/// the CPU buffers, which the `Vec`s release.
#[derive(Debug, Default)]
pub struct StaticBatchBuilder {
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    meshes: usize,
    source_vertices: usize,
}

impl StaticBatchBuilder {
    /// An empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// An empty builder sized for `vertices` vertices and `indices` indices.
    ///
    /// Worth using when the caller knows the scene size: growing a `Vec` of vertices
    /// reallocates and copies megabytes on a console.
    pub fn with_capacity(vertices: usize, indices: usize) -> Self {
        Self {
            vertices: Vec::with_capacity(vertices),
            indices: Vec::with_capacity(indices),
            meshes: 0,
            source_vertices: 0,
        }
    }

    /// Appends `vertices`/`indices` transformed by `model`.
    ///
    /// The normals get `model.normal_matrix()`, so a rotated or non-uniformly scaled
    /// piece is lit like the unbatched one instead of as though it had a different
    /// shape.
    ///
    /// Returns the builder, so calls chain.
    ///
    /// # Errors
    ///
    /// [`Error3D::InvalidArgument`] for an empty mesh, an index out of range, or when
    /// the combined index count would pass `u32::MAX` — a GPU-side out-of-bounds read
    /// otherwise, which on a console is a flickering surface and no error at all.
    pub fn add_mesh(
        mut self,
        vertices: &[Vertex],
        indices: &[u32],
        model: &Mat4,
    ) -> Result3D<Self> {
        if vertices.is_empty() {
            return Err(Error3D::InvalidArgument(
                "a batch piece needs at least one vertex".to_owned(),
            ));
        }
        if indices.is_empty() {
            return Err(Error3D::InvalidArgument(
                "a batch piece needs at least one index".to_owned(),
            ));
        }
        if let Some(bad) = indices
            .iter()
            .find(|index| **index as usize >= vertices.len())
        {
            return Err(Error3D::InvalidArgument(format!(
                "index {bad} is out of range for {} vertices",
                vertices.len()
            )));
        }

        let base = self.vertices.len() as u32;
        let needed = indices.len() as u64 + self.indices.len() as u64;
        if needed > u64::from(u32::MAX) {
            return Err(Error3D::InvalidArgument(format!(
                "a static batch can hold at most {} indices; this one would need {needed}",
                u32::MAX
            )));
        }

        let normal_matrix = model.normal_matrix();
        self.vertices.extend(
            vertices
                .iter()
                .map(|vertex| transform_vertex(vertex, model, &normal_matrix)),
        );
        self.indices
            .extend(indices.iter().map(|index| index + base));
        self.meshes += 1;
        self.source_vertices += vertices.len();

        Ok(self)
    }

    /// Number of pieces added so far.
    pub fn len(&self) -> usize {
        self.meshes
    }

    /// Whether nothing has been added.
    pub fn is_empty(&self) -> bool {
        self.meshes == 0
    }

    /// Vertices accumulated so far.
    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    /// Indices accumulated so far.
    pub fn index_count(&self) -> usize {
        self.indices.len()
    }

    /// The accumulated geometry, for tests and for callers that upload it themselves.
    pub fn geometry(&self) -> (&[Vertex], &[u32]) {
        (&self.vertices, &self.indices)
    }

    /// Uploads everything added so far as one mesh.
    ///
    /// # Errors
    ///
    /// [`Error3D::InvalidArgument`] when nothing was added — an empty batch is a caller
    /// mistake, and a zero-vertex GL buffer is the kind of thing that draws nothing
    /// forever without explaining why.
    pub fn build(self, gl: &glow::Context) -> Result3D<Mesh> {
        if self.vertices.is_empty() {
            return Err(Error3D::InvalidArgument(
                "a static batch needs at least one mesh".to_owned(),
            ));
        }
        Mesh::from_vertices_indices(gl, &self.vertices, &self.indices)
    }
}

/// One vertex carried through `model`, with its normal through `normal_matrix`.
fn transform_vertex(vertex: &Vertex, model: &Mat4, normal_matrix: &Mat4) -> Vertex {
    let position = model.transform_point(Vec3::new(
        vertex.position[0],
        vertex.position[1],
        vertex.position[2],
    ));
    let normal = transform_direction(normal_matrix, vertex.normal);

    Vertex {
        position: position.to_array(),
        // Renormalised here rather than in the shader: `normal_matrix` carries an
        // inverse-transpose, which changes length for a non-uniform scale, and a normal
        // that is not unit length breaks every `dot` in the lighting term.
        normal,
        uv: vertex.uv,
    }
}

/// Transforms a direction (no translation, no `w` divide) and normalises it.
fn transform_direction(matrix: &Mat4, direction: [f32; 3]) -> [f32; 3] {
    let data = &matrix.data;
    let x = data[0] * direction[0] + data[4] * direction[1] + data[8] * direction[2];
    let y = data[1] * direction[0] + data[5] * direction[1] + data[9] * direction[2];
    let z = data[2] * direction[0] + data[6] * direction[1] + data[10] * direction[2];
    let length = (x * x + y * y + z * z).sqrt();
    if length > 0.0 && length.is_finite() {
        [x / length, y / length, z / length]
    } else {
        // A degenerate normal is better replaced than propagated: NaN normals make the
        // whole surface black, and a zero-length one makes it flicker per pixel.
        [0.0, 1.0, 0.0]
    }
}

/// A batch that has been uploaded, plus the bookkeeping worth reporting.
///
/// The mesh itself is what gets drawn: [`StaticBatch::mesh`] returns a handle that
/// `draw_mesh` and `draw_mesh_colored` accept, because a batch is not a new kind of
/// draw — it is one mesh that happens to have been assembled from many.
///
/// # C ABI
///
/// Not exposed. `giochi_render_draw_mesh` draws a batch handle like any other.
#[derive(Debug)]
pub struct StaticBatch {
    mesh: crate::render3d::MeshHandle,
    pieces: usize,
    vertices: u32,
    indices: u32,
}

impl StaticBatch {
    /// Uploads `builder`'s geometry into `render` and returns the batch.
    ///
    /// Takes the renderer rather than a raw GL context so the mesh lands in the same
    /// registry as every other mesh: it is destroyed by `destroy_mesh`, counted by
    /// `mesh_count`, and released by `release_all`, with no second lifetime to track.
    pub fn build(
        render: &mut crate::render3d::RenderContext3D,
        builder: StaticBatchBuilder,
    ) -> Result3D<Self> {
        let pieces = builder.len();
        let (vertices, indices) = builder.geometry();
        let (vertex_count, index_count) = (vertices.len() as u32, indices.len() as u32);

        let mesh = render.create_mesh(vertices, indices)?;

        log::debug!(
            "static batch: {pieces} mesh(es), {vertex_count} vertices, \
             {index_count} indices, {vertex_count} × {} bytes uploaded once",
            std::mem::size_of::<Vertex>()
        );

        Ok(Self {
            mesh,
            pieces,
            vertices: vertex_count,
            indices: index_count,
        })
    }

    /// The handle to draw with.
    pub fn mesh(&self) -> crate::render3d::MeshHandle {
        self.mesh
    }

    /// How many meshes went into it, i.e. how many draw calls it replaced.
    pub fn pieces(&self) -> usize {
        self.pieces
    }

    /// Vertices in the combined buffer.
    pub fn vertex_count(&self) -> u32 {
        self.vertices
    }

    /// Indices in the combined buffer.
    pub fn index_count(&self) -> u32 {
        self.indices
    }
}

/// Whether a GLES version can hold a batch of `indices` indices.
///
/// `glDrawElements` with `GL_UNSIGNED_INT` indices needs
/// `GL_OES_element_index_uint` in GLES 2.0, which every driver measured here has, but
/// the engine already relies on that for every mesh — this exists so a driver check has
/// one place to live if a target ever lacks it.
pub fn supports_u32_indices(version: GlesVersion) -> bool {
    version.major() >= 2
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math3d::{Quat, Transform3D, Vec3};
    use crate::mesh::cube;

    fn cube_data() -> (Vec<Vertex>, Vec<u32>) {
        cube(1.0)
    }

    #[test]
    fn an_empty_builder_reports_nothing() {
        let builder = StaticBatchBuilder::new();
        assert!(builder.is_empty());
        assert_eq!(builder.len(), 0);
        assert_eq!(builder.vertex_count(), 0);
        assert_eq!(builder.index_count(), 0);
    }

    #[test]
    fn adding_a_mesh_counts_its_geometry() {
        let (vertices, indices) = cube_data();
        let builder = StaticBatchBuilder::new()
            .add_mesh(&vertices, &indices, &Mat4::IDENTITY)
            .expect("add");

        assert_eq!(builder.len(), 1);
        assert_eq!(builder.vertex_count(), vertices.len());
        assert_eq!(builder.index_count(), indices.len());
    }

    #[test]
    fn two_meshes_concatenate_their_index_buffers() {
        let (vertices, indices) = cube_data();
        let builder = StaticBatchBuilder::new()
            .add_mesh(&vertices, &indices, &Mat4::IDENTITY)
            .expect("first")
            .add_mesh(&vertices, &indices, &Mat4::IDENTITY)
            .expect("second");

        assert_eq!(builder.len(), 2);

        let (combined, combined_indices) = builder.geometry();
        assert_eq!(combined.len(), vertices.len() * 2);

        // The second cube's indices are offset by the first cube's vertex count. Without
        // the offset both cubes draw the first one's vertices: the batch renders one
        // cube where two should be, with no GL error.
        let offset = vertices.len() as u32;
        assert_eq!(combined_indices[0], indices[0]);
        assert_eq!(
            combined_indices[indices.len()],
            indices[0] + offset,
            "the second mesh's first index must point past the first mesh"
        );
        assert!(
            combined_indices
                .iter()
                .all(|index| (*index as usize) < combined.len()),
            "every index must be in range for the combined buffer"
        );
    }

    #[test]
    fn the_transform_is_baked_into_the_vertices() {
        let (vertices, indices) = cube_data();
        let model = Transform3D::from_translation(Vec3::new(10.0, 0.0, 0.0)).matrix();
        let builder = StaticBatchBuilder::new()
            .add_mesh(&vertices, &indices, &model)
            .expect("add");

        let (combined, _) = builder.geometry();
        // Every vertex moved by ten on X, and nothing else changed.
        for (before, after) in vertices.iter().zip(combined) {
            assert!((after.position[0] - (before.position[0] + 10.0)).abs() < 1e-5);
            assert!((after.position[1] - before.position[1]).abs() < 1e-5);
            assert_eq!(after.uv, before.uv);
        }
    }

    #[test]
    fn normals_are_transformed_and_stay_unit_length() {
        let (vertices, indices) = cube_data();
        // A quarter turn about Y, then a squash on X: the case `mat3(model)` gets wrong.
        let rotate = Transform3D::from_translation(Vec3::ZERO)
            .with_rotation(Quat::from_axis_angle(Vec3::UP, 1.2))
            .matrix();
        let builder = StaticBatchBuilder::new()
            .add_mesh(&vertices, &indices, &rotate)
            .expect("add");

        let (combined, _) = builder.geometry();
        for vertex in combined {
            let length =
                (vertex.normal[0].powi(2) + vertex.normal[1].powi(2) + vertex.normal[2].powi(2))
                    .sqrt();
            assert!(
                (length - 1.0).abs() < 1e-4,
                "normal {vertex:?} is not unit length"
            );
        }
    }

    #[test]
    fn a_rotated_normal_is_actually_rotated() {
        let (vertices, indices) = cube_data();
        // Half a turn about Y maps +Z to -Z.
        let turn = Transform3D::from_translation(Vec3::ZERO)
            .with_rotation(Quat::from_axis_angle(Vec3::UP, std::f32::consts::PI))
            .matrix();
        let builder = StaticBatchBuilder::new()
            .add_mesh(&vertices, &indices, &turn)
            .expect("add");

        let (combined, _) = builder.geometry();
        // Find a vertex whose original normal points along +Z; its batched normal must
        // point along -Z.
        let rotated = vertices
            .iter()
            .zip(combined)
            .find(|(before, _)| before.normal[2] > 0.9)
            .map(|(_, after)| after);
        let rotated = rotated.expect("a cube has a +Z face");
        assert!(
            rotated.normal[2] < -0.9,
            "normal {rotated:?} was not rotated"
        );
    }

    #[test]
    fn an_empty_piece_is_rejected() {
        let error = StaticBatchBuilder::new()
            .add_mesh(&[], &[], &Mat4::IDENTITY)
            .expect_err("an empty piece is a caller mistake");
        assert!(matches!(error, Error3D::InvalidArgument(_)));
    }

    #[test]
    fn an_out_of_range_index_is_rejected() {
        let (vertices, _) = cube_data();
        let error = StaticBatchBuilder::new()
            .add_mesh(&vertices, &[0, 1, 9999], &Mat4::IDENTITY)
            .expect_err("a bad index is a GPU-side out-of-bounds read");
        assert!(matches!(error, Error3D::InvalidArgument(_)));
    }

    #[test]
    fn one_mesh_without_indices_is_rejected() {
        let (vertices, _) = cube_data();
        let error = StaticBatchBuilder::new()
            .add_mesh(&vertices, &[], &Mat4::IDENTITY)
            .expect_err("indices are required");
        assert!(matches!(error, Error3D::InvalidArgument(_)));
    }

    #[test]
    fn a_degenerate_normal_is_replaced_rather_than_propagated() {
        // A scale of zero on every axis has no inverse, and `normal_matrix` falls back to
        // the identity; the direction handed in here is zero, which is the other way to
        // produce a NaN normal.
        let normal = transform_direction(&Mat4::IDENTITY, [0.0, 0.0, 0.0]);
        assert_eq!(normal, [0.0, 1.0, 0.0]);
    }

    #[test]
    fn the_u32_index_path_is_available_on_every_supported_version() {
        assert!(supports_u32_indices(GlesVersion::Gles2));
        assert!(supports_u32_indices(GlesVersion::Gles3));
    }
}
