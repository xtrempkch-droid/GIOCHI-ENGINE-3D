//! SDL2 initialisation and the platform context.
//!
//! On the production target (Linux consoles) the engine renders straight to the
//! DRM/KMS device through SDL2's `kmsdrm` video driver. Driver selection is
//! controlled by the environment so developers keep their desktop:
//!
//! * `SDL_VIDEODRIVER=kmsdrm` — force the KMSDRM backend. [`Platform::new`] sets
//!   this variable itself **only when it is absent**, so an explicit user choice
//!   (`x11`, `wayland`, `dummy`, …) is never overwritten.
//! * `SDL_KMSDRM_REQUIRE_DRM_MASTER=1` — require DRM master. Needed when the
//!   engine owns the console and must page-flip directly; a desktop session that
//!   holds DRM master makes the KMSDRM backend fail to start.
//! * `SDL_KMSDRM_DEVICE_INDEX=<n>` — select `/dev/dri/cardN` explicitly, for
//!   consoles with more than one DRM device.
//! * `SDL_VIDEO_EGL_DRIVER` — EGL driver name used by the KMSDRM backend.
//! * `SDL_VIDEO_GL_DRIVER` — GL/GLES library used for the EGL context.
//!
//! The 2D renderer uses SDL2's `render` API; no GL symbol is referenced from
//! Rust.

use std::cell::RefCell;
use std::fmt;
use std::path::Path;
use std::rc::Rc;

use sdl2::event::Event;

use crate::error::GiochiError;
use crate::platform::window::{Window, WindowConfig};

/// Environment variable used to select the SDL2 video driver.
pub const SDL_VIDEODRIVER_ENV: &str = "SDL_VIDEODRIVER";

/// Video driver used when [`SDL_VIDEODRIVER_ENV`] is not set.
pub const KMSDRM_VIDEO_DRIVER: &str = "kmsdrm";

/// Hint that makes the KMSDRM backend require DRM master.
pub const HINT_KMSDRM_REQUIRE_DRM_MASTER: &str = "SDL_KMSDRM_REQUIRE_DRM_MASTER";

/// Hint that selects a specific `/dev/dri/cardN` device.
pub const HINT_KMSDRM_DEVICE_INDEX: &str = "SDL_KMSDRM_DEVICE_INDEX";

/// Hint naming the EGL driver used by the KMSDRM backend.
pub const HINT_VIDEO_EGL_DRIVER: &str = "SDL_VIDEO_EGL_DRIVER";

/// Hint naming the GL/GLES library used for the EGL context.
pub const HINT_VIDEO_GL_DRIVER: &str = "SDL_VIDEO_GL_DRIVER";

/// Directory where the kernel exposes DRM devices.
pub const DRM_DIRECTORY: &str = "/dev/dri";

/// Why the platform layer needs to leave the main loop.
///
/// Mapping SDL events to this value keeps [`crate::core::game_loop`] free of any
/// `sdl2` import.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QuitSignal {
    /// Nothing to do.
    #[default]
    None = 0,
    /// The window manager asked to close the window.
    WindowClose = 1,
    /// The user pressed `Escape`.
    Escape = 2,
}

/// Translates an SDL event into a [`QuitSignal`].
pub fn quit_signal(event: &Event) -> QuitSignal {
    match event {
        Event::Quit { .. } => QuitSignal::WindowClose,
        Event::KeyDown {
            keycode: Some(sdl2::keyboard::Keycode::Escape),
            repeat: false,
            ..
        } => QuitSignal::Escape,
        _ => QuitSignal::None,
    }
}

/// How the video driver was chosen.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoDriverChoice {
    /// `SDL_VIDEODRIVER` was already set; the engine did not touch it.
    Explicit = 0,
    /// `SDL_VIDEODRIVER` was absent and the engine selected KMSDRM.
    AutoKmsdrm = 1,
    /// The engine selected KMSDRM, found no usable DRM device, and fell back to
    /// SDL's default desktop driver. Logged as a warning.
    AutoFallback = 2,
}

/// Returns the SDL2 version linked into the engine as `(major, minor, patch)`.
///
/// Useful for diagnostics: the engine builds SDL2 from source (`bundled`), so
/// this is the version the binary actually contains, not the one installed on
/// the system.
pub fn sdl_version() -> (u32, u32, u32) {
    let version = sdl2::version::version();
    (
        u32::from(version.major),
        u32::from(version.minor),
        u32::from(version.patch),
    )
}

/// SDL subsystems shared by the engine.
///
/// `Sdl` must outlive every window and the event pump, so it is owned together
/// with the subsystems behind an `Rc` instead of borrowed.
pub(crate) struct SdlContext {
    audio: Option<sdl2::AudioSubsystem>,
    video: sdl2::VideoSubsystem,
    timer: sdl2::TimerSubsystem,
    events: sdl2::EventSubsystem,
    game_controller: Option<sdl2::GameControllerSubsystem>,
    sdl: sdl2::Sdl,
}

impl fmt::Debug for SdlContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SdlContext").finish_non_exhaustive()
    }
}

impl SdlContext {
    pub(crate) fn video(&self) -> &sdl2::VideoSubsystem {
        &self.video
    }

    /// The mouse utility.
    ///
    /// `MouseUtil` is a zero-sized handle (`SdlDrop` only), so it is derived from the
    /// stored `Sdl` on demand instead of being kept as a second field. Never `None` in
    /// practice; the `Option` at the call site is about the *vanilla* driver having no
    /// pointer, which is a per-frame question, not a construction-time one.
    pub(crate) fn mouse(&self) -> Option<sdl2::mouse::MouseUtil> {
        Some(self.sdl.mouse())
    }
}

/// The engine's platform layer: SDL2 video, timer and event subsystems.
///
/// Create it once per process. Windows created from it keep the SDL context
/// alive, so dropping the `Platform` first is safe.
#[repr(C)]
pub struct Platform {
    context: Rc<SdlContext>,
    event_pump: RefCell<sdl2::EventPump>,
    choice: VideoDriverChoice,
    _reserved: [u32; 4],
}

impl fmt::Debug for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Platform")
            .field("driver", &self.video_driver_name())
            .field("choice", &self.choice)
            .finish_non_exhaustive()
    }
}

impl Platform {
    /// Initialises SDL2 and opens the video, timer and event subsystems.
    ///
    /// Fails with [`GiochiError::Platform`] when video cannot be brought up. When
    /// the engine auto-selected KMSDRM and it is unavailable (a desktop session
    /// holding DRM master, a container without `/dev/dri`, …) it logs a warning
    /// and retries with SDL's default driver, reporting
    /// [`VideoDriverChoice::AutoFallback`].
    pub fn new() -> Result<Platform, GiochiError> {
        let (sdl, choice) = init_sdl()?;

        let (video, choice) = match sdl.video() {
            Ok(video) => (video, choice),
            Err(err) if choice == VideoDriverChoice::AutoKmsdrm => {
                log::warn!(
                    "kmsdrm video driver unavailable ({err}); falling back to the desktop driver. \
                     Set {SDL_VIDEODRIVER_ENV} explicitly to silence this warning."
                );
                std::env::remove_var(SDL_VIDEODRIVER_ENV);
                match sdl.video() {
                    Ok(video) => (video, VideoDriverChoice::AutoFallback),
                    Err(fallback_err) => {
                        return Err(video_error(
                            &err,
                            &fallback_err,
                            VideoDriverChoice::AutoKmsdrm,
                        ))
                    }
                }
            }
            Err(err) => return Err(video_error(&err, &err, choice)),
        };

        let timer = sdl.timer().map_err(|err| map_sdl_error("timer", err))?;
        let events = sdl
            .event()
            .map_err(|err| map_sdl_error("event subsystem", err))?;

        // Audio is optional: a CI container or a console without a sound driver
        // must not stop the engine from running. `AudioSystem` reports silent mode.
        let audio = match sdl.audio() {
            Ok(subsystem) => Some(subsystem),
            Err(err) => {
                log::warn!("audio subsystem unavailable: {err}");
                None
            }
        };

        // Gamepads are optional: a console without a controller attached, or a
        // build without joystick support, must not stop the engine from running.
        let game_controller = match sdl.game_controller() {
            Ok(subsystem) => Some(subsystem),
            Err(err) => {
                log::warn!("gamepad subsystem unavailable: {err}");
                None
            }
        };
        let event_pump = sdl
            .event_pump()
            .map_err(|err| map_sdl_error("event pump", err))?;

        log::info!("video driver: {}", video.current_video_driver());

        Ok(Platform {
            context: Rc::new(SdlContext {
                audio,
                video,
                timer,
                events,
                game_controller,
                sdl,
            }),
            event_pump: RefCell::new(event_pump),
            choice,
            _reserved: [0; 4],
        })
    }

    /// Name of the video driver SDL2 actually initialised (`KMSDRM`, `x11`,
    /// `wayland`, …).
    ///
    /// Note the spelling: SDL2 registers its console backend as `KMSDRM` in
    /// capitals, unlike the lower-case `x11` and `wayland`. Consumers that
    /// branch on this name must compare case-insensitively.
    pub fn video_driver_name(&self) -> &'static str {
        self.context.video.current_video_driver()
    }

    /// How the video driver was selected.
    pub fn video_driver_choice(&self) -> VideoDriverChoice {
        self.choice
    }

    /// Drains the pending SDL events.
    ///
    /// The returned vector is empty when nothing happened. It owns SDL events, so
    /// it stays internal: gameplay code consumes [`QuitSignal`] and, from phase 3
    /// on, [`crate::input::Input`].
    pub fn poll_events(&self) -> Vec<Event> {
        self.event_pump.borrow_mut().poll_iter().collect()
    }

    /// Pushes an SDL event into the queue as if the window system had produced it.
    ///
    /// Intended for automated tests, scripted demos and replay tooling; regular
    /// gameplay should only read events. Fails only if SDL rejects the event.
    pub fn push_event(&self, event: Event) -> Result<(), GiochiError> {
        self.context
            .events
            .push_event(event)
            .map_err(|err| GiochiError::Platform(format!("failed to push SDL event: {err}")))
    }

    /// Sleeps for `ms` milliseconds.
    pub fn delay_ms(&self, ms: u32) {
        if ms > 0 {
            self.context.timer.delay(ms);
        }
    }

    /// Milliseconds elapsed since the SDL timer subsystem started.
    pub fn ticks_ms(&self) -> u64 {
        self.context.timer.ticks64()
    }

    /// Creates the engine's main window.
    pub fn create_window(&self, config: WindowConfig, title: &str) -> Result<Window, GiochiError> {
        Window::new(self, config, title)
    }

    /// Returns the audio subsystem, or a descriptive error when it could not be
    /// initialised.
    ///
    /// [`crate::audio::AudioSystem::new`] uses this; when it fails the engine runs
    /// in silent mode instead of refusing to start.
    pub fn audio_subsystem(&self) -> Result<sdl2::AudioSubsystem, GiochiError> {
        self.context.audio.clone().ok_or(GiochiError::Audio(
            "the SDL2 audio subsystem is not available".to_owned(),
        ))
    }

    /// Returns the game controller subsystem, or a descriptive error when it could
    /// not be initialised.
    ///
    /// Keyboard and mouse input never depend on this; only
    /// [`crate::input::Input::gamepad`] does.
    pub fn game_controller_subsystem(&self) -> Result<&sdl2::GameControllerSubsystem, GiochiError> {
        self.context
            .game_controller
            .as_ref()
            .ok_or(GiochiError::Unsupported(
                "the game controller subsystem is not available",
            ))
    }

    pub(crate) fn sdl_context(&self) -> &Rc<SdlContext> {
        &self.context
    }

    /// Returns the SDL mouse utility, or `None` when the video subsystem has no mouse.
    ///
    /// Needed by the 3D walker's relative-mouse mode. A headless run has no pointer, so
    /// this is an `Option` rather than an error: capturing the mouse is an enhancement,
    /// never a requirement to start.
    pub fn sdl_mouse(&self) -> Option<sdl2::mouse::MouseUtil> {
        // Derived on demand rather than stored: `MouseUtil` is a zero-sized handle
        // borrowed from the `Sdl` context, and the 2D path never needs it.
        self.context.mouse()
    }

    /// Returns the SDL video subsystem.
    ///
    /// Needed by the 3D crate, which must set `SDL_GL_*` attributes *before* the
    /// window exists: those are process-global in SDL2, not per-window. The 2D path
    /// never calls this — it has no OpenGL context.
    pub fn video_subsystem(&self) -> &sdl2::VideoSubsystem {
        &self.context.video
    }
}

/// Initialises SDL2, selecting KMSDRM when the user has no preference.
///
/// Never overwrites an explicit `SDL_VIDEODRIVER`.
fn init_sdl() -> Result<(sdl2::Sdl, VideoDriverChoice), GiochiError> {
    let choice = if std::env::var_os(SDL_VIDEODRIVER_ENV).is_some() {
        VideoDriverChoice::Explicit
    } else {
        std::env::set_var(SDL_VIDEODRIVER_ENV, KMSDRM_VIDEO_DRIVER);
        VideoDriverChoice::AutoKmsdrm
    };

    let sdl = sdl2::init().map_err(|err| map_sdl_error("core", err))?;
    Ok((sdl, choice))
}

fn map_sdl_error(subsystem: &str, message: String) -> GiochiError {
    GiochiError::Platform(format!("failed to initialise SDL2 {subsystem}: {message}"))
}

fn video_error(primary: &str, secondary: &str, choice: VideoDriverChoice) -> GiochiError {
    let mut message = format!("failed to initialise the SDL2 video subsystem: {primary}");

    if primary != secondary {
        message.push_str(&format!(" (fallback also failed: {secondary})"));
    }

    if choice == VideoDriverChoice::AutoKmsdrm {
        message.push_str(&format!(
            "\nThe engine selected '{KMSDRM_VIDEO_DRIVER}' because {SDL_VIDEODRIVER_ENV} is unset. \
             On a desktop session set it explicitly, e.g. {SDL_VIDEODRIVER_ENV}=x11."
        ));
    }

    message.push('\n');
    message.push_str(&drm_diagnostics());
    GiochiError::Platform(message)
}

/// Human-readable description of the DRM state, used when KMSDRM fails.
fn drm_diagnostics() -> String {
    match std::fs::read_dir(DRM_DIRECTORY) {
        Err(err) => format!(
            "{DRM_DIRECTORY} is not accessible ({err}). The engine needs read/write access to a \
             DRM device; add the user to the 'video' and 'render' groups, or set \
             {HINT_KMSDRM_DEVICE_INDEX} to a device the process may open."
        ),
        Ok(entries) => {
            let mut devices: Vec<String> = entries
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect();
            devices.sort();

            if devices.is_empty() {
                format!(
                    "{DRM_DIRECTORY} exists but contains no DRM device. Check that the kernel \
                     module is loaded and that {HINT_KMSDRM_DEVICE_INDEX} points at the right one."
                )
            } else {
                format!(
                    "DRM devices found in {DRM_DIRECTORY}: {}. If another process holds DRM \
                     master (a running desktop session does), stop it or run the engine from a \
                     virtual console. {HINT_KMSDRM_REQUIRE_DRM_MASTER} controls whether the \
                     backend insists on being master.",
                    devices.join(", ")
                )
            }
        }
    }
}

/// Returns `true` when [`DRM_DIRECTORY`] exposes at least one device.
pub fn has_drm_device() -> bool {
    Path::new(DRM_DIRECTORY)
        .read_dir()
        .map(|mut entries| entries.any(|entry| entry.is_ok()))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quit_signal_is_none_for_unrelated_events() {
        let event = Event::KeyDown {
            keycode: Some(sdl2::keyboard::Keycode::A),
            scancode: None,
            keymod: sdl2::keyboard::Mod::NOMOD,
            repeat: false,
            timestamp: 0,
            window_id: 0,
        };
        assert_eq!(quit_signal(&event), QuitSignal::None);
    }

    #[test]
    fn escape_maps_to_escape_signal() {
        let event = Event::KeyDown {
            keycode: Some(sdl2::keyboard::Keycode::Escape),
            scancode: None,
            keymod: sdl2::keyboard::Mod::NOMOD,
            repeat: false,
            timestamp: 0,
            window_id: 0,
        };
        assert_eq!(quit_signal(&event), QuitSignal::Escape);
    }

    #[test]
    fn repeated_escape_is_not_a_quit_signal() {
        let event = Event::KeyDown {
            keycode: Some(sdl2::keyboard::Keycode::Escape),
            scancode: None,
            keymod: sdl2::keyboard::Mod::NOMOD,
            repeat: true,
            timestamp: 0,
            window_id: 0,
        };
        assert_eq!(quit_signal(&event), QuitSignal::None);
    }

    #[test]
    fn window_close_is_a_quit_signal() {
        let event = Event::Quit { timestamp: 0 };
        assert_eq!(quit_signal(&event), QuitSignal::WindowClose);
    }

    #[test]
    fn drm_diagnostics_never_returns_an_empty_string() {
        assert!(!drm_diagnostics().is_empty());
    }

    #[test]
    fn default_quit_signal_is_none() {
        assert_eq!(QuitSignal::default(), QuitSignal::None);
    }
}
