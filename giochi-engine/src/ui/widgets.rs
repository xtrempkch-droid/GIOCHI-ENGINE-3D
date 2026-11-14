//! The widget facade: buttons, sliders, inputs and layouts, without an `egui` type in any
//! engine signature.
//!
//! # Why a facade at all
//!
//! The renderer is allowed to depend on egui; the *game* is not. A game that spells
//! `egui::Slider::new(&mut value, 0.0..=1.0)` has coupled its source to a version of egui,
//! and phase 24 has to expose the same widgets through the C ABI, where there is no Rust
//! at all. So the engine offers C-shaped values ([`Button`], [`Slider`], …) and a [`Ui`]
//! that draws them:
//!
//! ```no_run
//! use giochi_engine::ui::{Button, Slider, Ui};
//!
//! # fn menu(ui: &mut Ui<'_>) {
//! if ui.button(&Button::new("Start")) {
//!     // start the game
//! }
//! let mut volume = Slider::new(0.0, 1.0).with_value(0.8);
//! if ui.slider(&mut volume) {
//!     // volume.value() changed
//! }
//! # }
//! ```
//!
//! # State
//!
//! A widget that holds a value owns it, and `Ui::slider(&mut slider)` returns `true` when
//! the user changed it. That is one line more than egui's `&mut f32`, and it is what lets
//! the same widget exist in the C ABI as a struct with a `float value` and a
//! `uint32_t changed`.

use crate::math::{Color, Rect};
use crate::render::TextureHandle;

/// A push button.
///
/// The label is owned because egui's layout may outlive the frame's stack in a deferred
/// tooltip; the allocation is small and happens when the game builds the widget, which is
/// once per frame in the worst case.
#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Button {
    text: String,
    enabled: bool,
    width: Option<u32>,
    _reserved: [u32; 4],
}

impl Button {
    /// A button labelled `text`.
    pub fn new(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            enabled: true,
            width: None,
            _reserved: [0; 4],
        }
    }

    /// Disables it: drawn greyed out and never returns `true`.
    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    /// Fixes the width in points, instead of fitting the label.
    pub fn width(mut self, points: f32) -> Self {
        if points.is_finite() && points > 0.0 {
            self.width = Some(points as u32);
        }
        self
    }

    /// The label.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Whether it can be clicked.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}

/// A slider over a range.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Slider {
    value: f32,
    min: f32,
    max: f32,
    step: f32,
    _reserved: [u32; 4],
}

impl Slider {
    /// A slider from `min` to `max`, starting at `min`.
    ///
    /// A non-finite or inverted range is sanitised to `0.0..=1.0`: a debug UI must not be
    /// able to make egui panic, and the values come from a game's config.
    pub fn new(min: f32, max: f32) -> Self {
        let (min, max) = if min.is_finite() && max.is_finite() && max > min {
            (min, max)
        } else {
            (0.0, 1.0)
        };
        Self {
            value: min,
            min,
            max,
            step: 0.0,
            _reserved: [0; 4],
        }
    }

    /// Starts at `value`, clamped into the range.
    pub fn with_value(mut self, value: f32) -> Self {
        self.value = if value.is_finite() {
            value.clamp(self.min, self.max)
        } else {
            self.min
        };
        self
    }

    /// Quantises the slider to `step` (0 means continuous).
    pub fn with_step(mut self, step: f32) -> Self {
        self.step = if step.is_finite() && step > 0.0 {
            step
        } else {
            0.0
        };
        self
    }

    /// The current value.
    pub fn value(&self) -> f32 {
        self.value
    }

    /// Sets the value, clamped.
    pub fn set_value(&mut self, value: f32) {
        self.value = if value.is_finite() {
            value.clamp(self.min, self.max)
        } else {
            self.min
        };
    }

    /// The range.
    pub fn range(&self) -> (f32, f32) {
        (self.min, self.max)
    }

    /// The step, or `0.0` for continuous.
    pub fn step(&self) -> f32 {
        self.step
    }
}

/// A checkbox with a label.
#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkbox {
    text: String,
    checked: bool,
    _reserved: [u32; 4],
}

impl Checkbox {
    /// An unchecked box labelled `text`.
    pub fn new(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            checked: false,
            _reserved: [0; 4],
        }
    }

    /// Starts checked.
    pub fn with_checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    /// Whether it is checked.
    pub fn is_checked(&self) -> bool {
        self.checked
    }

    /// The label.
    pub fn text(&self) -> &str {
        &self.text
    }
}

/// A single-line text field.
#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextInput {
    text: String,
    hint: String,
    max_chars: usize,
    _reserved: [u32; 4],
}

impl TextInput {
    /// An empty field with a placeholder.
    pub fn new(hint: &str) -> Self {
        Self {
            text: String::new(),
            hint: hint.to_owned(),
            max_chars: 0,
            _reserved: [0; 4],
        }
    }

    /// Starts with `text`.
    pub fn with_text(mut self, text: &str) -> Self {
        self.text = text.to_owned();
        self
    }

    /// Caps the length in characters (0 means no cap).
    pub fn with_max_chars(mut self, max: usize) -> Self {
        self.max_chars = max;
        self
    }

    /// The current text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Replaces the text, truncating on a character boundary.
    pub fn set_text(&mut self, text: &str) {
        self.text = if self.max_chars == 0 {
            text.to_owned()
        } else {
            text.chars().take(self.max_chars).collect()
        };
    }
}

/// A drop-down list.
#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComboBox {
    label: String,
    options: Vec<String>,
    selected: usize,
    _reserved: [u32; 4],
}

impl ComboBox {
    /// An empty list labelled `label`.
    pub fn new(label: &str) -> Self {
        Self {
            label: label.to_owned(),
            options: Vec::new(),
            selected: 0,
            _reserved: [0; 4],
        }
    }

    /// Adds an option. Returns the widget so calls chain.
    pub fn option(mut self, text: &str) -> Self {
        self.options.push(text.to_owned());
        self
    }

    /// Selects `index`, ignored when out of range.
    pub fn with_selected(mut self, index: usize) -> Self {
        if index < self.options.len() {
            self.selected = index;
        }
        self
    }

    /// The selected index.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// The selected text, or `""` for an empty list.
    pub fn selected_text(&self) -> &str {
        self.options
            .get(self.selected)
            .map(String::as_str)
            .unwrap_or("")
    }

    /// The options.
    pub fn options(&self) -> &[String] {
        &self.options
    }
}

/// A progress bar, filled to `fraction`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProgressBar {
    fraction: f32,
    _reserved: [u32; 4],
}

impl ProgressBar {
    /// A bar filled to `fraction`, clamped to `0.0..=1.0`.
    ///
    /// The label is deliberately not part of this type: it owns a `String`, and a bar is
    /// the one widget that is drawn thousands of times per second (a health bar per
    /// entity). A caller that wants text uses [`crate::ui::Ui::label`] beside it.
    pub fn new(fraction: f32) -> Self {
        Self {
            fraction: if fraction.is_finite() {
                fraction.clamp(0.0, 1.0)
            } else {
                0.0
            },
            _reserved: [0; 4],
        }
    }

    /// The fill, `0.0..=1.0`.
    pub fn fraction(&self) -> f32 {
        self.fraction
    }
}

/// An image widget: an engine texture drawn at a size.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Image {
    texture: TextureHandle,
    width: f32,
    height: f32,
    tint: Color,
    _reserved: [u32; 4],
}

impl Image {
    /// `texture` drawn at `width`x`height` points.
    ///
    /// The texture is an engine handle; the backend maps it to the egui texture namespace
    /// directly, so an `Image` costs no upload and no copy.
    pub fn new(texture: TextureHandle, width: f32, height: f32) -> Self {
        Self {
            texture,
            width: if width.is_finite() && width > 0.0 {
                width
            } else {
                1.0
            },
            height: if height.is_finite() && height > 0.0 {
                height
            } else {
                1.0
            },
            tint: Color::rgb(255, 255, 255),
            _reserved: [0; 4],
        }
    }

    /// Multiplies the image by `tint` (white shows it unchanged).
    pub fn with_tint(mut self, tint: Color) -> Self {
        self.tint = tint;
        self
    }

    /// The texture handle.
    pub fn texture(&self) -> TextureHandle {
        self.texture
    }

    /// The size in points.
    pub fn size(&self) -> (f32, f32) {
        (self.width, self.height)
    }
}

/// A separador: the horizontal rule the layouts use between groups.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Separator {
    _reserved: [u32; 4],
}

/// Draws the widgets. Handed to [`crate::scene::Scene::render_ui`] once per frame.
///
/// The lifetime ties the facade to the frame's egui `Ui`: it cannot escape the callback,
/// which is what keeps a game from storing it.
#[repr(C)]
pub struct Ui<'a> {
    inner: &'a mut egui::Ui,
    theme: crate::ui::Theme,
    _reserved: [u32; 4],
}

impl std::fmt::Debug for Ui<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ui")
            .field("theme", &self.theme.name())
            .finish_non_exhaustive()
    }
}

impl<'a> Ui<'a> {
    /// Wraps an egui `Ui`. Crate-private: a game gets a [`Ui`] from the engine and never
    /// builds one.
    pub(crate) fn new(inner: &'a mut egui::Ui, theme: crate::ui::Theme) -> Self {
        Self {
            inner,
            theme,
            _reserved: [0; 4],
        }
    }

    /// The theme in use.
    pub fn theme(&self) -> crate::ui::Theme {
        self.theme
    }

    /// The space left in the current layout, in points.
    pub fn available_size(&self) -> crate::math::Vec2 {
        let size = self.inner.available_size();
        crate::math::Vec2::new(size.x, size.y)
    }

    /// Static text.
    pub fn label(&mut self, text: &str) {
        self.inner.label(text);
    }

    /// Static text at the heading size.
    pub fn heading(&mut self, text: &str) {
        self.inner.heading(text);
    }

    /// Text in the secondary colour, for hints.
    pub fn muted(&mut self, text: &str) {
        let colour = crate::ui::Theme::unpack(self.theme.muted);
        self.inner.colored_label(colour, text);
    }

    /// A push button. Returns `true` on the frame it is clicked.
    pub fn button(&mut self, button: &Button) -> bool {
        let mut widget = egui::Button::new(button.text());
        if let Some(width) = button.width {
            widget = widget.min_size(egui::vec2(width as f32, 0.0));
        }
        let response = self.inner.add_enabled(button.is_enabled(), widget);
        response.clicked()
    }

    /// A slider. Returns `true` when the value changed, which is also when
    /// `slider.value()` holds the new number.
    pub fn slider(&mut self, slider: &mut Slider) -> bool {
        let mut widget = egui::Slider::new(&mut slider.value, slider.min..=slider.max);
        if slider.step > 0.0 {
            widget = widget.step_by(slider.step as f64);
        }
        self.inner.add(widget).changed()
    }

    /// A checkbox. Returns `true` when toggled.
    pub fn checkbox(&mut self, checkbox: &mut Checkbox) -> bool {
        // The label is cloned first: `&mut checkbox.checked` and `checkbox.text()` in one
        // call is two borrows of the same struct.
        let label = checkbox.text().to_owned();
        self.inner.checkbox(&mut checkbox.checked, label).changed()
    }

    /// A single-line text field. Returns `true` while it has focus and is edited.
    pub fn text_input(&mut self, input: &mut TextInput) -> bool {
        let response = self.inner.add(
            egui::TextEdit::singleline(&mut input.text)
                .hint_text(input.hint.as_str())
                .char_limit(input.max_chars),
        );
        response.changed()
    }

    /// A drop-down. Returns `true` when the selection changed.
    pub fn combo_box(&mut self, combo: &mut ComboBox) -> bool {
        let mut changed = false;
        egui::ComboBox::from_label(combo.label.as_str())
            .selected_text(combo.selected_text())
            .show_ui(self.inner, |ui| {
                for (index, option) in combo.options.iter().enumerate() {
                    if ui
                        .selectable_value(&mut combo.selected, index, option)
                        .changed()
                    {
                        changed = true;
                    }
                }
            });
        changed
    }

    /// A progress bar.
    pub fn progress_bar(&mut self, bar: &ProgressBar) {
        self.inner
            .add(egui::ProgressBar::new(bar.fraction).desired_width(self.inner.available_width()));
    }

    /// An engine texture drawn as a widget.
    pub fn image(&mut self, image: &Image) {
        let texture = egui::load::SizedTexture::new(
            egui::TextureId::User(image.texture),
            egui::vec2(image.width, image.height),
        );
        let tint = image.tint;
        let colour = egui::Color32::from_rgba_unmultiplied(tint.r, tint.g, tint.b, tint.a);
        self.inner.add(egui::Image::new(texture).tint(colour));
    }

    /// A horizontal rule.
    pub fn separator(&mut self) {
        self.inner.separator();
    }

    /// Vertical space, in points.
    pub fn space(&mut self, points: f32) {
        if points.is_finite() && points > 0.0 {
            self.inner.add_space(points);
        }
    }

    /// Stacks the widgets vertically. `contents` runs immediately.
    pub fn vertical(&mut self, contents: &mut dyn FnMut(&mut Ui<'_>)) {
        let theme = self.theme;
        self.inner
            .vertical(|inner| contents(&mut Ui::new(inner, theme)));
    }

    /// Places the widgets side by side. `contents` runs immediately.
    pub fn horizontal(&mut self, contents: &mut dyn FnMut(&mut Ui<'_>)) {
        let theme = self.theme;
        self.inner
            .horizontal(|inner| contents(&mut Ui::new(inner, theme)));
    }

    /// A bordered group with the theme's surface colour — a panel inside a panel.
    pub fn group(&mut self, contents: &mut dyn FnMut(&mut Ui<'_>)) {
        let theme = self.theme;
        self.inner
            .group(|inner| contents(&mut Ui::new(inner, theme)));
    }

    /// Whether a rectangle of the screen is being used by the UI.
    ///
    /// A game that has a mouse-driven world (a level editor, an RTS) asks this before
    /// acting on a click, so clicking a button does not also select what is behind it.
    pub fn wants_pointer(&self) -> bool {
        self.inner.ctx().wants_pointer_input()
    }

    /// A `Rect` in screen points, for a game that wants to avoid the UI region.
    pub fn used_rect(&self) -> Rect {
        let rect = self.inner.min_rect();
        Rect::new(rect.min.x, rect.min.y, rect.width(), rect.height())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_button_has_a_label_and_is_enabled_by_default() {
        let button = Button::new("Start");
        assert_eq!(button.text(), "Start");
        assert!(button.is_enabled());
        assert!(!Button::new("Quit").disabled().is_enabled());
    }

    #[test]
    fn a_button_width_is_ignored_when_not_positive() {
        assert_eq!(Button::new("x").width(0.0).width, None);
        assert_eq!(Button::new("x").width(-3.0).width, None);
        assert_eq!(Button::new("x").width(f32::NAN).width, None);
        assert_eq!(Button::new("x").width(120.0).width, Some(120));
    }

    #[test]
    fn a_slider_clamps_its_value_and_sanitises_its_range() {
        let mut slider = Slider::new(0.0, 10.0).with_value(42.0);
        assert_eq!(slider.value(), 10.0, "clamped to the maximum");
        slider.set_value(-1.0);
        assert_eq!(slider.value(), 0.0);
        slider.set_value(f32::NAN);
        assert_eq!(slider.value(), 0.0, "a NaN becomes the minimum");

        let inverted = Slider::new(10.0, 0.0);
        assert_eq!(inverted.range(), (0.0, 1.0));
        let not_finite = Slider::new(f32::NAN, 1.0);
        assert_eq!(not_finite.range(), (0.0, 1.0));
    }

    #[test]
    fn a_slider_step_is_zero_for_continuous_or_bad_input() {
        assert_eq!(Slider::new(0.0, 1.0).step(), 0.0);
        assert_eq!(Slider::new(0.0, 1.0).with_step(0.1).step(), 0.1);
        assert_eq!(Slider::new(0.0, 1.0).with_step(0.0).step(), 0.0);
        assert_eq!(Slider::new(0.0, 1.0).with_step(-1.0).step(), 0.0);
    }

    #[test]
    fn a_checkbox_starts_unchecked_unless_asked() {
        assert!(!Checkbox::new("vsync").is_checked());
        assert!(Checkbox::new("vsync").with_checked(true).is_checked());
        assert_eq!(Checkbox::new("vsync").text(), "vsync");
    }

    #[test]
    fn a_text_input_caps_its_length_on_a_character_boundary() {
        let mut input = TextInput::new("name").with_max_chars(3);
        input.set_text("café");
        assert_eq!(input.text(), "caf", "the accented character is one char");
        input.set_text("ab");
        assert_eq!(input.text(), "ab");

        let mut unlimited = TextInput::new("name").with_text("anything");
        assert_eq!(unlimited.text(), "anything");
        unlimited.set_text("longer than three");
        assert_eq!(unlimited.text(), "longer than three");
    }

    #[test]
    fn a_combo_box_selects_only_a_valid_index() {
        let combo = ComboBox::new("resolution")
            .option("640x480")
            .option("960x540")
            .with_selected(1);
        assert_eq!(combo.selected(), 1);
        assert_eq!(combo.selected_text(), "960x540");

        let clamped = ComboBox::new("x").option("a").with_selected(9);
        assert_eq!(clamped.selected(), 0, "an out-of-range index is ignored");

        let empty = ComboBox::new("x");
        assert_eq!(empty.selected_text(), "");
    }

    #[test]
    fn a_progress_bar_clamps_its_fraction() {
        assert_eq!(ProgressBar::new(0.5).fraction(), 0.5);
        assert_eq!(ProgressBar::new(2.0).fraction(), 1.0);
        assert_eq!(ProgressBar::new(-1.0).fraction(), 0.0);
        assert_eq!(ProgressBar::new(f32::NAN).fraction(), 0.0);
    }

    #[test]
    fn an_image_refuses_a_degenerate_size() {
        let image = Image::new(7, 0.0, -4.0);
        assert_eq!(image.size(), (1.0, 1.0));
        assert_eq!(image.texture(), 7);

        let sized = Image::new(3, 64.0, 32.0).with_tint(Color::rgb(255, 0, 0));
        assert_eq!(sized.size(), (64.0, 32.0));
    }

    #[test]
    fn every_widget_zeroes_its_reserved_tail() {
        assert_eq!(Button::new("x")._reserved, [0; 4]);
        assert_eq!(Slider::new(0.0, 1.0)._reserved, [0; 4]);
        assert_eq!(Checkbox::new("x")._reserved, [0; 4]);
        assert_eq!(TextInput::new("x")._reserved, [0; 4]);
        assert_eq!(ComboBox::new("x")._reserved, [0; 4]);
        assert_eq!(ProgressBar::new(0.0)._reserved, [0; 4]);
        assert_eq!(Image::new(1, 1.0, 1.0)._reserved, [0; 4]);
        assert_eq!(Separator::default()._reserved, [0; 4]);
    }
}
