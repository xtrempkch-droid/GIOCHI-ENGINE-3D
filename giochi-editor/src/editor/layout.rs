//! The editor's panel layout.
//!
//! ```text
//! ┌──────────────────────────────────────────────────────┐
//! │ File  Edit  View  Play  Help           New scene     │  menu bar + status
//! ├────────────┬────────────────────────────┬────────────┤
//! │ Hierarchy  │                            │ Inspector  │
//! │            │        Viewport            │            │
//! ├────────────┴────────────────────────────┴────────────┤
//! │ Asset Browser                                        │
//! └──────────────────────────────────────────────────────┘
//! ```
//!
//! The viewport is rendered **before** egui (see [`crate::editor::viewport`]), using the
//! rectangle this function produced on the previous frame. The central panel therefore
//! carries no fill: the scene is already on the canvas underneath, and a filled frame
//! would paint over it.
//!
//! The layout returns [`LayoutMetrics`] instead of printing where it put things. The
//! viewport needs its rectangle to turn a pointer into a ray (phase 32), the gizmos need
//! it to place handles (phase 35), and the camera needs its height to pan at the right
//! scale.

use egui::{Align, Layout, Rect, RichText};

use crate::editor::dialog::DialogOutcome;
use crate::editor::editor_state::{Status, StatusKind};
use crate::editor::viewport::{self, ViewportInput};
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
    /// What the pointer did over the viewport this frame.
    pub input: ViewportInput,
}

/// Draws the whole editor chrome and returns the rectangles the rest of the editor needs.
pub fn draw(ctx: &egui::Context, state: &mut EditorState, theme: &EditorTheme) -> LayoutMetrics {
    menu_bar(ctx, state, theme);

    if state.panels.hierarchy {
        egui::SidePanel::left(HIERARCHY_PANEL)
            .resizable(true)
            .default_width(260.0)
            .width_range(180.0..=440.0)
            .show(ctx, |ui| {
                panel_header(ui, "HIERARCHY", theme);
                match &state.scene {
                    Some(scene) => {
                        ui.label(
                            RichText::new(format!(
                                "{} object(s) — tree arrives in phase 32",
                                scene.nodes.len()
                            ))
                            .color(theme.muted)
                            .small(),
                        );
                        for node in scene.nodes.iter().take(12) {
                            ui.label(
                                RichText::new(format!("{}  {}", node.kind(), node.name))
                                    .color(theme.muted)
                                    .small(),
                            );
                        }
                        if scene.nodes.len() > 12 {
                            ui.label(RichText::new("…").color(theme.muted).small());
                        }
                    }
                    None => hint(
                        ui,
                        "No scene open. The object tree appears here.",
                        "Phase 32 — Hierarchy + selection",
                        theme,
                    ),
                }
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
        // No fill: the scene was drawn on the canvas before this pass. A filled frame
        // here would hide it (ADR-064).
        .frame(egui::Frame::NONE)
        .show(ctx, |ui| {
            let rect = ui.max_rect();
            let _ = ui.allocate_rect(rect, egui::Sense::click_and_drag());
            metrics.viewport_rect = Some(rect);
            metrics.input = viewport::read_input(ctx, Some(rect));

            match &state.scene {
                Some(scene) => {
                    let camera = &state.viewport_camera;
                    ui.painter().text(
                        rect.left_top() + egui::vec2(10.0, 8.0),
                        egui::Align2::LEFT_TOP,
                        format!(
                            "{} — {} object(s)",
                            scene.name,
                            scene.nodes.len()
                        ),
                        egui::FontId::proportional(12.0),
                        theme.muted,
                    );
                    ui.painter().text(
                        rect.left_bottom() + egui::vec2(10.0, -8.0),
                        egui::Align2::LEFT_BOTTOM,
                        format!(
                            "orbit RMB · pan MMB · zoom wheel · dist {:.1}",
                            camera.distance
                        ),
                        egui::FontId::proportional(11.0),
                        theme.muted,
                    );
                }
                None => {
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "VIEWPORT\nFile > New Scene  (Ctrl+N)",
                        egui::FontId::proportional(15.0),
                        theme.muted,
                    );
                }
            }
        });

    file_dialog(ctx, state);

    metrics
}

fn menu_bar(ctx: &egui::Context, state: &mut EditorState, theme: &EditorTheme) {
    egui::TopBottomPanel::top(MENU_BAR_PANEL).show(ctx, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui
                    .add(egui::Button::new("New Scene").shortcut_text("Ctrl+N"))
                    .clicked()
                {
                    if confirm_replace(state) {
                        state.new_scene();
                    }
                    ui.close();
                }
                if ui
                    .add(egui::Button::new("Open Scene…").shortcut_text("Ctrl+O"))
                    .clicked()
                {
                    if confirm_replace(state) {
                        state.open_dialog();
                    }
                    ui.close();
                }
                ui.separator();
                if ui
                    .add(egui::Button::new("Save").shortcut_text("Ctrl+S"))
                    .clicked()
                {
                    state.save_or_ask();
                    ui.close();
                }
                if ui
                    .add(egui::Button::new("Save As…").shortcut_text("Ctrl+Shift+S"))
                    .clicked()
                {
                    state.save_dialog();
                    ui.close();
                }
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
                ui.separator();
                if ui.button("Frame Scene").clicked() {
                    if let Some(scene) = &state.scene {
                        if let Some((min, max)) = scene.mesh_bounds() {
                            state.viewport_camera.frame_aabb(min, max);
                        } else {
                            state.viewport_camera = Default::default();
                        }
                    }
                    ui.close();
                }
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

            // The status line is part of the menu bar rather than a second panel: the
            // bottom edge already belongs to the asset browser, and a one-line bar would
            // take height from the viewport for a message that is usually empty.
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if !state.status.text.is_empty() {
                    let color = match state.status.kind {
                        StatusKind::Error => theme.danger,
                        StatusKind::Warning => egui::Color32::from_rgb(0xd8, 0xa8, 0x40),
                        StatusKind::Idle | StatusKind::Info => theme.muted,
                    };
                    ui.label(RichText::new(&state.status.text).small().color(color));
                }
            });
        });
    });
}

/// Refuses to replace a scene with unsaved changes, and says why.
fn confirm_replace(state: &mut EditorState) -> bool {
    if state.dirty {
        state.report(Status::warning(
            "unsaved changes — Save (Ctrl+S) before replacing the scene",
        ));
        false
    } else {
        true
    }
}

fn file_dialog(ctx: &egui::Context, state: &mut EditorState) {
    let Some(mut dialog) = state.dialog.take() else {
        return;
    };
    match dialog.show(ctx) {
        DialogOutcome::Pending => state.dialog = Some(dialog),
        DialogOutcome::Open(path) => {
            if let Err(error) = state.open_scene(&path) {
                state.report(Status::error(error));
            }
        }
        DialogOutcome::Save(path) => {
            if let Err(error) = state.save_scene_as(&path) {
                state.report(Status::error(error));
            }
        }
        DialogOutcome::Cancel => {}
    }
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
        assert_eq!(state.dialog, None);
    }

    #[test]
    fn the_layout_draws_a_scene_without_panicking() {
        let mut state = EditorState::new();
        state.new_scene();
        let metrics = layout_at([1600.0, 900.0], &mut state);
        assert!(metrics.viewport_rect.is_some());
        assert!(state.scene.is_some());
    }

    #[test]
    fn a_dirty_scene_is_not_replaced_silently() {
        let mut state = EditorState::new();
        state.new_scene();
        state.mark_dirty();
        assert!(!confirm_replace(&mut state));
        assert_eq!(state.status.kind, StatusKind::Warning);

        state.mark_saved();
        assert!(confirm_replace(&mut state));
    }

    #[test]
    fn a_tiny_window_still_produces_a_layout() {
        let mut state = EditorState::new();
        // Smaller than the side panels' minima: egui clamps them, and the central panel
        // may collapse to nothing. The point is that drawing does not panic.
        let _ = layout_at([320.0, 240.0], &mut state);
    }
}
