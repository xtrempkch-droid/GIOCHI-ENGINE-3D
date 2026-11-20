//! Turning an editor scene into a circuit.
//!
//! The scene format belongs to the engine and the editor, so the game does not add a
//! component to it (R2). What it does instead is read the **names** the author already
//! types in the hierarchy panel:
//!
//! | Node name | Meaning |
//! |---|---|
//! | `Start` (or `StartPosition`, `Spawn`) | where the car is placed, and the finish line |
//! | `Checkpoint 1` … `Checkpoint 9` | the gates of a lap, taken in numeric order |
//! | anything with a mesh | drawn, and solid if it is a primitive |
//! | anything with a light | the track's sun (the first one wins) |
//!
//! Names are matched case-insensitively and ignoring spaces, `_` and `-`, so
//! `checkpoint_2`, `Checkpoint 2` and `CHECKPOINT2` are the same gate. A checkpoint
//! without a number is numbered in the order it appears in the file.
//!
//! The whole conversion is pure: no GL, no window, no physics world. That is what lets
//! `tests/track_specs.rs` assert the shipped tracks against the plan's dimensions on a
//! machine with no display.

use std::path::Path;

use giochi_engine_3d::math3d::{Transform3D, Vec3};
use giochi_engine_3d::physics::ColliderShape;
use giochi_engine_3d::scene::{Primitive, Scene, SceneError, SceneNode};

use super::{
    primitive_half_extents, yaw_from, Checkpoint, DrawShape, StartPosition, StaticCollider, Track,
    TrackInstance,
};

/// Gate radius when the node's `scale.x` is `1`, in metres.
///
/// A 10 m road with a 12 m disc is forgiving in both directions without letting a car
/// cut a corner and still score the gate.
pub const DEFAULT_GATE_RADIUS: f32 = 12.0;

/// Thickness given to a `Plane` collider, in metres.
///
/// The mesh is a surface with no thickness; the collider needs some, or the car falls
/// through the world. Half of it is below the surface, so the top face is exactly where
/// the plane is drawn.
pub const PLANE_THICKNESS: f32 = 1.0;

/// Why a scene could not be made into a track.
#[derive(Debug, thiserror::Error)]
pub enum TrackError {
    /// The file is not a readable scene.
    #[error(transparent)]
    Scene(#[from] SceneError),
    /// A scene the game can draw but cannot race: no start, or no geometry.
    #[error("track {path} is not a race track: {reason}")]
    NotARaceTrack {
        /// The file involved.
        path: String,
        /// What is missing.
        reason: String,
    },
}

/// Loads `path` as a circuit.
///
/// `path` is resolved as given; only meshes named inside the scene are resolved against
/// `asset_root`, because a scene's mesh paths are relative to the project that owns it.
pub fn load_track(path: impl AsRef<Path>) -> Result<Track, TrackError> {
    let path = path.as_ref();
    let scene = Scene::load_from_path(path)?;
    parse_track(&scene, path)
}

/// Turns an already-parsed scene into a circuit.
///
/// `path` is only used in error messages; pass the file it came from.
pub fn parse_track(scene: &Scene, path: impl AsRef<Path>) -> Result<Track, TrackError> {
    let path = path.as_ref();
    let mut instances = Vec::new();
    let mut colliders = Vec::new();
    let mut start = None;
    let mut numbered: Vec<(u32, Vec3, f32)> = Vec::new();
    let mut waypoints: Vec<(u32, Vec3, f32)> = Vec::new();
    let mut unnumbered_waypoints = 0;
    let mut unnumbered = 0u32;
    let mut light = None;

    for node in &scene.nodes {
        let world = world_transform(scene, node);
        let marker = MarkerKind::of(&node.name);

        if light.is_none() {
            light = node.light.map(|desc| desc.to_light());
        }

        match marker {
            MarkerKind::Start => {
                start = Some(StartPosition {
                    position: world.position,
                    yaw: yaw_from(world.rotation),
                });
            }
            MarkerKind::Checkpoint { number } => {
                let number = number.unwrap_or_else(|| {
                    unnumbered += 1;
                    unnumbered
                });
                numbered.push((number, world.position, gate_radius(world)));
            }
            MarkerKind::Waypoint { number } => {
                let number = number.unwrap_or_else(|| {
                    unnumbered_waypoints += 1;
                    unnumbered_waypoints
                });
                waypoints.push((number, world.position, waypoint_speed(world)));
            }
            MarkerKind::None => {}
        }

        // A marker is a point in space, not scenery: drawing a marker cube would put
        // geometry on the racing line.
        if marker != MarkerKind::None {
            continue;
        }
        let Some(source) = &node.mesh else {
            continue;
        };
        let shape = match source {
            giochi_engine_3d::scene::MeshSource::Primitive(primitive) => {
                DrawShape::Primitive(*primitive)
            }
            giochi_engine_3d::scene::MeshSource::Obj(path) => DrawShape::Obj(path.clone()),
            giochi_engine_3d::scene::MeshSource::Gltf(path) => DrawShape::Gltf(path.clone()),
        };
        if let Some(collider) = collider_for(&shape, &world) {
            colliders.push(collider);
        }
        instances.push(TrackInstance {
            shape,
            transform: world,
            material: node.material.clone(),
        });
    }

    let Some(start) = start else {
        return Err(TrackError::NotARaceTrack {
            path: path.display().to_string(),
            reason: "there is no node named 'Start' to place the car on".to_owned(),
        });
    };
    if instances.is_empty() {
        return Err(TrackError::NotARaceTrack {
            path: path.display().to_string(),
            reason: "the scene has no geometry to drive on".to_owned(),
        });
    }

    // Gate 0 is the start/finish line, which is the `Start` node itself. The rest follow
    // the numbers the author gave them, not their position in the file.
    numbered.sort_by_key(|(number, _, _)| *number);
    let mut entries = Vec::with_capacity(numbered.len() + 1);
    entries.push((start.position, DEFAULT_GATE_RADIUS));
    entries.extend(
        numbered
            .iter()
            .map(|(_, position, radius)| (*position, *radius)),
    );
    let start_forward = Vec3::new(start.yaw.sin(), 0.0, start.yaw.cos());

    // Waypoints are ordered by their number, like checkpoints, and the number is the only
    // thing the author has to get right: the racing line is the sequence, not a shape.
    waypoints.sort_by_key(|(number, _, _)| *number);
    let waypoints: Vec<super::Waypoint> = waypoints
        .into_iter()
        .enumerate()
        .map(|(index, (_, position, speed))| super::Waypoint {
            index,
            position,
            speed,
        })
        .collect();

    Ok(Track {
        name: scene.name.clone(),
        start,
        checkpoints: build_gates(&entries, start_forward),
        instances,
        colliders,
        light,
        waypoints,
    })
}

/// Builds the ordered gates, deriving each one's direction from the previous gate.
///
/// The direction is the **arrival** direction: how the car is travelling when it reaches
/// the gate. Deriving it from the neighbours means the author only has to place the gate
/// centres, which is the part they can see on screen.
fn build_gates(entries: &[(Vec3, f32)], start_forward: Vec3) -> Vec<Checkpoint> {
    let count = entries.len();
    (0..count)
        .map(|index| {
            let (position, radius) = entries[index];
            let direction = if count == 1 {
                start_forward
            } else {
                let previous = entries[(index + count - 1) % count].0;
                let delta = position.to_glam() - previous.to_glam();
                let horizontal = glam::Vec3::new(delta.x, 0.0, delta.z);
                if horizontal.length_squared() > 1e-6 {
                    Vec3::from_glam(horizontal.normalize())
                } else {
                    start_forward
                }
            };
            Checkpoint {
                index,
                position,
                radius,
                direction,
            }
        })
        .collect()
}

/// Composes a node's transform with its ancestors'.
fn world_transform(scene: &Scene, node: &SceneNode) -> Transform3D {
    let local = node.transform.to_transform3d();
    let mut world = local;
    let mut parent = node.parent;
    // A cycle would loop forever; `Scene::validate` refuses one, and the bound is a
    // second guarantee for a scene that was not validated.
    let mut hops = 0;
    while let Some(id) = parent {
        let Some(ancestor) = scene.node(id) else {
            break;
        };
        world = crate::utils::compose(&ancestor.transform.to_transform3d(), &world);
        parent = ancestor.parent;
        hops += 1;
        if hops > 64 {
            break;
        }
    }
    world
}

/// The waypoint's target speed: `scale.x` in km/h, converted to m/s.
///
/// `scale.x <= 1` means "not authored" and gets [`DEFAULT_WAYPOINT_SPEED_KMH`], for the
/// same reason the gate radius has a default: a node dropped in the editor has scale 1.
fn waypoint_speed(world: Transform3D) -> f32 {
    let scale = world.scale.to_glam().x.abs();
    let kmh = if scale > 1.0 {
        scale
    } else {
        DEFAULT_WAYPOINT_SPEED_KMH
    };
    kmh / 3.6
}

/// The speed a waypoint gets when its `scale.x` was left alone, in km/h.
pub const DEFAULT_WAYPOINT_SPEED_KMH: f32 = 80.0;

/// The gate's radius: `scale.x` when the author changed it, [`DEFAULT_GATE_RADIUS`] when not.
fn gate_radius(world: Transform3D) -> f32 {
    let scale = world.scale.to_glam().x.abs();
    if (scale - 1.0).abs() > 1e-3 {
        scale
    } else {
        DEFAULT_GATE_RADIUS
    }
}

/// A static collider matching `shape` at `world`, or `None` for scenery that should not
/// be solid (an imported model).
fn collider_for(shape: &DrawShape, world: &Transform3D) -> Option<StaticCollider> {
    let scale = world.scale.to_glam().abs();
    let (half_x, half_y, half_z) = primitive_half_extents(shape);
    let collider = match shape {
        DrawShape::Primitive(Primitive::Cube { .. }) => StaticCollider {
            shape: ColliderShape::cuboid(
                (half_x * scale.x).max(1e-3),
                (half_y * scale.y).max(1e-3),
                (half_z * scale.z).max(1e-3),
            ),
            position: world.position,
            rotation: world.rotation,
        },
        DrawShape::Primitive(Primitive::Sphere { .. }) => StaticCollider {
            shape: ColliderShape::ball((half_x * scale.max_element()).max(1e-3)),
            position: world.position,
            rotation: world.rotation,
        },
        DrawShape::Primitive(Primitive::Plane { .. }) => {
            let half_thickness = PLANE_THICKNESS * 0.5;
            StaticCollider {
                // The slab hangs below the surface, so the top face is the drawn plane.
                shape: ColliderShape::cuboid(
                    (half_x * scale.x).max(1e-3),
                    half_thickness,
                    (half_z * scale.z).max(1e-3),
                ),
                position: Vec3::from_glam(
                    world.position.to_glam() - glam::Vec3::new(0.0, half_thickness, 0.0),
                ),
                rotation: world.rotation,
            }
        }
        DrawShape::Obj(_) | DrawShape::Gltf(_) => return None,
    };
    Some(collider)
}

/// What a node's name says it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarkerKind {
    None,
    Start,
    Checkpoint {
        number: Option<u32>,
    },
    /// A point on the racing line. `scale.x` is the target speed in km/h.
    Waypoint {
        number: Option<u32>,
    },
}

impl MarkerKind {
    fn of(name: &str) -> Self {
        let key: String = name
            .chars()
            .filter(|character| !matches!(character, ' ' | '_' | '-'))
            .flat_map(char::to_lowercase)
            .collect();
        for (prefix, waypoint) in [("checkpoint", false), ("waypoint", true)] {
            if let Some(rest) = key.strip_prefix(prefix) {
                let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
                let number = digits.parse().ok();
                return if waypoint {
                    Self::Waypoint { number }
                } else {
                    Self::Checkpoint { number }
                };
            }
        }
        match key.as_str() {
            "start" | "startposition" | "startfinish" | "spawn" | "spawnpoint" => Self::Start,
            _ => Self::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use giochi_engine_3d::scene::{MeshSource, SceneNodeId};

    fn scene_with(nodes: Vec<SceneNode>) -> Scene {
        let mut scene = Scene::new("test track");
        for node in nodes {
            scene.add_node(node);
        }
        scene
    }

    fn node(name: &str, position: [f32; 3]) -> SceneNode {
        let mut node = SceneNode::new(SceneNodeId(0), name);
        node.transform.position = position;
        node
    }

    fn cube_node(name: &str, position: [f32; 3], size: f32, scale: [f32; 3]) -> SceneNode {
        let mut node = node(name, position);
        node.mesh = Some(MeshSource::Primitive(Primitive::Cube { size }));
        node.transform.scale = scale;
        node
    }

    fn start_node() -> SceneNode {
        let mut node = node("Start", [0.0, 0.5, 0.0]);
        node.transform.rotation = [
            0.0,
            (std::f32::consts::FRAC_PI_4).sin(),
            0.0,
            (std::f32::consts::FRAC_PI_4).cos(),
        ];
        node
    }

    #[test]
    fn a_start_and_a_floor_are_the_minimum_raceable_track() {
        let scene = scene_with(vec![
            start_node(),
            cube_node("Floor", [0.0, -1.0, 0.0], 1.0, [50.0, 1.0, 50.0]),
        ]);
        let track = parse_track(&scene, "memory").expect("a floor and a start are enough");
        assert_eq!(track.name, "test track");
        assert_eq!(track.start.position.to_array(), [0.0, 0.5, 0.0]);
        assert!((track.start.yaw - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
        assert_eq!(track.instances.len(), 1);
        assert_eq!(track.colliders.len(), 1);
        assert_eq!(track.gate_count(), 1, "the start line is the only gate");
    }

    #[test]
    fn a_scene_without_a_start_is_refused_by_name() {
        let scene = scene_with(vec![cube_node(
            "Floor",
            [0.0, 0.0, 0.0],
            1.0,
            [10.0, 1.0, 10.0],
        )]);
        let error = parse_track(&scene, "no_start.scene").expect_err("no start");
        assert!(error.to_string().contains("no_start.scene"));
        assert!(error.to_string().contains("Start"), "{error}");
    }

    #[test]
    fn a_scene_with_only_markers_is_refused_as_unraceable() {
        let scene = scene_with(vec![start_node(), node("Checkpoint 1", [10.0, 0.0, 0.0])]);
        let error = parse_track(&scene, "empty.scene").expect_err("no geometry");
        assert!(error.to_string().contains("no geometry"), "{error}");
    }

    #[test]
    fn checkpoints_are_ordered_by_their_number_not_by_file_order() {
        let scene = scene_with(vec![
            start_node(),
            cube_node("Floor", [0.0, -1.0, 0.0], 1.0, [50.0, 1.0, 50.0]),
            node("Checkpoint 3", [30.0, 0.0, 0.0]),
            node("Checkpoint 1", [10.0, 0.0, 0.0]),
            node("Checkpoint 2", [20.0, 0.0, 0.0]),
        ]);
        let track = parse_track(&scene, "memory").expect("a track");
        let positions: Vec<[f32; 3]> = track
            .checkpoints
            .iter()
            .map(|gate| gate.position.to_array())
            .collect();
        assert_eq!(
            positions,
            vec![
                [0.0, 0.5, 0.0],
                [10.0, 0.0, 0.0],
                [20.0, 0.0, 0.0],
                [30.0, 0.0, 0.0]
            ]
        );
        for (index, gate) in track.checkpoints.iter().enumerate() {
            assert_eq!(gate.index, index);
        }
    }

    #[test]
    fn an_unnumbered_checkpoint_is_numbered_in_file_order() {
        let scene = scene_with(vec![
            start_node(),
            cube_node("Floor", [0.0, -1.0, 0.0], 1.0, [50.0, 1.0, 50.0]),
            node("Checkpoint", [10.0, 0.0, 0.0]),
            node("checkpoint", [20.0, 0.0, 0.0]),
        ]);
        let track = parse_track(&scene, "memory").expect("a track");
        assert_eq!(track.checkpoints[1].position.to_array(), [10.0, 0.0, 0.0]);
        assert_eq!(track.checkpoints[2].position.to_array(), [20.0, 0.0, 0.0]);
    }

    #[test]
    fn a_gate_radius_comes_from_scale_when_the_author_set_one() {
        let mut gate = node("Checkpoint 1", [10.0, 0.0, 0.0]);
        gate.transform.scale = [18.0, 1.0, 1.0];
        let scene = scene_with(vec![
            start_node(),
            cube_node("Floor", [0.0, -1.0, 0.0], 1.0, [50.0, 1.0, 50.0]),
            gate,
        ]);
        let track = parse_track(&scene, "memory").expect("a track");
        assert_eq!(track.checkpoints[1].radius, 18.0);
        assert_eq!(track.checkpoints[0].radius, DEFAULT_GATE_RADIUS);
    }

    #[test]
    fn markers_are_not_drawn_and_not_solid() {
        let scene = scene_with(vec![
            start_node(),
            cube_node("Floor", [0.0, -1.0, 0.0], 1.0, [50.0, 1.0, 50.0]),
            cube_node("Checkpoint 1", [10.0, 0.0, 0.0], 2.0, [1.0, 1.0, 1.0]),
        ]);
        let track = parse_track(&scene, "memory").expect("a track");
        assert_eq!(track.instances.len(), 1, "only the floor is scenery");
        assert_eq!(track.colliders.len(), 1);
    }

    #[test]
    fn a_cube_collider_matches_the_cube_times_its_scale() {
        let scene = scene_with(vec![
            start_node(),
            cube_node("Wall", [0.0, 1.0, 0.0], 2.0, [1.0, 3.0, 0.5]),
        ]);
        let track = parse_track(&scene, "memory").expect("a track");
        match track.colliders[0].shape {
            ColliderShape::Cuboid {
                half_width,
                half_height,
                half_depth,
            } => {
                assert!((half_width - 1.0).abs() < 1e-6);
                assert!((half_height - 3.0).abs() < 1e-6);
                assert!((half_depth - 0.5).abs() < 1e-6);
            }
            other => panic!("expected a cuboid, got {other:?}"),
        }
    }

    #[test]
    fn a_plane_collider_hangs_below_its_surface() {
        let mut plane = node("Ground", [0.0, 0.0, 0.0]);
        plane.mesh = Some(MeshSource::Primitive(Primitive::Plane {
            size: 100.0,
            subdivisions: 1,
        }));
        let scene = scene_with(vec![start_node(), plane]);
        let track = parse_track(&scene, "memory").expect("a track");
        let collider = track.colliders[0];
        match collider.shape {
            ColliderShape::Cuboid {
                half_width,
                half_height,
                half_depth,
            } => {
                assert!((half_width - 50.0).abs() < 1e-6);
                assert!((half_height - PLANE_THICKNESS * 0.5).abs() < 1e-6);
                assert!((half_depth - 50.0).abs() < 1e-6);
            }
            other => panic!("expected a cuboid, got {other:?}"),
        }
        // Top face at y = 0, exactly where the plane is drawn.
        let top = collider.position.to_glam().y
            + match collider.shape {
                ColliderShape::Cuboid { half_height, .. } => half_height,
                _ => 0.0,
            };
        assert!(top.abs() < 1e-6, "top face at y={top}");
    }

    #[test]
    fn a_child_node_inherits_its_parent_transform() {
        // Ids are explicit so the parent link can be written before either node exists.
        let mut scene = Scene::new("hierarchy");
        let mut parent = node("Group", [10.0, 0.0, 0.0]);
        parent.id = SceneNodeId(1);
        scene.insert_node(parent).expect("the group");
        let mut child = cube_node("Wall", [0.0, 1.0, 0.0], 1.0, [1.0, 1.0, 1.0]);
        child.id = SceneNodeId(2);
        child.parent = Some(SceneNodeId(1));
        scene.insert_node(child).expect("the wall");
        let mut start = start_node();
        start.id = SceneNodeId(3);
        scene.insert_node(start).expect("the start");

        let track = parse_track(&scene, "memory").expect("a track");
        let wall = track
            .instances
            .iter()
            .find(|instance| instance.transform.position.to_array()[0] != 0.0)
            .expect("the wall");
        assert_eq!(wall.transform.position.to_array(), [10.0, 1.0, 0.0]);
    }

    #[test]
    fn an_imported_model_is_drawn_but_not_solid() {
        let mut model = node("Car", [0.0, 0.0, 0.0]);
        model.mesh = Some(MeshSource::Gltf("cars/car.gltf".to_owned()));
        let scene = scene_with(vec![
            start_node(),
            cube_node("Floor", [0.0, -1.0, 0.0], 1.0, [50.0, 1.0, 50.0]),
            model,
        ]);
        let track = parse_track(&scene, "memory").expect("a track");
        assert_eq!(track.instances.len(), 2);
        assert_eq!(track.colliders.len(), 1, "only the floor is solid");
    }

    #[test]
    fn the_first_light_becomes_the_suns() {
        let mut sun = node("Sun", [0.0, 10.0, 0.0]);
        sun.light = Some(giochi_engine_3d::scene::LightDesc::default());
        let scene = scene_with(vec![
            start_node(),
            cube_node("Floor", [0.0, -1.0, 0.0], 1.0, [50.0, 1.0, 50.0]),
            sun,
        ]);
        let track = parse_track(&scene, "memory").expect("a track");
        assert!(track.light.is_some());
    }

    #[test]
    fn marker_names_are_matched_leniently() {
        assert_eq!(MarkerKind::of("Start"), MarkerKind::Start);
        assert_eq!(MarkerKind::of("start_position"), MarkerKind::Start);
        assert_eq!(MarkerKind::of("Spawn Point"), MarkerKind::Start);
        assert_eq!(MarkerKind::of("wall"), MarkerKind::None);
        assert_eq!(
            MarkerKind::of("checkpoint_7"),
            MarkerKind::Checkpoint { number: Some(7) }
        );
        assert_eq!(
            MarkerKind::of("CHECKPOINT 2"),
            MarkerKind::Checkpoint { number: Some(2) }
        );
        assert_eq!(
            MarkerKind::of("Checkpoint"),
            MarkerKind::Checkpoint { number: None }
        );
        // A name that merely starts with the word is still a checkpoint; one that
        // contains it is not.
        assert_eq!(
            MarkerKind::of("CheckpointSign"),
            MarkerKind::Checkpoint { number: None }
        );
        assert_eq!(MarkerKind::of("Big Checkpoint"), MarkerKind::None);
    }
}
