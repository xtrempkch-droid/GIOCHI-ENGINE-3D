//! Instanced drawing: N copies of one mesh in one call.
//!
//! [`InstancedRenderer`] owns the per-instance buffer and the backend choice;
//! [`RenderContext3D::draw_instanced`](crate::RenderContext3D::draw_instanced) drives
//! it. The GL details live here rather than in the renderer so the buffer's lifetime
//! has one owner — a leaked instance VBO is invisible until a long session runs out of
//! memory.
//!
//! # What is actually instanced
//!
//! Only the model matrix. Geometry, normals, the view-projection, the light and the
//! colour are uniform across the batch. That covers the case the phase exists for — a
//! grid of identical crates, particles from one mesh, a wall of the same panel — and
//! nothing beyond it: a scene of 100 *different* meshes is a static-batch problem
//! ([`crate::mesh::StaticBatch`]), not an instancing one.
//!
//! # Fallback
//!
//! When the driver has no instanced-drawing extension, [`InstancedRenderer::draw`]
//! issues one draw per instance through the ordinary path. The API and the picture are
//! identical; only the draw-call count is not, which is what [`InstancedRenderer::last_draw_calls`]
//! reports so a caller cannot mistake the fallback for a win. `docs/PERF_LOG.md`
//! records both arms.

use glow::HasContext;

use crate::context::{InstancingBackend, InstancingSupport};
use crate::error::Result3D;
use crate::math3d::Mat4;

/// Default number of instances the buffer is sized for up front.
///
/// 256 × 64 bytes = 16 KiB, which is below anything that matters on the targets and
/// above the typical batch. The buffer grows by doubling when a scene draws more.
pub const DEFAULT_INSTANCE_CAPACITY: usize = 256;

/// The per-instance buffer and the provider that draws from it.
///
/// # C ABI
///
/// Not exposed: `giochi_render_draw_mesh` keeps its meaning and instancing is a Rust
/// optimisation. There is no C entry point in phase 17 (the ABI extensions are phase
/// 18c), so the layout is free to change.
pub struct InstancedRenderer {
    vbo: glow::Buffer,
    /// Column-major matrices, densely packed, uploaded on every draw.
    scratch: Vec<f32>,
    capacity: usize,
    backend: InstancingBackend,
    last_draw_calls: u32,
    last_instance_count: u32,
}

impl std::fmt::Debug for InstancedRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InstancedRenderer")
            .field("support", &self.backend.support())
            .field("capacity", &self.capacity)
            .field("last_draw_calls", &self.last_draw_calls)
            .finish_non_exhaustive()
    }
}

impl InstancedRenderer {
    /// Creates the instance buffer.
    ///
    /// Fails only when the GL implementation will not allocate a buffer — the same
    /// failure `create_mesh` reports, and equally fatal for the caller.
    pub(crate) fn new(gl: &glow::Context, backend: InstancingBackend) -> Result3D<Self> {
        // SAFETY: a buffer allocation is core GLES 2.0.
        let vbo = unsafe {
            gl.create_buffer().map_err(|err| {
                crate::error::Error3D::GlObject(format!("glGenBuffers (instances): {err}"))
            })
        }?;

        Ok(Self {
            vbo,
            scratch: Vec::with_capacity(DEFAULT_INSTANCE_CAPACITY * 16),
            capacity: DEFAULT_INSTANCE_CAPACITY,
            backend,
            last_draw_calls: 0,
            last_instance_count: 0,
        })
    }

    /// Which provider was detected at context creation.
    pub fn support(&self) -> InstancingSupport {
        self.backend.support()
    }

    /// The backend, for the mesh's attribute setup.
    pub(crate) fn backend(&self) -> InstancingBackend {
        self.backend
    }

    /// Number of backend draw calls the last `draw` issued.
    ///
    /// `1` when instancing is available, `instances` when it fell back, `0` for an
    /// empty batch.
    pub fn last_draw_calls(&self) -> u32 {
        self.last_draw_calls
    }

    /// Number of instances in the last draw.
    pub fn last_instance_count(&self) -> u32 {
        self.last_instance_count
    }

    /// Uploads `instances` and draws `index_count` indices per instance.
    ///
    /// Returns the number of backend draw calls issued.
    ///
    /// # Safety
    ///
    /// `program` must be bound with the view-projection and light uploaded, and the
    /// GL context current.
    pub(crate) unsafe fn draw(
        &mut self,
        gl: &glow::Context,
        mesh: &crate::mesh::Mesh,
        instances: &[Mat4],
    ) -> u32 {
        self.last_instance_count = instances.len() as u32;
        self.last_draw_calls = 0;
        if instances.is_empty() {
            return 0;
        }

        if !self.backend.support().is_available() {
            // The fallback cannot live here: it needs the program, the camera and the
            // colour, which belong to the renderer. The caller does it.
            return 0;
        }

        self.pack(instances);
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo));
        gl.buffer_data_u8_slice(
            glow::ARRAY_BUFFER,
            bytemuck::cast_slice(&self.scratch),
            glow::STREAM_DRAW,
        );

        mesh.draw_instanced(gl, self.backend, self.vbo, instances.len() as i32);

        self.last_draw_calls = 1;
        1
    }

    fn pack(&mut self, instances: &[Mat4]) {
        pack_matrices(instances, &mut self.scratch);
        if instances.len() > self.capacity {
            self.capacity = instances.len().next_power_of_two();
        }
    }

    /// Whether the instance buffer has grown to hold `count` matrices.
    pub fn holds(&self, count: usize) -> bool {
        self.capacity >= count
    }

    /// Deletes the GL buffer. Called from `RenderContext3D::release_all`.
    ///
    /// # Safety
    ///
    /// The context must be current and this renderer must not be used again.
    pub(crate) unsafe fn destroy(self, gl: &glow::Context) {
        gl.delete_buffer(self.vbo);
    }
}

/// Packs `instances` into `scratch` as dense column-major `f32`.
///
/// `Mat4` carries a reserved tail (`data: [f32; 16], _reserved: [u32; 4]`), so its
/// in-memory layout is 80 bytes and not the 64 the driver wants. The matrices are
/// therefore repacked instead of cast: a stride mismatch would read every instance
/// from a wrong offset, which draws garbage rather than failing.
fn pack_matrices(instances: &[Mat4], scratch: &mut Vec<f32>) {
    scratch.clear();
    let floats = instances.len() * 16;
    if scratch.capacity() < floats {
        // `reserve` is relative to `len`, which `clear` just set to zero. Reserving
        // `floats - capacity` therefore asked for *less* than the buffer needed and let
        // `extend_from_slice` grow it again — while the comment above promised this was
        // the growth step.
        scratch.reserve(floats);
    }
    for instance in instances {
        scratch.extend_from_slice(&instance.data);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_capacity_covers_a_typical_batch() {
        assert_eq!(DEFAULT_INSTANCE_CAPACITY, 256);
        // 16 KiB of instance data, well under any buffer limit and cheap to keep
        // around for the life of the context.
        assert_eq!(DEFAULT_INSTANCE_CAPACITY * 16 * 4, 16 * 1024);
    }

    #[test]
    fn packing_drops_the_reserved_tail_of_each_matrix() {
        // The whole reason `pack_matrices` exists: `Mat4` is larger than the 16 floats
        // the driver reads, so casting the slice would be silently wrong.
        assert!(std::mem::size_of::<Mat4>() > 16 * std::mem::size_of::<f32>());

        let mut scratch = Vec::new();
        pack_matrices(&[Mat4::IDENTITY, Mat4::IDENTITY], &mut scratch);

        assert_eq!(scratch.len(), 32, "two matrices are 32 floats, not 40");
        assert_eq!(scratch[0], 1.0);
        assert_eq!(scratch[15], 1.0);
        assert_eq!(scratch[16], 1.0);
        assert_eq!(scratch[31], 1.0);
    }

    #[test]
    fn packing_keeps_the_column_major_order() {
        // A translation lives in the fourth column (`data[12..15]`). If packing wrote
        // rows, the shader would read the translation from a row and every instance
        // would be misplaced — the classic transpose bug, and invisible on the identity.
        let mut matrix = Mat4::IDENTITY;
        matrix.data[12] = 7.0;
        matrix.data[13] = -3.0;
        matrix.data[14] = 2.5;

        let mut scratch = Vec::new();
        pack_matrices(&[matrix], &mut scratch);

        assert_eq!(&scratch[12..15], &[7.0, -3.0, 2.5]);
    }

    #[test]
    fn packing_an_empty_batch_empties_the_scratch() {
        let mut scratch = vec![1.0; 64];
        pack_matrices(&[], &mut scratch);
        assert!(scratch.is_empty());
    }

    #[test]
    fn a_reused_scratch_does_not_accumulate() {
        let mut scratch = Vec::new();
        pack_matrices(&[Mat4::IDENTITY], &mut scratch);
        pack_matrices(&[Mat4::IDENTITY], &mut scratch);
        assert_eq!(scratch.len(), 16, "the second pack replaces the first");
    }
}
