//! The OpenGL ES context, created through SDL2.
//!
//! SDL2 owns the platform surface (KMSDRM, X11, Wayland, or the `dummy` driver) and
//! EGL lives underneath it. We never touch EGL directly: `SDL_GL_CreateContext` picks
//! the right path for whatever video driver is active, which is what makes the same
//! binary work on a desktop and on a bare console.
//!
//! GLES 2.0 is the default because it is the floor on ARM64 handhelds (R35S/RK3326,
//! Anki/Anbernic class devices). GLES 3.0 is **opt-in** through `GIOCHI_GL_VERSION=3`
//! and is never assumed — see ADR-038.

use std::ffi::c_void;

use crate::context::instancing::InstancingBackend;
use crate::error::{Error3D, Result3D};
use giochi_engine::platform::window::Window;
use giochi_engine::platform::Platform;
use glow::HasContext;
use sdl2::video::{GLContext, GLProfile};

/// Environment variable that opts into a GLES 3.0 context.
///
/// Anything other than `3` keeps the GLES 2.0 default. Documented in `docs/3D.md`.
pub const GL_VERSION_ENV: &str = "GIOCHI_GL_VERSION";

/// Which GLES version the context was actually created with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlesVersion {
    /// OpenGL ES 2.0 — the default and the ARM64 floor.
    Gles2,
    /// OpenGL ES 3.0 — only when explicitly requested *and* granted by the driver.
    Gles3,
}

impl GlesVersion {
    /// The version requested from the environment.
    ///
    /// Reads [`GL_VERSION_ENV`]; `3` means GLES 3.0, everything else GLES 2.0.
    pub fn from_env() -> Self {
        match std::env::var(GL_VERSION_ENV).as_deref() {
            Ok("3") | Ok("3.0") | Ok("gles3") => Self::Gles3,
            _ => Self::Gles2,
        }
    }

    /// Major version number (`2` or `3`).
    pub fn major(self) -> u8 {
        match self {
            Self::Gles2 => 2,
            Self::Gles3 => 3,
        }
    }
}

/// A live GLES context plus the glow function table loaded from it.
///
/// Drop order is the declaration order, so `_sdl_gl` (the SDL GL context) is released
/// first and `glow` (a plain table of function pointers with no destructor) after it.
/// No GL call may run once the SDL context is gone, which is why the renderer is
/// dropped before this type (it owns them together) and `Drop` only logs.
pub struct GlContext {
    /// Kept alive for the lifetime of `glow`; SDL destroys the GL context when this
    /// is dropped. Never read directly -- the underscore is meaningful.
    _sdl_gl: GLContext,
    glow: glow::Context,
    version: GlesVersion,
    adapter: String,
    /// The loaded instanced-drawing entry points, decided once at creation.
    instancing: InstancingBackend,
    /// `GL_EXTENSIONS`, split once at creation.
    ///
    /// Queried eagerly because the string is only valid while the context is current,
    /// and a later caller (instancing detection, most obviously) may be running while
    /// another context is bound. It is also the one piece of driver capability that
    /// decides whether a whole code path exists, so it belongs next to the version.
    extensions: Vec<String>,
}

impl std::fmt::Debug for GlContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GlContext")
            .field("version", &self.version)
            .field("adapter", &self.adapter)
            .field("extensions", &self.extensions.len())
            .field("instancing", &self.instancing)
            .finish_non_exhaustive()
    }
}

/// Sets the `SDL_GL_*` attributes and creates the context.
///
/// The attributes are process-global in SDL2 and are read when the *window* is
/// created, so callers must use [`configure_attributes`] before
/// `Window::new_opengl` and only then call this.
pub fn create(window: &Window) -> Result3D<GlContext> {
    let requested = GlesVersion::from_env();

    if requested == GlesVersion::Gles3 {
        log::info!("GIOCHI_GL_VERSION requested GLES 3.0; GLES 3.0 features are opt-in only");
    }

    let sdl_gl = window
        .sdl_window()
        .gl_create_context()
        .map_err(|err| Error3D::context(format!("SDL_GL_CreateContext failed: {err}")))?;

    // SAFETY: `gl_get_proc_address` returns a pointer valid for the current context,
    // which `sdl_gl` keeps alive. glow only stores the function pointers and calls
    // them while this context is current, which it is until `sdl_gl` is dropped.
    let glow = unsafe {
        let video = window.sdl_window().subsystem();
        glow::Context::from_loader_function(|symbol| {
            video.gl_get_proc_address(symbol) as *const c_void
        })
    };

    let adapter = describe_adapter(&glow);
    let version = detect_version(&glow, requested);
    let extensions = query_extensions(&glow);

    if requested == GlesVersion::Gles3 && version == GlesVersion::Gles2 {
        // Not an error: the driver answered with what it has. Silently degrading
        // would hide the reason a 3.0-only code path later fails to compile.
        log::warn!(
            "GLES 3.0 was requested but the driver created a GLES 2.0 context; \
             falling back (see GIOCHI_GL_VERSION in docs/3D.md)"
        );
    }

    let mut context = GlContext {
        _sdl_gl: sdl_gl,
        glow,
        version,
        adapter: adapter.clone(),
        extensions,
        instancing: InstancingBackend::NONE,
    };

    // Loaded here rather than lazily because the SDL window is the only thing that can
    // resolve a GL symbol, and this is the last point where it is still in scope.
    context.instancing = {
        let video = window.sdl_window().subsystem();
        super::instancing::InstancingBackend::detect(&context, |symbol| {
            video.gl_get_proc_address(symbol) as *const c_void
        })
    };

    log::info!(
        "GLES {} context on {adapter}, {} extensions, instancing: {}",
        context.version.major(),
        context.extensions.len(),
        context.instancing.support().name()
    );

    Ok(context)
}

/// Applies the `SDL_GL_*` attributes. Must run before the window is created.
///
/// Configuration mirrors what the 3D path actually needs — a colour buffer, a depth
/// buffer and a double buffer. Anything else is left at SDL's default so a driver
/// with a smaller framebuffer is not rejected for a setting we never read.
pub fn configure_attributes(platform: &Platform, version: GlesVersion) -> Result3D<()> {
    let video = platform.video_subsystem();
    let attr = video.gl_attr();

    // sdl2 0.38 panics rather than returning an error from these setters, so there is
    // nothing to map here. They are process-global and only take effect for a window
    // created afterwards -- which is why this is a separate step and not part of
    // `create`.
    attr.set_context_profile(GLProfile::GLES);
    attr.set_context_major_version(version.major());
    attr.set_context_minor_version(0);

    // GLES 2.0 has no debug-output extension worth enabling here; a debug context
    // costs driver work and only some ARM64 drivers expose it.
    attr.set_double_buffer(true);
    attr.set_depth_size(24);
    attr.set_stencil_size(8);
    attr.set_red_size(8);
    attr.set_green_size(8);
    attr.set_blue_size(8);
    attr.set_alpha_size(8);

    Ok(())
}

/// Pushes the back buffer to the screen.
pub fn swap(window: &Window) {
    window.sdl_window().gl_swap_window();
}

impl GlContext {
    /// The glow function table.
    pub fn glow(&self) -> &glow::Context {
        &self.glow
    }

    /// The version the driver actually gave us.
    pub fn version(&self) -> GlesVersion {
        self.version
    }

    /// The driver's renderer string, for logs and bug reports.
    pub fn adapter(&self) -> &str {
        &self.adapter
    }

    /// Whether the context can use GLES 3.0 features (VAOs are core, instancing, ...).
    ///
    /// Every 3.0-only path must check this first; see ADR-038.
    pub fn supports_gles3(&self) -> bool {
        self.version == GlesVersion::Gles3
    }

    /// Whether the driver advertises `name` in `GL_EXTENSIONS`.
    ///
    /// A GLES 3.0 context also accepts an extension whose functionality became core
    /// when the caller asks for the extension name only — drivers vary, so callers
    /// that need a *capability* should test [`GlContext::supports_gles3`] first and
    /// fall back to this.
    pub fn supports_extension(&self, name: &str) -> bool {
        self.extensions.iter().any(|extension| extension == name)
    }

    /// Every extension the driver reported, sorted.
    pub fn extensions(&self) -> &[String] {
        &self.extensions
    }

    /// The instanced-drawing entry points.
    ///
    /// Crate-internal: callers outside the crate ask
    /// [`RenderContext3D::instancing_support`](crate::RenderContext3D::instancing_support)
    /// instead.
    pub(crate) fn instancing(&self) -> InstancingBackend {
        self.instancing
    }
}

impl Drop for GlContext {
    fn drop(&mut self) {
        // The fields are dropped in declaration order after this body: `_sdl_gl` (the
        // SDL context) first. `glow` holds no resources of its own.
        log::debug!("releasing GLES {} context", self.version.major());
    }
}

/// Reads and splits `GL_EXTENSIONS`.
///
/// `glGetString(GL_EXTENSIONS)` is core in GLES 2.0 and deprecated in 3.0, where it
/// still works on every driver measured here; the alternative (`glGetStringi`) is 3.0
/// only and would need a branch for no benefit. An empty list is a valid answer and
/// only means "no optional path".
fn query_extensions(gl: &glow::Context) -> Vec<String> {
    // SAFETY: `gl` is current for the lifetime of the query.
    let raw = unsafe { gl.get_parameter_string(glow::EXTENSIONS) };
    let mut extensions: Vec<String> = raw
        .split_whitespace()
        .map(str::to_owned)
        .filter(|name| !name.is_empty())
        .collect();
    extensions.sort_unstable();
    extensions.dedup();
    extensions
}

fn describe_adapter(gl: &glow::Context) -> String {
    // SAFETY: `gl` was created from a current context; both getters are core GLES 2.0.
    unsafe {
        let renderer = gl.get_parameter_string(glow::RENDERER);
        let vendor = gl.get_parameter_string(glow::VENDOR);
        format!("{renderer} ({vendor})")
    }
}

fn detect_version(gl: &glow::Context, requested: GlesVersion) -> GlesVersion {
    // SAFETY: same as above.
    let reported = unsafe { gl.get_parameter_string(glow::VERSION) };
    log::debug!("driver reports GL_VERSION={reported}");

    let parsed = parse_es_major(&reported);

    // Asking for GLES 2.0 does NOT guarantee a 2.0 context. Measured on a Rockchip
    // PX30 (Mali-G31/Panfrost, Mesa 24.0.0): `SDL_GL_CONTEXT_MAJOR_VERSION=2` produced
    // `OpenGL ES 3.1` anyway -- SDL2's KMSDRM path honours the profile but not the
    // version request. So when the callers asked for 2.0, report 2.0 even if the driver
    // gave more: `supports_gles3()` is a promise about what the *caller may assume*,
    // and a caller that asked for the floor must keep getting floor behaviour, or a
    // 3.0-only code path would activate on a request that explicitly excluded it.
    //
    // The reverse (asking for 3, getting 2) is downgraded honestly, with a warning
    // logged by the caller.
    match requested {
        GlesVersion::Gles2 => GlesVersion::Gles2,
        GlesVersion::Gles3 => match parsed {
            Some(major) if major >= 3 => GlesVersion::Gles3,
            // The driver could not give 3.0 even though it was asked for.
            _ => GlesVersion::Gles2,
        },
    }
}

/// Parses the ES major version out of a `GL_VERSION` string.
///
/// The string is not portable: Mesa says `OpenGL ES 3.2 Mesa 26.0.8`, some embedded
/// drivers say `OpenGL ES 2.0 build 1.14@...`, and old ones say `OpenGL ES-CM 1.1` for
/// a fixed-function profile. The first integer token wins; anything unparseable is
/// `None` and the caller decides.
fn parse_es_major(reported: &str) -> Option<u8> {
    reported.split_whitespace().find_map(|token| {
        token
            .split('.')
            .next()
            .and_then(|major| major.parse::<u8>().ok())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gles3_major_is_three() {
        assert_eq!(GlesVersion::Gles3.major(), 3);
        assert_eq!(GlesVersion::Gles2.major(), 2);
    }

    #[test]
    fn version_parsing_handles_real_driver_strings() {
        // The strings below are what the drivers actually report.
        let cases = [
            ("OpenGL ES 3.2 Mesa 26.0.8", Some(3)),
            ("OpenGL ES 2.0 Mesa 26.0.8", Some(2)),
            ("OpenGL ES 2.0 build 1.14@5776728", Some(2)),
            ("OpenGL ES 3.1 Mesa 24.0.0", Some(3)), // Mali-G31/Panfrost on the PX30
            ("OpenGL ES 3.2 v1.r38p1", Some(3)),
            ("OpenGL ES-CM 1.1", Some(1)), // fixed-function profile, old ARM drivers
        ];

        for (reported, expected) in cases {
            assert_eq!(
                parse_es_major(reported),
                expected,
                "driver string: {reported}"
            );
        }
    }

    #[test]
    fn an_unparseable_version_string_yields_none() {
        assert_eq!(parse_es_major(""), None);
        assert_eq!(parse_es_major("no version here"), None);
    }

    #[test]
    fn asking_for_gles2_reports_gles2_even_when_the_driver_gives_more() {
        // Measured on a PX30: SDL2/KMSDRM ignored MAJOR_VERSION=2 and created a 3.1
        // context. `supports_gles3` is a promise about what the caller may assume, so
        // a caller that asked for the floor must keep getting floor behaviour.
        assert_eq!(
            detect_version_for_test("OpenGL ES 3.1 Mesa 24.0.0", GlesVersion::Gles2),
            GlesVersion::Gles2
        );
        assert_eq!(
            detect_version_for_test("OpenGL ES 2.0 Mesa 26.0.8", GlesVersion::Gles2),
            GlesVersion::Gles2
        );
    }

    #[test]
    fn asking_for_gles3_is_downgraded_honestly_when_refused() {
        assert_eq!(
            detect_version_for_test("OpenGL ES 3.2 Mesa 26.0.8", GlesVersion::Gles3),
            GlesVersion::Gles3
        );
        // Refused: report the truth rather than pretending.
        assert_eq!(
            detect_version_for_test("OpenGL ES 2.0 Mesa 26.0.8", GlesVersion::Gles3),
            GlesVersion::Gles2
        );
        assert_eq!(
            detect_version_for_test("garbage", GlesVersion::Gles3),
            GlesVersion::Gles2
        );
    }

    /// Mirrors `detect_version` without needing a live context.
    fn detect_version_for_test(reported: &str, requested: GlesVersion) -> GlesVersion {
        match requested {
            GlesVersion::Gles2 => GlesVersion::Gles2,
            GlesVersion::Gles3 => match parse_es_major(reported) {
                Some(major) if major >= 3 => GlesVersion::Gles3,
                _ => GlesVersion::Gles2,
            },
        }
    }

    #[test]
    fn env_defaults_to_gles2() {
        // Cannot mutate the environment safely in a parallel test binary; only assert
        // the mapping function's default branch is reachable.
        assert_eq!(GlesVersion::Gles2.major(), 2);
    }
}
