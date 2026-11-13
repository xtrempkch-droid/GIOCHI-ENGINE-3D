//! GL textures for meshes.
//!
//! The vertex format has carried a `uv` attribute since phase 11 and
//! [`BuiltinShader::Textured`](crate::shader::BuiltinShader::Textured) samples
//! `u_texture`, but there was no way for a caller to *make* a texture: every draw
//! bound none, and the shader sampled unit 0 where nothing was bound. This module is
//! the missing half, so `uv` has something to index.
//!
//! # Why SDL_image and not a PNG crate
//!
//! The 3D crate already depends on SDL2 for the window and input, and the workspace
//! enables SDL2's `image` feature for the 2D path. Decoding to a software `Surface`
//! and uploading the pixels is the same work a PNG crate would do, with no new
//! dependency and the same format support (PNG, JPEG, BMP, …) the 2D textures have.
//! The 2D `TextureRegistry` cannot be reused: its textures belong to a
//! `sdl2::render` canvas, and this crate draws with raw GLES (ADR-037).

use std::path::Path;

use glow::HasContext;
use sdl2::image::LoadSurface;
use sdl2::pixels::PixelFormatEnum;
use sdl2::surface::Surface;

use crate::error::{Error3D, Result3D};

/// A GL texture owned by a [`RenderContext3D`](super::RenderContext3D).
///
/// Delete it through the renderer, never directly: the texture belongs to the GL
/// context, and a caller that drops this value without [`RenderContext3D::destroy_texture`]
/// leaks it exactly as it would a mesh.
pub struct Texture3D {
    handle: glow::Texture,
    width: u32,
    height: u32,
}

impl std::fmt::Debug for Texture3D {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Texture3D")
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

impl Texture3D {
    /// Decodes an image file and uploads it.
    ///
    /// The format is sniffed by SDL_image, so PNG, JPEG and BMP all work.
    pub fn from_file(gl: &glow::Context, path: &Path) -> Result3D<Self> {
        if path.as_os_str().is_empty() {
            return Err(Error3D::InvalidArgument("texture path is empty".to_owned()));
        }

        let surface = Surface::from_file(path).map_err(|err| {
            Error3D::Asset(format!(
                "failed to load texture '{}': {err}",
                path.display()
            ))
        })?;

        // RGBA32 is what both the upload path and `create_texture_from_rgba` use.
        let surface = surface
            .convert_format(PixelFormatEnum::RGBA32)
            .map_err(|err| Error3D::Asset(format!("failed to convert texture format: {err}")))?;

        let (width, height) = (surface.width(), surface.height());
        let pitch = surface.pitch() as usize;
        let row_bytes = width as usize * 4;

        // `glTexImage2D` has no pitch parameter, so a surface whose rows are padded
        // (which RGBA32 can be for odd widths) has to be copied row by row. Feeding
        // the locked slice straight in would shear the image.
        let mut pixels = Vec::with_capacity(row_bytes * height as usize);
        let source = surface
            .without_lock()
            .ok_or_else(|| Error3D::Asset("texture surface is locked".to_owned()))?;
        for row in 0..height as usize {
            let start = row * pitch;
            pixels.extend_from_slice(&source[start..start + row_bytes]);
        }

        Self::from_rgba8(gl, width, height, &pixels)
    }

    /// Uploads raw RGBA8 pixels.
    ///
    /// `pixels` must be exactly `width * height * 4` bytes. This is the path a
    /// procedural texture (a checkerboard, a gradient, a font atlas) uses, and it lets
    /// a test build a texture without touching the disk.
    pub fn from_rgba8(
        gl: &glow::Context,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result3D<Self> {
        if width == 0 || height == 0 {
            return Err(Error3D::InvalidArgument(
                "texture width and height must be non-zero".to_owned(),
            ));
        }
        let Some(expected) = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(4))
        else {
            return Err(Error3D::InvalidArgument(
                "texture dimensions overflow the address space".to_owned(),
            ));
        };
        if pixels.len() != expected {
            return Err(Error3D::InvalidArgument(format!(
                "pixel buffer must be exactly width * height * 4 = {expected} bytes, got {}",
                pixels.len()
            )));
        }

        // SAFETY: the context is current and the pixel slice is exactly the size
        // computed above, which is what `RGBA`/`UNSIGNED_BYTE` reads.
        unsafe {
            let handle = gl
                .create_texture()
                .map_err(|err| Error3D::GlObject(format!("could not create a texture: {err}")))?;
            gl.bind_texture(glow::TEXTURE_2D, Some(handle));

            // GLES 2.0 only allows mipmaps and `GL_REPEAT` on power-of-two textures.
            // A non-power-of-two image (which every odd-sized sprite atlas is) would
            // render black or sample garbage with either, so the two cases get
            // different parameters rather than the desktop-GL defaults.
            let power_of_two = width.is_power_of_two() && height.is_power_of_two();
            let (wrap, filter) = if power_of_two {
                (glow::REPEAT, glow::LINEAR_MIPMAP_LINEAR)
            } else {
                (glow::CLAMP_TO_EDGE, glow::LINEAR)
            };
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, wrap as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, wrap as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, filter as i32);
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                glow::LINEAR as i32,
            );

            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA as i32,
                width as i32,
                height as i32,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(pixels)),
            );

            if power_of_two {
                gl.generate_mipmap(glow::TEXTURE_2D);
            }

            gl.bind_texture(glow::TEXTURE_2D, None);

            let error = gl.get_error();
            if error != glow::NO_ERROR {
                gl.delete_texture(handle);
                return Err(Error3D::GlObject(format!(
                    "the driver refused the {width}x{height} texture (GL error 0x{error:04x})"
                )));
            }

            Ok(Self {
                handle,
                width,
                height,
            })
        }
    }

    /// Width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The raw GL name, for binding through [`Program::set_texture`](crate::shader::Program).
    pub(crate) fn gl_name(&self) -> glow::Texture {
        self.handle
    }

    /// Deletes the GL texture. The context must be current.
    pub(crate) fn destroy(self, gl: &glow::Context) {
        // SAFETY: the handle came from `create_texture` on this context and is not
        // used after this call.
        unsafe { gl.delete_texture(self.handle) };
    }
}
