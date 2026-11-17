//! Unit tests for the scene format: the file, the hierarchy and the invariants.

use super::*;

fn scene_with_hierarchy() -> Scene {
    let mut scene = Scene::new("hierarchy");
    let root = scene.add_node(SceneNode::new(SceneNodeId::new(0), "Root"));
    let mut child = SceneNode::new(SceneNodeId::new(0), "Child");
    child.parent = Some(root.id);
    let child = scene.add_node(child);
    let mut grandchild = SceneNode::new(SceneNodeId::new(0), "Grandchild");
    grandchild.parent = Some(child.id);
    scene.add_node(grandchild);
    scene
}

fn temporary_dir(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("giochi-scene-{name}-{}", std::process::id()))
}

#[test]
fn the_default_scene_has_the_three_objects_the_plan_asks_for() {
    let scene = Scene::default_scene();
    assert_eq!(scene.nodes.len(), 3);
    assert_eq!(scene.nodes.iter().filter(|n| n.is_mesh()).count(), 1);
    assert_eq!(scene.nodes.iter().filter(|n| n.is_light()).count(), 1);
    assert_eq!(scene.nodes.iter().filter(|n| n.is_camera()).count(), 1);
    assert!(scene.validate().expect("valid").is_empty());
    assert_eq!(scene.nodes[0].name, "Cube");
    assert_eq!(scene.nodes[0].transform.position, [0.0, 0.5, 0.0]);
}

#[test]
fn round_trip_through_json_preserves_everything() {
    let scene = Scene::default_scene();
    let json = scene.to_json().expect("serialisable");
    let back = Scene::from_json(&json).expect("deserialisable");
    assert_eq!(scene, back);
    assert_eq!(back.version, SCENE_FORMAT_VERSION);
    assert_eq!(back.format, SCENE_FORMAT);
}

#[test]
fn round_trip_through_a_file_preserves_hierarchy_and_ids() {
    let dir = temporary_dir("roundtrip");
    let path = dir.join("nested").join("test.scene");
    let scene = scene_with_hierarchy();
    scene.save_to_path(&path).expect("saved");
    let back = Scene::load_from_path(&path).expect("loaded");

    assert_eq!(back, scene);
    assert_eq!(back.roots().count(), 1);
    assert_eq!(back.children_of(Some(SceneNodeId(1))).count(), 1);
    let grandchild = back
        .nodes
        .iter()
        .find(|n| n.name == "Grandchild")
        .expect("grandchild");
    assert_eq!(grandchild.parent, Some(SceneNodeId(2)));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_save_leaves_no_temporary_file_behind() {
    let dir = temporary_dir("tmpfile");
    let path = dir.join("scene.scene");
    Scene::default_scene().save_to_path(&path).expect("saved");
    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .expect("dir")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ids_are_never_reused_after_a_delete() {
    let mut scene = Scene::new("ids");
    let a = scene.add_node(SceneNode::new(SceneNodeId::new(0), "A"));
    let b = scene.add_node(SceneNode::new(SceneNodeId::new(0), "B"));
    assert_eq!(b.id, SceneNodeId(a.id.raw() + 1));

    scene.remove_node(b.id);
    let c = scene.add_node(SceneNode::new(SceneNodeId::new(0), "C"));
    assert_ne!(c.id, b.id, "a deleted id must not come back");
    assert!(c.id.raw() > b.id.raw());
}

#[test]
fn removing_a_node_removes_its_whole_subtree() {
    let mut scene = scene_with_hierarchy();
    assert_eq!(scene.subtree(SceneNodeId(1)).len(), 3);
    assert_eq!(scene.remove_node(SceneNodeId(1)), 3);
    assert!(scene.nodes.is_empty());
}

#[test]
fn a_subtree_is_listed_parents_before_children() {
    let scene = scene_with_hierarchy();
    assert_eq!(
        scene.subtree(SceneNodeId(1)),
        vec![SceneNodeId(1), SceneNodeId(2), SceneNodeId(3)]
    );
    assert!(scene.subtree(SceneNodeId(99)).is_empty());
}

#[test]
fn validate_rejects_duplicate_ids_missing_parents_and_self_parenting() {
    let mut scene = Scene::new("bad");
    scene.add_node(SceneNode::new(SceneNodeId(1), "A"));
    assert!(scene
        .insert_node(SceneNode::new(SceneNodeId(1), "B"))
        .is_err());

    scene.add_node(SceneNode::new(SceneNodeId(2), "C"));
    scene.node_mut(SceneNodeId(2)).expect("node").parent = Some(SceneNodeId(9));
    assert!(scene
        .validate()
        .expect_err("missing parent")
        .contains("missing parent"));

    scene.node_mut(SceneNodeId(2)).expect("node").parent = Some(SceneNodeId(2));
    assert!(scene
        .validate()
        .expect_err("self parent")
        .contains("own parent"));
}

#[test]
fn validate_detects_a_cycle_between_two_nodes() {
    let mut scene = Scene::new("cycle");
    scene.nodes.push(SceneNode {
        parent: Some(SceneNodeId(2)),
        ..SceneNode::new(SceneNodeId(1), "A")
    });
    scene.nodes.push(SceneNode {
        parent: Some(SceneNodeId(1)),
        ..SceneNode::new(SceneNodeId(2), "B")
    });
    let error = scene.validate().expect_err("cycle");
    assert!(error.contains("cycle"), "{error}");
}

#[test]
fn validate_warns_about_duplicate_names_without_failing() {
    let mut scene = Scene::new("names");
    scene.add_node(SceneNode::new(SceneNodeId(0), "Same"));
    scene.add_node(SceneNode::new(SceneNodeId(0), "Same"));
    let warnings = scene.validate().expect("names are not fatal");
    assert!(warnings.iter().any(|w| w.contains("duplicate node name")));
}

#[test]
fn a_newer_file_is_refused_instead_of_guessed() {
    let mut scene = Scene::default_scene();
    scene.version = SCENE_FORMAT_VERSION + 1;
    let json = scene.to_json().expect("json");
    match Scene::from_json(&json) {
        Err(SceneError::UnsupportedVersion {
            found, supported, ..
        }) => {
            assert_eq!(found, SCENE_FORMAT_VERSION + 1);
            assert_eq!(supported, SCENE_FORMAT_VERSION);
        }
        other => panic!("expected UnsupportedVersion, got {other:?}"),
    }
}

#[test]
fn a_foreign_json_file_is_named_as_such() {
    let error = Scene::from_json(r#"{"format": "something-else"}"#).expect_err("not a scene");
    assert!(matches!(error, SceneError::NotAScene { .. }));
}

#[test]
fn a_minimal_hand_written_node_loads() {
    let json = r#"{
        "name": "minimal",
        "nodes": [
            { "id": 7, "name": "Cube", "mesh": { "primitive": { "kind": "cube" } } }
        ]
    }"#;
    let scene = Scene::from_json(json).expect("defaults fill the rest");
    assert_eq!(scene.version, SCENE_FORMAT_VERSION);
    assert_eq!(scene.nodes[0].id, SceneNodeId(7));
    assert_eq!(
        scene.nodes[0].mesh,
        Some(MeshSource::Primitive(Primitive::Cube { size: 1.0 }))
    );
    assert_eq!(scene.nodes[0].material.base_color, [0.8, 0.8, 0.8, 1.0]);
    assert_eq!(scene.next_id, 1, "next_id keeps its default when omitted");
}

#[test]
fn a_three_component_light_colour_is_accepted() {
    let json = r#"{
        "nodes": [
            { "id": 1, "name": "Sun",
              "light": { "direction": [0, -2, 0], "color": [1.0, 0.5, 0.0] } }
        ]
    }"#;
    let scene = Scene::from_json(json).expect("loads");
    let light = scene.first_light().expect("light");
    assert_eq!(light.color, [1.0, 0.5, 0.0, 1.0]);
    let engine = light.to_light();
    assert!((engine.direction.y + 1.0).abs() < 1e-6, "normalised");
    assert_eq!(engine.shininess, 48.0);
}

#[test]
fn an_unknown_field_is_an_error_not_silently_dropped() {
    let json = r#"{
        "nodes": [ { "id": 1, "name": "A", "colour": "red" } ]
    }"#;
    assert!(matches!(
        Scene::from_json(json),
        Err(SceneError::Json { .. })
    ));
}

#[test]
fn mesh_bounds_cover_every_mesh_node() {
    let mut scene = Scene::new("bounds");
    let mut left = scene.add_node(SceneNode::new(SceneNodeId::new(0), "Left"));
    left.transform.position = [-3.0, 0.0, 0.0];
    left.mesh = Some(MeshSource::Primitive(Primitive::Cube { size: 1.0 }));
    scene.replace_node(left);
    let mut right = scene.add_node(SceneNode::new(SceneNodeId::new(0), "Right"));
    right.transform.position = [3.0, 2.0, 0.0];
    right.mesh = Some(MeshSource::Primitive(Primitive::Cube { size: 1.0 }));
    scene.replace_node(right);

    let (min, max) = scene.mesh_bounds().expect("two meshes");
    assert!((min[0] + 3.5).abs() < 1e-5, "{min:?}");
    assert!((max[0] - 3.5).abs() < 1e-5, "{max:?}");
    assert!((max[1] - 2.5).abs() < 1e-5, "{max:?}");
    assert!(Scene::new("empty").mesh_bounds().is_none());
}

#[test]
fn a_scaled_node_grows_its_bounds() {
    let mut scene = Scene::new("scale");
    let mut node = scene.add_node(SceneNode::new(SceneNodeId::new(0), "Big"));
    node.mesh = Some(MeshSource::Primitive(Primitive::Cube { size: 1.0 }));
    node.transform.scale = [4.0, 2.0, 1.0];
    scene.replace_node(node);

    let (min, max) = scene.mesh_bounds().expect("one mesh");
    assert!(
        (max[0] - min[0] - 4.0).abs() < 1e-5,
        "x extent is the scale"
    );
    assert!(
        (max[1] - min[1] - 2.0).abs() < 1e-5,
        "y extent is the scale"
    );
    assert!(
        (max[2] - min[2] - 1.0).abs() < 1e-5,
        "z extent is the scale"
    );
}

#[test]
fn a_node_kind_names_what_it_is() {
    let mut node = SceneNode::new(SceneNodeId(1), "A");
    assert_eq!(node.kind(), "group");
    node.mesh = Some(MeshSource::Primitive(Primitive::default()));
    assert_eq!(node.kind(), "mesh");
    node.light = Some(LightDesc::default());
    assert_eq!(node.kind(), "light");
    node.camera = Some(CameraDesc::default());
    assert_eq!(node.kind(), "camera");
}

#[test]
fn the_zero_quaternion_becomes_identity_instead_of_nan() {
    let transform = NodeTransform::new([1.0, 2.0, 3.0], [0.0, 0.0, 0.0, 0.0], [1.0; 3]);
    let engine = transform.to_transform3d();
    assert!(engine.rotation.x.is_finite());
    assert!((engine.rotation.w - 1.0).abs() < 1e-6);
}

#[test]
fn a_zero_light_direction_falls_back_to_straight_down() {
    let light = LightDesc {
        direction: [0.0, 0.0, 0.0],
        ..LightDesc::default()
    };
    let engine = light.to_light();
    assert_eq!(
        (engine.direction.x, engine.direction.y, engine.direction.z),
        (0.0, -1.0, 0.0)
    );
}

#[test]
fn a_camera_description_becomes_the_engine_type() {
    let camera = CameraDesc::default().to_camera3d(1.5);
    assert_eq!(camera.aspect, 1.5);
    assert!((camera.fov_y - std::f32::consts::FRAC_PI_3).abs() < 1e-6);
    assert!(camera.near_plane < camera.far_plane);
}

#[test]
fn a_transform_survives_the_round_trip_through_the_engine_type() {
    let transform = crate::math3d::Transform3D::new(
        Vec3::new(1.0, -2.0, 3.5),
        Quat::new(
            0.0,
            std::f32::consts::FRAC_1_SQRT_2,
            0.0,
            std::f32::consts::FRAC_1_SQRT_2,
        ),
        Vec3::new(2.0, 2.0, 2.0),
    );
    let file = NodeTransform::from_transform3d(&transform);
    let back = file.to_transform3d();
    assert!((back.position.x - 1.0).abs() < 1e-6);
    assert!((back.position.y + 2.0).abs() < 1e-6);
    assert!((back.scale.x - 2.0).abs() < 1e-6);
    assert!((back.rotation.w - transform.rotation.w).abs() < 1e-5);
}
