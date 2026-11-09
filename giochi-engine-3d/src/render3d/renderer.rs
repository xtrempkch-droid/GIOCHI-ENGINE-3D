//! `RenderContext3D`: owns the GL context and the fixed-function state.
//!
//! Phase 9 only clears the screen. Mesh and shader drawing arrive in phase 11; the
//! type exists now so the frame skeleton (state → clear → swap) is fixed before
//! anything is added to it.

use giochi_engine::perf::FrameCounters;
use giochi_engine::platform::window::Window;

use glow::HasContext;

use crate::context::{GlContext, GlState, InstancingSupport};
use crate::error::{Error3D, Result3D};
use crate::math3d::{Camera3D, Color3D, Mat4, Transform3D};
use crate::mesh::{Mesh, Vertex};
use crate::registry::HandleRegistry;
use crate::render3d::{DirectionalLight, InstancedRenderer, Upscaler};
use crate::shader::{BuiltinShader, Program};

/// Opaque handle to a mesh. `0` is invalid.
///
/// # C ABI
///
/// `typedef uint64_t GiochiMeshHandle;` (`docs/C_ABI_CONTRACT.md` §9.5).
pub type MeshHandle = u64;
/// Opaque handle to a shader program. `0` is invalid.
///
/// # C ABI
///
/// `typedef uint64_t GiochiShaderHandle;`.
pub type ShaderHandle = u64;

/// The 3D renderer.
///
/// Deliberately **not** `#[repr(C)]`: it holds a `glow::Context`, which cannot cross a
/// C boundary. C consumers get an opaque pointer in phase 14 and reach it through
/// `giochi_renderer3d_*` functions, the same shape as `GiochiRenderContext` in 2D.
pub struct RenderContext3D {
    gl: GlContext,
    state: GlState,
    /// Viewport size in physical pixels, updated by `begin_frame`.
    drawable_size: (u32, u32),
    /// The one light every draw in the frame is shaded with.
    light: DirectionalLight,
    meshes: HandleRegistry<Mesh>,
    shaders: HandleRegistry<Program>,
    /// Set when the frame renders below the window size; `None` is full resolution.
    ///
    /// Stored as the requested scale rather than as a `bool` so a resize can rebuild the
    /// target at the same ratio instead of silently dropping back to 1.0.
    render_scale: Option<f32>,
    /// Rebuilt on demand by `begin_frame`; `None` when no scale is set.
    upscaler: Option<Upscaler>,
    /// The per-instance buffer, created with the context.
    ///
    /// `None` only when the driver refused the buffer, which is the same class of
    /// failure as a refused mesh. `draw_instanced` falls back to one draw per instance
    /// so the scene still renders, and logs the reason once.
    instanced: Option<InstancedRenderer>,
    /// Draw calls the last `draw_instanced` issued.
    ///
    /// Kept here rather than read from `InstancedRenderer`: the per-instance fallback
    /// never calls into that type, so asking it would report 0 while the frame issued N
    /// draws — the one number a caller uses to tell the two paths apart.
    last_instanced_draw_calls: u32,
    #[cfg(feature = "profiling")]
    counters: FrameCounters,
}

impl std::fmt::Debug for RenderContext3D {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderContext3D")
            .field("version", &self.gl.version())
            .field("adapter", &self.gl.adapter())
            .field("state", &self.state)
            .field("drawable_size", &self.drawable_size)
            .field("light", &self.light)
            .field("meshes", &self.meshes.len())
            .field("shaders", &self.shaders.len())
            .finish_non_exhaustive()
    }
}

impl RenderContext3D {
    /// Wraps an existing GL context.
    pub fn new(gl: GlContext) -> Self {
        let state = GlState::default();
        // Apply once at construction so the first frame does not inherit whatever the
        // driver's default state happens to be.
        state.apply(gl.glow());

        // Built before the struct takes ownership of `gl`; the buffer belongs to this
        // context either way, and a refusal is handled here rather than in every caller.
        let instanced = match InstancedRenderer::new(gl.glow(), gl.instancing()) {
            Ok(instanced) => Some(instanced),
            Err(err) => {
                log::warn!("instanced drawing is unavailable: {err}");
                None
            }
        };

        Self {
            gl,
            state,
            drawable_size: (1, 1),
            light: DirectionalLight::sun(),
            meshes: HandleRegistry::new(),
            shaders: HandleRegistry::new(),
            render_scale: None,
            upscaler: None,
            // `RenderContext3D::new` is infallible by contract, so a refused buffer
            // degrades to "draw one at a time" (below) rather than turning every caller
            // into a `Result`. The fallback draws the correct picture, only slower.
            instanced,
            last_instanced_draw_calls: 0,
            #[cfg(feature = "profiling")]
            counters: FrameCounters::new(),
        }
    }

    /// The glow function table, for the mesh and shader modules.
    pub fn glow(&self) -> &glow::Context {
        self.gl.glow()
    }

    /// The live GL state.
    pub fn state(&self) -> GlState {
        self.state
    }

    /// Replaces the GL state. Takes effect on the next [`RenderContext3D::begin_frame`].
    pub fn set_state(&mut self, state: GlState) {
        self.state = state;
    }

    /// Sets the colour the next frame is cleared with.
    pub fn set_clear_color(&mut self, color: Color3D) {
        self.state.clear_color = color.clamped();
    }

    /// Replaces the light every draw in the frame is shaded with.
    ///
    /// Sanitised here rather than at draw time so the cost is paid once per change: the
    /// per-draw path uploads already-valid numbers. Setting a light only affects
    /// shaders that read it ([`BuiltinShader::reads_lighting`]); on the others the
    /// uniforms are not present and the upload is skipped.
    pub fn set_light(&mut self, light: DirectionalLight) {
        self.light = light.sanitised();
    }

    /// The current light, as the shader sees it (already sanitised).
    pub fn light(&self) -> DirectionalLight {
        self.light
    }

    /// Applies the state, sets the viewport and clears colour and depth.
    ///
    /// Every frame starts here. Depth is cleared every frame because a 3D scene that
    /// reuses the previous frame's depth buffer renders back-to-front garbage.
    ///
    /// The viewport is taken from the **window's real drawable size**, not from the
    /// configured size. On KMSDRM those differ: the backend is fullscreen-only and
    /// ignores the requested size (ADR-035), so a viewport built from the configuration
    /// would draw into the corner of a panel and clip the rest. This is why the window
    /// is a parameter rather than state captured at construction — it can be resized.
    pub fn begin_frame(&mut self, window: &Window) {
        let (width, height) = window.drawable_size();
        self.resize(width, height);

        self.state.apply(self.gl.glow());
        self.state.clear(self.gl.glow());

        #[cfg(feature = "profiling")]
        self.counters.reset();
    }

    /// Snapshot of the draw counters recorded since [`RenderContext3D::begin_frame`].
    ///
    /// Without the `profiling` feature every field is zero.
    pub fn frame_counters(&self) -> FrameCounters {
        #[cfg(feature = "profiling")]
        {
            self.counters
        }
        #[cfg(not(feature = "profiling"))]
        {
            FrameCounters::new()
        }
    }

    /// Switches polygon rasterisation between filled and wireframe.
    ///
    /// Wireframe is the fastest way to tell "the face was culled" from "the face was
    /// never drawn", so it belongs in the renderer rather than in an example.
    pub fn set_wireframe(&self, on: bool) {
        // SAFETY: `polygon_mode` is core GLES 2.0 (GL_LINE is the only alternative it
        // allows). A driver that rejects it raises GL_INVALID_ENUM, which
        // `take_gl_error` surfaces.
        unsafe {
            self.gl.glow().polygon_mode(
                glow::FRONT_AND_BACK,
                if on { glow::LINE } else { glow::FILL },
            );
        }
    }

    /// Sets the GL viewport to `width`x`height` and records it for the camera.
    ///
    /// Split out of [`RenderContext3D::begin_frame`] so a test can drive it without a
    /// window, and so callers that manage their own loop can call it exactly once.
    pub fn resize(&mut self, width: u32, height: u32) {
        let width = width.max(1);
        let height = height.max(1);

        let Some(scale) = self.render_scale else {
            self.destroy_upscaler();
            // SAFETY: `viewport` and `bind_framebuffer` are core GLES 2.0, context current.
            unsafe {
                let gl = self.gl.glow();
                gl.bind_framebuffer(glow::FRAMEBUFFER, None);
                gl.viewport(0, 0, width as i32, height as i32);
            }
            self.drawable_size = (width, height);
            return;
        };

        let stale = self
            .upscaler
            .as_ref()
            .is_none_or(|upscaler| upscaler.target_size() != (width, height));
        if stale {
            self.destroy_upscaler();
            match Upscaler::new(self.gl.glow(), width, height, scale) {
                Ok(upscaler) => {
                    log::info!(
                        "rendering at {:?} and upscaling to {width}x{height} (scale {scale})",
                        upscaler.render_size()
                    );
                    self.upscaler = Some(upscaler);
                }
                Err(error) => {
                    // Falling back to full resolution is the right degradation: a
                    // refused framebuffer must not turn into a black screen.
                    log::warn!(
                        "could not create the {scale} render target ({error}); \
                         rendering at full resolution"
                    );
                    self.render_scale = None;
                    // SAFETY: as above.
                    unsafe {
                        let gl = self.gl.glow();
                        gl.bind_framebuffer(glow::FRAMEBUFFER, None);
                        gl.viewport(0, 0, width as i32, height as i32);
                    }
                    self.drawable_size = (width, height);
                    return;
                }
            }
        }

        self.drawable_size = self
            .upscaler
            .as_ref()
            .map_or((width, height), Upscaler::render_size);

        // SAFETY: the upscaler's buffers belong to this context, which is current.
        unsafe {
            let gl = self.gl.glow();
            if let Some(upscaler) = self.upscaler.as_ref() {
                upscaler.bind(gl);
            }
        }
    }

    /// Renders the next frames at `scale` of the window resolution.
    ///
    /// `None` (the default) renders at full resolution. A scale below `1.0` renders into
    /// a smaller framebuffer and stretches the result over the window with bilinear
    /// filtering; see [`Upscaler`]. [`MIN_SCALE`](crate::render3d::upscale::MIN_SCALE)
    /// and [`MAX_SCALE`](crate::render3d::upscale::MAX_SCALE) bound the value, and a
    /// `scale` of `1.0` is treated as `None`.
    ///
    /// Takes effect on the next [`RenderContext3D::begin_frame`].
    pub fn set_render_scale(&mut self, scale: Option<f32>) {
        let scale = scale.map(crate::render3d::upscale::sanitise_scale);
        let scale = scale.filter(|value| *value < crate::render3d::upscale::MAX_SCALE);
        if scale == self.render_scale {
            return;
        }
        self.render_scale = scale;
        // The target is rebuilt by the next `begin_frame` → `resize`, which is also the
        // only place that learns the real window size (KMSDRM ignores the requested one).
        self.destroy_upscaler();
    }

    /// The requested render scale, or `None` for full resolution.
    pub fn render_scale(&self) -> Option<f32> {
        self.render_scale
    }

    /// The size the scene is actually rendering at, which is smaller than
    /// [`RenderContext3D::drawable_size`] only if something is wrong.
    pub fn render_size(&self) -> (u32, u32) {
        self.upscaler
            .as_ref()
            .map_or(self.drawable_size, Upscaler::render_size)
    }

    fn destroy_upscaler(&mut self) {
        if let Some(upscaler) = self.upscaler.take() {
            // SAFETY: the upscaler's objects belong to this context, which is current,
            // and it is not used again.
            unsafe { upscaler.destroy(self.gl.glow()) };
        }
    }

    /// The viewport size last set, in physical pixels.
    ///
    /// Feed this to [`Camera3D::with_drawable_size`](crate::math3d::Camera3D::with_drawable_size)
    /// once per frame: it is the real surface, which on a console is the panel and not
    /// what the configuration asked for.
    pub fn drawable_size(&self) -> (u32, u32) {
        self.drawable_size
    }

    /// Aspect ratio of the last viewport, or `1.0` before the first frame.
    pub fn aspect(&self) -> f32 {
        let (width, height) = self.drawable_size;
        width as f32 / height as f32
    }

    /// Presents the back buffer, upscaling it first when a scale is set.
    pub fn end_frame(&self, window: &Window) {
        if let Some(upscaler) = self.upscaler.as_ref() {
            // SAFETY: the low-resolution frame is complete and the context is current.
            unsafe { upscaler.blit(self.gl.glow()) };
        }
        crate::context::swap(window);
    }

    /// Convenience for the phase 9 demo: clear with `color` and present.
    pub fn clear(&mut self, window: &Window, color: Color3D) {
        self.set_clear_color(color);
        self.begin_frame(window);
        self.end_frame(window);
    }

    /// GLES version the driver gave us.
    pub fn gles_version(&self) -> crate::context::GlesVersion {
        self.gl.version()
    }

    /// The driver's renderer string.
    pub fn adapter(&self) -> &str {
        self.gl.adapter()
    }

    /// Whether GLES 3.0-only features are available.
    pub fn supports_gles3(&self) -> bool {
        self.gl.supports_gles3()
    }

    /// Pops one GL error and clears it, returning `0` when there was none.
    ///
    /// `glGetError` keeps a queue; draining it matters because a stale error from
    /// start-up would otherwise be attributed to whichever frame happened to read it.
    /// The smoke example asserts this after every frame, which is how a broken state
    /// setup shows up as a failure instead of as a black screen.
    pub fn take_gl_error(&self) -> u32 {
        // SAFETY: `get_error` is core GLES 2.0 and the context is current.
        let code = unsafe { self.gl.glow().get_error() };

        if code != glow::NO_ERROR {
            log::error!("GL error 0x{code:04x}");
        }

        code
    }

    /// Reads back `width`x`height` RGBA8 pixels from the current read buffer.
    ///
    /// Exists so a test can assert on the *pixels* rather than on the absence of an
    /// error: "the clear call was issued" and "the surface is that colour" are
    /// different claims. Returns bottom-up rows, as GL does.
    ///
    /// # Errors
    ///
    /// [`Error3D::InvalidArgument`] for a zero or absurdly large region — a 16 K
    /// read-back is 1 GiB of allocation and is never what a caller meant.
    pub fn read_pixels(&self, width: u32, height: u32) -> crate::error::Result3D<Vec<u8>> {
        const MAX_DIMENSION: u32 = 8192;

        if width == 0 || height == 0 {
            return Err(crate::error::Error3D::InvalidArgument(
                "read_pixels needs a non-zero region".to_owned(),
            ));
        }
        if width > MAX_DIMENSION || height > MAX_DIMENSION {
            return Err(crate::error::Error3D::InvalidArgument(format!(
                "read_pixels region {width}x{height} exceeds {MAX_DIMENSION}"
            )));
        }

        let mut buffer = vec![0u8; width as usize * height as usize * 4];

        // With a scale set the scene lived in an off-screen target, and `glReadPixels`
        // reads whatever is bound. The caller asking for "the frame" means the presented
        // picture, so the upscale pass runs here if it has not already: a capture of the
        // low-resolution buffer would be a smaller image than the caller asked for and
        // would look like a bug in the caller.
        if let Some(upscaler) = self.upscaler.as_ref() {
            // SAFETY: the low-resolution frame is complete and the context is current.
            unsafe { upscaler.blit(self.gl.glow()) };
        }

        // SAFETY: `buffer` is exactly `width * height * 4` bytes, which is what
        // `glReadPixels` writes for RGBA/UNSIGNED_BYTE. The context is current.
        unsafe {
            self.gl.glow().read_pixels(
                0,
                0,
                width as i32,
                height as i32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(buffer.as_mut_slice()),
            );
        }

        Ok(buffer)
    }

    // ---- meshes ----------------------------------------------------------

    /// Uploads a mesh and returns its handle.
    ///
    /// # Errors
    ///
    /// [`Error3D::InvalidArgument`] for an empty mesh or an out-of-range index;
    /// [`Error3D::GlObject`] when the driver refuses a buffer.
    pub fn create_mesh(&mut self, vertices: &[Vertex], indices: &[u32]) -> Result3D<MeshHandle> {
        let mesh = Mesh::from_vertices_indices(self.gl.glow(), vertices, indices)?;
        Ok(self.meshes.insert(mesh))
    }

    /// Uploads one of the procedural primitives.
    ///
    /// The example uses this rather than a committed `.obj`; see
    /// [`crate::mesh::cube`] for why.
    pub fn create_cube_mesh(&mut self, size: f32) -> Result3D<MeshHandle> {
        let (vertices, indices) = crate::mesh::cube(size);
        self.create_mesh(&vertices, &indices)
    }

    /// Uploads a procedural ball (UV sphere).
    ///
    /// `radius` is the sphere's radius, which the call site also needs for the offset
    /// above the surface, so naming it here keeps the two in step.
    pub fn create_sphere_mesh(
        &mut self,
        radius: f32,
        rings: u32,
        segments: u32,
    ) -> Result3D<MeshHandle> {
        let (vertices, indices) = crate::mesh::sphere(radius, rings, segments);
        self.create_mesh(&vertices, &indices)
    }

    /// Uploads the procedural floor plane.
    pub fn create_plane_mesh(&mut self, size: f32, subdivisions: u32) -> Result3D<MeshHandle> {
        let (vertices, indices) = crate::mesh::plane(size, subdivisions);
        self.create_mesh(&vertices, &indices)
    }

    /// Loads an OBJ file and uploads it.
    pub fn load_obj_mesh(&mut self, path: &std::path::Path) -> Result3D<MeshHandle> {
        let (vertices, indices) = crate::mesh::load_obj(path)?;
        self.create_mesh(&vertices, &indices)
    }

    /// Releases a mesh. Returns `false` for `0` or an already-destroyed handle.
    pub fn destroy_mesh(&mut self, handle: MeshHandle) -> bool {
        // `take`, not `get`: `get` clones the `Rc`, and `Rc::try_unwrap` never succeeds on
        // a count of two — so every mesh used to keep its VAO, VBO and EBO alive in the
        // driver while the registry reported it gone. See `HandleRegistry::take`.
        let Some(mesh) = self.meshes.take(handle) else {
            return false;
        };

        // The unwrap can still fail when a caller holds a clone. That is now a real
        // fault worth reporting: the entry is gone either way, so the objects leak unless
        // the caller drops its clone first.
        match std::rc::Rc::try_unwrap(mesh) {
            Ok(mesh) => mesh.destroy(self.gl.glow()),
            Err(_) => {
                log::warn!("mesh {handle} is still referenced elsewhere; GL objects leaked");
            }
        }

        true
    }

    /// Number of live meshes.
    pub fn mesh_count(&self) -> usize {
        self.meshes.len()
    }

    /// Index count of `handle`, or `0` for an unknown handle.
    pub fn mesh_index_count(&self, handle: MeshHandle) -> u32 {
        self.meshes
            .get(handle)
            .map(|mesh| mesh.index_count())
            .unwrap_or(0)
    }

    /// Vertex count of `handle`, or `0` for an unknown handle.
    pub fn mesh_vertex_count(&self, handle: MeshHandle) -> u32 {
        self.meshes
            .get(handle)
            .map(|mesh| mesh.vertex_count())
            .unwrap_or(0)
    }

    /// Whether `handle` uses a vertex array object.
    ///
    /// Exposed because the two attribute paths are genuinely different code and a test
    /// has to be able to assert which one ran.
    pub fn mesh_uses_vao(&self, handle: MeshHandle) -> bool {
        self.meshes
            .get(handle)
            .map(|mesh| mesh.uses_vao())
            .unwrap_or(false)
    }

    // ---- shaders ---------------------------------------------------------

    /// Compiles one of the built-in shaders.
    pub fn create_builtin_shader(&mut self, which: BuiltinShader) -> Result3D<ShaderHandle> {
        let program = Program::from_source(
            self.gl.glow(),
            which.vertex_source(),
            which.fragment_source(),
        )?;
        Ok(self.shaders.insert(program))
    }

    /// Compiles a vertex/fragment pair read from disk.
    ///
    /// R10: a user shader is a file, never a string embedded in the caller's binary.
    pub fn load_shader(
        &mut self,
        vertex_path: &std::path::Path,
        fragment_path: &std::path::Path,
    ) -> Result3D<ShaderHandle> {
        let vertex = std::fs::read_to_string(vertex_path).map_err(|err| {
            Error3D::Asset(format!(
                "failed to read vertex shader '{}': {err}",
                vertex_path.display()
            ))
        })?;
        let fragment = std::fs::read_to_string(fragment_path).map_err(|err| {
            Error3D::Asset(format!(
                "failed to read fragment shader '{}': {err}",
                fragment_path.display()
            ))
        })?;

        let program = Program::from_source(self.gl.glow(), &vertex, &fragment)?;
        Ok(self.shaders.insert(program))
    }

    /// Releases a shader program. Returns `false` for `0` or an unknown handle.
    pub fn destroy_shader(&mut self, handle: ShaderHandle) -> bool {
        // `take`, not `get`: see `destroy_mesh` and `HandleRegistry::take`. Using `get`
        // here meant the GL program was never deleted.
        let Some(program) = self.shaders.take(handle) else {
            return false;
        };

        match std::rc::Rc::try_unwrap(program) {
            Ok(program) => program.destroy(self.gl.glow()),
            Err(_) => {
                log::warn!("shader {handle} is still referenced elsewhere; GL objects leaked");
            }
        }

        true
    }

    /// Number of live shader programs.
    pub fn shader_count(&self) -> usize {
        self.shaders.len()
    }

    /// Releases every mesh and shader, then reports whether anything is left.
    ///
    /// Exists for an orderly shutdown and for the leak check the demo prints: after
    /// destroying its resources a game should see `true` here. Anything still held
    /// elsewhere (an `Rc` clone) leaks, and that is worth surfacing rather than hiding.
    pub fn release_all(&mut self) -> bool {
        // A registry that empties is not the same claim as "the GL objects are gone": if a
        // caller still holds a clone, `destroy_*` logs the leak and the entry disappears
        // anyway. Counting the references first is what lets this answer the second
        // question, which is the one every caller of this function actually asks.
        let mut leaked = 0;
        for handle in self.meshes.handles() {
            if self.meshes.strong_count(handle) > Some(1) {
                leaked += 1;
            }
            self.destroy_mesh(handle);
        }
        for handle in self.shaders.handles() {
            if self.shaders.strong_count(handle) > Some(1) {
                leaked += 1;
            }
            self.destroy_shader(handle);
        }

        // Not a handle either: the render target is rebuilt on demand, so dropping it
        // here releases the framebuffer, its attachments and the upscale program.
        self.destroy_upscaler();

        // The instance buffer is not a handle, so nothing can hold a clone of it: after
        // this the renderer has no GL objects at all. It is safe because `meshes` is
        // empty, and `draw_instanced` returns `Ok(0)` for an unknown mesh before it can
        // reach the buffer.
        if let Some(instanced) = self.instanced.take() {
            // SAFETY: the context is current (this is the same requirement every other
            // `destroy_*` here has) and the buffer is not used again.
            unsafe { instanced.destroy(self.gl.glow()) };
        }

        if leaked > 0 {
            log::warn!("{leaked} resource(s) were still referenced elsewhere and leaked");
        }

        self.meshes.is_empty() && self.shaders.is_empty() && leaked == 0
    }

    // ---- drawing ---------------------------------------------------------

    /// Uploads the matrices and the light for one draw, binding `program` first.
    ///
    /// Shared by both draw entry points: a uniform that one of them forgets is a scene
    /// that is lit differently depending on which function drew it, which is close to
    /// impossible to spot on a console.
    fn bind_and_upload(
        &self,
        program: &Program,
        model: &Transform3D,
        camera: &Camera3D,
    ) -> Result3D<()> {
        self.bind_and_upload_matrix(program, &model.matrix(), camera)
    }

    /// [`RenderContext3D::bind_and_upload`] for a precomputed model matrix.
    ///
    /// `draw_instanced`'s fallback already holds matrices, and converting one back into
    /// a `Transform3D` (extracting a rotation from a matrix) would cost more than the
    /// uniform it sets — and could disagree with the instanced path's picture, which is
    /// the one thing an A/B cannot tolerate.
    fn bind_and_upload_matrix(
        &self,
        program: &Program,
        model_matrix: &Mat4,
        camera: &Camera3D,
    ) -> Result3D<()> {
        // The camera may legitimately be unresolved (aspect 0.0), and silently drawing
        // with a stale projection is how a frame ends up subtly wrong on a console.
        let resolved = camera.with_drawable_size(self.drawable_size.0, self.drawable_size.1);
        let view_proj = resolved
            .view_projection()
            .map_err(|reason| Error3D::InvalidArgument(reason.to_owned()))?;

        let gl = self.gl.glow();
        program.bind(gl);
        program.set_mat4(gl, "u_view_proj", &view_proj);

        program.set_mat4(gl, "u_model", model_matrix);
        // Phong reads normals through this; the flat and textured shaders do not have
        // the uniform, and `set_mat4` skips an unknown name.
        program.set_mat4(gl, "u_normal_matrix", &model_matrix.normal_matrix());

        self.upload_light_and_camera(program, &resolved);
        Ok(())
    }

    /// Binds `program` and uploads everything that does not depend on the model matrix.
    ///
    /// Used directly by [`RenderContext3D::draw_instanced`], where there is no single
    /// model matrix: the shader takes it from the per-instance attributes.
    fn bind_and_upload_shared(&self, program: &Program, camera: &Camera3D) -> Result3D<()> {
        let resolved = camera.with_drawable_size(self.drawable_size.0, self.drawable_size.1);
        let view_proj = resolved
            .view_projection()
            .map_err(|reason| Error3D::InvalidArgument(reason.to_owned()))?;

        let gl = self.gl.glow();
        program.bind(gl);
        program.set_mat4(gl, "u_view_proj", &view_proj);
        self.upload_light_and_camera(program, &resolved);
        Ok(())
    }

    fn upload_light_and_camera(&self, program: &Program, resolved: &Camera3D) {
        let gl = self.gl.glow();
        // Lighting. `u_light_color` is a `vec3`, so it goes through `set_vec3` and not
        // `set_color`: `glUniform4fv` on a `vec3` uniform is GL_INVALID_OPERATION, and
        // the scene would render black with the error only surfacing in `take_gl_error`.
        let light = self.light;
        program.set_vec3(gl, "u_light_direction", light.direction);
        program.set_vec3(
            gl,
            "u_light_color",
            crate::math3d::Vec3::new(light.color.r, light.color.g, light.color.b),
        );
        program.set_f32(gl, "u_ambient_strength", light.ambient);
        program.set_f32(gl, "u_specular_strength", light.specular);
        program.set_f32(gl, "u_shininess", light.shininess);
        program.set_vec3(gl, "u_camera_position", resolved.position);
    }

    /// Draws `mesh` with `shader` at `model`, seen through `camera`.
    ///
    /// Uniforms are set every call rather than cached per (mesh, shader) pair: with a
    /// handful of draws per frame the bookkeeping would cost more than the two
    /// `glUniformMatrix4fv` calls it saves, and stale-model bugs are expensive.
    ///
    /// The colour is the clear colour, which makes a quick "does it draw" testable
    /// without choosing a colour first. Use [`RenderContext3D::draw_mesh_colored`] for
    /// anything real.
    ///
    /// Returns `false` when either handle is unknown, so a caller can tell "drew
    /// nothing" from "drew a degenerate frame".
    pub fn draw_mesh(
        &mut self,
        mesh: MeshHandle,
        shader: ShaderHandle,
        model: &Transform3D,
        camera: &Camera3D,
    ) -> Result3D<bool> {
        let Some(mesh) = self.meshes.get(mesh) else {
            return Ok(false);
        };
        let Some(program) = self.shaders.get(shader) else {
            return Ok(false);
        };

        self.bind_and_upload(&program, model, camera)?;

        // The flat shader reads `u_color`; a textured one reads `u_tint`. Setting both
        // is free (a missing uniform is cached as `None` and skipped).
        let gl = self.gl.glow();
        let colour = self.state.clear_color;
        program.set_color(gl, "u_color", colour);
        program.set_color(gl, "u_tint", Color3D::WHITE);

        mesh.draw(gl, &program);

        #[cfg(feature = "profiling")]
        self.counters.record_mesh(
            u64::from(mesh.vertex_count()),
            u64::from(mesh.vertex_count()) * std::mem::size_of::<Vertex>() as u64,
        );

        Ok(true)
    }

    /// Draws `mesh` with an explicit colour, overriding the clear colour.
    pub fn draw_mesh_colored(
        &mut self,
        mesh: MeshHandle,
        shader: ShaderHandle,
        model: &Transform3D,
        camera: &Camera3D,
        colour: Color3D,
    ) -> Result3D<bool> {
        let Some(mesh) = self.meshes.get(mesh) else {
            return Ok(false);
        };
        let Some(program) = self.shaders.get(shader) else {
            return Ok(false);
        };

        self.bind_and_upload(&program, model, camera)?;
        program.set_color(self.gl.glow(), "u_color", colour.clamped());

        mesh.draw(self.gl.glow(), &program);

        #[cfg(feature = "profiling")]
        self.counters.record_mesh(
            u64::from(mesh.vertex_count()),
            u64::from(mesh.vertex_count()) * std::mem::size_of::<Vertex>() as u64,
        );

        Ok(true)
    }

    /// Draws `instances` copies of `mesh` with one call when the driver can.
    ///
    /// `shader` must be [`BuiltinShader::Instanced`] (or a user program that reads the
    /// model matrix from attributes 3..6). Shading is otherwise identical to
    /// [`RenderContext3D::draw_mesh_colored`], because the fragment shader is the same
    /// source — that is what makes an A/B of the two arms a fair comparison.
    ///
    /// Returns the number of backend draw calls issued: **1** when instancing is
    /// available, **`instances.len()`** on a driver without it, **0** for an empty
    /// slice. A caller that wants a specific draw-call count can assert on this instead
    /// of inferring it from the frame rate.
    ///
    /// The fallback is not an error state. It draws the same picture through the
    /// ordinary path, which is what keeps a game running on a driver that lacks the
    /// extension; the cost is exactly what phase 17 removes, so it is reported rather
    /// than hidden.
    pub fn draw_instanced(
        &mut self,
        mesh: MeshHandle,
        shader: ShaderHandle,
        instances: &[Mat4],
        camera: &Camera3D,
        colour: Color3D,
    ) -> Result3D<u32> {
        if instances.is_empty() {
            self.last_instanced_draw_calls = 0;
            return Ok(0);
        }
        if self.meshes.get(mesh).is_none() || self.shaders.get(shader).is_none() {
            self.last_instanced_draw_calls = 0;
            return Ok(0);
        }

        // Split the borrow: the program and the mesh come out of different registries,
        // and the instance buffer is a third field. Borrowing `self` wholesale would
        // make `bind_and_upload` (which needs `&self`) and `instanced` (which needs
        // `&mut`) conflict.
        let program = self
            .shaders
            .get(shader)
            .expect("checked immediately above")
            .clone();
        let geometry = self
            .meshes
            .get(mesh)
            .expect("checked immediately above")
            .clone();

        if !program.is_instanced() {
            // A `Phong`/`Flat` program has no instance attributes, so the single-call
            // path would draw every copy at whatever `u_model` happens to hold. The
            // per-instance fallback below is correct for such a program, so use it and
            // report the slower call rather than draw a wrong picture faster.
            log::warn!(
                "draw_instanced needs an instanced shader (BuiltinShader::Instanced); \
                 drawing one call per instance instead"
            );
        }

        // View-projection and the light are identical for every instance.
        self.bind_and_upload_shared(&program, camera)?;
        program.set_color(self.gl.glow(), "u_color", colour.clamped());

        let available = program.is_instanced()
            && self
                .instanced
                .as_ref()
                .is_some_and(|instanced| instanced.support().is_available());

        if available {
            let gl = self.gl.glow();
            let backend = self.instanced.as_ref().expect("checked above").backend();
            let mut instanced = self.instanced.take().expect("checked above");
            // SAFETY: the program is bound and the uniforms uploaded; the mesh and the
            // instance buffer belong to this context.
            let calls = unsafe { instanced.draw(gl, &geometry, instances) };
            self.instanced = Some(instanced);
            self.last_instanced_draw_calls = calls;

            #[cfg(feature = "profiling")]
            self.counters.record_draw_calls(calls);
            #[cfg(feature = "profiling")]
            self.counters.record_mesh_instanced(
                u64::from(geometry.vertex_count()),
                u64::from(geometry.vertex_count()) * std::mem::size_of::<Vertex>() as u64,
                u64::from(instances.len() as u32),
            );

            let _ = backend;
            return Ok(calls);
        }

        // Fallback: one ordinary draw per instance, with the same picture.
        //
        // The instanced vertex shader deliberately has no `u_model`; it reads the matrix
        // from attributes 3..6. `Mesh::draw` only knows about 0..2, so setting a uniform
        // here would do nothing and the copies would be transformed by whatever generic
        // attribute values the driver happened to hold — not the documented "same picture,
        // more calls". With the arrays disabled, GLES uses the current generic value for
        // that attribute, so writing the four columns before each draw is enough.
        let gl = self.gl.glow();
        if program.is_instanced() {
            // A previous draw may have left these slots enabled on the default VAO.
            for slot in
                crate::shader::ATTRIB_INSTANCE_MODEL..crate::shader::ATTRIB_INSTANCE_MODEL + 4
            {
                // SAFETY: attribute state on the default VAO, core GLES 2.0.
                unsafe { gl.disable_vertex_attrib_array(slot) };
            }
        }

        for model in instances {
            self.bind_and_upload_matrix(&program, model, camera)?;
            program.set_color(gl, "u_color", colour.clamped());
            if program.is_instanced() {
                for column in 0..4usize {
                    // SAFETY: a generic vertex attribute is core GLES 2.0 (the array is
                    // disabled above). `Mat4::data` is column-major, so each slice is one
                    // column of the matrix the shader reassembles.
                    unsafe {
                        gl.vertex_attrib_4_f32_slice(
                            crate::shader::ATTRIB_INSTANCE_MODEL + column as u32,
                            &model.data[column * 4..column * 4 + 4],
                        );
                    }
                }
            }
            geometry.draw(gl, &program);
        }

        #[cfg(feature = "profiling")]
        self.counters.record_draw_calls(instances.len() as u32);
        #[cfg(feature = "profiling")]
        self.counters.record_mesh_instanced(
            u64::from(geometry.vertex_count()),
            u64::from(geometry.vertex_count()) * std::mem::size_of::<Vertex>() as u64,
            u64::from(instances.len() as u32),
        );

        self.last_instanced_draw_calls = instances.len() as u32;
        Ok(instances.len() as u32)
    }

    /// Whether instanced drawing will use a single call on this driver.
    pub fn supports_instancing(&self) -> bool {
        self.instanced
            .as_ref()
            .is_some_and(|instanced| instanced.support().is_available())
    }

    /// Which instanced-drawing provider was detected, for logs and the overlay.
    pub fn instancing_support(&self) -> InstancingSupport {
        self.instanced
            .as_ref()
            .map_or(InstancingSupport::None, |instanced| instanced.support())
    }

    /// Draw calls issued by the last [`RenderContext3D::draw_instanced`].
    pub fn last_instanced_draw_calls(&self) -> u32 {
        self.last_instanced_draw_calls
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_context_starts_with_the_default_state() {
        // The state must be explicit even before any frame runs.
        let state = GlState::default();

        assert!(state.depth_test);
        assert!(state.cull_face);
        assert!(!state.blend);
    }

    #[test]
    fn setting_a_clear_color_clamps_it() {
        let state = GlState {
            clear_color: Color3D::new(2.0, -1.0, 0.5, 1.0).clamped(),
            ..GlState::default()
        };

        assert_eq!(state.clear_color.to_array(), [1.0, 0.0, 0.5, 1.0]);
    }

    thread_local! {
        /// Whether a GL context has already been created in this test binary.
        static CREATED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }

    /// Returns a headless `RenderContext3D`, or `None` when this machine has no GL driver.
    ///
    /// A unit test rather than an integration test because proving the objects were
    /// deleted needs `is_buffer`/`is_program` and the GL names, which are crate-internal.
    ///
    /// **One renderer per test binary, deliberately.** A second `Platform::new()` in the
    /// same process returns an error, so two tests that each built one would leave the
    /// second silently skipping — and a skipped test is reported as `ok`, which is a
    /// false green. Everything that needs a context lives in the single test below.
    fn headless_renderer() -> Option<RenderContext3D> {
        use giochi_engine::platform::sdl::Platform;

        // A second `Platform::new()` in one process fails, so a *second* test calling this
        // would print "skipped" and be recorded as `ok` — a false green, which is the exact
        // class of bug this file exists to catch. Failing loudly forces every new GL
        // assertion into the single test below.
        CREATED.with(|cell| {
            assert!(
                !cell.replace(true),
                "only one GL context per test binary: add your assertion to \
                 `destroying_a_resource_really_deletes_its_gl_objects` instead of starting \
                 a second context, which would silently skip"
            );
        });
        use giochi_engine::platform::window::{Window, WindowConfig};

        std::env::set_var("SDL_VIDEODRIVER", "offscreen");
        let platform = Platform::new().ok()?;
        crate::context::configure_attributes(&platform, crate::context::GlesVersion::Gles2).ok()?;
        let window = Window::new_opengl(&platform, WindowConfig::new(64, 64), "test").ok()?;
        let gl = crate::context::create(&window).ok()?;

        // The window and the platform must outlive the context, and keeping them in the
        // renderer would mean widening `RenderContext3D` for a test. A test binary that
        // exits in milliseconds leaks nothing anyone can observe.
        std::mem::forget(window);
        std::mem::forget(platform);

        Some(RenderContext3D::new(gl))
    }

    #[test]
    fn destroying_a_resource_really_deletes_its_gl_objects() {
        let Some(mut render) = headless_renderer() else {
            eprintln!("skipped: no OpenGL ES driver available");
            return;
        };

        // Everything is created and *then* destroyed, so that all the assertions below run
        // against the same driver state. Asserting straight after each destroy would stop
        // at the first failure and leave the other paths — `release_all` in particular —
        // unverified.
        let explicit_mesh = render.create_cube_mesh(1.0).expect("cube");
        let explicit_shader = render
            .create_builtin_shader(crate::shader::BuiltinShader::Flat)
            .expect("flat");
        let bulk_mesh = render.create_cube_mesh(2.0).expect("cube");
        let bulk_shader = render
            .create_builtin_shader(crate::shader::BuiltinShader::Phong)
            .expect("phong");

        // The test module lives inside this module, so it reads the registries directly;
        // no test-only accessor on the public type is needed.
        let (_, explicit_vbo, explicit_ebo) =
            render.meshes.get(explicit_mesh).expect("mesh").gl_names();
        let explicit_program = render
            .shaders
            .get(explicit_shader)
            .expect("shader")
            .gl_name();
        let (_, bulk_vbo, _) = render.meshes.get(bulk_mesh).expect("mesh").gl_names();
        let bulk_program = render.shaders.get(bulk_shader).expect("shader").gl_name();

        // SAFETY: the context is current and every name was alive a moment ago.
        unsafe {
            assert!(
                render.glow().is_buffer(explicit_vbo),
                "VBO missing before destroy"
            );
            assert!(
                render.glow().is_program(explicit_program),
                "program missing before destroy"
            );
        }

        // ---- explicit destroy, one resource at a time -------------------------
        assert!(render.destroy_mesh(explicit_mesh));
        assert!(render.destroy_shader(explicit_shader));

        // ---- release_all, which goes through the same two calls ---------------
        // It inherited the same leak, so both paths are asserted together and a future
        // fix cannot repair one and leave the other.
        assert!(render.release_all());
        assert_eq!(render.mesh_count(), 0);
        assert_eq!(render.shader_count(), 0);

        // SAFETY: the context is current. `take` removed the registry's reference and the
        // `Rc` was unwrapped, so the driver deleted the names; GLES answers `false` for a
        // deleted name rather than raising an error.
        unsafe {
            assert!(
                !render.glow().is_buffer(explicit_vbo),
                "the VBO survived destroy_mesh: GL objects are leaking"
            );
            assert!(
                !render.glow().is_buffer(explicit_ebo),
                "the EBO survived destroy_mesh: GL objects are leaking"
            );
            assert!(
                !render.glow().is_program(explicit_program),
                "the program survived destroy_shader: GL objects are leaking"
            );
            assert!(
                !render.glow().is_buffer(bulk_vbo),
                "release_all leaked a VBO"
            );
            assert!(
                !render.glow().is_program(bulk_program),
                "release_all leaked a program"
            );
        }

        // ---- release_all must not claim success while something is referenced --------
        // It used to answer "are the registries empty", a different question: a caller
        // holding a clone made `destroy_*` log a leak while the registry emptied, so the
        // function returned `true` about a cleanup that had not happened.
        let held = render.create_cube_mesh(3.0).expect("cube");
        let clone = render.meshes.get(held).expect("mesh");
        assert_eq!(std::rc::Rc::strong_count(&clone), 2);

        assert!(
            !render.release_all(),
            "release_all must not report success while an object is still referenced"
        );
        assert_eq!(
            render.mesh_count(),
            0,
            "the registry entry is gone even though the object leaked"
        );

        // Dropping the clone is what lets the objects go; the leak was real until here.
        drop(clone);
    }
}
