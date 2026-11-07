//! Error type for the 3D path.
//!
//! The 3D crate is a *parallel* path, not an extension of the 2D engine's error
//! enum: adding variants to `GiochiError` would make every 2D consumer compile
//! against 3D concepts it never touches (ADR-037). At the C ABI boundary the codes
//! are still plain `i32`, mapped in phase 14.

use thiserror::Error;

/// Anything that can go wrong on the 3D path.
#[derive(Debug, Error)]
pub enum Error3D {
    /// SDL2 could not provide the window or video subsystem.
    #[error("platform error: {0}")]
    Platform(String),

    /// The OpenGL ES context could not be created or configured.
    #[error("GL context error: {0}")]
    Context(String),

    /// A GL object (shader, program, buffer, texture) could not be created.
    #[error("GL object error: {0}")]
    GlObject(String),

    /// GLSL compilation or linking failed. Always carries the driver's own log.
    #[error("shader '{stage}' failed to compile: {log}")]
    Shader {
        /// `vertex` or `fragment`.
        stage: &'static str,
        /// The complete `glGetShaderInfoLog` / `glGetProgramInfoLog` output. Without
        /// this, debugging a shader is guesswork.
        log: String,
    },

    /// A mesh, texture or shader file could not be read or parsed.
    #[error("asset error: {0}")]
    Asset(String),

    /// A caller-supplied value was out of range or malformed.
    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    /// A handle was `0` or referred to something already destroyed.
    #[error("invalid handle: {0}")]
    InvalidHandle(u64),
}

impl Error3D {
    /// Short constructor for context failures. The other variants are built directly;
    /// adding a constructor per variant would be noise.
    pub(crate) fn context(message: impl Into<String>) -> Self {
        Self::Context(message.into())
    }
}

/// Convenience alias for the 3D path.
pub type Result3D<T> = Result<T, Error3D>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shader_errors_carry_the_driver_log() {
        let error = Error3D::Shader {
            stage: "vertex",
            log: "ERROR: 0:3: 'x' : undeclared identifier".to_owned(),
        };
        let text = error.to_string();

        assert!(text.contains("vertex"), "{text}");
        assert!(text.contains("undeclared identifier"), "{text}");
    }

    #[test]
    fn every_variant_renders_a_message() {
        let all = [
            Error3D::Platform("no video".to_owned()),
            Error3D::context("no egl"),
            Error3D::GlObject("no buffer".to_owned()),
            Error3D::Shader {
                stage: "fragment",
                log: "syntax error".to_owned(),
            },
            Error3D::Asset("missing.obj".to_owned()),
            Error3D::InvalidArgument("fov must be positive".to_owned()),
            Error3D::InvalidHandle(7),
        ];

        for error in all {
            assert!(!error.to_string().is_empty());
        }
    }
}
