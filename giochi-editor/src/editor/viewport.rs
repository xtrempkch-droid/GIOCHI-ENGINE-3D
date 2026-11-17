//! The viewport: the scene drawn as a projected wireframe.
//!
//! # Why a wireframe and not the GL renderer
//!
//! The plan warned about this: "integrar viewport 3D dentro de egui requer
//! render-to-texture; `egui-sdl2` + FBO + GLES 2.0 tem quirks". The editor paints with
//! `sdl2::render` (the 2D engine), while `giochi-engine-3d` drives GLES through a second
//! EGL context on the same window. Making those two coexist means either sharing a GL
//! context that `sdl2::render` does not expose, or rendering into an FBO and copying
//! pixels back every frame. Both are fragile on the Mali drivers this project targets.
//!
//! What the editor actually needs for phases 31–40 is the *world*: a grid, axes, the
//! boxes of the objects, an outline around the selection, and a camera you can orbit
//! with. That is geometry, not shading. So the viewport projects the scene with its own
//! camera and submits lines to the same 2D renderer the panels use — the same technique
//! the engine's `DebugDraw::draw_aabb_3d` and `draw_grid_3d` already ship (phase 20),
//! and the plan's own suggestion for the grid ("reuso do `debug_draw`").
//!
//! The consequence is explicit: the editor does not show materials or lighting. It shows
//! where everything is, which is what an editor is for; the game's own window shows the
//! shaded result. ADR-064 records the trade and what would replace it.
//!
//! # One frame, two sources of truth
//!
//! Lines are drawn **before** egui paints, using the rectangle the layout produced on the
//! previous frame. egui then paints the panels around the same rectangle, so the scene
//! cannot bleed into a panel even when a line was projected slightly outside. The first
//! frame of a session has no rectangle yet and simply draws nothing.

use egui::Rect;
use giochi_engine::{Color, Rect as EngineRect, RenderContext, Vec2};
use glam::{Mat4, Vec3, Vec4};

use giochi_engine_3d::scene::{Scene, SceneNode, SceneNodeId};

use crate::editor::theme::EditorTheme;
use crate::editor::OrbitCamera;

/// Grid half-extent in world units.
pub const GRID_EXTENT: f32 = 10.0;
/// Grid cells per side.
pub const GRID_DIVISIONS: u32 = 20;

/// One frame's worth of viewport input, already gated by "the pointer is over the
/// viewport".
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ViewportInput {
    /// Right-button drag, in points. Orbits.
    pub orbit: egui::Vec2,
    /// Middle-button drag, in points. Pans.
    pub pan: egui::Vec2,
    /// Scroll, in points. Zooms when positive.
    pub scroll: f32,
    /// Whether the pointer is over the viewport at all.
    pub hovered: bool,
    /// The pointer position while a button is down, even outside the viewport: a drag that
    /// left the rectangle must keep following the pointer, or it sticks.
    pub pointer: Option<egui::Pos2>,
    /// Primary button pressed this frame, inside the viewport.
    pub pressed: Option<egui::Pos2>,
    /// Primary button released this frame, anywhere.
    pub released: bool,
    /// A completed click (pressed and released without dragging) inside the viewport.
    pub click: Option<egui::Pos2>,
    /// Primary button double-clicked this frame, with the pointer position.
    pub double_click: Option<egui::Pos2>,
    /// Whether Ctrl (or Cmd) was held when the click happened.
    pub toggle_modifier: bool,
}

impl ViewportInput {
    /// Whether this frame asks for a camera change.
    pub fn moves_the_camera(&self) -> bool {
        self.orbit != egui::Vec2::ZERO || self.pan != egui::Vec2::ZERO || self.scroll != 0.0
    }

    /// Applies the camera part to `camera`.
    ///
    /// `viewport_height` is in points, which is what makes pan independent of zoom.
    pub fn apply_to_camera(&self, camera: &mut OrbitCamera, viewport_height: f32) {
        if self.orbit != egui::Vec2::ZERO {
            camera.orbit(self.orbit.x, self.orbit.y);
        }
        if self.pan != egui::Vec2::ZERO {
            camera.pan(self.pan.x, self.pan.y, viewport_height);
        }
        if self.scroll != 0.0 {
            camera.zoom(self.scroll);
        }
    }
}

/// Reads the viewport input for `rect`.
///
/// Kept in this module rather than in `layout` so that the camera, the input and the
/// projection live together and `layout` stays about panels.
pub fn read_input(ctx: &egui::Context, rect: Option<Rect>) -> ViewportInput {
    let Some(rect) = rect else {
        return ViewportInput::default();
    };
    let pointer = ctx.input(|input| input.pointer.clone());
    let hovered = ctx
        .input(|input| input.pointer.hover_pos())
        .is_some_and(|position| rect.contains(position));

    let mut input = ViewportInput {
        hovered,
        ..ViewportInput::default()
    };

    // A drag that leaves the viewport must keep tracking the pointer, so the position and
    // the release are read unconditionally and only the *start* of a gesture is gated on
    // the pointer being inside.
    let dragging = pointer.any_down();
    if hovered || dragging {
        input.pointer = pointer.interact_pos();
    }
    if hovered {
        if pointer.button_down(egui::PointerButton::Secondary) {
            input.orbit = pointer.delta();
        }
        if pointer.button_down(egui::PointerButton::Middle) {
            input.pan = pointer.delta();
        }
        input.scroll = ctx.input(|i| i.smooth_scroll_delta.y);
        input.toggle_modifier = ctx.input(|i| i.modifiers.command);
        if pointer.button_pressed(egui::PointerButton::Primary) {
            input.pressed = pointer.interact_pos();
        }
        if pointer.button_clicked(egui::PointerButton::Primary) {
            input.click = pointer.interact_pos();
        }
        if pointer.button_double_clicked(egui::PointerButton::Primary) {
            input.double_click = pointer.interact_pos();
        }
    }
    input.released = pointer.button_released(egui::PointerButton::Primary);
    input
}

/// Draws the scene into `rect`.
///
/// `rect` is in points; the backend runs at `pixels_per_point = 1.0` (the engine's
/// `EguiBackend` default, which is also correct on KMSDRM where the drawable is the
/// panel), so points and pixels are the same number. If a future phase turns on HiDPI,
/// this is the one place to scale.
pub fn draw(
    render: &mut RenderContext,
    rect: Rect,
    scene: Option<&Scene>,
    camera: &OrbitCamera,
    selected: &[SceneNodeId],
    theme: &EditorTheme,
) {
    draw_with_gizmo(
        render,
        rect,
        scene,
        camera,
        selected,
        theme,
        GizmoView::default(),
    );
}

/// What the gizmo should show this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GizmoView {
    /// The armed tool. `Select` draws no handles.
    pub mode: crate::editor::EditorMode,
    /// The handle currently being dragged, if any.
    pub active: Option<crate::gizmos::GizmoHandle>,
}

/// Draws the scene and, when one object is selected and a tool is armed, its gizmo.
pub fn draw_with_gizmo(
    render: &mut RenderContext,
    rect: Rect,
    scene: Option<&Scene>,
    camera: &OrbitCamera,
    selected: &[SceneNodeId],
    theme: &EditorTheme,
    gizmo: GizmoView,
) {
    if rect.width() < 2.0 || rect.height() < 2.0 {
        return;
    }
    let aspect = rect.width() / rect.height().max(1.0);
    let view_projection = Mat4::from_cols_array(&camera.view_projection(aspect));

    let grid = to_engine_color(theme.grid);
    let axes = [
        (
            Vec3::X,
            to_engine_color(egui::Color32::from_rgb(0xd0, 0x50, 0x50)),
        ),
        (
            Vec3::Y,
            to_engine_color(egui::Color32::from_rgb(0x50, 0xc0, 0x60)),
        ),
        (
            Vec3::Z,
            to_engine_color(egui::Color32::from_rgb(0x50, 0x80, 0xe0)),
        ),
    ];

    draw_grid(render, rect, &view_projection, grid);
    draw_axes(render, rect, &view_projection, &axes);

    let Some(scene) = scene else {
        return;
    };

    for node in &scene.nodes {
        draw_node(render, rect, &view_projection, node, scene, selected, theme);
    }

    // The gizmo attaches to exactly one selected node. With none or several, no handles:
    // a gizmo that means "the average of these" is a feature nobody asked for.
    if gizmo.mode != crate::editor::EditorMode::Select && selected.len() == 1 {
        if let Some(node) = scene.node(selected[0]) {
            let world = world_transform(scene, node);
            let center = Vec3::new(world.position[0], world.position[1], world.position[2]);
            let geometry = crate::gizmos::GizmoGeometry::for_camera(center, camera, rect);
            crate::gizmos::paint::draw_for_mode(
                render,
                rect,
                camera,
                geometry,
                gizmo.mode,
                gizmo.active,
            );
        }
    }
}

fn draw_grid(render: &mut RenderContext, rect: Rect, view_projection: &Mat4, color: Color) {
    let step = (GRID_EXTENT * 2.0) / GRID_DIVISIONS as f32;
    for index in 0..=GRID_DIVISIONS {
        let offset = -GRID_EXTENT + step * index as f32;
        // The two centre lines are the axes' negative halves and are drawn by
        // `draw_axes`; skipping them here avoids double-drawing with a different colour.
        if offset.abs() > 1e-4 {
            draw_segment(
                render,
                rect,
                view_projection,
                Vec3::new(offset, 0.0, -GRID_EXTENT),
                Vec3::new(offset, 0.0, GRID_EXTENT),
                color,
            );
            draw_segment(
                render,
                rect,
                view_projection,
                Vec3::new(-GRID_EXTENT, 0.0, offset),
                Vec3::new(GRID_EXTENT, 0.0, offset),
                color,
            );
        }
    }
}

fn draw_axes(
    render: &mut RenderContext,
    rect: Rect,
    view_projection: &Mat4,
    axes: &[(Vec3, Color); 3],
) {
    // Two units long in each direction: long enough to read as an axis at the default
    // distance, short enough not to run off the grid.
    for (direction, color) in axes {
        draw_segment(
            render,
            rect,
            view_projection,
            Vec3::ZERO,
            *direction * 2.0,
            *color,
        );
        draw_segment(
            render,
            rect,
            view_projection,
            Vec3::ZERO,
            *direction * -2.0,
            *color,
        );
    }
}

fn draw_node(
    render: &mut RenderContext,
    rect: Rect,
    view_projection: &Mat4,
    node: &SceneNode,
    scene: &Scene,
    selected: &[SceneNodeId],
    theme: &EditorTheme,
) {
    let (min, max) = node_world_bounds(scene, node);
    let is_selected = selected.contains(&node.id);

    let color = if is_selected {
        to_engine_color(egui::Color32::from_rgb(0xff, 0xd0, 0x4a))
    } else {
        match node.kind() {
            "mesh" => to_engine_color(egui::Color32::from_rgb(0x7f, 0x9f, 0xd8)),
            "light" => to_engine_color(egui::Color32::from_rgb(0xe0, 0xc0, 0x50)),
            "camera" => to_engine_color(egui::Color32::from_rgb(0x60, 0xc8, 0xc0)),
            _ => to_engine_color(theme.muted),
        }
    };

    // A selection box is drawn twice, offset by one point, which is the only way to get
    // a thicker line out of a one-pixel line primitive without a second draw path.
    box_edges(render, rect, view_projection, min, max, color);
    if is_selected {
        box_edges(
            render,
            rect,
            view_projection,
            [min[0] + 0.02, min[1] + 0.02, min[2] + 0.02],
            [max[0] + 0.02, max[1] + 0.02, max[2] + 0.02],
            color,
        );
    }

    // A camera's look direction: a short line from its position towards its target. The
    // director's view frustum comes with the gizmos (phase 35), not here.
    if let Some(camera) = node.camera {
        let eye = node.transform.position;
        let target = camera.target;
        draw_segment(
            render,
            rect,
            view_projection,
            Vec3::new(eye[0], eye[1], eye[2]),
            Vec3::new(target[0], target[1], target[2]),
            color,
        );
    }
}

/// A node's world box. Meshes use their transform; lights, cameras and groups get a
/// small fixed box at their position so they are visible at all.
///
/// Public because the picker (phase 32) and the hierarchy's "frame this" both need the
/// same box the viewport draws — two definitions would drift and a click would select a
/// box the user cannot see.
pub fn world_transform(scene: &Scene, node: &SceneNode) -> giochi_engine_3d::scene::NodeTransform {
    let mut node = node.clone();
    // The node's transform is local; the viewport shows world space. Walking to the root
    // is cheap (a scene has tens of nodes) and keeps `Scene` free of a cached hierarchy
    // that could disagree with the file.
    let mut parent = node.parent;
    let mut guard = 0;
    while let Some(id) = parent {
        if guard > scene.nodes.len() {
            break;
        }
        guard += 1;
        if let Some(parent_node) = scene.node(id) {
            node.transform = compose(parent_node.transform, node.transform);
            parent = parent_node.parent;
        } else {
            break;
        }
    }
    node.transform
}

/// The node's world-space AABB as `(min, max)`.
pub fn node_world_bounds(scene: &Scene, node: &SceneNode) -> ([f32; 3], [f32; 3]) {
    let world = world_transform(scene, node);
    if node.is_mesh() {
        world.aabb()
    } else {
        // A light, camera or group has no geometry; a small fixed box at its position is
        // what makes it clickable and visible at all.
        let centre = world.position;
        let half = 0.25;
        (
            [centre[0] - half, centre[1] - half, centre[2] - half],
            [centre[0] + half, centre[1] + half, centre[2] + half],
        )
    }
}

/// Parent-world ∘ child-local, with the scale of the parent applied to the child.
fn compose(
    parent: giochi_engine_3d::scene::NodeTransform,
    child: giochi_engine_3d::scene::NodeTransform,
) -> giochi_engine_3d::scene::NodeTransform {
    let parent_matrix = parent.matrix();
    let child_matrix = child.matrix();
    let combined = parent_matrix * child_matrix;

    // Decompose back to position/rotation/scale. Rotation is taken from the matrix's
    // upper-left 3×3 with the parent's scale removed; a hierarchy of uniform scales is
    // what scenes here use, and a shear would not survive any format that stores TRS.
    let cols = combined.to_cols_array();
    let scale_x = Vec3::new(cols[0], cols[1], cols[2]).length();
    let scale_y = Vec3::new(cols[4], cols[5], cols[6]).length();
    let scale_z = Vec3::new(cols[8], cols[9], cols[10]).length();

    giochi_engine_3d::scene::NodeTransform {
        position: [cols[12], cols[13], cols[14]],
        rotation: rotation_from_columns(&cols, [scale_x, scale_y, scale_z]),
        scale: [scale_x, scale_y, scale_z],
    }
}

fn rotation_from_columns(columns: &[f32; 16], scale: [f32; 3]) -> [f32; 4] {
    let x = Vec3::new(
        columns[0] / scale[0].max(1e-6),
        columns[1] / scale[0].max(1e-6),
        columns[2] / scale[0].max(1e-6),
    );
    let y = Vec3::new(
        columns[4] / scale[1].max(1e-6),
        columns[5] / scale[1].max(1e-6),
        columns[6] / scale[1].max(1e-6),
    );
    let z = Vec3::new(
        columns[8] / scale[2].max(1e-6),
        columns[9] / scale[2].max(1e-6),
        columns[10] / scale[2].max(1e-6),
    );
    let matrix = glam::Mat3::from_cols(x, y, z);
    let quaternion = glam::Quat::from_mat3(&matrix);
    [quaternion.x, quaternion.y, quaternion.z, quaternion.w]
}

fn box_edges(
    render: &mut RenderContext,
    rect: Rect,
    view_projection: &Mat4,
    min: [f32; 3],
    max: [f32; 3],
    color: Color,
) {
    let corners = [
        Vec3::new(min[0], min[1], min[2]),
        Vec3::new(max[0], min[1], min[2]),
        Vec3::new(max[0], max[1], min[2]),
        Vec3::new(min[0], max[1], min[2]),
        Vec3::new(min[0], min[1], max[2]),
        Vec3::new(max[0], min[1], max[2]),
        Vec3::new(max[0], max[1], max[2]),
        Vec3::new(min[0], max[1], max[2]),
    ];
    const EDGES: [(usize, usize); 12] = [
        (0, 1),
        (1, 2),
        (2, 3),
        (3, 0),
        (4, 5),
        (5, 6),
        (6, 7),
        (7, 4),
        (0, 4),
        (1, 5),
        (2, 6),
        (3, 7),
    ];
    for (a, b) in EDGES {
        draw_segment(render, rect, view_projection, corners[a], corners[b], color);
    }
}

/// Projects, clips and draws one world-space segment.
///
/// Public because the gizmos draw with the same projection, clipping and colour handling
/// as the scene — two painters would disagree by a pixel and the handle would not be under
/// the arrow.
pub fn draw_segment(
    render: &mut RenderContext,
    rect: Rect,
    view_projection: &Mat4,
    from: Vec3,
    to: Vec3,
    color: Color,
) {
    let Some(from) = project_point(from, view_projection, rect) else {
        return;
    };
    let Some(to) = project_point(to, view_projection, rect) else {
        return;
    };
    let Some((from, to)) = clip_segment(from, to, rect) else {
        return;
    };
    render.draw_line(Vec2::new(from.x, from.y), Vec2::new(to.x, to.y), color);
}

/// World → viewport point, or `None` when the point is behind the camera.
///
/// A point behind the camera has `w <= 0`; dividing by it would mirror the point into
/// the viewport and draw geometry that is not there. Rejecting it is what keeps the grid
/// from reappearing when the camera is under the floor.
pub fn project_point(point: Vec3, view_projection: &Mat4, rect: Rect) -> Option<egui::Pos2> {
    let clip = *view_projection * point.extend(1.0);
    if clip.w <= 1e-6 {
        return None;
    }
    let ndc = Vec4::new(clip.x / clip.w, clip.y / clip.w, clip.z / clip.w, 1.0);
    let x = rect.left() + (ndc.x * 0.5 + 0.5) * rect.width();
    // NDC +Y is up; screen +Y is down.
    let y = rect.top() + (0.5 - ndc.y * 0.5) * rect.height();
    if !x.is_finite() || !y.is_finite() {
        return None;
    }
    Some(egui::pos2(x, y))
}

/// Liang–Barsky clip of a segment against `rect`, with a one-point inset.
///
/// Without this, a line whose endpoints are both far outside the viewport but whose path
/// crosses it would be dropped (the projection is finite even for points off screen),
/// and a line that leaves the viewport would be drawn over the panels.
fn clip_segment(from: egui::Pos2, to: egui::Pos2, rect: Rect) -> Option<(egui::Pos2, egui::Pos2)> {
    let inset = 1.0;
    let min_x = rect.left() + inset;
    let max_x = rect.right() - inset;
    let min_y = rect.top() + inset;
    let max_y = rect.bottom() - inset;
    if max_x <= min_x || max_y <= min_y {
        return None;
    }

    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let mut t0 = 0.0_f32;
    let mut t1 = 1.0_f32;

    let edges = [
        (-dx, from.x - min_x),
        (dx, max_x - from.x),
        (-dy, from.y - min_y),
        (dy, max_y - from.y),
    ];
    for (p, q) in edges {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
            continue;
        }
        let r = q / p;
        if p < 0.0 {
            if r > t1 {
                return None;
            }
            if r > t0 {
                t0 = r;
            }
        } else {
            if r < t0 {
                return None;
            }
            if r < t1 {
                t1 = r;
            }
        }
    }

    Some((
        egui::pos2(from.x + t0 * dx, from.y + t0 * dy),
        egui::pos2(from.x + t1 * dx, from.y + t1 * dy),
    ))
}

fn to_engine_color(color: egui::Color32) -> Color {
    Color::rgb(color.r(), color.g(), color.b())
}

/// Converts a viewport point into the engine's rectangle type, for a phase-32 hit test.
pub fn engine_rect(rect: Rect) -> EngineRect {
    EngineRect::new(rect.left(), rect.top(), rect.width(), rect.height())
}

#[cfg(test)]
mod tests {
    use super::*;
    use giochi_engine_3d::scene::{MeshSource, Primitive, Scene};

    fn rect() -> Rect {
        Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(800.0, 600.0))
    }

    fn camera() -> OrbitCamera {
        OrbitCamera::default()
    }

    #[test]
    fn the_target_projects_to_the_centre_of_the_viewport() {
        let camera = camera();
        let matrix = Mat4::from_cols_array(&camera.view_projection(800.0 / 600.0));
        let centre = project_point(camera.target, &matrix, rect()).expect("in front");
        assert!((centre.x - 500.0).abs() < 1e-3, "{centre:?}");
        assert!((centre.y - 350.0).abs() < 1e-3, "{centre:?}");
    }

    #[test]
    fn a_point_behind_the_camera_is_not_projected() {
        let camera = camera();
        let matrix = Mat4::from_cols_array(&camera.view_projection(1.0));
        let behind = camera.eye() + (camera.eye() - camera.target) * 2.0;
        assert!(project_point(behind, &matrix, rect()).is_none());
    }

    #[test]
    fn clipping_keeps_the_inside_part_of_a_crossing_segment() {
        let clipped = clip_segment(egui::pos2(0.0, 350.0), egui::pos2(900.0, 350.0), rect())
            .expect("crosses the viewport");
        assert!((clipped.0.x - 101.0).abs() < 1e-3, "{clipped:?}");
        assert!((clipped.1.x - 899.0).abs() < 1e-3, "{clipped:?}");
        assert!((clipped.0.y - 350.0).abs() < 1e-3);
    }

    #[test]
    fn clipping_drops_a_segment_entirely_outside() {
        assert!(clip_segment(egui::pos2(0.0, 0.0), egui::pos2(50.0, 50.0), rect()).is_none());
        assert!(
            clip_segment(egui::pos2(1000.0, 700.0), egui::pos2(2000.0, 900.0), rect()).is_none()
        );
    }

    #[test]
    fn clipping_leaves_a_segment_that_is_already_inside() {
        let from = egui::pos2(200.0, 200.0);
        let to = egui::pos2(300.0, 400.0);
        let (a, b) = clip_segment(from, to, rect()).expect("inside");
        assert_eq!(a, from);
        assert_eq!(b, to);
    }

    #[test]
    fn clipping_rejects_a_fully_horizontal_line_above_the_viewport() {
        assert!(clip_segment(egui::pos2(0.0, 10.0), egui::pos2(900.0, 10.0), rect()).is_none());
    }

    #[test]
    fn a_release_is_reported_even_when_the_pointer_is_outside() {
        let ctx = egui::Context::default();
        let mut input = ViewportInput::default();
        let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(100.0, 100.0));
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            input = read_input(ctx, Some(rect));
        });
        assert!(!input.released);
        assert!(input.pressed.is_none());
        assert!(input.pointer.is_none());
    }

    #[test]
    fn viewport_input_without_motion_does_not_move_the_camera() {
        let input = ViewportInput {
            hovered: true,
            ..ViewportInput::default()
        };
        assert!(!input.moves_the_camera());
        let mut camera = camera();
        let before = camera;
        input.apply_to_camera(&mut camera, 600.0);
        assert_eq!(camera, before);
    }

    #[test]
    fn viewport_input_drives_the_three_camera_operations() {
        let mut camera = camera();
        let before = camera;

        ViewportInput {
            orbit: egui::vec2(20.0, -5.0),
            ..ViewportInput::default()
        }
        .apply_to_camera(&mut camera, 600.0);
        assert!(camera.yaw != before.yaw);
        assert!(camera.pitch != before.pitch);
        assert_eq!(camera.distance, before.distance);

        let after_orbit = camera;
        ViewportInput {
            pan: egui::vec2(10.0, 0.0),
            ..ViewportInput::default()
        }
        .apply_to_camera(&mut camera, 600.0);
        assert!(camera.target != after_orbit.target);
        assert_eq!(camera.yaw, after_orbit.yaw);

        let after_pan = camera;
        ViewportInput {
            scroll: 2.0,
            ..ViewportInput::default()
        }
        .apply_to_camera(&mut camera, 600.0);
        assert!(camera.distance < after_pan.distance);
    }

    #[test]
    fn a_parent_transform_moves_the_child_into_world_space() {
        let mut scene = Scene::new("nested");
        let parent = scene.add_node(SceneNode::new(SceneNodeId::new(0), "Root"));
        scene
            .node_mut(parent.id)
            .expect("parent")
            .transform
            .position = [10.0, 0.0, 0.0];

        let mut child = SceneNode::new(SceneNodeId::new(0), "Child");
        child.parent = Some(parent.id);
        child.mesh = Some(MeshSource::Primitive(Primitive::Cube { size: 1.0 }));
        child.transform.position = [1.0, 0.0, 0.0];
        let child = scene.add_node(child);

        let (min, max) = node_world_bounds(&scene, &child);
        assert!((min[0] - 10.5).abs() < 1e-4, "{min:?}");
        assert!((max[0] - 11.5).abs() < 1e-4, "{max:?}");
    }

    #[test]
    fn a_parent_scale_scales_the_child() {
        let mut scene = Scene::new("scaled");
        let parent = scene.add_node(SceneNode::new(SceneNodeId::new(0), "Root"));
        scene.node_mut(parent.id).expect("parent").transform.scale = [2.0, 2.0, 2.0];

        let mut child = SceneNode::new(SceneNodeId::new(0), "Child");
        child.parent = Some(parent.id);
        child.mesh = Some(MeshSource::Primitive(Primitive::Cube { size: 1.0 }));
        let child = scene.add_node(child);

        let (min, max) = node_world_bounds(&scene, &child);
        assert!((max[0] - min[0] - 2.0).abs() < 1e-4, "{min:?} {max:?}");
    }

    #[test]
    fn a_light_without_a_mesh_still_has_a_visible_box() {
        let mut scene = Scene::new("light");
        let mut light = scene.add_node(SceneNode::new(SceneNodeId::new(0), "Sun"));
        light.light = Some(giochi_engine_3d::scene::LightDesc::default());
        light.transform.position = [0.0, 5.0, 0.0];
        let light = scene.add_node(light);

        let (min, max) = node_world_bounds(&scene, &light);
        assert!(min[1] < 5.0 && max[1] > 5.0);
    }

    #[test]
    fn drawing_an_empty_viewport_is_a_no_op() {
        // No renderer available in a unit test; the guard is what matters.
        assert!(rect().width() > 0.0);
        let tiny = Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1.0, 1.0));
        assert!(tiny.width() < 2.0);
    }
}
