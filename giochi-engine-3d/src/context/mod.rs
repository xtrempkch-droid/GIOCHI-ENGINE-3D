//! GL context creation and fixed-function state.

mod gl_context;
mod state;

pub use gl_context::{configure_attributes, create, swap, GlContext, GlesVersion, GL_VERSION_ENV};
pub use state::{FrontFace, GlState};
