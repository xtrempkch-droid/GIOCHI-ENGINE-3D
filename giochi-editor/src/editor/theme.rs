//! The editor's look: a dark, neutral palette that keeps the scene legible.
//!
//! The engine's [`giochi_engine::ui::Theme`] is for *games*: three presets a player can
//! read a menu with. An editor needs the opposite of a preset — one consistent surface
//! that never competes with the scene being edited — so this is its own palette rather
//! than a fourth game theme. The viewport keeps the darkest fill on purpose: content,
//! not chrome, is the bright thing on screen.

use egui::{Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Visuals};

/// Colours, spacing and type scale for the editor's chrome.
///
/// Palette values are `Color32` rather than the engine's packed `u32` because the theme
/// is never seen by a C caller — it is applied to an [`egui::Context`] and nothing else.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EditorTheme {
    /// Window background, behind every panel.
    pub background: Color32,
    /// Panel backgrounds.
    pub panel: Color32,
    /// Inputs, headers and inactive buttons.
    pub surface: Color32,
    /// Hovered surfaces.
    pub surface_hover: Color32,
    /// Text and icons.
    pub foreground: Color32,
    /// Secondary text: empty-panel hints, phase notes.
    pub muted: Color32,
    /// Selection, focused borders, the active tool.
    pub accent: Color32,
    /// Destructive actions and errors.
    pub danger: Color32,
    /// The viewport's clear colour.
    pub viewport: Color32,
    /// Grid lines in the viewport.
    pub grid: Color32,
    /// Padding inside panels and between widgets, in points.
    pub spacing: f32,
    /// Corner radius in points.
    pub rounding: u8,
}

impl EditorTheme {
    /// The default editor theme: near-black chrome, blue accent, dark viewport.
    pub fn dark() -> Self {
        Self {
            background: Color32::from_rgb(0x0d, 0x0f, 0x14),
            panel: Color32::from_rgb(0x13, 0x17, 0x20),
            surface: Color32::from_rgb(0x1b, 0x20, 0x2b),
            surface_hover: Color32::from_rgb(0x26, 0x2d, 0x3b),
            foreground: Color32::from_rgb(0xdd, 0xe5, 0xf2),
            muted: Color32::from_rgb(0x8b, 0x97, 0xab),
            accent: Color32::from_rgb(0x4c, 0x8d, 0xff),
            danger: Color32::from_rgb(0xff, 0x5d, 0x5d),
            viewport: Color32::from_rgb(0x08, 0x09, 0x0d),
            grid: Color32::from_rgb(0x2a, 0x31, 0x40),
            spacing: 8.0,
            rounding: 5,
        }
    }

    /// The name a preferences file will use (phase 40).
    pub const fn name(&self) -> &'static str {
        "editor-dark"
    }

    /// Applies the palette and the type scale to `ctx`.
    ///
    /// Called once per frame; egui compares the style and only rebuilds when it changed,
    /// so calling it every frame costs a comparison.
    pub fn apply(&self, ctx: &egui::Context) {
        let mut visuals = Visuals::dark();
        visuals.override_text_color = Some(self.foreground);
        visuals.panel_fill = self.panel;
        visuals.window_fill = self.panel;
        visuals.extreme_bg_color = self.background;
        visuals.faint_bg_color = self.surface;
        visuals.selection.bg_fill = self.accent.gamma_multiply(0.45);
        visuals.selection.stroke = Stroke::new(1.0_f32, self.accent);
        visuals.hyperlink_color = self.accent;
        visuals.warn_fg_color = self.danger;
        visuals.error_fg_color = self.danger;
        visuals.widgets.noninteractive.bg_fill = self.panel;
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, self.foreground);
        visuals.widgets.inactive.bg_fill = self.surface;
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, self.foreground);
        visuals.widgets.hovered.bg_fill = self.surface_hover;
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.5_f32, self.accent);
        visuals.widgets.active.bg_fill = self.surface_hover;
        visuals.widgets.active.fg_stroke = Stroke::new(1.5_f32, self.accent);
        visuals.widgets.open.bg_fill = self.surface_hover;

        let rounding = CornerRadius::same(self.rounding);
        for widget in [
            &mut visuals.widgets.noninteractive,
            &mut visuals.widgets.inactive,
            &mut visuals.widgets.hovered,
            &mut visuals.widgets.active,
            &mut visuals.widgets.open,
        ] {
            widget.corner_radius = rounding;
        }
        visuals.window_corner_radius = rounding;
        ctx.set_visuals(visuals);

        let mut style = (*ctx.style()).clone();
        style.spacing.item_spacing = egui::vec2(self.spacing, self.spacing * 0.6);
        style.spacing.button_padding = egui::vec2(self.spacing, self.spacing * 0.35);
        style.spacing.window_margin = egui::Margin::same(self.spacing as i8);

        let size = 13.5;
        let family = FontFamily::Proportional;
        style.text_styles = [
            (TextStyle::Heading, FontId::new(size * 1.35, family.clone())),
            (TextStyle::Body, FontId::new(size, family.clone())),
            (
                TextStyle::Monospace,
                FontId::new(size, FontFamily::Monospace),
            ),
            (TextStyle::Button, FontId::new(size, family.clone())),
            (TextStyle::Small, FontId::new(size * 0.85, family)),
        ]
        .into();
        ctx.set_style(style);
    }
}

impl Default for EditorTheme {
    fn default() -> Self {
        Self::dark()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_viewport_is_darker_than_the_panels() {
        let theme = EditorTheme::dark();
        let viewport = theme.viewport.r() as u16 + theme.viewport.g() as u16;
        let panel = theme.panel.r() as u16 + theme.panel.g() as u16;
        assert!(viewport < panel, "content must be the darkest surface");
        assert!(theme.panel.r() < theme.foreground.r());
    }

    #[test]
    fn applying_the_theme_reaches_the_context() {
        let ctx = egui::Context::default();
        let theme = EditorTheme::dark();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            theme.apply(ctx);
            let style = ctx.style();
            assert_eq!(style.visuals.panel_fill, theme.panel);
            assert_eq!(style.visuals.override_text_color, Some(theme.foreground));
            assert_eq!(style.spacing.item_spacing.x, theme.spacing);
        });
    }

    #[test]
    fn dark_is_the_default_and_has_a_stable_name() {
        assert_eq!(EditorTheme::default(), EditorTheme::dark());
        assert_eq!(EditorTheme::dark().name(), "editor-dark");
    }
}
