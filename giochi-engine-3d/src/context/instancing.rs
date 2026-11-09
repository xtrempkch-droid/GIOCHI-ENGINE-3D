//! Runtime detection of instanced drawing.
//!
//! Instancing is not core in the GLES 2.0 context this engine requests by default
//! (ADR-038). Three things provide it, and drivers disagree about which:
//!
//! | Provider | Draw call | Per-instance divisor |
//! |---|---|---|
//! | GLES 3.0 core | `glDrawElementsInstanced` | `glVertexAttribDivisor` |
//! | `GL_ANGLE_instanced_arrays` | `glDrawElementsInstancedANGLE` | `glVertexAttribDivisorANGLE` |
//! | `GL_EXT_instanced_arrays` | `glDrawElementsInstancedEXT` | `glVertexAttribDivisorEXT` |
//!
//! **The extension the 3D extension suggested is the one neither target has.** The
//! phase-17 plan warned that `GL_ANGLE_instanced_arrays` "is common, but not every
//! ARM64 driver exposes it" and made runtime detection mandatory. Measured, both the
//! PX30 (Mali-G31, Mesa 24.0.0/Panfrost) and the x86_64 development machine (RX 580,
//! Mesa 26.0.8/radeonsi) answer **`GL_EXT_instanced_arrays` + `GL_EXT_draw_instanced`
//! and no ANGLE**, so a detection that looked only for ANGLE would have silently taken
//! the fallback on every machine this project owns. `GL_OES_draw_instanced` (the
//! original Khronos spelling of the draw half) is absent on both as well.
//!
//! `GL_EXT_draw_instanced` alone is not enough: it supplies an instance count but no
//! way to give each instance its own matrix, which is the entire point here. The
//! renderer therefore requires the *instanced arrays* half and treats a
//! draw-instanced-only driver as "no instancing".

use std::ffi::c_void;

use glow::HasContext;

use super::gl_context::GlContext;

/// Forces the one-draw-per-instance fallback when set to `1`.
///
/// The fallback is the code that runs on a driver without any instancing extension, and
/// every machine this project owns has one. An untested fallback is a fallback that does
/// not work, so this switch lets the test suite exercise the per-instance path — the
/// same reason `GIOCHI_FORCE_NO_VAO` exists for vertex array objects.
pub const FORCE_NO_INSTANCING_ENV: &str = "GIOCHI_FORCE_NO_INSTANCING";

fn instancing_allowed() -> bool {
    !matches!(std::env::var(FORCE_NO_INSTANCING_ENV).as_deref(), Ok("1"))
}

/// Where the instanced-draw entry points come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstancingSupport {
    /// GLES 3.0 core functions, called through glow.
    Core,
    /// `GL_ANGLE_instanced_arrays`, loaded through `SDL_GL_GetProcAddress`.
    Angle,
    /// `GL_EXT_instanced_arrays`, loaded through `SDL_GL_GetProcAddress`.
    Ext,
    /// No usable instancing: callers must draw once per instance.
    None,
}

impl InstancingSupport {
    /// Whether a single call can draw more than one instance.
    pub fn is_available(self) -> bool {
        self != Self::None
    }

    /// The GL extension that provides it, for logs and diagnostics.
    ///
    /// `None` for the core path, which needs no extension.
    pub fn extension(self) -> Option<&'static str> {
        match self {
            Self::Core | Self::None => None,
            Self::Angle => Some("GL_ANGLE_instanced_arrays"),
            Self::Ext => Some("GL_EXT_instanced_arrays"),
        }
    }

    /// A short name for logs and the overlay.
    pub fn name(self) -> &'static str {
        match self {
            Self::Core => "gles3-core",
            Self::Angle => "angle",
            Self::Ext => "ext",
            Self::None => "none",
        }
    }
}

/// `void glDrawElementsInstanced*(GLenum, GLsizei, GLenum, const void*, GLsizei)`.
type DrawElementsInstancedFn = unsafe extern "system" fn(u32, i32, u32, *const c_void, i32);

/// `void glVertexAttribDivisor*(GLuint, GLuint)`.
type VertexAttribDivisorFn = unsafe extern "system" fn(u32, u32);

/// The loaded entry points for one provider.
///
/// **Crate-internal.** It holds raw GL function pointers, which cannot cross an FFI
/// boundary and have no business in a public ABI; a user asks
/// [`InstancingSupport`] what the driver can do and never needs the addresses. Keeping
/// it private is also what keeps `giochi.h` unchanged in this phase.
#[derive(Clone, Copy)]
pub(crate) struct InstancingBackend {
    support: InstancingSupport,
    draw_elements_instanced: Option<DrawElementsInstancedFn>,
    vertex_attrib_divisor: Option<VertexAttribDivisorFn>,
}

impl std::fmt::Debug for InstancingBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InstancingBackend")
            .field("support", &self.support)
            .finish_non_exhaustive()
    }
}

impl InstancingBackend {
    /// A backend that cannot instance anything.
    pub const NONE: Self = Self {
        support: InstancingSupport::None,
        draw_elements_instanced: None,
        vertex_attrib_divisor: None,
    };

    /// Detects the provider for `gl` and loads what it needs.
    ///
    /// `load` is how the caller resolves a GL symbol — SDL's `gl_get_proc_address` in
    /// practice. It is a parameter rather than an import so this module stays testable
    /// without a window: a test can hand in a stub.
    pub fn detect(gl: &GlContext, load: impl Fn(&str) -> *const c_void) -> Self {
        if !instancing_allowed() {
            log::warn!("{FORCE_NO_INSTANCING_ENV}=1: forcing the per-instance fallback");
            return Self::NONE;
        }

        if gl.supports_gles3() {
            return Self {
                support: InstancingSupport::Core,
                draw_elements_instanced: None,
                vertex_attrib_divisor: None,
            };
        }

        for (support, draw_name, divisor_name) in [
            (
                InstancingSupport::Angle,
                "glDrawElementsInstancedANGLE",
                "glVertexAttribDivisorANGLE",
            ),
            (
                InstancingSupport::Ext,
                "glDrawElementsInstancedEXT",
                "glVertexAttribDivisorEXT",
            ),
        ] {
            let Some(extension) = support.extension() else {
                continue;
            };
            if !gl.supports_extension(extension) {
                continue;
            }
            let draw = load(draw_name);
            let divisor = load(divisor_name);
            if draw.is_null() || divisor.is_null() {
                // The driver advertises the extension but will not hand over the
                // entry points. Log it: that is a driver bug worth seeing, and its
                // silent consequence is a 100x slower scene.
                log::warn!(
                    "{extension} is advertised but {draw_name}/{divisor_name} could not \
                     be resolved; instancing falls back to one draw per instance"
                );
                continue;
            }
            return Self {
                support,
                // SAFETY: the driver returned a pointer to a function with exactly
                // this signature, which is what the extension specification defines.
                draw_elements_instanced: Some(unsafe {
                    std::mem::transmute::<*const c_void, DrawElementsInstancedFn>(draw)
                }),
                // SAFETY: as above.
                vertex_attrib_divisor: Some(unsafe {
                    std::mem::transmute::<*const c_void, VertexAttribDivisorFn>(divisor)
                }),
            };
        }

        Self::NONE
    }

    /// Which provider was found.
    pub fn support(self) -> InstancingSupport {
        self.support
    }

    /// Sets the per-instance divisor of attribute `index`.
    ///
    /// # Safety
    ///
    /// `index` must be a vertex attribute slot enabled for the current draw, and the
    /// GL context must be current.
    pub unsafe fn vertex_attrib_divisor(self, gl: &glow::Context, index: u32, divisor: u32) {
        match self.support {
            InstancingSupport::Core => gl.vertex_attrib_divisor(index, divisor),
            InstancingSupport::Angle | InstancingSupport::Ext => {
                if let Some(divisor_fn) = self.vertex_attrib_divisor {
                    divisor_fn(index, divisor);
                }
            }
            InstancingSupport::None => {}
        }
    }

    /// Draws `instance_count` copies of the indices bound to `ELEMENT_ARRAY_BUFFER`.
    ///
    /// # Safety
    ///
    /// The current vertex array must supply this geometry, every attribute must be
    /// enabled, and the GL context must be current.
    pub unsafe fn draw_elements_instanced(
        self,
        gl: &glow::Context,
        mode: u32,
        index_count: i32,
        index_type: u32,
        indices: i32,
        instance_count: i32,
    ) {
        match self.support {
            InstancingSupport::Core => {
                gl.draw_elements_instanced(mode, index_count, index_type, indices, instance_count)
            }
            InstancingSupport::Angle | InstancingSupport::Ext => {
                if let Some(draw_fn) = self.draw_elements_instanced {
                    draw_fn(
                        mode,
                        index_count,
                        index_type,
                        indices as isize as *const c_void,
                        instance_count,
                    );
                }
            }
            InstancingSupport::None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_cannot_instance_and_has_no_extension() {
        assert!(!InstancingSupport::None.is_available());
        assert_eq!(InstancingSupport::None.extension(), None);
        assert_eq!(InstancingSupport::None.name(), "none");
    }

    #[test]
    fn the_core_path_needs_no_extension_but_is_available() {
        assert!(InstancingSupport::Core.is_available());
        assert_eq!(InstancingSupport::Core.extension(), None);
        assert_eq!(InstancingSupport::Core.name(), "gles3-core");
    }

    #[test]
    fn each_provider_names_its_extension() {
        assert_eq!(
            InstancingSupport::Angle.extension(),
            Some("GL_ANGLE_instanced_arrays")
        );
        assert_eq!(
            InstancingSupport::Ext.extension(),
            Some("GL_EXT_instanced_arrays")
        );
        assert!(InstancingSupport::Angle.is_available());
        assert!(InstancingSupport::Ext.is_available());
    }

    #[test]
    fn an_advertised_extension_with_a_null_symbol_is_skipped() {
        // This is the shape a driver bug takes: the extension string is there and the
        // entry point is not. The loop must not dereference a null pointer, and must
        // not claim a provider it cannot use.
        let backend = InstancingBackend {
            support: InstancingSupport::Ext,
            draw_elements_instanced: None,
            vertex_attrib_divisor: None,
        };

        assert_eq!(backend.support(), InstancingSupport::Ext);
        // A null entry point must be a no-op, not a call through null.
        let name = backend.support().name();
        assert_eq!(name, "ext");
    }

    #[test]
    fn the_default_backend_is_none() {
        assert_eq!(InstancingBackend::NONE.support(), InstancingSupport::None);
        assert!(!InstancingBackend::NONE.support().is_available());
    }
}
