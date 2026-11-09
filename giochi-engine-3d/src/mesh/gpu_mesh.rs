//! Indexed triangle mesh on the GPU.
//!
//! # VAOs are not core in GLES 2.0
//!
//! This is the trap the 3D extension warns about. `glGenVertexArrays` is core in GLES
//! 3.0; in 2.0 the same functionality lives in `GL_OES_vertex_array_object`, and a
//! driver may expose neither. So [`Mesh`] holds an optional VAO and, when there is
//! none, re-specifies the attribute pointers before every draw. The caller never sees
//! the difference.
//!
//! Because the attribute slots are fixed by `Program` (`ATTRIB_POSITION` and friends,
//! bound before linking), one VAO per mesh is valid for every program — which is what
//! makes the optional-VAO design work at all.

use glow::HasContext;

use super::Vertex;
use crate::context::InstancingBackend;
use crate::error::{Error3D, Result3D};
use crate::shader::{Program, ATTRIB_INSTANCE_MODEL, ATTRIB_NORMAL, ATTRIB_POSITION, ATTRIB_UV};

/// Bytes per instance matrix: four `vec4` columns, which is one `Mat4`.
pub const INSTANCE_STRIDE: i32 = 16 * core::mem::size_of::<f32>() as i32;

/// Bytes per vertex, used as the attribute stride.
const STRIDE: i32 = core::mem::size_of::<Vertex>() as i32;

/// Forces the no-VAO path when set to `1`.
///
/// The fallback is the code that runs on a GLES 2.0 device without
/// `GL_OES_vertex_array_object`, and no development machine has that configuration. An
/// untested fallback is a fallback that does not work, so this switch lets the test
/// suite exercise both paths everywhere.
pub const FORCE_NO_VAO_ENV: &str = "GIOCHI_FORCE_NO_VAO";

fn vao_allowed() -> bool {
    !matches!(std::env::var(FORCE_NO_VAO_ENV).as_deref(), Ok("1"))
}

/// A mesh: one VBO, one index buffer, and a VAO when the driver has them.
#[derive(Debug)]
pub struct Mesh {
    vao: Option<glow::VertexArray>,
    vbo: glow::Buffer,
    ebo: glow::Buffer,
    index_count: i32,
    vertex_count: i32,
}

impl Mesh {
    /// Uploads `vertices` and `indices`.
    ///
    /// # Errors
    ///
    /// [`Error3D::InvalidArgument`] for an empty mesh or an index out of range —
    /// a bad index is a GPU-side out-of-bounds read, which on a console shows up as a
    /// flickering surface with no error at all.
    pub fn from_vertices_indices(
        gl: &glow::Context,
        vertices: &[Vertex],
        indices: &[u32],
    ) -> Result3D<Self> {
        if vertices.is_empty() {
            return Err(Error3D::InvalidArgument(
                "a mesh needs at least one vertex".to_owned(),
            ));
        }
        if indices.is_empty() {
            return Err(Error3D::InvalidArgument(
                "a mesh needs at least one index".to_owned(),
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

        // SAFETY: every call below is GLES 2.0 (buffers always, VAO optionally). The
        // buffers are deleted on the error paths.
        unsafe {
            let vbo = gl
                .create_buffer()
                .map_err(|err| Error3D::GlObject(format!("glGenBuffers (VBO): {err}")))?;
            let ebo = match gl.create_buffer() {
                Ok(ebo) => ebo,
                Err(err) => {
                    gl.delete_buffer(vbo);
                    return Err(Error3D::GlObject(format!("glGenBuffers (EBO): {err}")));
                }
            };

            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
            gl.buffer_data_u8_slice(
                glow::ARRAY_BUFFER,
                bytemuck::cast_slice(vertices),
                glow::STATIC_DRAW,
            );
            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(ebo));
            gl.buffer_data_u8_slice(
                glow::ELEMENT_ARRAY_BUFFER,
                bytemuck::cast_slice(indices),
                glow::STATIC_DRAW,
            );

            let mut mesh = Self {
                vao: None,
                vbo,
                ebo,
                index_count: indices.len() as i32,
                vertex_count: vertices.len() as i32,
            };

            // Optional: GLES 3.0 core, or the OES extension. A driver may have neither.
            let created = if vao_allowed() {
                Some(gl.create_vertex_array())
            } else {
                None
            };

            match created {
                Some(Ok(vao)) => {
                    gl.bind_vertex_array(Some(vao));
                    gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
                    gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(ebo));
                    mesh.specify_attributes(gl);
                    gl.bind_vertex_array(None);
                    mesh.vao = Some(vao);
                }
                Some(Err(_)) | None => {
                    log::debug!(
                        "no vertex array objects here (GLES 2.0 without \
                         GL_OES_vertex_array_object, or {FORCE_NO_VAO_ENV}=1); \
                         specifying attributes per draw"
                    );
                }
            }

            // Leave the global state clean: a stray bound EBO is the classic reason a
            // later mesh draws with the previous mesh's indices.
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, None);

            Ok(mesh)
        }
    }

    /// Points the fixed attribute slots at this mesh's VBO.
    ///
    /// # Safety
    ///
    /// The caller must have the VBO bound to `GL_ARRAY_BUFFER`.
    unsafe fn specify_attributes(&self, gl: &glow::Context) {
        // position: 3 floats at offset 0
        gl.enable_vertex_attrib_array(ATTRIB_POSITION);
        gl.vertex_attrib_pointer_f32(ATTRIB_POSITION, 3, glow::FLOAT, false, STRIDE, 0);

        // normal: 3 floats at offset 12
        gl.enable_vertex_attrib_array(ATTRIB_NORMAL);
        gl.vertex_attrib_pointer_f32(ATTRIB_NORMAL, 3, glow::FLOAT, false, STRIDE, 12);

        // uv: 2 floats at offset 24
        gl.enable_vertex_attrib_array(ATTRIB_UV);
        gl.vertex_attrib_pointer_f32(ATTRIB_UV, 2, glow::FLOAT, false, STRIDE, 24);
    }

    /// Draws the mesh as triangles.
    ///
    /// The program is unused in this signature on purpose: attribute slots are fixed,
    /// so the mesh does not need to know which shader will shade it. It is taken as a
    /// parameter to make that contract explicit at the call site rather than an
    /// assumption in a comment.
    pub fn draw(&self, gl: &glow::Context, _program: &Program) {
        // SAFETY: core GLES 2.0 draw path. Whether the attributes come from a VAO or
        // are re-specified, the buffers are the ones this mesh owns.
        unsafe {
            match self.vao {
                Some(vao) => gl.bind_vertex_array(Some(vao)),
                None => {
                    gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo));
                    gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(self.ebo));
                    self.specify_attributes(gl);
                }
            }

            gl.draw_elements(glow::TRIANGLES, self.index_count, glow::UNSIGNED_INT, 0);

            if self.vao.is_some() {
                gl.bind_vertex_array(None);
            }
        }
    }

    /// Draws `instance_count` copies of this mesh in one call.
    ///
    /// `instance_vbo` holds `instance_count` column-major `mat4` values, uploaded by
    /// the caller; attributes 3..6 read one four-float column each, advancing once per
    /// instance.
    ///
    /// # The mesh VAO is deliberately not used here
    ///
    /// `glVertexAttribDivisor` is *vertex array state*. Enabling it on a mesh's VAO
    /// would leave the divisor set for that mesh's next non-instanced draw, where
    /// attributes 3..6 would then be read from a buffer that is not bound — undefined
    /// values, or an immediate GL error. So the instanced path binds no VAO and
    /// re-specifies every attribute for the duration of the call. Seven
    /// `vertexAttribPointer` calls per instanced draw is nothing next to the N draws
    /// it replaces.
    ///
    /// # Safety
    ///
    /// The caller must have `program` bound, the GL context current, and
    /// `instance_vbo` filled with at least `instance_count` matrices.
    pub(crate) unsafe fn draw_instanced(
        &self,
        gl: &glow::Context,
        backend: InstancingBackend,
        instance_vbo: glow::Buffer,
        instance_count: i32,
    ) {
        gl.bind_vertex_array(None);
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo));
        self.specify_attributes(gl);
        // The divisor default is the only sensible one for mesh attributes, but a
        // previous instanced draw may have left 1 on these slots — `bind_vertex_array(None)`
        // does not reset it, because with no VAO bound the state is per-attribute global.
        for slot in [ATTRIB_POSITION, ATTRIB_NORMAL, ATTRIB_UV] {
            backend.vertex_attrib_divisor(gl, slot, 0);
        }

        gl.bind_buffer(glow::ARRAY_BUFFER, Some(instance_vbo));
        for column in 0..4u32 {
            let slot = ATTRIB_INSTANCE_MODEL + column;
            gl.enable_vertex_attrib_array(slot);
            gl.vertex_attrib_pointer_f32(
                slot,
                4,
                glow::FLOAT,
                false,
                INSTANCE_STRIDE,
                (column as i32) * 4 * core::mem::size_of::<f32>() as i32,
            );
            backend.vertex_attrib_divisor(gl, slot, 1);
        }

        gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(self.ebo));
        backend.draw_elements_instanced(
            gl,
            glow::TRIANGLES,
            self.index_count,
            glow::UNSIGNED_INT,
            0,
            instance_count,
        );

        // Leave nothing enabled that a following non-instanced draw would inherit.
        for slot in [3, 4, 5, 6] {
            backend.vertex_attrib_divisor(gl, slot, 0);
            gl.disable_vertex_attrib_array(slot);
        }
        gl.bind_buffer(glow::ARRAY_BUFFER, None);
        gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, None);
    }

    /// Number of indices, i.e. three per triangle.
    pub fn index_count(&self) -> u32 {
        self.index_count as u32
    }

    /// Number of unique vertices.
    pub fn vertex_count(&self) -> u32 {
        self.vertex_count as u32
    }

    /// Whether this mesh uses a vertex array object.
    pub fn uses_vao(&self) -> bool {
        self.vao.is_some()
    }

    /// The raw GL names this mesh owns, for tests that must prove they were deleted.
    ///
    /// Exists only for the regression test: the registry leak it catches was invisible
    /// precisely because nothing could ask the driver "is this buffer still alive after
    /// `destroy`".
    #[cfg(test)]
    pub(crate) fn gl_names(&self) -> (Option<glow::VertexArray>, glow::Buffer, glow::Buffer) {
        (self.vao, self.vbo, self.ebo)
    }

    /// Releases every GL object.
    ///
    /// Explicit for the same reason as [`Program::destroy`]: a `Drop` impl cannot see
    /// the context. The registry calls this on `destroy_mesh`.
    pub fn destroy(self, gl: &glow::Context) {
        // SAFETY: the objects belong to `gl`, and `self` is consumed so none is deleted
        // twice.
        unsafe {
            if let Some(vao) = self.vao {
                gl.delete_vertex_array(vao);
            }
            gl.delete_buffer(self.vbo);
            gl.delete_buffer(self.ebo);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stride_matches_the_vertex_size() {
        // A wrong stride shifts every attribute by four bytes per component and
        // produces geometry that looks like noise.
        assert_eq!(STRIDE as usize, core::mem::size_of::<Vertex>());
        assert_eq!(STRIDE, 32);
    }

    #[test]
    fn the_attribute_offsets_are_the_documented_ones() {
        // These numbers are duplicated in `specify_attributes`; pinning them here is
        // what makes the duplicates safe.
        assert_eq!(core::mem::offset_of!(Vertex, position), 0);
        assert_eq!(core::mem::offset_of!(Vertex, normal), 12);
        assert_eq!(core::mem::offset_of!(Vertex, uv), 24);
    }

    #[test]
    fn the_attribute_slots_are_distinct() {
        let slots = [ATTRIB_POSITION, ATTRIB_NORMAL, ATTRIB_UV];
        let mut sorted = slots.to_vec();
        sorted.sort_unstable();
        sorted.dedup();

        assert_eq!(sorted.len(), slots.len(), "two attributes share a slot");
    }
}
