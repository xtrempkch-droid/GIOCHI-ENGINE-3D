//! GL state that must be explicit every frame.
//!
//! Golden rule 6 of the 3D extension: depth test, culling and blending are stated by
//! the renderer instead of assumed. GL is a state machine and a driver that starts
//! with depth testing off — or a previous draw call that left it off — produces a
//! scene that looks like a sorting bug.

use glow::HasContext;

use crate::math3d::Color3D;

/// Fixed-function state the renderer owns.
///
/// Deliberately plain data: it is compared against the live GL state so a redundant
/// `glEnable` is not issued every frame, and it serialises trivially for logs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlState {
    /// Whether `GL_DEPTH_TEST` is on. On for every 3D scene; off is only useful for
    /// a full-screen overlay.
    pub depth_test: bool,
    /// Whether `GL_CULL_FACE` is on.
    pub cull_face: bool,
    /// Which winding is treated as front-facing.
    pub front_face: FrontFace,
    /// Blending, needed for transparent meshes and text later.
    pub blend: bool,
    /// Colour written when the colour buffer is cleared.
    pub clear_color: Color3D,
}

/// Winding order treated as the front of a triangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrontFace {
    /// Counter-clockwise, GL's default and what OBJ files use.
    CounterClockwise,
    /// Clockwise.
    Clockwise,
}

impl FrontFace {
    fn to_gl(self) -> u32 {
        match self {
            Self::CounterClockwise => glow::CCW,
            Self::Clockwise => glow::CW,
        }
    }
}

impl Default for GlState {
    fn default() -> Self {
        Self {
            depth_test: true,
            cull_face: true,
            front_face: FrontFace::CounterClockwise,
            // Off by default: an opaque scene does not need it and leaving it on is a
            // classic source of "why is my cube see-through".
            blend: false,
            clear_color: Color3D::new(0.05, 0.07, 0.12, 1.0),
        }
    }
}

impl GlState {
    /// Pushes this state into the GL context.
    ///
    /// Called at the start of every frame; `glow` calls are cheap but not free, and
    /// this avoids re-deriving the same state at each draw call.
    pub fn apply(&self, gl: &glow::Context) {
        // SAFETY: every function below is core GLES 2.0 and the context is current for
        // the duration of the call.
        unsafe {
            if self.depth_test {
                gl.enable(glow::DEPTH_TEST);
                gl.depth_func(glow::LEQUAL);
            } else {
                gl.disable(glow::DEPTH_TEST);
            }

            if self.cull_face {
                gl.enable(glow::CULL_FACE);
                gl.cull_face(glow::BACK);
                gl.front_face(self.front_face.to_gl());
            } else {
                gl.disable(glow::CULL_FACE);
            }

            if self.blend {
                gl.enable(glow::BLEND);
                gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
            } else {
                gl.disable(glow::BLEND);
            }
        }
    }

    /// Clears the colour and depth buffers with this state's clear colour.
    pub fn clear(&self, gl: &glow::Context) {
        let [r, g, b, a] = self.clear_color.to_array();

        // SAFETY: core GLES 2.0 calls on the current context.
        unsafe {
            gl.clear_color(r, g, b, a);
            gl.clear_depth_f32(1.0);
            gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state_is_opaque_and_depth_tested() {
        let state = GlState::default();

        assert!(
            state.depth_test,
            "3D without depth testing is a sorting bug"
        );
        assert!(state.cull_face);
        assert_eq!(state.front_face, FrontFace::CounterClockwise);
        assert!(!state.blend, "blending is opt-in");
        assert_eq!(state.clear_color.to_array(), [0.05, 0.07, 0.12, 1.0]);
    }

    #[test]
    fn front_face_maps_to_the_gl_enum() {
        assert_eq!(FrontFace::CounterClockwise.to_gl(), glow::CCW);
        assert_eq!(FrontFace::Clockwise.to_gl(), glow::CW);
        assert_ne!(glow::CCW, glow::CW);
    }

    #[test]
    fn state_is_comparable_so_redundant_changes_can_be_skipped() {
        let a = GlState::default();
        let mut b = a;
        assert_eq!(a, b);

        b.blend = true;
        assert_ne!(a, b);
    }
}
