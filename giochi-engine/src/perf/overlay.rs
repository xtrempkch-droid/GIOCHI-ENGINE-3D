//! In-game debug overlay.
//!
//! The overlay is deliberately implemented on `sdl2::render` instead of `egui` or
//! `imgui`: the engine's 2D path is an SDL renderer (including `kmsdrm`), and both GUI
//! crates bring an OpenGL integration and a dependency tree that the production image
//! does not otherwise need. A bitmap font rasterised into one texture per refresh draws
//! the same numbers with one draw call and no new dependency (ADR-042).
//!
//! [`DebugOverlay::draw`] is a no-op without the `profiling` feature, so the engine can
//! call it unconditionally.

use std::time::{Duration, Instant};

use crate::math::{Color, Rect};
use crate::perf::text::rasterize;
use crate::perf::{FrameStats, FrameTimeSummary, Profiler};
use crate::render::{RenderContext, TextureHandle};

/// How often the panel refreshes its content. The numbers stay readable at 1 Hz — the
/// FPS figure is already an average over the last 60 frames — and rebuilding more
/// often would make the profiler a measurable part of the frame it is supposed to be
/// observing. One refresh per second keeps the panel inside the 2 % budget on the
/// Mali-G31; at 4 Hz it pushed the 1000-sprite run over it (ADR-042).
const REFRESH_INTERVAL: Duration = Duration::from_millis(1_000);

/// Distance from the top-left corner, in pixels.
const MARGIN: f32 = 8.0;

const BACKGROUND: [u8; 4] = [8, 10, 18, 232];
const FOREGROUND: [u8; 4] = [222, 232, 246, 255];
const BORDER: Color = Color::rgb(74, 108, 158);

/// A toggleable panel showing the profiler's latest frame and window summary.
///
/// The engine owns one and toggles it with `F1`. A host that drives a `RenderContext`
/// directly can own one too:
///
/// ```no_run
/// use giochi_engine::perf::{DebugOverlay, Profiler};
/// # use giochi_engine::render::RenderContext;
/// # fn example(render: &mut RenderContext, profiler: &Profiler) {
/// let mut overlay = DebugOverlay::new();
/// overlay.draw(render, profiler);
/// # }
/// ```
///
/// Draw it **after** the frame's stats have been taken and **before** `present`, so the
/// panel itself is not counted in the numbers it displays.
///
/// An overlay belongs to the `RenderContext` it first drew into: it keeps one panel
/// texture alive there and updates it in place. Dropping the overlay does not free that
/// texture (there is no context available at drop time); the context releases every
/// texture when it is dropped, which is the same ownership rule as the rest of the
/// renderer.
///
/// # C ABI
///
/// Not exposed; the overlay is a developer tool, not engine state.
#[repr(C)]
pub struct DebugOverlay {
    visible: bool,
    last_update: Option<Instant>,
    /// Panel texture, kept alive between refreshes and updated in place.
    texture: TextureHandle,
    width: u32,
    height: u32,
    _reserved: [u32; 4],
}

impl std::fmt::Debug for DebugOverlay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DebugOverlay")
            .field("visible", &self.visible)
            .field("size", &(self.width, self.height))
            .finish_non_exhaustive()
    }
}

impl DebugOverlay {
    /// Creates a visible overlay when the `profiling` feature is enabled, and an
    /// inert one otherwise.
    pub fn new() -> Self {
        Self {
            visible: cfg!(feature = "profiling"),
            last_update: None,
            texture: 0,
            width: 0,
            height: 0,
            _reserved: [0; 4],
        }
    }

    /// Whether the panel is currently shown.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Shows or hides the panel.
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    /// Flips the panel's visibility. This is what `F1` does.
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    /// Size of the last rendered panel, or `(0, 0)` before the first draw.
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Draws the panel, if it is visible and the refresh interval has elapsed.
    ///
    /// The sample and the window summary are read **after** the throttle check, so a
    /// hidden or between-refresh panel costs one `Instant` comparison and nothing else.
    /// Errors from texture creation or drawing are logged and the frame continues: a
    /// developer tool must never take the game down.
    pub fn draw(&mut self, render: &mut RenderContext, profiler: &Profiler) {
        self.draw_with(render, profiler, &mut Vec::new);
    }

    /// Draws the panel with `extra` lines appended (phase 20's expanded overlay).
    ///
    /// `extra` is a **builder**, not a slice, and it is called only when the panel actually
    /// refreshes (1 Hz). Building those lines on every frame cost nine allocations per
    /// empty frame, which the frame-budget regression test caught — and that test exists
    /// precisely so a debug tool cannot quietly become a per-frame cost.
    pub fn draw_with(
        &mut self,
        render: &mut RenderContext,
        profiler: &Profiler,
        extra: &mut dyn FnMut() -> Vec<String>,
    ) {
        if !self.visible || !cfg!(feature = "profiling") {
            return;
        }
        if self
            .last_update
            .is_some_and(|last| last.elapsed() < REFRESH_INTERVAL)
        {
            return;
        }

        let stats = profiler.snapshot();
        let summary = profiler.summary();

        // A 5x7 glyph at scale 2 is comfortable on a 1280-wide desktop. A 640x480
        // console panel gets scale 1: the text is still legible at arm's length, and
        // the upload is four times smaller — which is the difference between fitting
        // the 2 % profiling budget and not (measured on the Mali-G31, ADR-042).
        let scale = if render.size().0 < 800 { 1 } else { 2 };
        let mut lines = lines(stats, summary);
        let extra = extra();
        if !extra.is_empty() {
            lines.push(String::new());
            lines.extend(extra);
        }
        let Some(text) = rasterize(&lines, scale, BACKGROUND, FOREGROUND) else {
            return;
        };

        if !self.upload(render, &text) {
            return;
        }

        let destination = Rect::new(MARGIN, MARGIN, text.width as f32, text.height as f32);
        if let Err(error) = render.draw_texture(self.texture, destination, Rect::ZERO) {
            log::warn!("debug overlay: could not draw the panel: {error}");
        }
        render.stroke_rect(destination, BORDER);

        self.width = text.width;
        self.height = text.height;
        self.last_update = Some(Instant::now());
    }

    /// Creates the panel texture once, then updates it in place on every refresh.
    ///
    /// The create/destroy pair used by the first version allocated a driver texture at
    /// 4 Hz; on the Mali-G31 that was measured at roughly 10 % of the frame. An in-place
    /// `SDL_UpdateTexture` is just an upload. If the size changes (a wider FPS value) or
    /// the texture is still shared with a renderer, the texture is replaced instead.
    fn upload(&mut self, render: &mut RenderContext, text: &crate::perf::text::RasterText) -> bool {
        if self.texture != 0 && self.width == text.width && self.height == text.height {
            match render.update_texture_rgba(self.texture, text.width, text.height, &text.pixels) {
                Ok(()) => return true,
                Err(error) => {
                    log::debug!(
                        "debug overlay: updating the panel texture failed ({error}); recreating"
                    );
                }
            }
        }

        let handle = match render.create_texture_from_rgba(text.width, text.height, &text.pixels) {
            Ok(handle) => handle,
            Err(error) => {
                log::warn!("debug overlay: could not create the text texture: {error}");
                return false;
            }
        };
        if self.texture != 0 {
            render.destroy_texture(self.texture);
        }
        self.texture = handle;
        true
    }
}

impl Default for DebugOverlay {
    fn default() -> Self {
        Self::new()
    }
}

/// Builds the panel's text. All labels are upper-case because the bitmap font is.
fn lines(stats: FrameStats, summary: FrameTimeSummary) -> Vec<String> {
    let mut lines = Vec::with_capacity(14);
    lines.push("GIOCHI PERF".to_owned());
    lines.push(format!("FPS {:.1}", summary.fps));
    lines.push(format!("FRAME {:.2} MS", stats.frame_time_ms()));
    lines.push(format!("P99 {:.2} MS", summary.p99_ms()));
    lines.push(format!("CPU {:.2} MS", stats.cpu_time_ms()));
    lines.push(format!("DRAW {}", stats.draw_calls));
    lines.push(format!("SPRITES {}", stats.sprites_drawn));
    lines.push(format!("MESHES {}", stats.meshes_drawn));
    lines.push(format!("VERTS {}", stats.vertices_uploaded));
    lines.push(format!("UPDATE {:.2} MS", stats.update_time_ms()));
    lines.push(format!("RENDER2D {:.2} MS", stats.render2d_time_ms()));
    lines.push(format!("RENDER3D {:.2} MS", stats.render3d_time_ms()));
    lines.push(format!("AUDIO {:.2} MS", stats.audio_time_ms()));
    lines.push(memory_label(resident_memory_bytes()));
    lines
}

/// Formats resident memory, or `MEM N/A` where `/proc` is unavailable.
fn memory_label(bytes: u64) -> String {
    if bytes == 0 {
        return "MEM N/A".to_owned();
    }
    const MIB: f64 = 1024.0 * 1024.0;
    if bytes >= 1024 * 1024 * 1024 {
        format!("MEM {:.1} GB", bytes as f64 / (MIB * 1024.0))
    } else {
        format!("MEM {:.1} MB", bytes as f64 / MIB)
    }
}

/// Reads `VmRSS` from `/proc/self/status`, in bytes.
///
/// Returns `0` on any other operating system or when the file is unreadable; the panel
/// then shows `MEM N/A` rather than inventing a number.
fn resident_memory_bytes() -> u64 {
    match std::fs::read_to_string("/proc/self/status") {
        Ok(status) => parse_vm_rss(&status).unwrap_or(0),
        Err(_) => 0,
    }
}

/// Parses a `/proc/self/status` document, returning `VmRSS` in bytes.
fn parse_vm_rss(status: &str) -> Option<u64> {
    for line in status.lines() {
        let Some(rest) = line.strip_prefix("VmRSS:") else {
            continue;
        };
        let kilobytes: u64 = rest.split_whitespace().next()?.parse().ok()?;
        return Some(kilobytes.saturating_mul(1024));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_overlay_starts_visible_only_when_profiling_is_compiled() {
        let overlay = DebugOverlay::new();

        assert_eq!(overlay.is_visible(), cfg!(feature = "profiling"));
        assert_eq!(overlay.size(), (0, 0));
    }

    #[test]
    fn toggling_flips_visibility() {
        let mut overlay = DebugOverlay::new();
        let before = overlay.is_visible();

        overlay.toggle();
        assert_eq!(overlay.is_visible(), !before);
        overlay.toggle();
        assert_eq!(overlay.is_visible(), before);
    }

    #[test]
    fn vm_rss_is_parsed_in_bytes() {
        let status = "Name:\tgiochi\nVmPeak:\t  102400 kB\nVmRSS:\t   20480 kB\n";

        assert_eq!(parse_vm_rss(status), Some(20_480 * 1024));
        assert_eq!(parse_vm_rss("Name:\tgiochi\n"), None);
        assert_eq!(parse_vm_rss("VmRSS:\tnot a number kB\n"), None);
    }

    #[test]
    fn the_memory_label_is_readable() {
        assert_eq!(memory_label(0), "MEM N/A");
        assert_eq!(memory_label(48 * 1024 * 1024), "MEM 48.0 MB");
        assert_eq!(memory_label(2 * 1024 * 1024 * 1024), "MEM 2.0 GB");
    }

    #[test]
    fn the_panel_has_the_documented_rows() {
        let stats = FrameStats {
            frame_time_ns: 16_000_000,
            cpu_time_ns: 2_000_000,
            draw_calls: 3,
            sprites_drawn: 10,
            meshes_drawn: 1,
            vertices_uploaded: 36,
            ..FrameStats::new()
        };
        let summary = FrameTimeSummary {
            samples: 60,
            fps: 60.0,
            p99_ns: 18_000_000,
            ..FrameTimeSummary::default()
        };

        let lines = lines(stats, summary);

        assert_eq!(lines.len(), 14);
        assert_eq!(lines[0], "GIOCHI PERF");
        assert_eq!(lines[1], "FPS 60.0");
        assert_eq!(lines[2], "FRAME 16.00 MS");
        assert_eq!(lines[5], "DRAW 3");
        assert_eq!(lines[6], "SPRITES 10");
    }
}
