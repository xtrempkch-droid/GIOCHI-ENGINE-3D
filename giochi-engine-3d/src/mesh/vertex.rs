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
    fn the_flat_constructor_zeroes_the_uv() {
        let vertex = Vertex::flat([1.0, 2.0, 3.0], [0.0, 1.0, 0.0]);

        assert_eq!(vertex.uv, [0.0, 0.0]);
    }
}
