//! Vertex layout for the 3D pipeline.
//!
//! Interleaved `position / normal / uv` in one buffer, which is what a GLES 2.0 driver
//! handles best and what `tobj` produces naturally. The struct is `#[repr(C)]` and
//! `Pod` so it can be uploaded straight from a slice with no conversion pass.

/// One vertex: position, normal and texture coordinate.
///
/// 32 bytes, tightly packed, `#[repr(C)]`. The `Pod` derive is safe here because every
/// field is `f32` and there is no padding (8 floats = 32 bytes, 4-byte aligned).
///
/// # C ABI
///
/// Not part of the C ABI: callers pass meshes by handle, never vertex data
/// (`docs/C_ABI_CONTRACT.md` §11).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    /// World-space (model-space) position.
    pub position: [f32; 3],
    /// Surface normal, assumed unit length after [`Vertex::normalized`].
    pub normal: [f32; 3],
    /// Texture coordinate, `0.0..=1.0`.
    pub uv: [f32; 2],
}

impl Vertex {
    /// Number of `f32` components per vertex.
    pub const COMPONENTS: usize = 8;

    /// Builds a vertex.
    pub const fn new(position: [f32; 3], normal: [f32; 3], uv: [f32; 2]) -> Self {
        Self {
            position,
            normal,
            uv,
        }
    }

    /// Builds a vertex with no texture coordinate, for the flat shader.
    pub const fn flat(position: [f32; 3], normal: [f32; 3]) -> Self {
        Self::new(position, normal, [0.0, 0.0])
    }

    /// Returns the same vertex with its normal normalised.
    ///
    /// OBJ files are not required to store unit normals, and a non-unit normal makes
    /// lighting wrong in a way that looks like a colour bug. A zero normal is left
    /// alone rather than turned into NaN.
    pub fn normalized(self) -> Self {
        let [x, y, z] = self.normal;
        let length = (x * x + y * y + z * z).sqrt();

        if length == 0.0 || !length.is_finite() {
            return self;
        }

        Self {
            normal: [x / length, y / length, z / length],
            ..self
        }
    }
}

/// Whether any vertex needs [`prepare_normals`] before upload.
///
/// Cheap enough to run on the upload path: it only inspects the normals, so a mesh built
/// by the primitives or by the OBJ loader (both already normalised) pays one pass over
/// eight floats per vertex and no allocation.
pub fn needs_normal_repair(vertices: &[Vertex]) -> bool {
    vertices.iter().any(|vertex| {
        let [x, y, z] = vertex.normal;
        if !x.is_finite() || !y.is_finite() || !z.is_finite() {
            return true;
        }
        let length_squared = x * x + y * y + z * z;
        // Zero and near-zero are both unusable: `normalize` of a zero vector is
        // undefined in GLSL.
        if length_squared < 1e-12 {
            return true;
        }
        (length_squared - 1.0).abs() > 1e-4
    })
}

/// Makes every normal usable for the shaders: unit length, finite, and never zero.
///
/// A zero normal is the dangerous case. `normalize(vec3(0.0))` is **undefined** by the
/// GLSL ES 1.00 specification, so a mesh uploaded with zero normals shades differently on
/// every driver: the AMD driver measured here returns a zero vector (ambient-only grey)
/// while others produce `NaN`, which makes the whole surface black with no GL error.
/// Deriving the face normal is what the OBJ loader already does for files without `vn`,
/// and doing it here means every entry point agrees.
///
/// Vertices with a valid normal are only renormalised; nothing else is touched.
pub fn prepare_normals(vertices: &mut [Vertex], indices: &[u32]) {
    if vertices.is_empty() {
        return;
    }

    // Accumulate the (unnormalised) face normal of every triangle a vertex belongs to.
    // Only needed for the vertices that have no usable normal of their own, but computing
    // it for all of them is one pass and keeps the code simple.
    let mut accumulated = vec![[0.0_f32; 3]; vertices.len()];
    for triangle in indices.chunks_exact(3) {
        let [a, b, c] = [
            triangle[0] as usize,
            triangle[1] as usize,
            triangle[2] as usize,
        ];
        if a >= vertices.len() || b >= vertices.len() || c >= vertices.len() {
            // An out-of-range index is rejected later by `Mesh::from_vertices_indices`;
            // skipping it here keeps this function total.
            continue;
        }

        let p0 = vertices[a].position;
        let p1 = vertices[b].position;
        let p2 = vertices[c].position;
        let u = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
        let v = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];
        let normal = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];

        for corner in [a, b, c] {
            for axis in 0..3 {
                accumulated[corner][axis] += normal[axis];
            }
        }
    }

    for (index, vertex) in vertices.iter_mut().enumerate() {
        let [x, y, z] = vertex.normal;
        let length_squared = x * x + y * y + z * z;
        let usable = x.is_finite() && y.is_finite() && z.is_finite() && length_squared >= 1e-12;

        if usable {
            *vertex = vertex.normalized();
            continue;
        }

        let fallback = accumulated[index];
        let fallback_squared =
            fallback[0] * fallback[0] + fallback[1] * fallback[1] + fallback[2] * fallback[2];
        if fallback_squared >= 1e-12 {
            let length = fallback_squared.sqrt();
            vertex.normal = [
                fallback[0] / length,
                fallback[1] / length,
                fallback[2] / length,
            ];
        } else {
            // A degenerate triangle has no normal at all. Point it at the camera-ish
            // default rather than leaving a zero that turns into NaN in the shader.
            vertex.normal = [0.0, 0.0, 1.0];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::size_of;

    #[test]
    fn layout_is_32_bytes_with_no_padding() {
        // A padding byte here would silently shift every attribute pointer.
        assert_eq!(size_of::<Vertex>(), 32);
        assert_eq!(
            size_of::<Vertex>(),
            Vertex::COMPONENTS * size_of::<f32>(),
            "the struct must be exactly 8 floats, or the stride lies about the data"
        );
    }

    #[test]
    fn a_vertex_is_pod_so_it_uploads_without_a_conversion_pass() {
        // `bytemuck::cast_slice` is what the mesh upload uses; this asserts the bound
        // holds rather than trusting the derive.
        let vertices = [Vertex::flat([0.0; 3], [0.0, 1.0, 0.0]); 3];
        let bytes: &[u8] = bytemuck::cast_slice(&vertices);

        assert_eq!(bytes.len(), 3 * 32);
    }

    #[test]
    fn normalizing_produces_a_unit_normal() {
        let vertex = Vertex::new([0.0; 3], [0.0, 3.0, 4.0], [0.0, 0.0]).normalized();

        assert!((vertex.normal[1] - 0.6).abs() < 1e-6);
        assert!((vertex.normal[2] - 0.8).abs() < 1e-6);
    }

    #[test]
    fn a_zero_normal_is_left_alone_instead_of_becoming_nan() {
        let vertex = Vertex::new([1.0, 2.0, 3.0], [0.0; 3], [0.5, 0.5]).normalized();

        assert_eq!(vertex.normal, [0.0; 3]);
        assert!(vertex.normal.iter().all(|value| value.is_finite()));
        assert_eq!(vertex.position, [1.0, 2.0, 3.0], "other fields untouched");
    }

    #[test]
    fn zero_normals_are_derived_from_the_triangles() {
        // The GLSL `normalize(vec3(0))` result is undefined, so a zero normal must never
        // reach the shader.
        let mut vertices = [
            Vertex::new([0.0, 0.0, 0.0], [0.0; 3], [0.0, 0.0]),
            Vertex::new([1.0, 0.0, 0.0], [0.0; 3], [1.0, 0.0]),
            Vertex::new([0.0, 1.0, 0.0], [0.0; 3], [0.0, 1.0]),
        ];
        prepare_normals(&mut vertices, &[0, 1, 2]);

        for vertex in &vertices {
            assert!(
                vertex.normal.iter().all(|value| value.is_finite()),
                "derived a non-finite normal: {vertex:?}"
            );
            assert!(vertex.normal[2].abs() > 0.99, "expected +Z, got {vertex:?}");
        }
    }

    #[test]
    fn a_non_unit_normal_is_renormalised_and_a_valid_one_is_untouched() {
        let mut vertices = [
            Vertex::new([0.0, 0.0, 0.0], [0.0, 3.0, 4.0], [0.0, 0.0]),
            Vertex::new([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0]),
        ];
        prepare_normals(&mut vertices, &[0, 1, 1]);

        assert!((vertices[0].normal[1] - 0.6).abs() < 1e-6);
        assert!((vertices[0].normal[2] - 0.8).abs() < 1e-6);
        assert_eq!(vertices[1].normal, [0.0, 1.0, 0.0]);
    }

    #[test]
    fn a_nan_normal_is_replaced_instead_of_reaching_the_shader() {
        let mut vertices = [
            Vertex::new([0.0, 0.0, 0.0], [f32::NAN; 3], [0.0, 0.0]),
            Vertex::new([1.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0]),
            Vertex::new([0.0, 1.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0]),
        ];
        prepare_normals(&mut vertices, &[0, 1, 2]);

        assert!(vertices
            .iter()
            .all(|v| v.normal.iter().all(|n| n.is_finite())));
    }

    #[test]
    fn a_degenerate_triangle_still_gets_a_finite_normal() {
        // All three points are the same: there is no face normal to derive.
        let mut vertices = [
            Vertex::new([0.0; 3], [0.0; 3], [0.0, 0.0]),
            Vertex::new([0.0; 3], [0.0; 3], [0.0, 0.0]),
            Vertex::new([0.0; 3], [0.0; 3], [0.0, 0.0]),
        ];
        prepare_normals(&mut vertices, &[0, 1, 2]);

        for vertex in &vertices {
            let [x, y, z] = vertex.normal;
            let length = (x * x + y * y + z * z).sqrt();
            assert!((length - 1.0).abs() < 1e-5, "not unit: {vertex:?}");
        }
    }

    #[test]
    fn needs_normal_repair_detects_the_three_bad_cases() {
        let unit = Vertex::new([0.0; 3], [0.0, 1.0, 0.0], [0.0, 0.0]);
        assert!(!needs_normal_repair(&[unit]));

        let zero = Vertex::new([0.0; 3], [0.0; 3], [0.0, 0.0]);
        assert!(needs_normal_repair(&[zero]));

        let nan = Vertex::new([0.0; 3], [f32::NAN, 0.0, 0.0], [0.0, 0.0]);
        assert!(needs_normal_repair(&[nan]));

        let scaled = Vertex::new([0.0; 3], [0.0, 2.0, 0.0], [0.0, 0.0]);
        assert!(needs_normal_repair(&[scaled]));
    }

    #[test]
    fn the_flat_constructor_zeroes_the_uv() {
        let vertex = Vertex::flat([1.0, 2.0, 3.0], [0.0, 1.0, 0.0]);

        assert_eq!(vertex.uv, [0.0, 0.0]);
    }
}
