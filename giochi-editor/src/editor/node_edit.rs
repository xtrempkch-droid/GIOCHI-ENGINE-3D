//! Creating, deleting, duplicating and reparenting nodes.
//!
//! Every operation here is a **pure function of the scene** rather than a method on
//! `EditorState`. That is what lets every rule below be tested without a renderer, an egui
//! context or a window — and the rules are where the bugs live: a duplicate that reuses an
//! id silently overwrites a node, and a reparent that allows a cycle makes the scene
//! unserialisable (the format's own `validate` would refuse to load what the editor just
//! saved).
//!
//! # Names
//!
//! New nodes take a unique name (`Cube`, `Cube 2`, `Cube 3`). The scene format allows
//! duplicates and only warns about them, but two rows called "Cube" in the hierarchy are
//! indistinguishable to the person clicking them, and the console logs the name.
//!
//! # Duplicates
//!
//! `Ctrl+D` copies the whole subtree, not the single node: duplicating a car without its
//! wheels produces two objects that look broken together. Ids are freshly assigned, the
//! parent links between copied nodes are preserved, and the copy is inserted as a sibling of
//! the original — with a small offset so it does not hide exactly behind it.

use giochi_engine_3d::scene::{
    CameraDesc, LightDesc, MeshSource, NodeTransform, Primitive, Scene, SceneNode, SceneNodeId,
};

/// What "create" can make.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewNodeKind {
    /// A unit cube.
    Cube,
    /// A unit sphere.
    Sphere,
    /// A 4×4 plane on the floor.
    Plane,
    /// An empty, for grouping.
    Group,
    /// A directional light.
    Light,
    /// A camera.
    Camera,
}

/// Every kind, in the order the menu lists them.
pub const NEW_NODE_KINDS: [NewNodeKind; 6] = [
    NewNodeKind::Cube,
    NewNodeKind::Sphere,
    NewNodeKind::Plane,
    NewNodeKind::Group,
    NewNodeKind::Light,
    NewNodeKind::Camera,
];

impl NewNodeKind {
    /// The menu label, which is also the base of the node's name.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Cube => "Cube",
            Self::Sphere => "Sphere",
            Self::Plane => "Plane",
            Self::Group => "Group",
            Self::Light => "Light",
            Self::Camera => "Camera",
        }
    }

    /// The icon the hierarchy uses for it.
    pub const fn icon(self) -> &'static str {
        match self {
            Self::Cube | Self::Sphere | Self::Plane => "mesh",
            Self::Group => "group",
            Self::Light => "light",
            Self::Camera => "camera",
        }
    }

    /// The node itself, before it is added, so the caller can adjust it first.
    pub fn node(self) -> SceneNode {
        let mut node = SceneNode::new(SceneNodeId::new(0), self.label());
        match self {
            Self::Cube => {
                node.mesh = Some(MeshSource::Primitive(Primitive::Cube { size: 1.0 }));
                // On the floor rather than half-buried in it: a cube created at the origin
                // with its centre at y = 0 is the first thing every user then fixes by hand.
                node.transform.position = [0.0, 0.5, 0.0];
            }
            Self::Sphere => {
                node.mesh = Some(MeshSource::Primitive(Primitive::Sphere {
                    radius: 0.5,
                    rings: 16,
                    segments: 24,
                }));
                node.transform.position = [0.0, 0.5, 0.0];
            }
            Self::Plane => {
                node.mesh = Some(MeshSource::Primitive(Primitive::Plane {
                    size: 4.0,
                    subdivisions: 1,
                }));
            }
            Self::Group => {}
            Self::Light => {
                node.light = Some(LightDesc::default());
                node.transform.position = [0.0, 4.0, 0.0];
            }
            Self::Camera => {
                node.camera = Some(CameraDesc::default());
                node.transform.position = [4.0, 3.0, 6.0];
            }
        }
        node
    }
}

/// A name not already taken in `scene`.
///
/// `base`, then `base 2`, `base 3`… A name that already ends in a number is not treated
/// specially: `Cube 2` copied gives `Cube 2 2`, which is ugly but unambiguous, and guessing
/// at "increment the trailing number" surprises more than it helps.
pub fn unique_name(scene: &Scene, base: &str) -> String {
    if !scene.nodes.iter().any(|node| node.name == base) {
        return base.to_owned();
    }
    let mut index = 2;
    loop {
        let candidate = format!("{base} {index}");
        if !scene.nodes.iter().any(|node| node.name == candidate) {
            return candidate;
        }
        index += 1;
    }
}

/// The node `kind` names, positioned at `position` relative to its parent and parented to
/// `parent`.
///
/// # Errors
///
/// Fails when `parent` is not in the scene — the one input the caller cannot check itself
/// after the `Scene` borrow has been taken.
pub fn create(
    scene: &mut Scene,
    kind: NewNodeKind,
    parent: Option<SceneNodeId>,
    position: Option<[f32; 3]>,
) -> Result<SceneNode, String> {
    if let Some(parent) = parent {
        if scene.node(parent).is_none() {
            return Err(format!("parent {parent} does not exist"));
        }
    }
    let mut node = kind.node();
    node.name = unique_name(scene, kind.label());
    node.parent = parent;
    if let Some(position) = position {
        node.transform.position = position;
    }
    Ok(scene.add_node(node))
}

/// Copies `id` and its whole subtree, returning the new ids (the copy first).
///
/// The copy is a sibling of the original: same parent, offset by `offset` in its parent's
/// space. An empty result means `id` was not in the scene.
pub fn duplicate(scene: &mut Scene, id: SceneNodeId, offset: [f32; 3]) -> Vec<SceneNodeId> {
    let Some(original) = scene.node(id).cloned() else {
        return Vec::new();
    };
    let members = scene.subtree(id);

    // Two passes: add the nodes so they all have ids, then fix the parent links. Doing it
    // in one pass would need the map to already contain a parent that has not been added
    // yet, and the subtree is ordered parents-first anyway, but the two passes make the
    // invariant explicit instead of relying on that ordering.
    let mut mapping: Vec<(SceneNodeId, SceneNodeId)> = Vec::with_capacity(members.len());
    for member in &members {
        let Some(node) = scene.node(*member) else {
            continue;
        };
        let mut copy = node.clone();
        copy.id = SceneNodeId::new(0);
        copy.name = unique_name(scene, &node.name);
        if *member != id {
            copy.parent = None;
        }
        let stored = scene.add_node(copy);
        mapping.push((*member, stored.id));
    }

    for (source, copy) in &mapping {
        if *source == id {
            continue;
        }
        let Some(original_parent) = scene.node(*source).map(|node| node.parent) else {
            continue;
        };
        let new_parent = original_parent.and_then(|parent| {
            mapping
                .iter()
                .find(|(from, _)| *from == parent)
                .map(|(_, to)| *to)
        });
        if let Some(mut node) = scene.node(*copy).cloned() {
            node.parent = new_parent;
            scene.replace_node(node);
        }
    }

    // The copy sits with the original, offset so both are visible.
    if let Some((_, root_copy)) = mapping.first().copied() {
        if let Some(mut node) = scene.node(root_copy).cloned() {
            node.parent = original.parent;
            node.transform.position = [
                original.transform.position[0] + offset[0],
                original.transform.position[1] + offset[1],
                original.transform.position[2] + offset[2],
            ];
            scene.replace_node(node);
        }
    }

    mapping.into_iter().map(|(_, to)| to).collect()
}

/// Deletes every node in `ids`, plus their subtrees. Returns how many nodes went away.
///
/// A node whose ancestor is also in `ids` is skipped: `remove_node` already took it, and
/// asking twice would count it twice and, worse, look for a node that no longer exists.
pub fn delete(scene: &mut Scene, ids: &[SceneNodeId]) -> usize {
    let mut removed = 0;
    let mut gone: Vec<SceneNodeId> = Vec::new();
    for id in ids {
        if gone.contains(id) {
            continue;
        }
        let subtree = scene.subtree(*id);
        if subtree.is_empty() {
            continue;
        }
        removed += scene.remove_node(*id);
        gone.extend(subtree);
    }
    removed
}

/// Moves `id` under `new_parent` (or to the root when `None`).
///
/// The transform is preserved **as a local transform**: the node keeps its numbers, so
/// re-parenting an object does not teleport it — unless the new parent has a transform, in
/// which case the object moves with it, exactly as a child should.
///
/// # Errors
///
/// - the node or the new parent is not in the scene;
/// - `new_parent` is `id` itself;
/// - `new_parent` is a descendant of `id`, which would make a cycle: the scene would still
///   render (the tree walk would loop forever) and could not be saved and reloaded.
pub fn reparent(
    scene: &mut Scene,
    id: SceneNodeId,
    new_parent: Option<SceneNodeId>,
) -> Result<(), String> {
    if scene.node(id).is_none() {
        return Err(format!("node {id} does not exist"));
    }
    if let Some(parent) = new_parent {
        if parent == id {
            return Err("a node cannot be its own parent".to_owned());
        }
        if scene.node(parent).is_none() {
            return Err(format!("parent {parent} does not exist"));
        }
        if scene.subtree(id).contains(&parent) {
            return Err("that would put the node inside itself".to_owned());
        }
    }
    let Some(mut node) = scene.node(id).cloned() else {
        return Err(format!("node {id} does not exist"));
    };
    if node.parent == new_parent {
        return Ok(());
    }
    node.parent = new_parent;
    scene.replace_node(node);
    Ok(())
}

/// `true` when `candidate` may become `id`'s parent. The hierarchy panel greys out rows
/// with this, so the user is not offered a drop that would be refused.
pub fn can_parent(scene: &Scene, id: SceneNodeId, candidate: SceneNodeId) -> bool {
    candidate != id && !scene.subtree(id).contains(&candidate) && scene.node(candidate).is_some()
}

/// A node's depth from the root, for indenting a list or for a status line.
pub fn depth(scene: &Scene, id: SceneNodeId) -> usize {
    let mut depth = 0;
    let mut current = scene.node(id).and_then(|node| node.parent);
    while let Some(parent) = current {
        depth += 1;
        if depth > scene.nodes.len() {
            break;
        }
        current = scene.node(parent).and_then(|node| node.parent);
    }
    depth
}

/// A transform with a unit scale and no rotation, for a caller that only wants a position.
pub fn at(position: [f32; 3]) -> NodeTransform {
    NodeTransform {
        position,
        ..NodeTransform::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene() -> Scene {
        let mut scene = Scene::new("edit");
        let mut root = SceneNode::new(SceneNodeId::new(0), "Root");
        root.mesh = Some(MeshSource::Primitive(Primitive::Cube { size: 1.0 }));
        let root_id = scene.add_node(root).id;
        let mut child = SceneNode::new(SceneNodeId::new(0), "Child");
        child.parent = Some(root_id);
        child.mesh = Some(MeshSource::Primitive(Primitive::Sphere {
            radius: 0.5,
            rings: 8,
            segments: 8,
        }));
        scene.add_node(child);
        let child_id = scene.nodes[1].id;
        let mut grandchild = SceneNode::new(SceneNodeId::new(0), "Grandchild");
        grandchild.parent = Some(child_id);
        scene.add_node(grandchild);
        scene
    }

    #[test]
    fn a_new_node_gets_a_unique_name() {
        let mut scene = scene();
        let first = create(&mut scene, NewNodeKind::Cube, None, Some([0.0; 3])).expect("created");
        assert_eq!(first.name, "Cube");
        let second = create(&mut scene, NewNodeKind::Cube, None, Some([0.0; 3])).expect("created");
        assert_eq!(second.name, "Cube 2");
        assert_ne!(first.id, second.id);
        assert_eq!(scene.nodes.len(), 5);
    }

    #[test]
    fn each_kind_creates_the_thing_it_names() {
        let mut scene = Scene::new("kinds");
        for kind in NEW_NODE_KINDS {
            let node = create(&mut scene, kind, None, Some([1.0, 2.0, 3.0])).expect("created");
            assert_eq!(node.transform.position, [1.0, 2.0, 3.0]);
            match kind {
                NewNodeKind::Cube | NewNodeKind::Sphere | NewNodeKind::Plane => {
                    assert!(node.is_mesh(), "{kind:?} should be a mesh");
                }
                NewNodeKind::Group => {
                    assert!(!node.is_mesh() && !node.is_light() && !node.is_camera());
                    assert_eq!(node.kind(), "group");
                }
                NewNodeKind::Light => assert!(node.is_light()),
                NewNodeKind::Camera => assert!(node.is_camera()),
            }
        }
    }

    #[test]
    fn a_mesh_is_created_on_the_floor_not_buried_in_it() {
        let mut scene = Scene::new("floor");
        let cube = create(&mut scene, NewNodeKind::Cube, None, None).expect("created");
        let (min, _) = cube.transform.aabb();
        assert!(min[1] >= -1e-6, "the cube's bottom sits on y = 0: {min:?}");
    }

    #[test]
    fn creating_under_a_missing_parent_is_refused() {
        let mut scene = scene();
        let error = create(
            &mut scene,
            NewNodeKind::Cube,
            Some(SceneNodeId(99)),
            Some([0.0; 3]),
        );
        assert!(error.is_err());
    }

    #[test]
    fn creating_under_a_parent_links_to_it() {
        let mut scene = scene();
        let child = create(
            &mut scene,
            NewNodeKind::Group,
            Some(SceneNodeId(1)),
            Some([0.0; 3]),
        )
        .expect("created");
        assert_eq!(child.parent, Some(SceneNodeId(1)));
        assert_eq!(scene.subtree(SceneNodeId(1)).len(), 4);
    }

    #[test]
    fn duplicating_copies_the_whole_subtree_with_new_ids() {
        let mut scene = scene();
        let before = scene.nodes.len();
        let copies = duplicate(&mut scene, SceneNodeId(1), [1.0, 0.0, 0.0]);
        assert_eq!(copies.len(), 3, "the node, its child and its grandchild");
        assert_eq!(scene.nodes.len(), before + 3);

        // Every id is new and every name is unique.
        for (index, copy) in copies.iter().enumerate() {
            assert!(scene.node(*copy).is_some(), "copy {index} is in the scene");
        }
        let mut names: Vec<&str> = scene.nodes.iter().map(|node| node.name.as_str()).collect();
        let count = names.len();
        names.dedup();
        names.sort_unstable();
        names.dedup();
        let unique = names.len();
        assert_eq!(unique, count, "all {count} names are distinct");

        // The structure came along: the copies keep their internal parenting and the root
        // copy is a sibling of the original.
        let root_copy = copies[0];
        let child_copy = copies[1];
        let grandchild_copy = copies[2];
        assert_eq!(scene.node(root_copy).expect("root copy").parent, None);
        assert_eq!(
            scene.node(child_copy).expect("child copy").parent,
            Some(root_copy)
        );
        assert_eq!(
            scene.node(grandchild_copy).expect("grandchild copy").parent,
            Some(child_copy)
        );

        // And it is offset, not on top of the original.
        let original = scene.node(SceneNodeId(1)).expect("original");
        let copy = scene.node(root_copy).expect("root copy");
        assert_eq!(
            copy.transform.position[0],
            original.transform.position[0] + 1.0
        );
        assert_eq!(copy.transform.position[1], original.transform.position[1]);
    }

    #[test]
    fn duplicating_a_missing_node_does_nothing() {
        let mut scene = scene();
        let before = scene.nodes.len();
        assert!(duplicate(&mut scene, SceneNodeId(99), [1.0, 0.0, 0.0]).is_empty());
        assert_eq!(scene.nodes.len(), before);
    }

    #[test]
    fn duplicating_a_child_keeps_it_under_the_same_parent() {
        let mut scene = scene();
        let copies = duplicate(&mut scene, SceneNodeId(2), [0.0, 1.0, 0.0]);
        assert!(!copies.is_empty());
        assert_eq!(
            scene.node(copies[0]).expect("copy").parent,
            Some(SceneNodeId(1))
        );
    }

    #[test]
    fn deleting_removes_the_subtree_and_counts_it_once() {
        let mut scene = scene();
        // Asking for the parent and the child at once must not count the child twice.
        let removed = delete(&mut scene, &[SceneNodeId(1), SceneNodeId(2)]);
        assert_eq!(removed, 3);
        assert!(scene.nodes.is_empty());
    }

    #[test]
    fn deleting_a_missing_node_is_harmless() {
        let mut scene = scene();
        let before = scene.nodes.len();
        assert_eq!(delete(&mut scene, &[SceneNodeId(99)]), 0);
        assert_eq!(scene.nodes.len(), before);
    }

    #[test]
    fn reparenting_moves_the_node_and_keeps_its_numbers() {
        let mut scene = scene();
        let before = scene.node(SceneNodeId(3)).expect("grandchild").transform;
        reparent(&mut scene, SceneNodeId(3), Some(SceneNodeId(1))).expect("reparented");
        let node = scene.node(SceneNodeId(3)).expect("grandchild");
        assert_eq!(node.parent, Some(SceneNodeId(1)));
        assert_eq!(node.transform, before, "re-parenting is not a move");
        // …and back to the root.
        reparent(&mut scene, SceneNodeId(3), None).expect("reparented");
        assert_eq!(scene.node(SceneNodeId(3)).expect("grandchild").parent, None);
    }

    #[test]
    fn reparenting_into_yourself_is_refused() {
        let mut scene = scene();
        assert!(reparent(&mut scene, SceneNodeId(1), Some(SceneNodeId(1))).is_err());
        assert!(reparent(&mut scene, SceneNodeId(1), Some(SceneNodeId(2))).is_err());
        // Nothing was changed by the refusals.
        assert_eq!(scene.node(SceneNodeId(1)).expect("root").parent, None);
        assert_eq!(
            scene.node(SceneNodeId(2)).expect("child").parent,
            Some(SceneNodeId(1))
        );
    }

    #[test]
    fn reparenting_under_a_missing_parent_is_refused() {
        let mut scene = scene();
        assert!(reparent(&mut scene, SceneNodeId(2), Some(SceneNodeId(99))).is_err());
        assert!(reparent(&mut scene, SceneNodeId(99), None).is_err());
    }

    #[test]
    fn a_node_can_be_dropped_where_a_cycle_would_not_appear() {
        let scene = scene();
        // A leaf may be dropped onto anything above it.
        assert!(can_parent(&scene, SceneNodeId(3), SceneNodeId(2)));
        assert!(can_parent(&scene, SceneNodeId(3), SceneNodeId(1)));
        // A node may not be dropped onto itself or onto its own descendants.
        assert!(!can_parent(&scene, SceneNodeId(1), SceneNodeId(1)));
        assert!(!can_parent(&scene, SceneNodeId(1), SceneNodeId(2)));
        assert!(!can_parent(&scene, SceneNodeId(1), SceneNodeId(3)));
        // …and not onto something that is not there.
        assert!(!can_parent(&scene, SceneNodeId(1), SceneNodeId(99)));
    }

    #[test]
    fn depth_counts_the_way_the_tree_indents() {
        let scene = scene();
        assert_eq!(depth(&scene, SceneNodeId(1)), 0);
        assert_eq!(depth(&scene, SceneNodeId(2)), 1);
        assert_eq!(depth(&scene, SceneNodeId(3)), 2);
    }
}
