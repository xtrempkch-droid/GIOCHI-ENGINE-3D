//! Rendering at a lower resolution and scaling up on presentation.
//!
//! A 3D frame on a small panel is usually **fill-bound**: the cost is the number of
//! pixels shaded, not the number of triangles submitted. Rendering into a smaller
//! framebuffer and stretching the result over the panel multiplies the frame rate by
//! roughly the square of the scale — 0.75² = 56 % of the pixels, 0.5² = 25 % — at the
//! cost of a softer image.
//!
//! # What this is not
//!
//! Not FSR, and the name matters because the phase-17 plan asked for "FSR-like". FSR 1.0
//! is a compute-shader pair (EASU + RCAS); GLES 2.0 has no compute, and this engine
//! targets GLES 2.0 by default (ADR-038). FSR 2.x needs motion vectors and temporal
//! accumulation. What is implemented here is a **bilinear** stretch, which is the
//! cheapest thing that works everywhere and whose cost is paid by the texture unit
//! rather than by a shader.
//!
//! See `docs/3D.md` for the trade-offs and `docs/PERF_LOG.md` for the measured gain.
//!
//! # How the frame flows
//!
//! ```text
//! begin_frame:  bind FBO, viewport = low-res, clear
//! draw calls:   unchanged — they render into the small buffer
//! end_frame:    bind FBO 0, viewport = window, draw one fullscreen quad, swap
//! ```
//!
//! Nothing between those steps knows the resolution changed: the camera already takes
//! its aspect from [`RenderContext3D::drawable_size`](crate::RenderContext3D::drawable_size),
//! which reports the **low-res**
//! size while a scale is set. That is the same "the real surface is not what you asked
//! for" rule KMSDRM forced on the engine in phase 9, reused rather than special-cased.

use glow::HasContext;

use crate::error::{Error3D, Result3D};
use crate::shader::Program;

/// The smallest scale worth honouring.
///
/// Below this the image is closer to a smear than a picture, and the buffers get small
/// enough that driver minimums could be hit instead. `0.25` is a 4x reduction.
pub const MIN_SCALE: f32 = 0.25;

/// The largest scale that still saves work.
///
/// `1.0` means "do not upscale at all", which the renderer expresses as `None`; anything
/// above it would be supersampling, which is the opposite optimisation and not what this
/// type does.
pub const MAX_SCALE: f32 = 1.0;

/// Four vertices of a clip-space quad: position and UV.
///
/// A triangle strip, so four vertices and no indices. Clockwise from the bottom-left;
/// winding does not matter here because the upscale pass runs with culling disabled —
/// the renderer's own state has already been changed back to the default by then, but
/// relying on that would make this module depend on a caller's state.
const QUAD: [[f32; 4]; 4] = [
    // x,   y,   u,   v
    [-1.0, -1.0, 0.0, 0.0],
    [1.0, -1.0, 1.0, 0.0],
    [-1.0, 1.0, 0.0, 1.0],
    [1.0, 1.0, 1.0, 1.0],
];

/// Bytes per quad vertex.
const QUAD_STRIDE: i32 = 4 * core::mem::size_of::<f32>() as i32;

/// A low-resolution colour target plus the depth buffer it needs, and the pass that
/// stretches it over the window.
///
/// # C ABI
///
/// Not exposed. `GIOCHI_RENDER_SCALE` and the Rust API are the ways to reach it; no C
/// entry point is added in phase 17.
pub struct Upscaler {
    program: Program,
    vbo: glow::Buffer,
    framebuffer: glow::Framebuffer,
    /// Colour attachment; `None` only between construction and the first `allocate`.
    colour: Option<glow::Texture>,
    /// Depth attachment; `None` before the first `allocate`.
    depth: Option<glow::Renderbuffer>,
    /// Low-resolution size, in pixels.
    width: u32,
    height: u32,
    /// Size of the window the result is stretched over.
    target_width: u32,
    target_height: u32,
    scale: f32,
    /// `a_position` and `a_uv` locations in the private program.
    ///
    /// Resolved by name rather than through the fixed slots the mesh pipeline uses:
    /// this program shares no attribute numbering with a mesh draw, and forcing it into
    /// slots 0/1 would make `Mesh::draw_instanced`'s "reset the divisors" logic apply to
    /// a program that never has instance attributes.
    position_location: u32,
    uv_location: u32,
}

impl std::fmt::Debug for Upscaler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Upscaler")
            .field("scale", &self.scale)
            .field("render_size", &(self.width, self.height))
            .field("target_size", &(self.target_width, self.target_height))
            .finish_non_exhaustive()
    }
}

impl Upscaler {
    /// Creates the low-resolution target for a `target_width`x`target_height` window.
    ///
    /// `scale` is clamped to [`MIN_SCALE`]..=[`MAX_SCALE`]; the renderer only constructs
    /// one when the requested scale is below `1.0`.
    pub fn new(
        gl: &glow::Context,
        target_width: u32,
        target_height: u32,
        scale: f32,
    ) -> Result3D<Self> {
        let scale = sanitise_scale(scale);
        let (width, height) = scaled_size(target_width, target_height, scale);

        let program = Program::from_source(
            gl,
            include_str!("../../shaders/upscale.vert"),
            include_str!("../../shaders/upscale.frag"),
        )?;

        // SAFETY: every call below is core GLES 2.0. On any error the objects created so
        // far are deleted before returning, so a refused allocation does not leak.
        unsafe {
            let position_location = gl.get_attrib_location(program.gl_name(), "a_position");
            let uv_location = gl.get_attrib_location(program.gl_name(), "a_uv");
            let (Some(position_location), Some(uv_location)) = (position_location, uv_location)
            else {
                program.destroy(gl);
                return Err(Error3D::Shader {
                    stage: "link",
                    log: "the upscale shader lost a_position/a_uv to the optimiser".to_owned(),
                });
            };

            let vbo = gl
                .create_buffer()
                .map_err(|err| Error3D::GlObject(format!("glGenBuffers (upscale quad): {err}")))?;
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
            let flat: Vec<f32> = QUAD.iter().flatten().copied().collect();
            gl.buffer_data_u8_slice(
                glow::ARRAY_BUFFER,
                bytemuck::cast_slice(&flat),
                glow::STATIC_DRAW,
            );
            gl.bind_buffer(glow::ARRAY_BUFFER, None);

            let mut upscaler = Self {
                program,
                vbo,
                framebuffer: gl
                    .create_framebuffer()
                    .map_err(|err| Error3D::GlObject(format!("glGenFramebuffers: {err}")))?,
                colour: None,
                depth: None,
                width,
                height,
                target_width,
                target_height,
                scale,
                position_location,
                uv_location,
            };
            if let Err(error) = upscaler.allocate(gl) {
                gl.delete_framebuffer(upscaler.framebuffer);
                gl.delete_buffer(upscaler.vbo);
                upscaler.program.destroy(gl);
                return Err(error);
            }
            Ok(upscaler)
        }
    }

    /// (Re)creates the colour texture and the depth renderbuffer for the current size.
    ///
    /// # Safety
    ///
    /// The context must be current. Existing attachments are deleted first, so this is
    /// also the resize path.
    unsafe fn allocate(&mut self, gl: &glow::Context) -> Result3D<()> {
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.framebuffer));

        let colour = gl
            .create_texture()
            .map_err(|err| Error3D::GlObject(format!("glGenTextures (upscale colour): {err}")))?;
        gl.bind_texture(glow::TEXTURE_2D, Some(colour));
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MIN_FILTER,
            glow::LINEAR as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MAG_FILTER,
            glow::LINEAR as i32,
        );
        // Clamping matters: the quad samples exactly 0..1, but a driver's linear filter
        // reaches half a texel outside it, and `REPEAT` there would wrap the opposite
        // edge into the frame — a one-pixel seam on the left and top.
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_S,
            glow::CLAMP_TO_EDGE as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_T,
            glow::CLAMP_TO_EDGE as i32,
        );
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            glow::RGBA as i32,
            self.width as i32,
            self.height as i32,
            0,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            None,
        );
        gl.framebuffer_texture_2d(
            glow::FRAMEBUFFER,
            glow::COLOR_ATTACHMENT0,
            glow::TEXTURE_2D,
            Some(colour),
            0,
        );

        let depth = gl.create_renderbuffer().map_err(|err| {
            Error3D::GlObject(format!("glGenRenderbuffers (upscale depth): {err}"))
        })?;
        gl.bind_renderbuffer(glow::RENDERBUFFER, Some(depth));
        // `DEPTH_COMPONENT16` is the only depth format GLES 2.0 core guarantees for a
        // renderbuffer. A scene that needs more precision (a huge far plane) would
        // notice z-fighting here and not before.
        gl.renderbuffer_storage(
            glow::RENDERBUFFER,
            glow::DEPTH_COMPONENT16,
            self.width as i32,
            self.height as i32,
        );
        gl.framebuffer_renderbuffer(
            glow::FRAMEBUFFER,
            glow::DEPTH_ATTACHMENT,
            glow::RENDERBUFFER,
            Some(depth),
        );

        let status = gl.check_framebuffer_status(glow::FRAMEBUFFER);
        gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        gl.bind_texture(glow::TEXTURE_2D, None);
        gl.bind_renderbuffer(glow::RENDERBUFFER, None);

        if status != glow::FRAMEBUFFER_COMPLETE {
            gl.delete_texture(colour);
            gl.delete_renderbuffer(depth);
            return Err(Error3D::GlObject(format!(
                "the {}x{} render target is incomplete (status 0x{status:04x})",
                self.width, self.height
            )));
        }

        // Only now are the previous attachments replaced, so a failed allocation leaves
        // the upscaler holding the old, working buffers rather than dangling names.
        if let Some(previous) = self.colour.replace(colour) {
            gl.delete_texture(previous);
        }
        if let Some(previous) = self.depth.replace(depth) {
            gl.delete_renderbuffer(previous);
        }
        Ok(())
    }

    /// The scale actually used.
    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// The size the scene renders at.
    pub fn render_size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// The size the result is stretched to.
    pub fn target_size(&self) -> (u32, u32) {
        (self.target_width, self.target_height)
    }

    /// Binds the low-resolution target and sets the viewport to it.
    ///
    /// After this every draw lands in the small buffer, and
    /// [`RenderContext3D::drawable_size`](crate::RenderContext3D::drawable_size) reports
    /// its size so the camera's aspect ratio matches.
    ///
    /// # Safety
    ///
    /// The context must be current.
    pub unsafe fn bind(&self, gl: &glow::Context) {
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.framebuffer));
        gl.viewport(0, 0, self.width as i32, self.height as i32);
    }

    /// Stretches the low-resolution image over the default framebuffer.
    ///
    /// Leaves the default framebuffer bound and the viewport at the window size, so a
    /// caller can read the presented image afterwards.
    ///
    /// # Safety
    ///
    /// The context must be current, and the low-resolution frame must be complete.
    pub unsafe fn blit(&self, gl: &glow::Context) {
        gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        gl.viewport(0, 0, self.target_width as i32, self.target_height as i32);

        // The upscale pass must not be depth-tested against the depth buffer it just
        // rendered, and must not be culled: the quad's winding depends on the driver's
        // `FrontFace` setting, which the renderer owns.
        gl.disable(glow::DEPTH_TEST);
        gl.disable(glow::CULL_FACE);
        gl.disable(glow::BLEND);
        gl.disable(glow::SCISSOR_TEST);
        gl.depth_mask(false);

        self.program.bind(gl);
        if let Some(colour) = self.colour {
            self.program.set_texture(gl, "u_source", colour, 0);
        }

        gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo));
        gl.enable_vertex_attrib_array(self.position_location);
        gl.vertex_attrib_pointer_f32(
            self.position_location,
            2,
            glow::FLOAT,
            false,
            QUAD_STRIDE,
            0,
        );
        gl.enable_vertex_attrib_array(self.uv_location);
        gl.vertex_attrib_pointer_f32(
            self.uv_location,
            2,
            glow::FLOAT,
            false,
            QUAD_STRIDE,
            2 * core::mem::size_of::<f32>() as i32,
        );

        gl.draw_arrays(glow::TRIANGLE_STRIP, 0, 4);

        gl.disable_vertex_attrib_array(self.position_location);
        gl.disable_vertex_attrib_array(self.uv_location);
        gl.bind_buffer(glow::ARRAY_BUFFER, None);
        gl.bind_texture(glow::TEXTURE_2D, None);
    }

    /// Deletes every GL object this owns.
    ///
    /// # Safety
    ///
    /// The context must be current and the upscaler must not be used again.
    pub unsafe fn destroy(self, gl: &glow::Context) {
        gl.delete_framebuffer(self.framebuffer);
        if let Some(colour) = self.colour {
            gl.delete_texture(colour);
        }
        if let Some(depth) = self.depth {
            gl.delete_renderbuffer(depth);
        }
        gl.delete_buffer(self.vbo);
        self.program.destroy(gl);
    }
}

/// Clamps a scale into the supported range, treating a bad value as "no upscaling".
///
/// `NaN` and negatives fall back to `1.0` rather than to [`MIN_SCALE`]: a caller who
/// passed nonsense gets a normal frame, not a blurry one they did not ask for.
pub fn sanitise_scale(scale: f32) -> f32 {
    if !scale.is_finite() || scale <= 0.0 {
        return MAX_SCALE;
    }
    scale.clamp(MIN_SCALE, MAX_SCALE)
}

/// The low-resolution size for a target size and a scale, never zero.
///
/// A zero-sized render target is an incomplete framebuffer, and on a driver that pads
/// its allocations it can also be a different size from the one the viewport assumes.
pub fn scaled_size(width: u32, height: u32, scale: f32) -> (u32, u32) {
    let scale = sanitise_scale(scale);
    (
        ((width as f32 * scale).round() as u32).max(1),
        ((height as f32 * scale).round() as u32).max(1),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scale_is_clamped_into_range() {
        assert_eq!(sanitise_scale(0.75), 0.75);
        assert_eq!(sanitise_scale(1.0), 1.0);
        assert_eq!(sanitise_scale(0.25), 0.25);
        assert_eq!(sanitise_scale(0.01), MIN_SCALE);
        assert_eq!(sanitise_scale(2.0), MAX_SCALE);
    }

    #[test]
    fn a_nonsense_scale_means_no_upscaling() {
        // Not `MIN_SCALE`: a caller who passed garbage should get a normal frame rather
        // than the blurriest one the engine can produce.
        assert_eq!(sanitise_scale(f32::NAN), MAX_SCALE);
        assert_eq!(sanitise_scale(f32::INFINITY), MAX_SCALE);
        assert_eq!(sanitise_scale(-1.0), MAX_SCALE);
        assert_eq!(sanitise_scale(0.0), MAX_SCALE);
    }

    #[test]
    fn the_render_size_scales_and_never_reaches_zero() {
        assert_eq!(scaled_size(1280, 720, 0.75), (960, 540));
        assert_eq!(scaled_size(1280, 720, 0.5), (640, 360));
        assert_eq!(scaled_size(640, 480, 0.75), (480, 360));
        // A tiny window at the minimum scale would round to zero without the clamp.
        assert_eq!(scaled_size(3, 2, 0.25), (1, 1));
        assert_eq!(scaled_size(1, 1, 0.25), (1, 1));
    }

    #[test]
    fn the_quad_covers_clip_space_and_its_texture() {
        // Four corners, and the UVs have to follow the positions or the image is
        // flipped: `(-1,-1)` is the bottom-left of the screen and `v = 0` is the bottom
        // of a `glReadPixels` buffer, so they must agree.
        assert_eq!(QUAD.len(), 4);
        for [x, y, u, v] in QUAD {
            assert!((x == -1.0 || x == 1.0) && (y == -1.0 || y == 1.0));
            assert_eq!(u, (x + 1.0) / 2.0, "u must track x");
            assert_eq!(v, (y + 1.0) / 2.0, "v must track y");
        }
    }

    #[test]
    fn every_quad_vertex_is_four_floats() {
        assert_eq!(QUAD_STRIDE, 16);
        assert_eq!(core::mem::size_of_val(&QUAD), 4 * 16);
    }
}
