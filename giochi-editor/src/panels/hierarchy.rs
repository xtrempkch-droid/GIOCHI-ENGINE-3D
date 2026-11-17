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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HierarchyAction {
    /// A node was clicked, with Ctrl held (toggle) or not (replace).
    pub selection: Option<(SceneNodeId, bool)>,
    /// A node was double-clicked and should be framed.
    pub focus: Option<SceneNodeId>,
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

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for index in snapshot.roots.clone() {
                draw_row(ui, &snapshot, index, icons, &selected, theme, &mut action);
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

    if node.children.is_empty() {
        row_contents(ui, node, is_selected, icons, action);
        return;
    }

    // A branch keeps its children inside a header, which is what makes a deep hierarchy
    // readable. `default_open(true)` because a collapsed tree on open would hide the
    // scene the user just loaded.
    egui::CollapsingHeader::new(row_label(node, is_selected, theme))
        .id_salt(("giochi-hierarchy", node.id.raw()))
        .default_open(true)
        .show(ui, |ui| {
            for child in node.children.clone() {
                draw_row(ui, snapshot, child, icons, selected, theme, action);
            }
        });
}

fn row_contents(
    ui: &mut egui::Ui,
    node: &TreeNode,
    is_selected: bool,
    icons: Option<&IconSet>,
    action: &mut HierarchyAction,
) {
    ui.horizontal(|ui| {
        match icons {
            Some(icons) => icons.paint(ui, node.kind, 14.0),
            None => {
                ui.label(egui::RichText::new(node.kind).small().monospace());
            }
        }
        let response = ui.selectable_label(is_selected, &node.label);
        if response.clicked() {
            let toggle = ui.input(|input| input.modifiers.command);
            action.selection = Some((node.id, toggle));
        }
        if response.double_clicked() {
            action.focus = Some(node.id);
        }
        response.on_hover_text(format!("{} {}", node.kind, node.id));
    });
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
