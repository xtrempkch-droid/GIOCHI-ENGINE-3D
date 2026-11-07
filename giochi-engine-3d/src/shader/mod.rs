//! GLSL programs.
//!
//! Shader source is a file, never a Rust string literal in the renderer (R10). The
//! built-ins are `include_str!`ed from `shaders/`; a user shader is loaded from a path.
//!
//! Every compilation failure carries the driver's own `glGetShaderInfoLog` /
//! `glGetProgramInfoLog`, because GLES drivers disagree about diagnostics enough that
//! the log is the only portable clue (golden rule 5).

mod builtin;
mod program;

pub use builtin::BuiltinShader;
pub use program::{Program, ATTRIB_NORMAL, ATTRIB_POSITION, ATTRIB_UV};
