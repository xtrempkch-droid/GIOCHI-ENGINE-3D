//! Undo and redo: every edit becomes a command object.
//!
//! # Why a command and not a snapshot
//!
//! The alternative — serialise the whole scene before every edit and restore it to undo — is
//! thirty lines long and works, until the scene has a texture the renderer already uploaded
//! and ids the selection still refers to. Restoring a snapshot also loses the *reason* for
//! the change, which is what the Edit menu has to print ("Undo Move X").
//!
//! A command carries exactly the before and after of the thing it changed. That is why
//! [`SetNode`] stores two whole nodes rather than a delta: a delta needs a schema for every
//! field, and the inspector already has the two nodes in hand.
//!
//! # One drag, one command
//!
//! Rule 6 of the plan: a drag is one undo step, not one per frame. The gizmo code already
//! recomputes the transform from the state the drag started in (ADR-066), so the release
//! handler has the two nodes and pushes a single [`SetNode`]. Ten frames of dragging produce
//! one entry in the stack.
//!
//! # The stack
//!
//! [`CommandStack`] keeps two `Vec`s. `push` clears the redo side: once the user edits after
//! undoing, the future they undid is gone — that is what every editor does, and keeping it
//! would need a tree. The depth is capped, so a long session cannot grow without bound; the
//! cap drops the *oldest* entry, because that is the one the user is least likely to want
//! back.
//!
//! Dirty tracking is a marker: the stack remembers its depth when the scene was last saved,
//! and "modified" means the depth differs. Undoing back to the save point is therefore
//! correctly *not* dirty, which a boolean flag cannot express.

use giochi_engine_3d::scene::{Scene, SceneNode, SceneNodeId};

/// One undoable edit.
///
/// `apply` and `undo` must be exact inverses; the tests for each command check both
/// directions, because a command that applies but does not undo is worse than no undo at
/// all — the user loses the work twice.
pub trait Command: std::fmt::Debug + Send {
    /// What the Edit menu and the status bar call it, e.g. `"Move Cube"`.
    fn label(&self) -> String;

    /// Performs the edit. Called once when pushed, and again on every redo.
    fn apply(&mut self, scene: &mut Scene);

    /// Reverses the edit.
    fn undo(&mut self, scene: &mut Scene);

    /// `self`, for the stack's merge check.
    ///
    /// A `Command` is a trait object, so merging two of them needs a downcast, and a
    /// downcast needs `Any`. There is no blanket implementation that could provide this
    /// without `Self: Sized` breaking object safety, so each command spells it out.
    fn as_any(&self) -> &dyn std::any::Any;

    /// `self`, mutably, for the same reason as [`Command::as_any`].
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}

/// Replaces a node with a modified copy: the inspector, and every finished gizmo drag.
#[derive(Debug, Clone, PartialEq)]
pub struct SetNode {
    /// The node as it was.
    pub before: SceneNode,
    /// The node as it should be.
    pub after: SceneNode,
    /// The name the menu shows, e.g. `"Change Transform"`.
    pub label: String,
}

impl SetNode {
    /// A command that takes `before` to `after`.
    pub fn new(before: SceneNode, after: SceneNode, label: impl Into<String>) -> Self {
        Self {
            before,
            after,
            label: label.into(),
        }
    }
}

impl Command for SetNode {
    fn label(&self) -> String {
        self.label.clone()
    }

    fn apply(&mut self, scene: &mut Scene) {
        scene.replace_node(self.after.clone());
    }

    fn undo(&mut self, scene: &mut Scene) {
        scene.replace_node(self.before.clone());
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// Adds one node: the create menu, and a duplicate of a single node.
#[derive(Debug, Clone, PartialEq)]
pub struct AddNode {
    /// The node to add, id included.
    pub node: SceneNode,
}

impl AddNode {
    /// A command that adds `node`.
    pub fn new(node: SceneNode) -> Self {
        Self { node }
    }
}

impl Command for AddNode {
    fn label(&self) -> String {
        format!("Add {}", self.node.name)
    }

    fn apply(&mut self, scene: &mut Scene) {
        // The node already has its id, so `insert_node` is the honest call: it refuses a
        // duplicate instead of quietly renumbering the node the selection points at.
        let _ = scene.insert_node(self.node.clone());
    }

    fn undo(&mut self, scene: &mut Scene) {
        scene.remove_node(self.node.id);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// Adds several nodes at once: a duplicate of a subtree.
///
/// The nodes must be in **document order** (parents before children), which is exactly the
/// order [`crate::editor::node_edit::duplicate`] returns.
#[derive(Debug, Clone, PartialEq)]
pub struct AddNodes {
    /// The nodes, parents first.
    pub nodes: Vec<SceneNode>,
    /// The name the menu shows.
    pub label: String,
}

impl AddNodes {
    /// A command that adds `nodes`, parents first.
    pub fn new(nodes: Vec<SceneNode>, label: impl Into<String>) -> Self {
        Self {
            nodes,
            label: label.into(),
        }
    }
}

impl Command for AddNodes {
    fn label(&self) -> String {
        self.label.clone()
    }

    fn apply(&mut self, scene: &mut Scene) {
        for node in &self.nodes {
            let _ = scene.insert_node(node.clone());
        }
    }

    fn undo(&mut self, scene: &mut Scene) {
        // Removing the first node would take the whole subtree with it; removing them one by
        // one keeps the command's data valid for a redo either way.
        for node in self.nodes.iter().rev() {
            scene.remove_node(node.id);
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// Removes one or more subtrees, remembering what they were.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoveNodes {
    /// Every removed node, in document order, parents first.
    pub nodes: Vec<SceneNode>,
    /// The name the menu shows.
    pub label: String,
}

impl RemoveNodes {
    /// Captures `ids` and their subtrees, ready to be removed.
    ///
    /// Reads the scene, so the caller can build the command before the removal happens —
    /// which is the only order that works: after it, the nodes are gone.
    pub fn capture(scene: &Scene, ids: &[SceneNodeId], label: impl Into<String>) -> Self {
        let mut nodes = Vec::new();
        let mut seen: Vec<SceneNodeId> = Vec::new();
        for id in ids {
            for member in scene.subtree(*id) {
                if seen.contains(&member) {
                    continue;
                }
                if let Some(node) = scene.node(member) {
                    nodes.push(node.clone());
                    seen.push(member);
                }
            }
        }
        Self {
            nodes,
            label: label.into(),
        }
    }
}

impl Command for RemoveNodes {
    fn label(&self) -> String {
        self.label.clone()
    }

    fn apply(&mut self, scene: &mut Scene) {
        for node in self.nodes.iter().rev() {
            scene.remove_node(node.id);
        }
    }

    fn undo(&mut self, scene: &mut Scene) {
        for node in &self.nodes {
            let _ = scene.insert_node(node.clone());
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// Moves a node under a different parent.
#[derive(Debug, Clone, PartialEq)]
pub struct Reparent {
    /// The node.
    pub id: SceneNodeId,
    /// Its parent before.
    pub before: Option<SceneNodeId>,
    /// Its parent after.
    pub after: Option<SceneNodeId>,
}

impl Command for Reparent {
    fn label(&self) -> String {
        match self.after {
            Some(parent) => format!("Parent {} to {}", self.id, parent),
            None => format!("Unparent {}", self.id),
        }
    }

    fn apply(&mut self, scene: &mut Scene) {
        if let Some(mut node) = scene.node(self.id).cloned() {
            node.parent = self.after;
            scene.replace_node(node);
        }
    }

    fn undo(&mut self, scene: &mut Scene) {
        if let Some(mut node) = scene.node(self.id).cloned() {
            node.parent = self.before;
            scene.replace_node(node);
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// The undo and redo history.
#[derive(Debug)]
pub struct CommandStack {
    undo: Vec<Box<dyn Command>>,
    redo: Vec<Box<dyn Command>>,
    limit: usize,
    saved_depth: usize,
}

/// How many edits are remembered.
///
/// 256 is not a memory budget — every entry is a node or two — it is a patience budget: past
/// a few hundred steps nobody is pressing `Ctrl+Z` to get back, they are opening the file.
pub const DEFAULT_LIMIT: usize = 256;

impl Default for CommandStack {
    fn default() -> Self {
        Self::new(DEFAULT_LIMIT)
    }
}

impl CommandStack {
    /// A stack that remembers at most `limit` edits.
    pub fn new(limit: usize) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            limit: limit.max(1),
            saved_depth: 0,
        }
    }

    /// Applies `command` and pushes it onto the undo side.
    ///
    /// The redo side is dropped: the user has just started a new future.
    pub fn push(&mut self, scene: &mut Scene, mut command: Box<dyn Command>) {
        command.apply(scene);
        self.undo.push(command);
        self.redo.clear();
        self.trim();
    }

    /// Applies `command`, merging it into the previous one when it continues the same
    /// gesture.
    ///
    /// Dragging a `DragValue` for a second fires a change per frame. Pushed separately, that
    /// is sixty undo steps for one nudge, and the user has to press `Ctrl+Z` sixty times to
    /// take it back. With `merge`, the second and later changes of the same gesture replace
    /// the *after* of the entry already on the stack, so the before is still the value from
    /// before the gesture started — undo goes back all the way, in one step.
    ///
    /// Merging only ever happens between two [`SetNode`] commands on the same node with the
    /// same label; the caller decides *when* a gesture is continuing (the pointer is down, or
    /// the field has focus) because only the UI knows that.
    pub fn push_mergeable(
        &mut self,
        scene: &mut Scene,
        mut command: Box<dyn Command>,
        merge: bool,
    ) {
        command.apply(scene);
        if merge {
            if let Some(last) = self.undo.last_mut() {
                if let (Some(last), Some(next)) = (
                    last.as_any_mut().downcast_mut::<SetNode>(),
                    command.as_any().downcast_ref::<SetNode>(),
                ) {
                    if last.after.id == next.after.id && last.label == next.label {
                        last.after = next.after.clone();
                        self.redo.clear();
                        return;
                    }
                }
            }
        }
        self.undo.push(command);
        self.redo.clear();
        self.trim();
    }

    fn trim(&mut self) {
        while self.undo.len() > self.limit {
            self.undo.remove(0);
            self.saved_depth = self.saved_depth.saturating_sub(1);
        }
    }

    /// Undoes the most recent command, if any. Returns its label.
    pub fn undo(&mut self, scene: &mut Scene) -> Option<String> {
        let mut command = self.undo.pop()?;
        command.undo(scene);
        let label = command.label();
        self.redo.push(command);
        Some(label)
    }

    /// Redoes the most recently undone command, if any. Returns its label.
    pub fn redo(&mut self, scene: &mut Scene) -> Option<String> {
        let mut command = self.redo.pop()?;
        command.apply(scene);
        let label = command.label();
        self.undo.push(command);
        Some(label)
    }

    /// Whether there is anything to undo.
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Whether there is anything to redo.
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// The label of the next undo, for the Edit menu.
    pub fn undo_label(&self) -> Option<String> {
        self.undo.last().map(|command| command.label())
    }

    /// The label of the next redo, for the Edit menu.
    pub fn redo_label(&self) -> Option<String> {
        self.redo.last().map(|command| command.label())
    }

    /// How many edits are on the undo side.
    pub fn depth(&self) -> usize {
        self.undo.len()
    }

    /// Records that the scene on disk matches the current state.
    pub fn mark_saved(&mut self) {
        self.saved_depth = self.undo.len();
        self.redo.clear();
    }

    /// Whether the scene has changed since the last save.
    ///
    /// A depth comparison, not a flag: undoing back to the save point makes the scene clean
    /// again, and there is no way to express that with a boolean.
    pub fn is_dirty(&self) -> bool {
        self.undo.len() != self.saved_depth
    }

    /// Forgets the history, for a freshly opened scene.
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.saved_depth = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use giochi_engine_3d::scene::{MeshSource, Primitive};

    fn scene() -> Scene {
        let mut scene = Scene::new("stack");
        for name in ["A", "B"] {
            let mut node = SceneNode::new(SceneNodeId::new(0), name);
            node.mesh = Some(MeshSource::Primitive(Primitive::Cube { size: 1.0 }));
            scene.add_node(node);
        }
        scene
    }

    fn moved(node: &SceneNode, x: f32) -> SceneNode {
        let mut copy = node.clone();
        copy.transform.position = [x, 0.0, 0.0];
        copy
    }

    #[test]
    fn a_command_applies_when_pushed_and_reverses_when_undone() {
        let mut scene = scene();
        let before = scene.node(SceneNodeId(1)).expect("node").clone();
        let after = moved(&before, 5.0);
        let mut stack = CommandStack::default();
        stack.push(&mut scene, Box::new(SetNode::new(before, after, "Move A")));

        assert_eq!(
            scene.node(SceneNodeId(1)).expect("node").transform.position[0],
            5.0
        );
        assert_eq!(stack.undo(&mut scene).as_deref(), Some("Move A"));
        assert_eq!(
            scene.node(SceneNodeId(1)).expect("node").transform.position[0],
            0.0
        );
        assert_eq!(stack.redo(&mut scene).as_deref(), Some("Move A"));
        assert_eq!(
            scene.node(SceneNodeId(1)).expect("node").transform.position[0],
            5.0
        );
    }

    #[test]
    fn a_new_command_after_an_undo_drops_the_redo_side() {
        let mut scene = scene();
        let before = scene.node(SceneNodeId(1)).expect("node").clone();
        let mut stack = CommandStack::default();
        stack.push(
            &mut scene,
            Box::new(SetNode::new(before.clone(), moved(&before, 1.0), "One")),
        );
        stack.push(
            &mut scene,
            Box::new(SetNode::new(
                moved(&before, 1.0),
                moved(&before, 2.0),
                "Two",
            )),
        );
        stack.undo(&mut scene);
        assert!(stack.can_redo());
        let current = scene.node(SceneNodeId(1)).expect("node").clone();
        stack.push(
            &mut scene,
            Box::new(SetNode::new(current.clone(), moved(&current, 9.0), "Three")),
        );
        assert!(!stack.can_redo(), "the undone future is gone");
        assert!(stack.can_undo());
    }

    #[test]
    fn undoing_back_to_the_save_point_is_clean_again() {
        let mut scene = scene();
        let mut stack = CommandStack::default();
        stack.mark_saved();
        assert!(!stack.is_dirty());

        let before = scene.node(SceneNodeId(1)).expect("node").clone();
        stack.push(
            &mut scene,
            Box::new(SetNode::new(before.clone(), moved(&before, 1.0), "Move")),
        );
        assert!(stack.is_dirty(), "one edit after a save");

        stack.undo(&mut scene);
        assert!(!stack.is_dirty(), "back where it was saved");
        stack.redo(&mut scene);
        assert!(stack.is_dirty());
    }

    #[test]
    fn saving_at_a_depth_after_an_undo_is_clean_and_forgets_the_redo() {
        let mut scene = scene();
        let mut stack = CommandStack::default();
        let before = scene.node(SceneNodeId(1)).expect("node").clone();
        stack.push(
            &mut scene,
            Box::new(SetNode::new(
                moved(&before, 1.0),
                moved(&before, 2.0),
                "Two",
            )),
        );
        stack.undo(&mut scene);
        // Saved while the scene is at the state before "Two".
        stack.mark_saved();
        assert!(!stack.is_dirty());
        assert!(
            !stack.can_redo(),
            "a save is a new baseline; the redo side belonged to the old file"
        );
    }

    #[test]
    fn the_stack_drops_the_oldest_entry_past_its_limit() {
        let mut scene = scene();
        let mut stack = CommandStack::new(2);
        let mut current = scene.node(SceneNodeId(1)).expect("node").clone();
        for step in 1..=4 {
            let next = moved(&current, step as f32);
            stack.push(
                &mut scene,
                Box::new(SetNode::new(
                    current.clone(),
                    next.clone(),
                    format!("Step {step}"),
                )),
            );
            current = next;
        }
        assert_eq!(stack.depth(), 2);
        assert_eq!(stack.undo_label().as_deref(), Some("Step 4"));
        stack.undo(&mut scene);
        stack.undo(&mut scene);
        assert!(!stack.can_undo(), "only the two newest are remembered");
    }

    #[test]
    fn labels_describe_the_next_undo_and_redo() {
        let mut scene = scene();
        let mut stack = CommandStack::default();
        assert!(stack.undo_label().is_none() && stack.redo_label().is_none());
        let before = scene.node(SceneNodeId(1)).expect("node").clone();
        stack.push(
            &mut scene,
            Box::new(SetNode::new(before.clone(), moved(&before, 3.0), "Move A")),
        );
        assert_eq!(stack.undo_label().as_deref(), Some("Move A"));
        stack.undo(&mut scene);
        assert_eq!(stack.redo_label().as_deref(), Some("Move A"));
    }

    #[test]
    fn adding_a_node_and_undoing_removes_it() {
        let mut scene = scene();
        let mut node = SceneNode::new(SceneNodeId::new(0), "New");
        node.parent = Some(SceneNodeId(1));
        let node = scene.add_node(node);
        // The node was added by the scene above; undo it through the command and redo it.
        let mut stack = CommandStack::default();
        scene.remove_node(node.id);
        stack.push(&mut scene, Box::new(AddNode::new(node.clone())));
        assert!(scene.node(node.id).is_some());
        stack.undo(&mut scene);
        assert!(scene.node(node.id).is_none());
        stack.redo(&mut scene);
        assert_eq!(
            scene.node(node.id).expect("back").parent,
            Some(SceneNodeId(1))
        );
    }

    #[test]
    fn deleting_a_subtree_restores_it_with_its_parent() {
        let mut scene = scene();
        let mut child = SceneNode::new(SceneNodeId::new(0), "Child");
        child.parent = Some(SceneNodeId(2));
        let child = scene.add_node(child);
        let mut stack = CommandStack::default();
        let command = RemoveNodes::capture(&scene, &[SceneNodeId(2)], "Delete B");

        stack.push(&mut scene, Box::new(command));
        assert!(scene.node(SceneNodeId(2)).is_none());
        assert!(scene.node(child.id).is_none(), "the child went with it");

        stack.undo(&mut scene);
        assert!(scene.node(SceneNodeId(2)).is_some());
        assert_eq!(
            scene.node(child.id).expect("child").parent,
            Some(SceneNodeId(2)),
            "and it came back under its parent"
        );
    }

    #[test]
    fn adding_several_nodes_at_once_and_undoing_takes_them_all() {
        let mut scene = scene();
        let mut parent = SceneNode::new(SceneNodeId::new(0), "P");
        let mut child = SceneNode::new(SceneNodeId::new(0), "C");
        child.parent = Some(parent.id);
        parent = scene.add_node(parent);
        child.parent = Some(parent.id);
        let child = scene.add_node(child);
        let mut stack = CommandStack::default();

        // Remove both, then add them back through one command.
        scene.remove_node(parent.id);
        stack.push(
            &mut scene,
            Box::new(AddNodes::new(
                vec![parent.clone(), child.clone()],
                "Add P and C",
            )),
        );
        assert!(scene.node(parent.id).is_some() && scene.node(child.id).is_some());
        stack.undo(&mut scene);
        assert!(scene.node(parent.id).is_none());
        assert!(scene.node(child.id).is_none());
    }

    #[test]
    fn re_parenting_is_reversible() {
        let mut scene = scene();
        let mut stack = CommandStack::default();
        stack.push(
            &mut scene,
            Box::new(Reparent {
                id: SceneNodeId(2),
                before: None,
                after: Some(SceneNodeId(1)),
            }),
        );
        assert_eq!(
            scene.node(SceneNodeId(2)).expect("node").parent,
            Some(SceneNodeId(1))
        );
        stack.undo(&mut scene);
        assert_eq!(scene.node(SceneNodeId(2)).expect("node").parent, None);
    }

    #[test]
    fn a_continuous_gesture_merges_into_one_entry() {
        // Dragging one value for five frames is one undo step, not five, and undo goes back
        // to the value from before the gesture started.
        let mut scene = scene();
        let mut stack = CommandStack::default();
        let before = scene.node(SceneNodeId(1)).expect("node").clone();
        for step in 1..=5 {
            let current = scene.node(SceneNodeId(1)).expect("node").clone();
            let next = moved(&current, step as f32);
            stack.push_mergeable(
                &mut scene,
                Box::new(SetNode::new(current, next, "Change Transform")),
                true,
            );
        }
        assert_eq!(stack.depth(), 1, "one entry for the whole drag");
        assert_eq!(
            scene.node(SceneNodeId(1)).expect("node").transform.position[0],
            5.0
        );
        stack.undo(&mut scene);
        assert_eq!(
            scene.node(SceneNodeId(1)).expect("node").transform.position,
            before.transform.position,
            "undo returns to the value before the gesture"
        );
    }

    #[test]
    fn a_merge_does_not_cross_a_gap_in_the_gesture() {
        let mut scene = scene();
        let mut stack = CommandStack::default();
        let first = scene.node(SceneNodeId(1)).expect("node").clone();
        stack.push_mergeable(
            &mut scene,
            Box::new(SetNode::new(
                first.clone(),
                moved(&first, 1.0),
                "Change Transform",
            )),
            true,
        );
        // Pointer released: a new gesture starts a new entry.
        let second = scene.node(SceneNodeId(1)).expect("node").clone();
        stack.push_mergeable(
            &mut scene,
            Box::new(SetNode::new(
                second.clone(),
                moved(&second, 2.0),
                "Change Transform",
            )),
            false,
        );
        assert_eq!(stack.depth(), 2);
    }

    #[test]
    fn a_merge_does_not_cross_two_different_nodes() {
        let mut scene = scene();
        let mut stack = CommandStack::default();
        let a = scene.node(SceneNodeId(1)).expect("node").clone();
        let b = scene.node(SceneNodeId(2)).expect("node").clone();
        stack.push_mergeable(
            &mut scene,
            Box::new(SetNode::new(a.clone(), moved(&a, 1.0), "Change Transform")),
            true,
        );
        stack.push_mergeable(
            &mut scene,
            Box::new(SetNode::new(b.clone(), moved(&b, 1.0), "Change Transform")),
            true,
        );
        assert_eq!(stack.depth(), 2, "different nodes are different edits");
    }

    #[test]
    fn a_merge_does_not_cross_two_different_labels() {
        let mut scene = scene();
        let mut stack = CommandStack::default();
        let a = scene.node(SceneNodeId(1)).expect("node").clone();
        stack.push_mergeable(
            &mut scene,
            Box::new(SetNode::new(a.clone(), moved(&a, 1.0), "Change Transform")),
            true,
        );
        let b = scene.node(SceneNodeId(1)).expect("node").clone();
        stack.push_mergeable(
            &mut scene,
            Box::new(SetNode::new(b.clone(), moved(&b, 2.0), "Change Material")),
            true,
        );
        assert_eq!(stack.depth(), 2, "different fields are different edits");
    }

    #[test]
    fn clearing_the_stack_forgets_everything_and_is_clean() {
        let mut scene = scene();
        let mut stack = CommandStack::default();
        let before = scene.node(SceneNodeId(1)).expect("node").clone();
        stack.push(
            &mut scene,
            Box::new(SetNode::new(before.clone(), moved(&before, 1.0), "Move")),
        );
        stack.clear();
        assert!(!stack.can_undo() && !stack.can_redo());
        assert!(!stack.is_dirty());
    }
}
