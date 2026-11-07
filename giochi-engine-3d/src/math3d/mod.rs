//! 3D maths.
//!
//! The public types are `#[repr(C)]` and built from `f32`, so they can cross the C ABI
//! unchanged. `glam` is used for the arithmetic and never appears in a public field: it
//! is an implementation detail, not part of the surface.
//!
//! Two conventions are fixed here and stated in `docs/C_ABI_CONTRACT.md` because they
//! are the ones that fail invisibly:
//!
//! * quaternions are `(x, y, z, w)`;
//! * `Transform3D` composes as **scale → rotate → translate**.
//!
//! Matrices are column-major so [`Mat4::as_ptr`] can be handed straight to
//! `glUniformMatrix4fv` with `transpose = false`.

mod camera;
mod color;
mod controller;
mod mat4;
mod quat;
mod transform;
mod vec3;

pub use camera::Camera3D;
pub use color::Color3D;
pub use controller::{AxisInput, CameraController, ControllerConfig};
pub use mat4::Mat4;
pub use quat::Quat;
pub use transform::Transform3D;
pub use vec3::Vec3;
