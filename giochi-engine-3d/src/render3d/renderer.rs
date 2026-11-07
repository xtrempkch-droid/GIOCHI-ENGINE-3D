//! `RenderContext3D`: owns the GL context and the fixed-function state.
//!
//! Phase 9 only clears the screen. Mesh and shader drawing arrive in phase 11; the
//! type exists now so the frame skeleton (state → clear → swap) is fixed before
//! anything is added to it.

use giochi_engine::platform::window::Window;

use glow::HasContext;

use crate::context::{GlContext, GlState};
use crate::math3d::Color3D;

/// The 3D renderer.
///
/// Deliberately **not** `#[repr(C)]`: it holds a `glow::Context`, which cannot cross a
/// C boundary. C consumers get an opaque pointer in phase 14 and reach it through
/// `giochi_renderer3d_*` functions, the same shape as `GiochiRenderContext` in 2D.
pub struct RenderContext3D {
    gl: GlContext,
    state: GlState,
}

impl std::fmt::Debug for RenderContext3D {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderContext3D")
            .field("version", &self.gl.version())
            .field("adapter", &self.gl.adapter())
            .field("state", &self.state)
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

        Self { gl, state }
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

    /// Applies the state and clears colour and depth.
    ///
    /// Every frame starts here. Depth is cleared every frame because a 3D scene that
    /// reuses the previous frame's depth buffer renders back-to-front garbage.
    pub fn begin_frame(&mut self) {
        self.state.apply(self.gl.glow());
        self.state.clear(self.gl.glow());
    }

    /// Presents the back buffer.
    pub fn end_frame(&self, window: &Window) {
        crate::context::swap(window);
    }

    /// Convenience for the phase 9 demo: clear with `color` and present.
    pub fn clear(&mut self, window: &Window, color: Color3D) {
        self.set_clear_color(color);
        self.begin_frame();
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
}
