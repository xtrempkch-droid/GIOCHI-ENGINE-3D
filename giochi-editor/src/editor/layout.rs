//! The editor's panel layout.
//!
//! ```text
//! ┌──────────────────────────────────────────────────────┐
//! │ File  Edit  View  Play  Help                         │  menu bar
//! ├────────────┬────────────────────────────┬────────────┤
//! │ Hierarchy  │                            │ Inspector  │
//! │            │        Viewport            │            │
//! ├────────────┴────────────────────────────┴────────────┤
//! │ Asset Browser                                        │
//! └──────────────────────────────────────────────────────┘
//! ```
//!
//! Phase 30 draws the frame and nothing in it: every panel is a titled placeholder that
//! names the phase that fills it (32, 33 and 34), and the viewport is the clear colour
//! phase 31 will render a scene into.
//!
//! The layout returns [`LayoutMetrics`] instead of printing where it put things. Phase 31
//! needs the viewport rectangle to turn a mouse position into a ray, and phase 35 needs
//! it to place gizmos; returning it here is cheaper than recomputing the same rectangles.

use egui::{Align, Layout, Rect, RichText};

use crate::editor::{EditorState, EditorTheme};

/// Identifier of the menu bar panel, stable across frames.
pub const MENU_BAR_PANEL: &str = "giochi_editor_menu_bar";
/// Identifier of the left panel.
pub const HIERARCHY_PANEL: &str = "giochi_editor_hierarchy";
/// Identifier of the right panel.
pub const INSPECTOR_PANEL: &str = "giochi_editor_inspector";
/// Identifier of the bottom panel.
pub const ASSET_BROWSER_PANEL: &str = "giochi_editor_asset_browser";

/// What the layout produced this frame.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LayoutMetrics {
    /// The central rectangle the scene is rendered into, in points.
    ///
    /// `None` only if egui refused to allocate a central panel, which happens when the
    /// window is smaller than the side panels' minimum widths.
    pub viewport_rect: Option<Rect>,
}

/// Draws the whole editor chrome and returns the rectangles the rest of the editor needs.
pub fn draw(ctx: &egui::Context, state: &mut EditorState, theme: &EditorTheme) -> LayoutMetrics {
    menu_bar(ctx, state);

    if state.panels.hierarchy {
        egui::SidePanel::left(HIERARCHY_PANEL)
            .resizable(true)
            .default_width(260.0)
            .width_range(180.0..=440.0)
            .show(ctx, |ui| {
                panel_header(ui, "HIERARCHY", theme);
                hint(
                    ui,
                    "No scene open. The object tree appears here.",
                    "Phase 32 — Hierarchy + selection",
                    theme,
                );
            });
    }

    if state.panels.inspector {
        egui::SidePanel::right(INSPECTOR_PANEL)
            .resizable(true)
            .default_width(300.0)
            .width_range(220.0..=520.0)
            .show(ctx, |ui| {
                panel_header(ui, "INSPECTOR", theme);
                hint(
                    ui,
                    "Nothing selected. Transform, Mesh, Material, Light and Camera \
                     appear here.",
                    "Phase 33 — Inspector (editor MVP)",
                    theme,
                );
            });
    }

    if state.panels.asset_browser {
        egui::TopBottomPanel::bottom(ASSET_BROWSER_PANEL)
            .resizable(true)
            .default_height(170.0)
            .height_range(110.0..=420.0)
            .show(ctx, |ui| {
                panel_header(ui, "ASSET BROWSER", theme);
                hint(
                    ui,
                    "The project's assets appear here.",
                    "Phase 34 — Asset browser",
                    theme,
                );
            });
    }

    let mut metrics = LayoutMetrics::default();
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE.fill(theme.viewport))
        .show(ctx, |ui| {
            let rect = ui.max_rect();
            metrics.viewport_rect = Some(rect);
            ui.painter().rect_filled(rect, 0.0, theme.viewport);
            ui.centered_and_justified(|ui| {
                ui.label(
                    RichText::new("VIEWPORT")
                        .strong()
                        .color(theme.muted)
                        .size(16.0),
                );
                ui.label(
                    RichText::new("The scene renders here in phase 31.")
                        .color(theme.muted)
                        .small(),
                );
            });
        });

    metrics
}

fn menu_bar(ctx: &egui::Context, state: &mut EditorState) {
    egui::TopBottomPanel::top(MENU_BAR_PANEL).show(ctx, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                planned(ui, "New Scene…", "", 31);
                planned(ui, "Open Scene…", "Ctrl+O", 31);
                planned(ui, "Save", "Ctrl+S", 38);
                planned(ui, "Save As…", "Ctrl+Shift+S", 38);
                ui.separator();
                if ui.button("Exit").clicked() {
                    state.request_quit();
                    ui.close();
                }
            });

            ui.menu_button("Edit", |ui| {
                planned(ui, "Undo", "Ctrl+Z", 37);
                planned(ui, "Redo", "Ctrl+Y", 37);
                ui.separator();
                planned(ui, "Create", "", 36);
                planned(ui, "Duplicate", "Ctrl+D", 36);
                planned(ui, "Delete", "Del", 36);
                ui.separator();
                planned(ui, "Preferences…", "", 40);
            });

            ui.menu_button("View", |ui| {
                ui.checkbox(&mut state.panels.hierarchy, "Hierarchy");
                ui.checkbox(&mut state.panels.inspector, "Inspector");
                ui.checkbox(&mut state.panels.asset_browser, "Asset Browser");
            });

            ui.menu_button("Play", |ui| {
                planned(ui, "Play", "F5", 39);
                planned(ui, "Pause", "F7", 39);
                planned(ui, "Stop", "F6", 39);
            });

            ui.menu_button("Help", |ui| {
                planned(ui, "Keyboard Shortcuts", "", 40);
                planned(ui, "About giochi-editor", "", 40);
            });
        });
    });
}

/// A menu entry that exists in the plan but not in this phase.
///
/// Disabled rather than hidden: an editor whose menus change shape between phases is
/// harder to review than one that says which phase each action lands in.
fn planned(ui: &mut egui::Ui, label: &str, shortcut: &str, phase: u8) {
    let button = egui::Button::new(label).shortcut_text(shortcut);
    ui.add_enabled(false, button)
        .on_disabled_hover_text(format!("Phase {phase}"));
}

fn panel_header(ui: &mut egui::Ui, title: &str, theme: &EditorTheme) {
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(title)
                .strong()
                .size(12.0)
                .color(theme.foreground),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new("empty").small().color(theme.muted));
        });
    });
    ui.separator();
}

fn hint(ui: &mut egui::Ui, what: &str, phase: &str, theme: &EditorTheme) {
    ui.add_space(6.0);
    ui.label(RichText::new(what).color(theme.muted).small());
    ui.add_space(2.0);
    ui.label(RichText::new(phase).color(theme.muted).small().italics());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::EditorMode;

    /// Runs the layout for a couple of passes at `size`.
    fn layout_at(size: [f32; 2], state: &mut EditorState) -> LayoutMetrics {
        let ctx = egui::Context::default();
        let theme = EditorTheme::dark();
        let input = || egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(size[0], size[1]),
            )),
            ..Default::default()
        };
        // Two passes: egui settles the panels' default sizes on the first.
        let _ = ctx.run(input(), |ctx| {
            theme.apply(ctx);
            draw(ctx, state, &theme);
        });
        let mut metrics = LayoutMetrics::default();
        let _ = ctx.run(input(), |ctx| {
            theme.apply(ctx);
            metrics = draw(ctx, state, &theme);
        });
        metrics
    }

    #[test]
    fn the_viewport_gets_the_space_the_panels_leave() {
        let mut state = EditorState::new();
        let metrics = layout_at([1600.0, 900.0], &mut state);
        let rect = metrics
            .viewport_rect
            .expect("a 1600x900 window has a viewport");

        // Left 260 + right 300 + the window's chrome: the viewport is the middle strip,
        // and it must be the largest region on screen.
        assert!(
            rect.width() > 500.0,
            "viewport is {} wide, expected the central strip",
            rect.width()
        );
        assert!(
            rect.height() > 300.0,
            "viewport is {} tall, expected to sit above the asset browser",
            rect.height()
        );
    }

    #[test]
    fn hiding_a_panel_gives_its_space_to_the_viewport() {
        let mut with_all = EditorState::new();
        let all = layout_at([1600.0, 900.0], &mut with_all)
            .viewport_rect
            .expect("viewport");

        let mut without_side = EditorState::new();
        without_side.panels.hierarchy = false;
        without_side.panels.inspector = false;
        let wide = layout_at([1600.0, 900.0], &mut without_side)
            .viewport_rect
            .expect("viewport");

        assert!(
            wide.width() > all.width(),
            "{} should be wider than {}",
            wide.width(),
            all.width()
        );
    }

    #[test]
    fn the_layout_draws_without_a_scene_and_changes_nothing() {
        let mut state = EditorState::new();
        let metrics = layout_at([1280.0, 720.0], &mut state);
        assert!(metrics.viewport_rect.is_some());
        assert!(!state.dirty);
        assert!(!state.quit_requested());
        assert_eq!(state.mode, EditorMode::Select);
    }

    #[test]
    fn a_tiny_window_still_produces_a_layout() {
        let mut state = EditorState::new();
        // Smaller than the side panels' minima: egui clamps them, and the central panel
        // may collapse to nothing. The point is that drawing does not panic.
        let _ = layout_at([320.0, 240.0], &mut state);
    }
}
