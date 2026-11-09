//! GL context creation and fixed-function state.

mod gl_context;
mod instancing;
mod state;

pub use gl_context::{configure_attributes, create, swap, GlContext, GlesVersion, GL_VERSION_ENV};
pub(crate) use instancing::InstancingBackend;
pub use instancing::{InstancingSupport, FORCE_NO_INSTANCING_ENV};
pub use state::{FrontFace, GlState};
