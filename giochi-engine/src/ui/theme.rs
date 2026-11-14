//! Colours, spacing and text styles for the widget layer.
//!
//! A theme is a plain value, not a global: the engine applies it to the egui context when
//! it draws the UI, so two scenes can use different themes and a screenshot taken with
//! `Theme::retro()` is reproducible.
//!
//! # Why three presets
//!
//! `Dark` is the default and the one the console uses. `Light` exists because a game with a
//! bright art style needs a UI that does not disappear into it. `Retro` is the plan's
//! "verde fosforescente": a CRT palette with a monospace face, which is also the honest
//! test that the theme is doing something — if switching to it changes nothing on screen,
//! the widgets are not reading the theme.

use egui::{Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Visuals};

/// A palette and a type scale.
///
/// ```no_run
/// use giochi_engine::ui::Theme;
///
/// let retro = Theme::retro();
/// assert!(retro.name() == "retro");
/// ```
///
/// # C ABI
///
/// Not exposed. A C consumer gets the default theme; phase 24 can add a
/// `giochi_ui_set_theme` over the same three values.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeKind {
    /// Near-black surfaces with a blue-grey accent.
    Dark = 0,
    /// Near-white surfaces with a dark accent.
    Light = 1,
    /// Black surfaces with a phosphor-green accent and a monospace face.
    Retro = 2,
}

impl ThemeKind {
    /// Every preset, in the order `Theme::presets()` returns them.
    pub const ALL: [ThemeKind; 3] = [ThemeKind::Dark, ThemeKind::Light, ThemeKind::Retro];

    /// The name a config file or a console command would use.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
            Self::Retro => "retro",
        }
    }

    /// Parses a preset name, case-insensitively. `None` for anything else.
    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "dark" => Some(Self::Dark),
            "light" => Some(Self::Light),
            "retro" => Some(Self::Retro),
            _ => None,
        }
    }
}

/// A resolved theme: everything the widget layer needs to draw.
///
/// The fields are `#[repr(C)]` scalars so phase 24 can expose the palette without a second
/// layout. Colours are `u32` in `0xRRGGBBAA`, the same packing as the engine's
/// `Color::rgba` helpers.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    kind: ThemeKind,
    /// Window and panel background.
    pub background: u32,
    /// Raised surfaces: buttons at rest, input fields.
    pub surface: u32,
    /// Hovered surfaces.
    pub surface_hover: u32,
    /// Text and icons.
    pub foreground: u32,
    /// Secondary text.
    pub muted: u32,
    /// Accent: selection, progress fill, focused borders.
    pub accent: u32,
    /// Error and destructive actions.
    pub danger: u32,
    /// Padding inside a panel, in points.
    pub spacing: f32,
    /// Corner rounding, in points. Zero is a deliberate retro look.
    pub rounding: f32,
    /// Reserved for future ABI-compatible fields. Must be zero.
    pub _reserved: [u32; 4],
}

impl Theme {
    /// `0xRRGGBBAA` → egui colour, for the widget layer.
    pub(crate) fn unpack(packed: u32) -> egui::Color32 {
        unpack(packed)
    }

    /// The default: dark surfaces, blue-grey accent.
    pub fn dark() -> Self {
        Self {
            kind: ThemeKind::Dark,
            background: 0x121620ff,
            surface: 0x1e2430ff,
            surface_hover: 0x2a3242ff,
            foreground: 0xdee8f6ff,
            muted: 0x8b98abff,
            accent: 0x5c8fd8ff,
            danger: 0xd85050ff,
            spacing: 8.0,
            rounding: 4.0,
            _reserved: [0; 4],
        }
    }

    /// Light surfaces, dark text.
    pub fn light() -> Self {
        Self {
            kind: ThemeKind::Light,
            background: 0xf2f4f8ff,
            surface: 0xffffffff,
            surface_hover: 0xe2e8f0ff,
            foreground: 0x1a1f28ff,
            muted: 0x5a6472ff,
            accent: 0x2f6fd0ff,
            danger: 0xc02020ff,
            spacing: 8.0,
            rounding: 4.0,
            _reserved: [0; 4],
        }
    }

    /// Black and phosphor green, square corners, monospace.
    pub fn retro() -> Self {
        Self {
            kind: ThemeKind::Retro,
            background: 0x000000ff,
            surface: 0x0a1a0aff,
            surface_hover: 0x143014ff,
            foreground: 0x44ff66ff,
            muted: 0x2c9c44ff,
            accent: 0x88ffaaff,
            danger: 0xff5533ff,
            spacing: 10.0,
            rounding: 0.0,
            _reserved: [0; 4],
        }
    }

    /// The preset for `kind`.
    pub fn preset(kind: ThemeKind) -> Self {
        match kind {
            ThemeKind::Dark => Self::dark(),
            ThemeKind::Light => Self::light(),
            ThemeKind::Retro => Self::retro(),
        }
    }

    /// Which preset this is.
    pub fn kind(&self) -> ThemeKind {
        self.kind
    }

    /// The preset's name.
    pub fn name(&self) -> &'static str {
        self.kind.name()
    }

    /// Every preset, for a settings menu.
    pub fn presets() -> [Theme; 3] {
        [Self::dark(), Self::light(), Self::retro()]
    }

    /// The background as an egui colour.
    pub fn background_color(&self) -> Color32 {
        unpack(self.background)
    }

    /// The foreground as an egui colour.
    pub fn foreground_color(&self) -> Color32 {
        unpack(self.foreground)
    }

    /// The accent as an egui colour.
    pub fn accent_color(&self) -> Color32 {
        unpack(self.accent)
    }

    /// The danger colour.
    pub fn danger_color(&self) -> Color32 {
        unpack(self.danger)
    }

    /// Applies the palette and the type scale to `ctx`.
    ///
    /// Called once per frame by the engine while a UI is being drawn. Cheap: egui compares
    /// the style and only rebuilds when it changed.
    pub(crate) fn apply(&self, ctx: &egui::Context) {
        let mut visuals = if self.kind == ThemeKind::Light {
            Visuals::light()
        } else {
            Visuals::dark()
        };

        visuals.override_text_color = Some(self.foreground_color());
        visuals.panel_fill = self.background_color();
        visuals.window_fill = self.background_color();
        visuals.extreme_bg_color = unpack(self.surface);
        visuals.faint_bg_color = unpack(self.surface);
        visuals.selection.bg_fill = self.accent_color().linear_multiply(0.45);
        visuals.selection.stroke = Stroke::new(1.0_f32, self.accent_color());
        visuals.hyperlink_color = self.accent_color();
        visuals.warn_fg_color = unpack(self.danger);
        visuals.widgets.noninteractive.bg_fill = unpack(self.surface);
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, self.foreground_color());
        visuals.widgets.inactive.bg_fill = unpack(self.surface);
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, self.foreground_color());
        visuals.widgets.hovered.bg_fill = unpack(self.surface_hover);
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.5_f32, self.accent_color());
        visuals.widgets.active.bg_fill = self.accent_color().linear_multiply(0.6);
        visuals.widgets.active.fg_stroke = Stroke::new(1.5_f32, self.accent_color());
        visuals.widgets.open.bg_fill = unpack(self.surface_hover);
        let rounding = CornerRadius::same(self.rounding as u8);
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
        style.spacing.item_spacing = egui::vec2(self.spacing, self.spacing * 0.75);
        style.spacing.button_padding = egui::vec2(self.spacing, self.spacing * 0.5);
        style.spacing.window_margin = egui::Margin::same(self.spacing as i8);

        let size = if self.kind == ThemeKind::Retro {
            15.0
        } else {
            14.0
        };
        let family = if self.kind == ThemeKind::Retro {
            FontFamily::Monospace
        } else {
            FontFamily::Proportional
        };
        style.text_styles = [
            (TextStyle::Heading, FontId::new(size * 1.6, family.clone())),
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

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

/// `0xRRGGBBAA` → egui colour.
fn unpack(packed: u32) -> Color32 {
    let [r, g, b, a] = packed.to_be_bytes();
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_three_presets_are_the_ones_the_plan_asks_for() {
        assert_eq!(Theme::presets().len(), 3);
        assert_eq!(Theme::dark().kind(), ThemeKind::Dark);
        assert_eq!(Theme::light().kind(), ThemeKind::Light);
        assert_eq!(Theme::retro().kind(), ThemeKind::Retro);
    }

    #[test]
    fn the_presets_differ_in_more_than_a_name() {
        let dark = Theme::dark();
        let retro = Theme::retro();
        assert_ne!(dark.background, retro.background);
        assert_ne!(dark.accent, retro.accent);
        assert_ne!(dark.rounding, retro.rounding, "retro is square");
        assert_ne!(dark.spacing, retro.spacing);
    }

    #[test]
    fn the_retro_palette_is_green_on_black() {
        let retro = Theme::retro();
        let [r, g, b, _] = retro.foreground.to_be_bytes();
        assert!(g > r && g > b, "the phosphor must be green");
        assert_eq!(retro.background, 0x000000ff);
        assert_eq!(retro.rounding, 0.0);
    }

    #[test]
    fn the_light_palette_is_dark_text_on_white() {
        let light = Theme::light();
        let [r, g, b, _] = light.foreground.to_be_bytes();
        assert!(r < 64 && g < 64 && b < 64, "text must be dark");
        let [r, g, b, _] = light.background.to_be_bytes();
        assert!(r > 200 && g > 200 && b > 200, "the page must be light");
    }

    #[test]
    fn names_round_trip_and_reject_the_rest() {
        for kind in ThemeKind::ALL {
            assert_eq!(ThemeKind::from_name(kind.name()), Some(kind));
            assert_eq!(Theme::preset(kind).name(), kind.name());
        }
        assert_eq!(ThemeKind::from_name(" DARK "), Some(ThemeKind::Dark));
        assert_eq!(ThemeKind::from_name("neon"), None);
        assert_eq!(ThemeKind::from_name(""), None);
    }

    #[test]
    fn the_packed_colours_unpack_in_rgba_order() {
        // Opaque, because egui stores premultiplied and round-trips a translucent colour
        // through it; the point here is the channel order, not the rounding.
        let colour = unpack(0x1122_33ff);
        assert_eq!(colour.to_srgba_unmultiplied(), [0x11, 0x22, 0x33, 0xff]);
    }

    #[test]
    fn a_theme_zeroes_its_reserved_tail() {
        for theme in Theme::presets() {
            assert_eq!(theme._reserved, [0; 4]);
        }
    }
}
