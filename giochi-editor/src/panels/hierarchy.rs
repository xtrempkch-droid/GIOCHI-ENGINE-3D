//! The hierarchy panel: the scene's tree, and clicking it.
//!
//! The panel does not walk the scene while holding a borrow of it: it first takes a
//! [`TreeSnapshot`] (a flat, owned list with parent links), draws that, and reports what
//! the pointer did. The state is then mutated in one place. The alternative — recursing
//! over `&Scene` while calling `&mut EditorState` — is a borrow-checker fight that also
//! makes "click selects" spread over the recursion.
//!
//! Selection rules are the ones every DCC tool uses:
//!
//! | Gesture | Effect |
//! |---|---|
//! | click | select this node only |
//! | Ctrl+click | add/remove this node from the selection |
//! | double-click | select and frame it in the viewport |

use giochi_engine_3d::scene::{Scene, SceneNodeId};

use crate::editor::icons::IconSet;
use crate::editor::{EditorState, EditorTheme};

/// One row of the tree, flattened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeNode {
    /// The node this row is.
    pub id: SceneNodeId,
    /// What the row shows.
    pub label: String,
    /// `mesh`, `light`, `camera` or `group`.
    pub kind: &'static str,
    /// Index into [`TreeSnapshot::nodes`] of the parent, if any.
    pub parent: Option<usize>,
    /// Indices of the direct children, in document order.
    pub children: Vec<usize>,
}

/// A flat, owned copy of the scene's hierarchy.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TreeSnapshot {
    /// Every node, parents before children.
    pub nodes: Vec<TreeNode>,
    /// Indices of the roots, in document order.
    pub roots: Vec<usize>,
}

impl TreeSnapshot {
    /// Builds the snapshot from a scene.
    ///
    /// A node whose parent is missing becomes a root: the tree must render everything the
    /// file contains, even a file that failed validation.
    pub fn from_scene(scene: &Scene) -> Self {
        let mut nodes: Vec<TreeNode> = scene
            .nodes
            .iter()
            .map(|node| TreeNode {
                id: node.id,
                label: node.name.clone(),
                kind: node.kind(),
                parent: None,
                children: Vec::new(),
            })
            .collect();

        let index_of =
            |id: SceneNodeId, nodes: &[TreeNode]| nodes.iter().position(|node| node.id == id);

        let mut roots = Vec::new();
        for index in 0..nodes.len() {
            let parent_id = scene.nodes[index].parent;
            match parent_id.and_then(|parent| index_of(parent, &nodes)) {
                Some(parent_index) if parent_index != index => {
                    nodes[index].parent = Some(parent_index);
                    nodes[parent_index].children.push(index);
                }
                _ => roots.push(index),
            }
        }

        Self { nodes, roots }
    }

    /// Whether the scene has no nodes at all.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// The row at `index`.
    pub fn node(&self, index: usize) -> Option<&TreeNode> {
        self.nodes.get(index)
    }
}

/// What the user did in the hierarchy this frame.
///
/// A plain old data record rather than a set of closures: the panel is drawn inside a
/// borrow of the layout, and every mutation of the scene has to happen after it, once, in
/// [`draw`]. Every field is at most one action, because egui reports at most one per frame
/// anyway — a record with `Option`s cannot express a state the input cannot produce.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HierarchyAction {
    /// A node was clicked, with Ctrl held (toggle) or not (replace).
    pub selection: Option<(SceneNodeId, bool)>,
    /// A node was double-clicked and should be framed.
    pub focus: Option<SceneNodeId>,
    /// A node was dragged onto another: `(child, new_parent)`.
    pub reparent: Option<(SceneNodeId, Option<SceneNodeId>)>,
    /// A kind was chosen from the create menu.
    pub create: Option<crate::editor::node_edit::NewNodeKind>,
    /// Delete was chosen for a node.
    pub delete: Option<SceneNodeId>,
    /// Duplicate was chosen for a node.
    pub duplicate: Option<SceneNodeId>,
}

impl HierarchyAction {
    /// Whether anything happened this frame.
    pub fn is_empty(&self) -> bool {
        self.selection.is_none()
            && self.focus.is_none()
            && self.reparent.is_none()
            && self.create.is_none()
            && self.delete.is_none()
            && self.duplicate.is_none()
    }
}

/// The payload a hierarchy row carries while it is being dragged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeDrag {
    /// The node being dragged.
    pub id: SceneNodeId,
}

/// Draws the tree and applies what was clicked.
///
/// Returns the action, which is also what the panel applied — the value is for the tests
/// and for phase 36's context menu.
pub fn draw(
    ui: &mut egui::Ui,
    state: &mut EditorState,
    icons: Option<&IconSet>,
    theme: &EditorTheme,
) -> HierarchyAction {
    let snapshot = state
        .scene
        .as_ref()
        .map(TreeSnapshot::from_scene)
        .unwrap_or_default();

    if snapshot.is_empty() {
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new("No scene open. File > New Scene (Ctrl+N).")
                .color(theme.muted)
                .small(),
        );
        return HierarchyAction::default();
    }

    let mut action = HierarchyAction::default();
    let selected = state.selected.clone();

    // The create menu lives in the panel header row rather than in the Edit menu: creating a
    // node is the most frequent thing an editor does, and it belongs next to the tree it
    // adds to. The Edit menu keeps the same actions for discoverability.
    ui.horizontal(|ui| {
        ui.menu_button("+", |ui| {
            for kind in crate::editor::node_edit::NEW_NODE_KINDS {
                if ui.button(kind.label()).clicked() {
                    action.create = Some(kind);
                    ui.close();
                }
            }
        })
        .response
        .on_hover_text("Create a node");
        ui.label(
            egui::RichText::new(format!("{} node(s)", snapshot.nodes.len()))
                .color(theme.muted)
                .small(),
        );
    });

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for index in snapshot.roots.clone() {
                draw_row(ui, &snapshot, index, icons, &selected, theme, &mut action);
            }
            // Dropping on the empty space below the tree moves a node to the root: the tree
            // is the only place in the editor where "outside everything" is reachable.
            let empty = ui.available_rect_before_wrap();
            let response = ui.interact(
                empty,
                egui::Id::new("giochi-hierarchy-root-drop"),
                egui::Sense::hover(),
            );
            if let Some(payload) = response.dnd_release_payload::<NodeDrag>() {
                action.reparent = Some((payload.id, None));
            }
        });

    if let Some(id) = action.focus {
        if let Some((min, max)) =
            state
                .scene
                .as_ref()
                .and_then(|scene| scene.node(id))
                .map(|node| {
                    crate::editor::viewport::node_world_bounds(
                        state.scene.as_ref().expect("scene"),
                        node,
                    )
                })
        {
            state.viewport_camera.frame_aabb(min, max);
            state.select(Some(id));
            state.status =
                crate::editor::Status::info(format!("Framed {}", label_of(&snapshot, id)));
        }
    } else if let Some((id, toggle)) = action.selection {
        if toggle {
            state.toggle_selection(id);
        } else {
            state.select(Some(id));
        }
    }

    if let Some(kind) = action.create {
        state.create_node(kind);
    }
    if let Some(id) = action.delete {
        // Acting on one row deletes that row and its subtree, not the whole selection: the
        // menu said "Delete" next to *this* node.
        state.select(Some(id));
        state.delete_selected();
    }
    if let Some(id) = action.duplicate {
        if !state.is_selected(id) {
            state.select(Some(id));
        }
        state.duplicate_selected();
    }
    if let Some((child, parent)) = action.reparent {
        state.reparent(child, parent);
    }

    action
}

fn label_of(snapshot: &TreeSnapshot, id: SceneNodeId) -> String {
    snapshot
        .nodes
        .iter()
        .find(|node| node.id == id)
        .map(|node| node.label.clone())
        .unwrap_or_else(|| id.to_string())
}

fn draw_row(
    ui: &mut egui::Ui,
    snapshot: &TreeSnapshot,
    index: usize,
    icons: Option<&IconSet>,
    selected: &[SceneNodeId],
    theme: &EditorTheme,
    action: &mut HierarchyAction,
) {
    let Some(node) = snapshot.node(index) else {
        return;
    };
    let is_selected = selected.contains(&node.id);
    let node = node.clone();

    if node.children.is_empty() {
        let response = ui.horizontal(|ui| {
            match icons {
                Some(icons) => icons.paint(ui, node.kind, 14.0),
                None => {
                    ui.label(egui::RichText::new(node.kind).small().monospace());
                }
            }
            ui.selectable_label(is_selected, &node.label)
        });
        row_interactions(ui, &response.inner, &node, snapshot, index, theme, action);
        if response.inner.clicked() {
            let toggle = ui.input(|input| input.modifiers.command);
            action.selection = Some((node.id, toggle));
        }
        if response.inner.double_clicked() {
            action.focus = Some(node.id);
        }
        return;
    }

    // A branch keeps its children inside a header, which is what makes a deep hierarchy
    // readable. `default_open(true)` because a collapsed tree on open would hide the
    // scene the user just loaded.
    let collapsing = egui::CollapsingHeader::new(row_label(&node, is_selected, theme))
        .id_salt(("giochi-hierarchy", node.id.raw()))
        .default_open(true)
        .show(ui, |ui| {
            for child in node.children.clone() {
                draw_row(ui, snapshot, child, icons, selected, theme, action);
            }
        });
    // The header is the row for a branch: clicking it selects (and, as the widget's own job,
    // opens or closes it), and it is the handle to drag the whole group by.
    row_interactions(
        ui,
        &collapsing.header_response,
        &node,
        snapshot,
        index,
        theme,
        action,
    );
    if collapsing.header_response.clicked() {
        let toggle = ui.input(|input| input.modifiers.command);
        action.selection = Some((node.id, toggle));
    }
    if collapsing.header_response.double_clicked() {
        action.focus = Some(node.id);
    }
}

/// The parts of a row that are the same for a leaf and for a group header: dragging it,
/// accepting a drop on it, and the per-node menu.
fn row_interactions(
    ui: &egui::Ui,
    response: &egui::Response,
    node: &TreeNode,
    snapshot: &TreeSnapshot,
    index: usize,
    theme: &EditorTheme,
    action: &mut HierarchyAction,
) {
    // A payload is set when the drag *starts* and read by whichever row the pointer is over.
    // `dnd_drag_source` would do this, but it wraps the whole widget — children included —
    // so dragging a child out of a group would drag the group.
    if response.drag_started() {
        egui::DragAndDrop::set_payload(ui.ctx(), NodeDrag { id: node.id });
    }

    if let Some(payload) = response.dnd_release_payload::<NodeDrag>() {
        let legal = payload.id != node.id && !subtree_contains(snapshot, payload.id, index);
        if legal {
            action.reparent = Some((payload.id, Some(node.id)));
        }
    }

    if let Some(payload) = response.dnd_hover_payload::<NodeDrag>() {
        // The row under the pointer is outlined: red when the drop would be refused (a node
        // into itself or into its own child), accent when it would work. A drop that
        // silently does nothing is the worst of the three outcomes.
        let legal = payload.id != node.id && !subtree_contains(snapshot, payload.id, index);
        let color = if legal { theme.accent } else { theme.danger };
        ui.painter().rect_stroke(
            response.rect,
            2.0,
            egui::Stroke::new(1.0_f32, color),
            egui::StrokeKind::Inside,
        );
    }

    let _ = response
        .clone()
        .on_hover_text(format!("{} {}", node.kind, node.id));

    response.context_menu(|ui| {
        if ui.button("Duplicate  Ctrl+D").clicked() {
            action.duplicate = Some(node.id);
            ui.close();
        }
        if ui.button("Delete  Del").clicked() {
            action.delete = Some(node.id);
            ui.close();
        }
        ui.separator();
        if ui.button("Move to root").clicked() {
            action.reparent = Some((node.id, None));
            ui.close();
        }
    });
}

/// Whether `descendant` is inside `ancestor`'s subtree, by snapshot index.
fn subtree_contains(snapshot: &TreeSnapshot, ancestor: SceneNodeId, descendant: usize) -> bool {
    let Some(root) = snapshot.nodes.iter().position(|node| node.id == ancestor) else {
        return false;
    };
    let mut stack = vec![root];
    while let Some(index) = stack.pop() {
        if let Some(node) = snapshot.node(index) {
            if node.children.contains(&descendant) {
                return true;
            }
            stack.extend(node.children.iter().copied());
        }
    }
    false
}

fn row_label<'a>(node: &'a TreeNode, is_selected: bool, theme: &'a EditorTheme) -> egui::RichText {
    let mut text = egui::RichText::new(&node.label);
    if is_selected {
        text = text.color(theme.accent).strong();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use giochi_engine_3d::scene::{MeshSource, Primitive, SceneNode};

    fn scene() -> Scene {
        let mut scene = Scene::new("tree");
        let root = scene.add_node(SceneNode::new(SceneNodeId::new(0), "Root"));
        let mut child_a = SceneNode::new(SceneNodeId::new(0), "Child A");
        child_a.parent = Some(root.id);
        child_a.mesh = Some(MeshSource::Primitive(Primitive::Cube { size: 1.0 }));
        let child_a = scene.add_node(child_a);
        let mut grandchild = SceneNode::new(SceneNodeId::new(0), "Grandchild");
        grandchild.parent = Some(child_a.id);
        scene.add_node(grandchild);
        let mut child_b = SceneNode::new(SceneNodeId::new(0), "Child B");
        child_b.parent = Some(root.id);
        scene.add_node(child_b);
        scene
    }

    #[test]
    fn the_snapshot_respects_the_hierarchy() {
        let snapshot = TreeSnapshot::from_scene(&scene());
        assert_eq!(snapshot.roots.len(), 1);
        let root = &snapshot.nodes[snapshot.roots[0]];
        assert_eq!(root.label, "Root");
        assert_eq!(root.children.len(), 2);

        let child_a = &snapshot.nodes[root.children[0]];
        assert_eq!(child_a.label, "Child A");
        assert_eq!(child_a.kind, "mesh");
        assert_eq!(child_a.children.len(), 1);
        assert_eq!(snapshot.nodes[child_a.children[0]].label, "Grandchild");

        let child_b = &snapshot.nodes[root.children[1]];
        assert_eq!(child_b.label, "Child B");
        assert!(child_b.children.is_empty());
        assert_eq!(child_b.kind, "group");
    }

    #[test]
    fn an_empty_scene_has_an_empty_snapshot() {
        let snapshot = TreeSnapshot::from_scene(&Scene::new("empty"));
        assert!(snapshot.is_empty());
        assert!(snapshot.roots.is_empty());
    }

    #[test]
    fn an_orphan_becomes_a_root_instead_of_disappearing() {
        let mut scene = Scene::new("orphan");
        let mut node = SceneNode::new(SceneNodeId(4), "Orphan");
        node.parent = Some(SceneNodeId(99));
        scene.nodes.push(node);

        let snapshot = TreeSnapshot::from_scene(&scene);
        assert_eq!(snapshot.roots.len(), 1);
        assert_eq!(snapshot.nodes[0].label, "Orphan");
    }

    #[test]
    fn a_self_parent_is_treated_as_a_root() {
        let mut scene = Scene::new("self");
        let mut node = SceneNode::new(SceneNodeId(1), "Self");
        node.parent = Some(SceneNodeId(1));
        scene.nodes.push(node);

        let snapshot = TreeSnapshot::from_scene(&scene);
        assert_eq!(snapshot.roots, vec![0]);
        assert!(snapshot.nodes[0].children.is_empty());
    }

    #[test]
    fn a_cycle_does_not_hang_the_snapshot() {
        let mut scene = Scene::new("cycle");
        scene.nodes.push(SceneNode {
            parent: Some(SceneNodeId(2)),
            ..SceneNode::new(SceneNodeId(1), "A")
        });
        scene.nodes.push(SceneNode {
            parent: Some(SceneNodeId(1)),
            ..SceneNode::new(SceneNodeId(2), "B")
        });
        let snapshot = TreeSnapshot::from_scene(&scene);
        // Each points at the other, so neither is a root; the snapshot must still be
        // finite and complete.
        assert_eq!(snapshot.nodes.len(), 2);
    }

    #[test]
    fn a_drop_into_a_descendants_row_is_refused_before_it_is_sent() {
        // The panel filters illegal drops against the snapshot, which is what keeps the
        // scene from ever seeing a cycle: `reparent` would refuse it too, but the user would
        // get a warning for a drop the panel could have greyed out.
        let scene = scene();
        let snapshot = TreeSnapshot::from_scene(&scene);
        let child = snapshot
            .nodes
            .iter()
            .position(|node| node.id == SceneNodeId(2))
            .expect("child");
        let grandchild = snapshot
            .nodes
            .iter()
            .position(|node| node.id == SceneNodeId(3))
            .expect("grandchild");
        assert!(subtree_contains(&snapshot, SceneNodeId(1), grandchild));
        assert!(subtree_contains(&snapshot, SceneNodeId(1), child));
        assert!(!subtree_contains(&snapshot, SceneNodeId(3), child));
        assert!(!subtree_contains(&snapshot, SceneNodeId(99), child));
    }

    #[test]
    fn an_empty_action_reports_nothing_happened() {
        assert!(HierarchyAction::default().is_empty());
        let action = HierarchyAction {
            duplicate: Some(SceneNodeId(2)),
            ..HierarchyAction::default()
        };
        assert!(!action.is_empty());
    }

    #[test]
    fn the_default_scene_shows_three_roots_in_order() {
        let snapshot = TreeSnapshot::from_scene(&Scene::default_scene());
        assert_eq!(snapshot.roots.len(), 3);
        let labels: Vec<&str> = snapshot
            .roots
            .iter()
            .map(|index| snapshot.nodes[*index].label.as_str())
            .collect();
        assert_eq!(labels, vec!["Cube", "Sun", "Camera"]);
        let kinds: Vec<&str> = snapshot
            .roots
            .iter()
            .map(|index| snapshot.nodes[*index].kind)
            .collect();
        assert_eq!(kinds, vec!["mesh", "light", "camera"]);
    }
}
