//! The expanded debug overlay: what the phase-15 panel does not show.
//!
//! The base panel answers "how fast is a frame". This one answers the questions you ask
//! *after* that: how much video memory is in use, how many entities are alive, which
//! commands exist, whether hot reload is on and what it has done.
//!
//! It is a pure function from state to lines, which is what makes it testable: no
//! renderer, no window, no timing.

/// Everything the expanded section reports, gathered by the caller.
///
/// A plain struct rather than a pile of arguments, because the panel grows and a call site
/// with eight booleans is unreadable.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct OverlayExtra {
    /// Draw calls issued this frame, total.
    pub draw_calls: u32,
    /// Sprites drawn this frame.
    pub sprites_drawn: u32,
    /// Meshes drawn this frame.
    pub meshes_drawn: u32,
    /// Live textures in the 2D registry.
    pub textures: u32,
    /// Estimated VRAM in bytes: `sum(width * height * 4)` over the live textures.
    pub texture_vram_bytes: u64,
    /// Entities the game reported through `DevTools::set_entity_count`.
    pub entities: u32,
    /// Commands registered in the console.
    pub commands: u32,
    /// Whether the console is on screen.
    pub console_open: bool,
    /// Whether debug geometry is being drawn.
    pub debug_draw: bool,
    /// The current simulation time scale.
    pub time_scale: f32,
    /// Whether hot reload is compiled in and enabled.
    pub hot_reload: bool,
    /// Events applied by the hot reloader, if it is compiled in.
    pub reloads_applied: u64,
    /// `true` when `god` is on, `false` otherwise; a game decides what it means.
    pub god_mode: bool,
    /// Reserved for future ABI-compatible fields. Must be zero.
    pub _reserved: [u32; 4],
}

impl OverlayExtra {
    /// A zeroed value with `time_scale` at 1.0.
    pub fn new() -> Self {
        Self {
            time_scale: 1.0,
            ..Self::default()
        }
    }
}

/// Builds the expanded section's lines.
///
/// Kept upper-case like the base panel: the bitmap font has no lower case, so anything
/// else would be rendered as upper case anyway and the source would lie about it.
///
/// Deliberately takes no [`FrameStats`](crate::perf::FrameStats): the panel above already
/// prints the frame numbers, and asking for a summary here forced the engine to compute a
/// percentile window on every frame — nine allocations per empty frame, which the
/// regression test caught (`docs/PERF_LOG.md`, "After Phase 20").
pub(crate) fn lines(extra: &OverlayExtra) -> Vec<String> {
    let mut lines = Vec::with_capacity(12);

    // Draw calls split by the category that produced them. `draw_calls` is the total, so
    // the UI figure is what is left over; reporting it as a subtraction keeps the panel
    // honest when a new category appears.
    let ui = extra
        .draw_calls
        .saturating_sub(extra.sprites_drawn.saturating_add(extra.meshes_drawn));
    lines.push(format!(
        "DRAW {} SPR {} MSH {} UI {}",
        extra.draw_calls, extra.sprites_drawn, extra.meshes_drawn, ui
    ));

    lines.push(format!(
        "TEX {} VRAM {}",
        extra.textures,
        memory_label(extra.texture_vram_bytes)
    ));
    lines.push(format!("ENTITIES {}", extra.entities));
    lines.push(format!(
        "CMDS {} CONSOLE {} DRAW3D {}",
        extra.commands,
        on_off(extra.console_open),
        on_off(extra.debug_draw)
    ));
    lines.push(format!("TIME_SCALE {:.2}", extra.time_scale));
    lines.push(format!(
        "HOT_RELOAD {} RELOADS {}",
        on_off(extra.hot_reload),
        extra.reloads_applied
    ));
    if extra.god_mode {
        lines.push("GOD ON".to_owned());
    }

    lines
}

fn on_off(value: bool) -> &'static str {
    if value {
        "ON"
    } else {
        "OFF"
    }
}

/// Formats bytes as MiB/KiB with one decimal, or `0 B`.
fn memory_label(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    let bytes_f = bytes as f64;
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes_f / MIB)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes_f / KIB)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_extra_starts_at_time_scale_one_and_zero_everything_else() {
        let extra = OverlayExtra::new();
        assert_eq!(extra.time_scale, 1.0);
        assert_eq!(extra.textures, 0);
        assert_eq!(extra.entities, 0);
        assert_eq!(extra._reserved, [0; 4]);
    }

    #[test]
    fn every_field_reaches_the_panel() {
        let mut extra = OverlayExtra::new();
        extra.textures = 7;
        extra.texture_vram_bytes = 2 * 1024 * 1024;
        extra.entities = 42;
        extra.commands = 12;
        extra.console_open = true;
        extra.debug_draw = true;
        extra.time_scale = 0.5;
        extra.hot_reload = true;
        extra.reloads_applied = 3;
        extra.god_mode = true;

        let text = lines(&extra).join("\n");
        assert!(text.contains("TEX 7 VRAM 2.0 MB"), "{text}");
        assert!(text.contains("ENTITIES 42"), "{text}");
        assert!(text.contains("CMDS 12 CONSOLE ON DRAW3D ON"), "{text}");
        assert!(text.contains("TIME_SCALE 0.50"), "{text}");
        assert!(text.contains("HOT_RELOAD ON RELOADS 3"), "{text}");
        assert!(text.contains("GOD ON"), "{text}");
    }

    #[test]
    fn god_is_absent_when_off_rather_than_off_on_its_own_line() {
        let extra = OverlayExtra::new();
        assert!(!lines(&extra).iter().any(|line| line.starts_with("GOD")));
    }

    #[test]
    fn the_draw_split_subtracts_rather_than_guessing() {
        let mut extra = OverlayExtra::new();
        extra.textures = 1;
        extra.draw_calls = 100;
        extra.sprites_drawn = 60;
        extra.meshes_drawn = 30;
        let text = lines(&extra).join("\n");
        assert!(text.contains("DRAW 100 SPR 60 MSH 30 UI 10"), "{text}");
    }

    #[test]
    fn the_draw_split_never_underflows_when_counters_disagree() {
        let mut extra = OverlayExtra::new();
        extra.textures = 1;
        // A build without the `profiling` feature reports zeros; a mismatch can also
        // happen when a draw is counted outside the scene.
        extra.draw_calls = 2;
        extra.sprites_drawn = 10;
        extra.meshes_drawn = 10;
        let text = lines(&extra).join("\n");
        assert!(text.contains("UI 0"), "{text}");
    }

    #[test]
    fn memory_is_formatted_in_the_unit_that_fits() {
        assert_eq!(memory_label(0), "0 B");
        assert_eq!(memory_label(512), "512 B");
        assert_eq!(memory_label(2048), "2.0 KB");
        assert_eq!(memory_label(3 * 1024 * 1024), "3.0 MB");
    }
}
