//! Acceptance for phase 36 through the editor's own state: build a tree, edit it, save it,
//! reload it.
//!
//! No engine, no window and no egui context is involved, so these are plain `#[test]`
//! functions and each one gets its own thread — the SDL singleton that forces the pixel
//! tests to be one function does not apply here.
//!
//! The last test is the one that matters most: everything the editor can build has to be a
//! scene the format accepts on reload. A duplicate that forgets to relocate a child, or a
//! reparent that leaves a cycle behind, would only surface days later as a file that will
//! not open.

use giochi_editor::editor::node_edit::NewNodeKind;
use giochi_editor::editor::EditorState;
use giochi_editor::editor::EntityId;
use giochi_engine_3d::scene::Scene;

fn state_with_scene() -> EditorState {
    let mut state = EditorState::new();
    state.new_scene();
    state
}

fn temp_path(name: &str) -> std::path::PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "giochi-node-edit-{name}-{}-{:?}.scene",
        std::process::id(),
        std::thread::current().id()
    ));
    path
}

#[test]
fn a_built_tree_survives_a_save_and_a_reload() {
    let mut state = state_with_scene();

    // A group at the root, a sphere inside it, and a cube duplicated.
    state.clear_selection();
    state.create_node(NewNodeKind::Group);
    let group = state.primary().expect("the group is selected");

    state.create_node(NewNodeKind::Sphere);
    let sphere = state.primary().expect("the sphere is selected");
    let scene = state.scene.as_ref().expect("scene");
    assert_eq!(
        scene.node(sphere).expect("sphere").parent,
        Some(group),
        "creating with one node selected nests under it"
    );

    state.select(Some(EntityId(1)));
    state.duplicate_selected();
    let duplicate = state.primary().expect("the copy is selected");
    assert_ne!(duplicate, EntityId(1));

    state.reparent(sphere, None);
    state.select(Some(group));
    state.delete_selected();
    assert!(
        state.scene.as_ref().expect("scene").node(group).is_none(),
        "the group went away"
    );
    assert!(
        state.scene.as_ref().expect("scene").node(sphere).is_some(),
        "…and the sphere, which left it first, did not"
    );

    let path = temp_path("roundtrip");
    state.save_scene_as(&path).expect("saved");

    let reloaded = Scene::load_from_path(&path).expect("reloaded");
    assert_eq!(
        reloaded.nodes.len(),
        state.scene.as_ref().expect("scene").nodes.len()
    );
    assert!(
        reloaded.validate().expect("valid").is_empty(),
        "the file the editor wrote has no warnings"
    );
    for node in &reloaded.nodes {
        assert!(
            node.parent.is_none() || reloaded.node(node.parent.expect("parent")).is_some(),
            "{} has a parent that exists",
            node.name
        );
    }
    let _ = std::fs::remove_file(&path);
}

#[test]
fn ids_are_never_reused_after_a_delete() {
    // The scene format promises stable ids; a delete followed by a create must not hand out
    // the id of the node that went away, or a scene file's references would silently point
    // at a different object.
    let mut state = state_with_scene();
    let doomed = EntityId(2);
    state.select(Some(doomed));
    state.delete_selected();
    state.clear_selection();
    state.create_node(NewNodeKind::Cube);
    let created = state.primary().expect("selected");
    assert_ne!(created, doomed);
    assert!(created.raw() > doomed.raw());
}

#[test]
fn a_duplicate_is_independent_of_its_original() {
    let mut state = state_with_scene();
    state.select(Some(EntityId(1)));
    state.duplicate_selected();
    let copy = state.primary().expect("copy");
    let scene = state.scene.as_mut().expect("scene");
    let mut edited = scene.node(copy).expect("copy").clone();
    edited.transform.position = [9.0, 9.0, 9.0];
    scene.replace_node(edited);
    let original = scene.node(EntityId(1)).expect("original");
    assert_ne!(original.transform.position, [9.0, 9.0, 9.0]);
}

#[test]
fn every_kind_can_be_created_and_deleted_without_leaving_debris() {
    let mut state = state_with_scene();
    let before = state.scene.as_ref().expect("scene").nodes.len();
    for kind in giochi_editor::editor::node_edit::NEW_NODE_KINDS {
        state.clear_selection();
        state.create_node(kind);
    }
    assert_eq!(state.scene.as_ref().expect("scene").nodes.len(), before + 6);
    // Select everything and delete it in one go.
    let ids: Vec<EntityId> = state
        .scene
        .as_ref()
        .expect("scene")
        .nodes
        .iter()
        .map(|node| node.id)
        .collect();
    for id in ids {
        state.select(Some(id));
        state.delete_selected();
    }
    assert!(state.scene.as_ref().expect("scene").nodes.is_empty());
}
