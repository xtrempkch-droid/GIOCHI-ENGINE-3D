//! GLSL program compilation and uniforms.
//!
//! Two rules from the 3D extension are enforced here:
//!
//! * **R10** — shader source lives in files, never as a string literal inside
//!   `renderer.rs`. The built-ins are `include_str!`ed from `shaders/`, user shaders are
//!   loaded from a path.
//! * **Golden rule 5** — a compilation failure always carries the driver's complete
//!   `glGetShaderInfoLog`/`glGetProgramInfoLog`. Without it, debugging a shader is
//!   guesswork, and GLES drivers differ enough that the log is the only portable clue.

use std::cell::RefCell;
use std::collections::HashMap;

use glow::HasContext;

use crate::error::{Error3D, Result3D};
use crate::math3d::{Color3D, Mat4, Vec3};

/// Attribute slot for `a_position`.
pub const ATTRIB_POSITION: u32 = 0;
/// Attribute slot for `a_normal`.
pub const ATTRIB_NORMAL: u32 = 1;
/// Attribute slot for `a_uv`.
pub const ATTRIB_UV: u32 = 2;

/// First of the four attribute slots that carry a per-instance matrix.
///
/// Slots 3, 4, 5 and 6 are the columns of one `mat4`. They live in the same fixed
/// numbering as the mesh attributes so an instanced program gets the same matrix
/// columns on every driver, and so a VAO can be reasoned about by inspection.
pub const ATTRIB_INSTANCE_MODEL: u32 = 3;

/// A compiled, linked GLSL program with cached uniform locations.
pub struct Program {
    handle: glow::Program,
    /// Locations are cached behind a `RefCell` so a uniform can be set through a
    /// shared reference: the handle registry hands out `Rc<Program>`, and this is
    /// the only interior mutability the type needs.
    uniforms: RefCell<HashMap<String, Option<glow::UniformLocation>>>,
    /// Values written through the setters, kept only while [`Program::set_capture`] is
    /// on.
    ///
    /// Recording is off by default because every setter runs once per uniform per draw,
    /// and a hash insert on that path is exactly the kind of cost phase 18 measured. The
    /// hot reloader turns it on for the programs it manages: there the point is to
    /// re-apply a constant (`u_tint`, `u_bias`) after a recompile, which happens once per
    /// edit rather than once per frame.
    retained: RefCell<HashMap<String, RetainedUniform>>,
    /// Whether the setters record into `retained`.
    capture: std::cell::Cell<bool>,
    /// Kept for diagnostics: the driver reports which shader failed by object, and the
    /// source is what a human needs to see.
    vertex_source: String,
    fragment_source: String,
    /// Whether the vertex shader declares the per-instance matrix attributes.
    ///
    /// Asked of the driver after linking rather than inferred from the source text: a
    /// shader that declares `a_model_0` but never reads it is optimised away, and the
    /// answer we need is "will this program accept the attributes", not "does the file
    /// mention them".
    instanced: bool,
}

/// A uniform value recorded by a [`Program`] setter while capture is on.
#[derive(Debug, Clone)]
enum RetainedUniform {
    Float(f32),
    Vec3([f32; 3]),
    Color([f32; 4]),
    Matrix([f32; 16]),
    /// The texture object, not a handle: it is what `bind_texture` needs to replay.
    Texture {
        texture: glow::Texture,
        unit: u32,
    },
}

impl std::fmt::Debug for Program {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Program")
            .field("uniforms", &self.uniforms.borrow().len())
            .finish_non_exhaustive()
    }
}

impl Program {
    /// Compiles and links a vertex/fragment pair.
    ///
    /// # Errors
    ///
    /// [`Error3D::Shader`] with the driver's full log when compilation or linking
    /// fails. The log includes the stage and the offending source line the driver
    /// reported, which is why the sources are kept on the struct.
    pub fn from_source(
        gl: &glow::Context,
        vertex_source: &str,
        fragment_source: &str,
    ) -> Result3D<Self> {
        let program = Self::compile(gl, vertex_source, fragment_source)?;

        // SAFETY: the program is linked and current-context; `get_attrib_location`
        // returns -1 for a name the program does not use.
        let instanced = unsafe {
            gl.get_attrib_location(program, "a_model_0").is_some()
                && gl.get_attrib_location(program, "a_model_3").is_some()
        };

        Ok(Self {
            handle: program,
            uniforms: RefCell::new(HashMap::new()),
            retained: RefCell::new(HashMap::new()),
            capture: std::cell::Cell::new(false),
            vertex_source: vertex_source.to_owned(),
            fragment_source: fragment_source.to_owned(),
            instanced,
        })
    }

    fn compile(
        gl: &glow::Context,
        vertex_source: &str,
        fragment_source: &str,
    ) -> Result3D<glow::Program> {
        // SAFETY: every call below is core GLES 2.0 on the current context. The shader
        // objects are deleted on every exit path; the program object only survives on
        // success.
        unsafe {
            let program = gl
                .create_program()
                .map_err(|err| Error3D::GlObject(format!("glCreateProgram failed: {err}")))?;

            let mut shaders = Vec::with_capacity(2);
            for (stage, source) in [("vertex", vertex_source), ("fragment", fragment_source)] {
                let shader = match compile_stage(gl, stage, source) {
                    Ok(shader) => shader,
                    Err(error) => {
                        for shader in shaders {
                            gl.delete_shader(shader);
                        }
                        gl.delete_program(program);
                        return Err(error);
                    }
                };
                gl.attach_shader(program, shader);
                shaders.push(shader);
            }

            // Attribute locations are bound explicitly *before* linking. GLES 2.0 has
            // no `layout(location = N)` qualifier, and without this the driver assigns
            // whatever index it likes per program — so a vertex array object built once
            // for mesh+program A would feed the wrong slots to program B. Fixed
            // locations make the attribute setup independent of the program, which is
            // what lets a VAO be created once per mesh (see `mesh::Mesh`).
            gl.bind_attrib_location(program, ATTRIB_POSITION, "a_position");
            gl.bind_attrib_location(program, ATTRIB_NORMAL, "a_normal");
            gl.bind_attrib_location(program, ATTRIB_UV, "a_uv");
            // Only the instanced shader declares these. Binding a name a program does
            // not have is a no-op in GLES, so the binding is unconditional and the
            // attribute numbering stays one table rather than two.
            for (offset, name) in ["a_model_0", "a_model_1", "a_model_2", "a_model_3"]
                .iter()
                .enumerate()
            {
                gl.bind_attrib_location(program, ATTRIB_INSTANCE_MODEL + offset as u32, name);
            }

            gl.link_program(program);

            if !gl.get_program_link_status(program) {
                let log = gl.get_program_info_log(program);
                for shader in shaders {
                    gl.detach_shader(program, shader);
                    gl.delete_shader(shader);
                }
                gl.delete_program(program);
                return Err(Error3D::Shader {
                    stage: "link",
                    log: format!("{log}\n\n--- vertex ---\n{vertex_source}\n--- fragment ---\n{fragment_source}"),
                });
            }

            // Shaders can be detached once linked; the program keeps what it needs.
            for shader in shaders {
                gl.detach_shader(program, shader);
                gl.delete_shader(shader);
            }

            Ok(program)
        }
    }

    /// Whether this program takes its model matrix from per-instance attributes.
    pub fn is_instanced(&self) -> bool {
        self.instanced
    }

    /// Whether this program has a `u_model` uniform.
    ///
    /// The instanced shader deliberately does not, so that forgetting the instance
    /// matrix is a link-time error rather than a scene drawn at the origin.
    pub fn has_model_uniform(&self, gl: &glow::Context) -> bool {
        self.location(gl, "u_model").is_some()
    }

    /// Makes this program current.
    pub fn bind(&self, gl: &glow::Context) {
        // SAFETY: core GLES 2.0, current context.
        unsafe { gl.use_program(Some(self.handle)) };
    }

    /// Returns the location of `name`, caching the lookup.
    ///
    /// A missing uniform is not an error: the driver optimises out uniforms a shader
    /// does not read, and a caller setting `u_unused` should not fail a frame. `None`
    /// is cached too, so the driver is asked only once.
    fn location(&self, gl: &glow::Context, name: &str) -> Option<glow::UniformLocation> {
        if let Some(cached) = self.uniforms.borrow().get(name) {
            return *cached;
        }

        // SAFETY: `get_uniform_location` is core GLES 2.0 and returns `None` for an
        // unknown name.
        let location = unsafe { gl.get_uniform_location(self.handle, name) };
        self.uniforms.borrow_mut().insert(name.to_owned(), location);
        location
    }

    /// Uploads a `mat4`.
    ///
    /// `transpose = false`: [`Mat4`] is column-major, which is what GL expects. Passing
    /// `true` here would work only if the storage were row-major, and the mixup is the
    /// classic "my model is inside-out" bug.
    pub fn set_mat4(&self, gl: &glow::Context, name: &str, value: &Mat4) {
        if let Some(location) = self.location(gl, name) {
            let columns = value.to_cols_array();
            // SAFETY: 16 floats, column-major, matching the uniform type.
            unsafe {
                gl.uniform_matrix_4_f32_slice(Some(&location), false, &columns);
            }
            self.retain(name, RetainedUniform::Matrix(columns));
        }
    }

    /// Uploads a `vec3`.
    pub fn set_vec3(&self, gl: &glow::Context, name: &str, value: Vec3) {
        if let Some(location) = self.location(gl, name) {
            // SAFETY: 3 floats, matching the uniform type.
            unsafe { gl.uniform_3_f32(Some(&location), value.x, value.y, value.z) };
            self.retain(name, RetainedUniform::Vec3([value.x, value.y, value.z]));
        }
    }

    /// Uploads an `f32`.
    pub fn set_f32(&self, gl: &glow::Context, name: &str, value: f32) {
        if let Some(location) = self.location(gl, name) {
            // SAFETY: one float, matching the uniform type.
            unsafe { gl.uniform_1_f32(Some(&location), value) };
            self.retain(name, RetainedUniform::Float(value));
        }
    }

    /// Uploads a colour as a `vec4`.
    pub fn set_color(&self, gl: &glow::Context, name: &str, color: Color3D) {
        if let Some(location) = self.location(gl, name) {
            let [r, g, b, a] = color.to_array();
            // SAFETY: 4 floats, matching the uniform type.
            unsafe { gl.uniform_4_f32(Some(&location), r, g, b, a) };
            self.retain(name, RetainedUniform::Color([r, g, b, a]));
        }
    }

    /// Binds `texture` to `unit` and points the sampler uniform at it.
    ///
    /// The sampler must be set to the same unit number that the texture is bound to;
    /// setting one without the other is a black texture with no error.
    pub fn set_texture(&self, gl: &glow::Context, name: &str, texture: glow::Texture, unit: u32) {
        // SAFETY: `active_texture` and `bind_texture` are core GLES 2.0; `unit` is
        // clamped by the caller to the implementation's limit.
        unsafe {
            gl.active_texture(glow::TEXTURE0 + unit);
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
        }

        if let Some(location) = self.location(gl, name) {
            // SAFETY: a sampler uniform takes one integer, the texture unit index.
            unsafe { gl.uniform_1_i32(Some(&location), unit as i32) };
            self.retain(name, RetainedUniform::Texture { texture, unit });
        }
    }

    /// The raw GL program name.
    ///
    /// Crate-internal: used by the regression test that proves the program was deleted,
    /// and by the upscale pass, which resolves its own attribute locations by name
    /// because it shares no attribute numbering with the mesh pipeline.
    pub(crate) fn gl_name(&self) -> glow::Program {
        self.handle
    }
    /// Turns uniform recording on or off.
    ///
    /// Off by default. On means every setter keeps the value it just uploaded, so
    /// [`Program::apply_retained`] can replay it into a recompiled program. The hot
    /// reloader enables it for the programs it manages; nothing else should, because the
    /// recording costs a hash write per uniform per draw.
    pub fn set_capture(&self, on: bool) {
        self.capture.set(on);
    }

    /// Whether the setters are recording values.
    pub fn capture_enabled(&self) -> bool {
        self.capture.get()
    }

    /// How many uniform values are recorded.
    pub fn retained_count(&self) -> usize {
        self.retained.borrow().len()
    }

    /// Re-applies every recorded uniform to this program.
    ///
    /// Called on a **freshly linked** program after a reload. Textures are re-bound as
    /// well as re-pointed: a sampler without its texture unit is a black surface with no
    /// GL error, which is the failure mode this exists to avoid.
    ///
    /// Returns how many values were applied. `0` is normal for a program whose uniforms
    /// are all written by the draw call itself.
    pub fn apply_retained(&self, gl: &glow::Context) -> usize {
        let retained = self.retained.borrow();
        for (name, value) in retained.iter() {
            let Some(location) = self.location(gl, name) else {
                // The recompiled shader no longer reads this uniform; re-applying is
                // impossible and harmless.
                log::debug!("uniform '{name}' is gone after the reload");
                continue;
            };
            match value {
                RetainedUniform::Float(value) => {
                    // SAFETY: one float, matching the uniform type.
                    unsafe { gl.uniform_1_f32(Some(&location), *value) };
                }
                RetainedUniform::Vec3(value) => {
                    // SAFETY: 3 floats, matching the uniform type.
                    unsafe { gl.uniform_3_f32(Some(&location), value[0], value[1], value[2]) };
                }
                RetainedUniform::Color(value) => {
                    // SAFETY: 4 floats, matching the uniform type.
                    unsafe {
                        gl.uniform_4_f32(Some(&location), value[0], value[1], value[2], value[3]);
                    }
                }
                RetainedUniform::Matrix(value) => {
                    // SAFETY: 16 floats, column-major, matching the uniform type.
                    unsafe { gl.uniform_matrix_4_f32_slice(Some(&location), false, value) };
                }
                RetainedUniform::Texture { texture, unit } => {
                    // SAFETY: core GLES 2.0; the object outlives the program (textures
                    // belong to the render context, not to a shader).
                    unsafe {
                        gl.active_texture(glow::TEXTURE0 + *unit);
                        gl.bind_texture(glow::TEXTURE_2D, Some(*texture));
                        gl.uniform_1_i32(Some(&location), *unit as i32);
                    }
                }
            }
        }
        retained.len()
    }

    /// Copies the recording state and retained values from `previous` into `self`.
    ///
    /// Used by a reload: the new program is linked first, then told everything the old
    /// one knew, then asked to apply it. Doing it in this order means a compile error
    /// leaves the old program — and its state — untouched.
    pub(crate) fn inherit_retained(&self, previous: &Program) {
        self.capture.set(previous.capture.get());
        if !previous.capture.get() {
            return;
        }
        let inherited: HashMap<String, RetainedUniform> = previous
            .retained
            .borrow()
            .iter()
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect();
        *self.retained.borrow_mut() = inherited;
    }

    /// Records `value` under `name`, when capture is on.
    fn retain(&self, name: &str, value: RetainedUniform) {
        if !self.capture.get() {
            return;
        }
        self.retained.borrow_mut().insert(name.to_owned(), value);
    }

    /// The vertex source this program was built from, for diagnostics.
    pub fn vertex_source(&self) -> &str {
        &self.vertex_source
    }

    /// The fragment source this program was built from, for diagnostics.
    pub fn fragment_source(&self) -> &str {
        &self.fragment_source
    }
}

impl Program {
    /// Deletes the GL program.
    ///
    /// Explicit rather than `Drop`: deleting a GL object needs the context that
    /// created it, and a `Drop` impl has no access to one. Dropping without calling
    /// this leaks exactly one GL object, which is why programs are only reachable
    /// through [`RenderContext3D`](crate::RenderContext3D)'s registry — every path a
    /// caller can take goes through `destroy_shader`, which calls this.
    pub fn destroy(self, gl: &glow::Context) {
        // SAFETY: `gl` is the context that created the handle. `self` is consumed, so
        // the handle cannot be deleted twice.
        unsafe { gl.delete_program(self.handle) };
    }
}

fn compile_stage(gl: &glow::Context, stage: &'static str, source: &str) -> Result3D<glow::Shader> {
    // SAFETY: core GLES 2.0 on the current context; the shader is deleted by the
    // caller on failure and after linking on success.
    unsafe {
        let kind = if stage == "vertex" {
            glow::VERTEX_SHADER
        } else {
            glow::FRAGMENT_SHADER
        };

        let shader = gl
            .create_shader(kind)
            .map_err(|err| Error3D::GlObject(format!("glCreateShader({stage}) failed: {err}")))?;

        gl.shader_source(shader, source);
        gl.compile_shader(shader);

        if !gl.get_shader_compile_status(shader) {
            let log = gl.get_shader_info_log(shader);
            gl.delete_shader(shader);
            return Err(Error3D::Shader {
                stage,
                log: format!("{log}\n\n--- source ---\n{source}"),
            });
        }

        Ok(shader)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shader_error_names_the_stage_and_keeps_the_log() {
        // The message shape is relied on for support: it must name the stage and keep
        // the driver text verbatim.
        let error = Error3D::Shader {
            stage: "fragment",
            log: "0:4(12): error: `undefined_thing` undeclared".to_owned(),
        };
        let text = error.to_string();

        assert!(text.contains("fragment"), "{text}");
        assert!(text.contains("undeclared"), "{text}");
    }
}
