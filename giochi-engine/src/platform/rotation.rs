//! Panel rotation for consoles whose screen is mounted turned.
//!
//! Some handhelds have a **portrait** panel that the user reads landscape — the Powkiddy
//! x55 is the measured case (720×1280 DSI panel, `fbcon=rotate:3`). The kernel option turns
//! the *console text*, not an application's output, so a KMS/GBM app that draws an upright
//! image into the 720×1280 buffer appears rotated on the physical screen.
//!
//! `docs/KMSDRM.md` §9 is the measurement and the two candidate fixes. This module is the
//! first half of both: **read the angle from the system instead of from a device name**,
//! exactly as the GIOCHI-OS launcher does. Applying it belongs to whoever owns the final
//! present — the 3D renderer's upscale pass, or the primary plane's `rotation` property —
//! and is phase 45/46 work, deliberately not guessed here.
//!
//! ```no_run
//! use giochi_engine::platform::{panel_rotation, PanelRotation};
//!
//! let rotation = panel_rotation();
//! if rotation != PanelRotation::None {
//!     let (width, height) = rotation.apply_to_size((1280, 720));
//!     // draw into a `width`x`height` target, then present it turned by `rotation`
//! }
//! ```

use std::fmt;
use std::path::Path;

/// Where the kernel exposes the console rotation, `0`–`3`.
///
/// The same path the GIOCHI-OS launcher reads (`/sys/class/graphics/fbcon/rotate`).
pub const FBCON_ROTATE_PATH: &str = "/sys/class/graphics/fbcon/rotate";

/// How much the application must turn its own output so the picture is upright.
///
/// The kernel's `fbcon=rotate:N` turns the console by `N` quarter turns; an application
/// that ignores it therefore has to undo the same amount. The mapping is by measurement,
/// not by guesswork: the x55 reports `3` and the launcher's note says its apps need a
/// **counter-clockwise** quarter turn, which is `270` clockwise — [`Self::QuarterTurnCcw`].
///
/// # C ABI
///
/// Not exposed. A C consumer reads it through the Rust facade or repeats the one-line
/// sysfs read; no `giochi_*` symbol is added in this pass.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PanelRotation {
    /// No rotation: the ordinary case, and what every desktop reports.
    #[default]
    None = 0,
    /// Turn the drawing 90° clockwise.
    QuarterTurnCw = 1,
    /// Turn the drawing 180°.
    HalfTurn = 2,
    /// Turn the drawing 90° counter-clockwise (the x55 case, `fbcon=rotate:3`).
    QuarterTurnCcw = 3,
}

impl PanelRotation {
    /// Maps the value read from `/sys/class/graphics/fbcon/rotate` to the rotation an
    /// application must apply.
    ///
    /// `None` (file absent, unreadable) and out-of-range values both mean
    /// [`Self::None`]: the safe default is an unrotated picture, because a wrong rotation
    /// is worse than a missing one — a desktop without that sysfs file is every
    /// development machine.
    pub const fn from_fbcon(value: Option<u8>) -> Self {
        match value {
            Some(1) => Self::QuarterTurnCw,
            Some(2) => Self::HalfTurn,
            Some(3) => Self::QuarterTurnCcw,
            _ => Self::None,
        }
    }

    /// `true` when the width and height of a target have to be swapped.
    pub const fn swaps_dimensions(self) -> bool {
        matches!(self, Self::QuarterTurnCw | Self::QuarterTurnCcw)
    }

    /// `true` for the no-op rotation, so callers can skip an entire pass.
    pub const fn is_identity(self) -> bool {
        matches!(self, Self::None)
    }

    /// The size of the buffer an application must render into before turning it.
    ///
    /// For a quarter turn the panel is `panel_width`×`panel_height` but the drawing is
    /// the other way round, which is what makes a 720×1280 panel usable as a 1280×720
    /// landscape screen.
    pub const fn apply_to_size(self, panel_size: (u32, u32)) -> (u32, u32) {
        let (width, height) = panel_size;
        if self.swaps_dimensions() {
            (height, width)
        } else {
            (width, height)
        }
    }
}

impl fmt::Display for PanelRotation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::None => "none",
            Self::QuarterTurnCw => "90° clockwise",
            Self::HalfTurn => "180°",
            Self::QuarterTurnCcw => "90° counter-clockwise",
        };
        f.write_str(text)
    }
}

/// The rotation the current panel needs, read from the kernel.
///
/// Returns [`PanelRotation::None`] on any desktop or console whose panel is mounted the
/// right way up. Never panics and never fails: a missing file is the normal case, not an
/// error.
pub fn panel_rotation() -> PanelRotation {
    panel_rotation_from_path(Path::new(FBCON_ROTATE_PATH))
}

/// [`panel_rotation`] with the file named, so the reader is testable without sysfs.
pub fn panel_rotation_from_path(path: &Path) -> PanelRotation {
    PanelRotation::from_fbcon(fbcon_rotate_value_from_path(path))
}

/// The raw `fbcon` value, for diagnostics and logs.
///
/// `None` when the file is absent or does not hold a number. Kept separate from
/// [`panel_rotation`] so a caller can distinguish "reports 0" from "does not report".
pub fn fbcon_rotate_value() -> Option<u8> {
    fbcon_rotate_value_from_path(Path::new(FBCON_ROTATE_PATH))
}

/// [`fbcon_rotate_value`] with the file named.
pub fn fbcon_rotate_value_from_path(path: &Path) -> Option<u8> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| text.trim().parse::<u8>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_file(contents: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "giochi-fbcon-rotate-{}-{:?}.txt",
            std::process::id(),
            std::thread::current().id()
        ));
        let mut file = std::fs::File::create(&path).expect("create temp file");
        file.write_all(contents.as_bytes())
            .expect("write temp file");
        path
    }

    #[test]
    fn zero_and_absent_mean_no_rotation() {
        assert_eq!(PanelRotation::from_fbcon(Some(0)), PanelRotation::None);
        assert_eq!(PanelRotation::from_fbcon(None), PanelRotation::None);
    }

    #[test]
    fn the_x55_value_three_is_a_counter_clockwise_quarter_turn() {
        assert_eq!(
            PanelRotation::from_fbcon(Some(3)),
            PanelRotation::QuarterTurnCcw
        );
    }

    #[test]
    fn one_and_two_are_the_clockwise_quarter_turn_and_the_half_turn() {
        assert_eq!(
            PanelRotation::from_fbcon(Some(1)),
            PanelRotation::QuarterTurnCw
        );
        assert_eq!(PanelRotation::from_fbcon(Some(2)), PanelRotation::HalfTurn);
    }

    #[test]
    fn out_of_range_values_fall_back_to_no_rotation() {
        for value in [4, 5, 42, u8::MAX] {
            assert_eq!(PanelRotation::from_fbcon(Some(value)), PanelRotation::None);
        }
    }

    #[test]
    fn a_quarter_turn_swaps_the_drawing_size() {
        let portrait = (720, 1280);
        assert_eq!(
            PanelRotation::QuarterTurnCcw.apply_to_size(portrait),
            (1280, 720)
        );
        assert_eq!(
            PanelRotation::QuarterTurnCw.apply_to_size(portrait),
            (1280, 720)
        );
        assert_eq!(PanelRotation::HalfTurn.apply_to_size(portrait), portrait);
        assert_eq!(PanelRotation::None.apply_to_size(portrait), portrait);
    }

    #[test]
    fn only_the_quarter_turns_swap_dimensions() {
        assert!(PanelRotation::QuarterTurnCw.swaps_dimensions());
        assert!(PanelRotation::QuarterTurnCcw.swaps_dimensions());
        assert!(!PanelRotation::HalfTurn.swaps_dimensions());
        assert!(!PanelRotation::None.swaps_dimensions());
    }

    #[test]
    fn identity_is_only_none() {
        assert!(PanelRotation::None.is_identity());
        for rotation in [
            PanelRotation::QuarterTurnCw,
            PanelRotation::HalfTurn,
            PanelRotation::QuarterTurnCcw,
        ] {
            assert!(!rotation.is_identity());
        }
    }

    #[test]
    fn the_file_is_read_and_trimmed() {
        let path = temp_file("3\n");
        assert_eq!(
            panel_rotation_from_path(&path),
            PanelRotation::QuarterTurnCcw
        );
        assert_eq!(fbcon_rotate_value_from_path(&path), Some(3));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_missing_file_is_no_rotation_rather_than_an_error() {
        let path = std::path::Path::new("/nonexistent/giochi/fbcon/rotate");
        assert_eq!(panel_rotation_from_path(path), PanelRotation::None);
        assert_eq!(fbcon_rotate_value_from_path(path), None);
    }

    #[test]
    fn text_that_is_not_a_number_is_no_rotation() {
        let path = temp_file("landscape\n");
        assert_eq!(panel_rotation_from_path(&path), PanelRotation::None);
        assert_eq!(fbcon_rotate_value_from_path(&path), None);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn display_mentions_the_angle() {
        assert_eq!(PanelRotation::None.to_string(), "none");
        assert!(PanelRotation::QuarterTurnCcw
            .to_string()
            .contains("counter"));
    }
}
