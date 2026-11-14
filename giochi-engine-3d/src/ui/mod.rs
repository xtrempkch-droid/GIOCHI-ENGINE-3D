//! A HUD over the 3D scene: `egui` tessellated onto GLES 2.0.
//!
//! This is the phase-21 pendency. `giochi-engine/src/ui` draws egui with `sdl2::render`,
//! which the 3D path deliberately does not use (ADR-037), so a HUD over the 3D scene needs
//! its own painter: same `egui::Context`, different backend.
//!
//! ```no_run
//! # use giochi_engine_3d::ui::GlesHud;
//! # fn demo(gl: &glow::Context) -> Result<(), Box<dyn std::error::Error>> {
//! let mut hud = GlesHud::new(gl)?;
//! hud.draw(gl, (1280, 720), 1.0 / 60.0, &mut |ctx| {
//!     egui::Window::new("hud").show(ctx, |ui| ui.label("120 fps"));
//! })?;
//! hud.destroy(gl);
//! # Ok(())
//! # }
//! ```
//!
//! # Why the painter is small
//!
//! egui hands over triangles with positions in *points* and premultiplied sRGB colours.
//! That maps onto GLES 2.0 with three vertex attributes and one `texture2D` sample, so the
//! whole backend is a shader pair, two buffers and a `glScissor` per draw. Two things do
//! need care:
//!
//! * **No `u32` indices in GLES 2.0** without `OES_element_index_uint`. `Mesh::split_to_u16`
//!   is what makes a text-heavy HUD drawable on a console that lacks the extension.
//! * **No VAOs in GLES 2.0** either. The attribute pointers are set on every chunk, which
//!   is a handful of `vertexAttribPointer` calls per draw and is not measurable at HUD
//!   sizes.
//!
//! # What it deliberately does not do
//!
//! There is no input translation yet: the HUD is display-only (`mini_fps` shows fps,
//! position and physics state). Pointer/keyboard translation for egui lives in
//! `giochi-engine/src/ui/egui_integration.rs` and would have to be lifted into a shared
//! crate to be reused here; until a caller needs to click something, that is not worth a
//! dependency of the 3D crate on `giochi-engine`'s input types.

use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use glow::HasContext;

use crate::error::{Error3D, Result3D};
use crate::shader::Program;

const VERTEX_SHADER: &str = "\
attribute vec2 a_pos;
attribute vec2 a_uv;
attribute vec4 a_color;
uniform vec2 u_screen;
varying vec2 v_uv;
varying vec4 v_color;
void main() {
    // Points come in with (0,0) at the top left; GL clip space has (0,0) in the middle
    // and +y up, so the y is flipped here and nowhere else.
    vec2 clip = (a_pos / u_screen) * 2.0 - 1.0;
    gl_Position = vec4(clip.x, -clip.y, 0.0, 1.0);
    v_uv = a_uv;
    v_color = a_color;
}
";

const FRAGMENT_SHADER: &str = "\
precision mediump float;
uniform sampler2D u_texture;
varying vec2 v_uv;
varying vec4 v_color;
void main() {
    // `v_color` is premultiplied sRGB, which is why the blend is (ONE, ONE_MINUS_SRC_ALPHA)
    // rather than (SRC_ALPHA, ONE_MINUS_SRC_ALPHA).
    gl_FragColor = v_color * texture2D(u_texture, v_uv);
}
";

/// One vertex as GLES 2.0 sees it: two floats of position, two of texture coordinate and
/// four premultiplied colour bytes. 20 bytes, no padding.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
struct HudVertex {
    position: [f32; 2],
    uv: [f32; 2],
    colour: [u8; 4],
}

/// An `egui` HUD painted with GLES 2.0, for the 3D renderer.
///
/// Owns egui's context, the font atlas (and every other texture egui asks for) and the
/// shader and buffers one frame needs. Create it once, call [`GlesHud::draw`] once per
/// frame after the scene, and [`GlesHud::destroy`] with the context still current.
pub struct GlesHud {
    ctx: egui::Context,
    program: Program,
    vbo: glow::Buffer,
    ibo: glow::Buffer,
    screen_uniform: Option<glow::UniformLocation>,
    texture_uniform: Option<glow::UniformLocation>,
    pos_attribute: u32,
    uv_attribute: u32,
    colour_attribute: u32,
    textures: HashMap<egui::TextureId, glow::Texture>,
    time: f64,
    vertices: Vec<HudVertex>,
    indices: Vec<u16>,
}

impl std::fmt::Debug for GlesHud {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GlesHud")
            .field("textures", &self.textures.len())
            .field("time", &self.time)
            .finish_non_exhaustive()
    }
}

impl GlesHud {
    /// Compiles the painter's program and creates its buffers.
    ///
    /// The GL context must be current. The font atlas is not uploaded here: egui produces
    /// it on the first frame, and the delta is applied by [`GlesHud::draw`].
    pub fn new(gl: &glow::Context) -> Result3D<GlesHud> {
        let program = Program::from_source(gl, VERTEX_SHADER, FRAGMENT_SHADER)?;
        // SAFETY: core GLES 2.0 on the current context; every failure path deletes what it
        // created.
        unsafe {
            let vbo = gl.create_buffer().map_err(|error| {
                Error3D::GlObject(format!("glGenBuffers (vbo) failed: {error}"))
            })?;
            let ibo = match gl.create_buffer() {
                Ok(ibo) => ibo,
                Err(error) => {
                    gl.delete_buffer(vbo);
                    return Err(Error3D::GlObject(format!(
                        "glGenBuffers (ibo) failed: {error}"
                    )));
                }
            };
            let name = program.gl_name();
            let pos_attribute = gl.get_attrib_location(name, "a_pos").ok_or_else(|| {
                Error3D::GlObject("the HUD shader has no a_pos attribute".to_owned())
            })?;
            let uv_attribute = gl.get_attrib_location(name, "a_uv").ok_or_else(|| {
                Error3D::GlObject("the HUD shader has no a_uv attribute".to_owned())
            })?;
            let colour_attribute = gl.get_attrib_location(name, "a_color").ok_or_else(|| {
                Error3D::GlObject("the HUD shader has no a_color attribute".to_owned())
            })?;

            Ok(GlesHud {
                ctx: egui::Context::default(),
                program,
                vbo,
                ibo,
                screen_uniform: gl.get_uniform_location(name, "u_screen"),
                texture_uniform: gl.get_uniform_location(name, "u_texture"),
                pos_attribute,
                uv_attribute,
                colour_attribute,
                textures: HashMap::new(),
                time: 0.0,
                vertices: Vec::new(),
                indices: Vec::new(),
            })
        }
    }

    /// How many textures egui has uploaded (the font atlas, plus any image a widget used).
    pub fn texture_count(&self) -> usize {
        self.textures.len()
    }

    /// The egui context, for callers that want to build state outside the draw closure.
    pub fn context(&self) -> &egui::Context {
        &self.ctx
    }

    /// Runs one egui frame and draws it, returning how many draw calls it took.
    ///
    /// `screen` is in physical pixels. Call it *after* the scene and before the frame is
    /// presented; it restores the GL state it changes, so the caller can keep drawing.
    pub fn draw(
        &mut self,
        gl: &glow::Context,
        screen: (u32, u32),
        delta_seconds: f32,
        ui: &mut dyn FnMut(&egui::Context),
    ) -> Result3D<usize> {
        if screen.0 == 0 || screen.1 == 0 {
            return Ok(0);
        }
        self.time += f64::from(delta_seconds.max(0.0));

        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(screen.0 as f32, screen.1 as f32),
            )),
            time: Some(self.time),
            ..egui::RawInput::default()
        };
        let output = self.ctx.run(input, |ctx| ui(ctx));

        for (id, delta) in &output.textures_delta.set {
            match &delta.image {
                egui::ImageData::Color(image) => self.upload_texture(gl, *id, image),
            }
        }

        let primitives = self.ctx.tessellate(output.shapes, output.pixels_per_point);
        let draws = self.paint(gl, screen, &primitives, output.pixels_per_point)?;

        for id in &output.textures_delta.free {
            if let Some(texture) = self.textures.remove(id) {
                // SAFETY: created by `upload_texture` on this context and removed from the
                // map, so it is deleted exactly once.
                unsafe { gl.delete_texture(texture) };
            }
        }
        Ok(draws)
    }

    fn upload_texture(
        &mut self,
        gl: &glow::Context,
        id: egui::TextureId,
        image: &egui::ColorImage,
    ) {
        let width = image.size[0] as i32;
        let height = image.size[1] as i32;
        if width == 0 || height == 0 {
            return;
        }
        let mut pixels = Vec::with_capacity(image.pixels.len() * 4);
        for pixel in &image.pixels {
            pixels.extend_from_slice(&pixel.to_array());
        }

        // SAFETY: core GLES 2.0. `pixels` is tightly packed RGBA8 and the row length
        // matches the width, so `UNPACK_ALIGNMENT` of 1 (set by the renderer) is enough.
        unsafe {
            let texture = match self.textures.get(&id) {
                Some(texture) => *texture,
                None => match gl.create_texture() {
                    Ok(texture) => {
                        gl.bind_texture(glow::TEXTURE_2D, Some(texture));
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
                        self.textures.insert(id, texture);
                        texture
                    }
                    Err(error) => {
                        log::error!("HUD texture creation failed: {error}");
                        return;
                    }
                },
            };
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA as i32,
                width,
                height,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(&pixels)),
            );
        }
    }

    fn paint(
        &mut self,
        gl: &glow::Context,
        screen: (u32, u32),
        primitives: &[egui::ClippedPrimitive],
        pixels_per_point: f32,
    ) -> Result3D<usize> {
        let mut draws = 0;
        // SAFETY: every call below is core GLES 2.0 with objects this type owns. The state
        // changes are undone at the end of the function.
        unsafe {
            gl.enable(glow::BLEND);
            gl.blend_equation(glow::FUNC_ADD);
            gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA);
            gl.disable(glow::DEPTH_TEST);
            gl.disable(glow::CULL_FACE);
            gl.enable(glow::SCISSOR_TEST);
            gl.active_texture(glow::TEXTURE0);
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo));
            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(self.ibo));

            self.program.bind(gl);
            if let Some(location) = &self.screen_uniform {
                gl.uniform_2_f32(Some(location), screen.0 as f32, screen.1 as f32);
            }
            if let Some(location) = &self.texture_uniform {
                gl.uniform_1_i32(Some(location), 0);
            }
            gl.enable_vertex_attrib_array(self.pos_attribute);
            gl.enable_vertex_attrib_array(self.uv_attribute);
            gl.enable_vertex_attrib_array(self.colour_attribute);
            let stride = std::mem::size_of::<HudVertex>() as i32;
            gl.vertex_attrib_pointer_f32(self.pos_attribute, 2, glow::FLOAT, false, stride, 0);
            gl.vertex_attrib_pointer_f32(self.uv_attribute, 2, glow::FLOAT, false, stride, 8);
            gl.vertex_attrib_pointer_f32(
                self.colour_attribute,
                4,
                glow::UNSIGNED_BYTE,
                true,
                stride,
                16,
            );

            for primitive in primitives {
                let egui::epaint::Primitive::Mesh(mesh) = &primitive.primitive else {
                    continue;
                };
                let Some(texture) = self.textures.get(&mesh.texture_id) else {
                    continue;
                };
                gl.bind_texture(glow::TEXTURE_2D, Some(*texture));
                self.set_scissor(gl, primitive.clip_rect, screen, pixels_per_point);

                // GLES 2.0 without `OES_element_index_uint` cannot take `u32` indices.
                for chunk in mesh.clone().split_to_u16() {
                    self.vertices.clear();
                    self.vertices
                        .extend(chunk.vertices.iter().map(|vertex| HudVertex {
                            position: [vertex.pos.x, vertex.pos.y],
                            uv: [vertex.uv.x, vertex.uv.y],
                            colour: vertex.color.to_array(),
                        }));
                    self.indices.clear();
                    self.indices.extend_from_slice(&chunk.indices);

                    gl.buffer_data_u8_slice(
                        glow::ARRAY_BUFFER,
                        bytemuck::cast_slice(&self.vertices),
                        glow::STREAM_DRAW,
                    );
                    gl.buffer_data_u8_slice(
                        glow::ELEMENT_ARRAY_BUFFER,
                        bytemuck::cast_slice(&self.indices),
                        glow::STREAM_DRAW,
                    );
                    gl.draw_elements(
                        glow::TRIANGLES,
                        self.indices.len() as i32,
                        glow::UNSIGNED_SHORT,
                        0,
                    );
                    draws += 1;
                }
            }

            gl.disable_vertex_attrib_array(self.pos_attribute);
            gl.disable_vertex_attrib_array(self.uv_attribute);
            gl.disable_vertex_attrib_array(self.colour_attribute);
            gl.disable(glow::SCISSOR_TEST);
            gl.enable(glow::DEPTH_TEST);
            gl.enable(glow::CULL_FACE);
            gl.disable(glow::BLEND);
        }
        Ok(draws)
    }

    /// Sets the scissor rectangle for egui's clip rect.
    ///
    /// egui measures from the top left in points; `glScissor` measures from the bottom
    /// left in pixels, so the y has to be flipped and both axes scaled.
    fn set_scissor(
        &self,
        gl: &glow::Context,
        clip: egui::Rect,
        screen: (u32, u32),
        pixels_per_point: f32,
    ) {
        let scale = if pixels_per_point.is_finite() && pixels_per_point > 0.0 {
            pixels_per_point
        } else {
            1.0
        };
        let left = (clip.min.x * scale).floor().max(0.0) as i32;
        let top = (clip.min.y * scale).floor().max(0.0) as i32;
        let right = ((clip.max.x * scale).ceil() as i32).min(screen.0 as i32);
        let bottom = ((clip.max.y * scale).ceil() as i32).min(screen.1 as i32);
        let width = (right - left).max(0);
        let height = (bottom - top).max(0);
        let flipped = (screen.1 as i32 - bottom).max(0);
        // SAFETY: `glScissor` with a non-negative rectangle inside the framebuffer or
        // outside it (which simply clips everything).
        unsafe { gl.scissor(left, flipped, width, height) };
    }

    /// Deletes the buffers, the program and every texture egui uploaded.
    pub fn destroy(self, gl: &glow::Context) {
        // SAFETY: all objects were created by this type on the current context and are
        // dropped here exactly once.
        unsafe {
            for (_, texture) in self.textures {
                gl.delete_texture(texture);
            }
            gl.delete_buffer(self.vbo);
            gl.delete_buffer(self.ibo);
        }
        self.program.destroy(gl);
    }
}
