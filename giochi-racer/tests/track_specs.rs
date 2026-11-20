//! Checks the shipped tracks against the plan's specifications.
//!
//! The tracks are `.scene` files, so nothing in the game's code stops them drifting from
//! "200 m oval, 10 m wide, four checkpoints". This file is what stops that: it loads the
//! real files, measures them, and compares against the numbers in
//! `docs/TRACK_SPECS.md`. If a track is re-authored in the editor and becomes a different
//! circuit, this fails and the specification is updated in the same commit — which is the
//! whole point of writing the numbers down twice.

use giochi_engine_3d::math3d::Vec3;
use giochi_engine_3d::scene::Scene;
use giochi_racer::track::lap::{LapEvent, LapTracker};
use giochi_racer::track::loading::{load_track, DEFAULT_GATE_RADIUS};
use giochi_racer::track::Track;

/// The circuits the game ships: file, scene name, checkpoints, and the lap length the
/// specification names. Only the oval's length is measured gate-to-gate here, because its
/// gates sit exactly on the four corners; the two new circuits place gates on the
/// straights, so their length is the one the driven lap reports (`tests/lap_logic.rs`).
const TRACKS: [(&str, &str, usize, f32); 3] = [
    ("assets/tracks/track_easy.scene", "OVAL EASY", 4, 200.0),
    ("assets/tracks/track_medium.scene", "FIGURE EIGHT", 6, 414.0),
    (
        "assets/tracks/track_hard.scene",
        "SERPENTINE HARD",
        8,
        640.0,
    ),
];

fn path(relative: &str) -> String {
    format!("{}/{relative}", env!("CARGO_MANIFEST_DIR"))
}

fn load(relative: &str) -> Track {
    load_track(path(relative)).unwrap_or_else(|error| panic!("{relative}: {error}"))
}

#[test]
fn every_shipped_track_has_the_gates_the_plan_asks_for() {
    for (file, name, checkpoints, _) in TRACKS {
        let track = load(file);
        assert_eq!(track.name, name, "{file}");
        assert_eq!(
            track.checkpoints.len(),
            checkpoints + 1,
            "{file}: the finish line plus {checkpoints} checkpoints"
        );
        assert_eq!(track.checkpoint_count(), checkpoints, "{file}");
        for (index, gate) in track.checkpoints.iter().enumerate() {
            assert_eq!(gate.index, index, "{file}: gate {index} out of order");
            assert!(
                gate.radius >= DEFAULT_GATE_RADIUS,
                "{file}: gate {index} too small"
            );
            let direction = gate.direction.to_glam();
            let length = (direction.x * direction.x + direction.z * direction.z).sqrt();
            assert!(
                (length - 1.0).abs() < 1e-4,
                "{file}: gate {index} has a non-unit direction ({length})"
            );
            assert!(direction.y.abs() < 1e-6, "{file}: gate {index} points up");
        }
    }
}

#[test]
fn the_easy_track_is_a_200_metre_oval() {
    let track = load(TRACKS[0].0);
    // The gates sit at the corners of the centreline, so the sum of the gate-to-gate
    // distances is the length of the lap.
    let gates = &track.checkpoints;
    let mut perimeter = 0.0;
    for index in 0..gates.len() {
        let from = gates[index].position.to_glam();
        let to = gates[(index + 1) % gates.len()].position.to_glam();
        perimeter += ((to.x - from.x).powi(2) + (to.z - from.z).powi(2)).sqrt();
    }
    assert!(
        (perimeter - TRACKS[0].3).abs() < 1.0,
        "the centreline is {perimeter:.1} m, not {}",
        TRACKS[0].3
    );
}

#[test]
fn the_easy_track_is_ten_metres_wide_between_its_barriers() {
    // Measured from the scene itself: the road slabs run between the barrier faces, so
    // the gap between the inner and outer barrier on the south straight is the width a
    // car can use.
    let scene = Scene::load_from_path(path(TRACKS[0].0)).expect("the scene loads");
    let barrier = |name: &str| {
        scene
            .nodes
            .iter()
            .find(|node| node.name == name)
            .unwrap_or_else(|| panic!("no node named {name}"))
            .clone()
    };
    let outer = barrier("Barrier Outer South");
    let inner = barrier("Barrier Inner South");
    // The south straight is at negative z, so the outer barrier's *inner* face is the
    // larger number and the inner barrier's *outer* face is the smaller one.
    let outer_face = outer.transform.position[2] + outer.transform.scale[2] * 0.5;
    let inner_face = inner.transform.position[2] - inner.transform.scale[2] * 0.5;
    let width = inner_face - outer_face;
    assert!((width - 10.0).abs() < 0.01, "the road is {width:.2} m wide");
}

#[test]
fn the_easy_track_is_flat() {
    let scene = Scene::load_from_path(path(TRACKS[0].0)).expect("the scene loads");
    // Everything that is meant to be driven on shares one top surface at y = 0.
    for node in &scene.nodes {
        let is_ground = node.name.starts_with("Road")
            || node.name.starts_with("Infield")
            || node.name.starts_with("Outfield");
        if !is_ground {
            continue;
        }
        let top = node.transform.position[1] + node.transform.scale[1] * 0.5;
        assert!(top.abs() < 1e-4, "{} has its top at y = {top}", node.name);
    }
    // And so does the starting position, which is half a metre above it.
    let track = load(TRACKS[0].0);
    assert!(track.start.position.to_glam().y > 0.3);
    assert!(track.start.position.to_glam().y < 1.0);
}

#[test]
fn the_barriers_are_taller_than_the_road_and_sit_on_it() {
    let scene = Scene::load_from_path(path(TRACKS[0].0)).expect("the scene loads");
    let barriers: Vec<_> = scene
        .nodes
        .iter()
        .filter(|node| node.name.starts_with("Barrier"))
        .collect();
    assert_eq!(barriers.len(), 8, "four outside and four inside");
    for barrier in barriers {
        let bottom = barrier.transform.position[1] - barrier.transform.scale[1] * 0.5;
        let top = barrier.transform.position[1] + barrier.transform.scale[1] * 0.5;
        assert!(
            bottom.abs() < 1e-4,
            "{} floats or sinks: {bottom}",
            barrier.name
        );
        assert!(top > 0.5, "{} is too low to be a wall: {top}", barrier.name);
    }
}

#[test]
fn every_track_starts_facing_along_its_first_straight() {
    // All three circuits open on an eastbound straight, so the car has to face +X
    // (yaw = π/2) or the first thing the player sees is a wall.
    for (file, _, _, _) in TRACKS {
        let track = load(file);
        let forward = Vec3::new(track.start.yaw.sin(), 0.0, track.start.yaw.cos());
        assert!(
            (forward.to_glam().x - 1.0).abs() < 1e-3 && forward.to_glam().z.abs() < 1e-3,
            "{file}: the start faces {forward:?}"
        );
    }
}

#[test]
fn every_track_has_a_sun_and_a_playable_extent() {
    for (file, _, _, _) in TRACKS {
        let track = load(file);
        let light = track.light.unwrap_or_else(|| panic!("{file}: no light"));
        assert!(
            light.direction.to_glam().y < 0.0,
            "{file}: the sun points up"
        );
        let (min, max) = track
            .bounds()
            .unwrap_or_else(|| panic!("{file}: no bounds"));
        let span = max.to_glam() - min.to_glam();
        assert!(
            span.x > 100.0 && span.z > 100.0,
            "{file}: the play area is only {span:?}"
        );
        assert_eq!(
            track.colliders.len(),
            track.instances.len(),
            "{file}: every primitive in these tracks is solid"
        );
    }
}

#[test]
fn the_start_faces_along_the_first_straight() {
    let track = load(TRACKS[0].0);
    // The lap runs +X down the south straight, so the car must face +X (yaw = π/2).
    let forward = Vec3::new(track.start.yaw.sin(), 0.0, track.start.yaw.cos());
    assert!(
        (forward.to_glam().x - 1.0).abs() < 1e-4,
        "the start faces {forward:?}"
    );
    assert!(forward.to_glam().z.abs() < 1e-4);
}

#[test]
fn a_lap_of_the_easy_track_can_be_driven_and_counted() {
    let track = load(TRACKS[0].0);
    let mut tracker = LapTracker::new(track.checkpoints.clone(), 3, track.start.position);
    // Drive the gates in order, arriving along each gate's own direction.
    for _ in 0..3 {
        for (index, gate) in track.checkpoints.iter().enumerate().skip(1) {
            let event = tracker.update(gate.position, gate.direction, 1.0);
            assert_eq!(event, LapEvent::Checkpoint(index), "gate {index}");
        }
        let finish = track.checkpoints[0];
        let event = tracker.update(finish.position, finish.direction, 1.0);
        assert!(
            matches!(event, LapEvent::LapComplete { .. } | LapEvent::RaceFinished),
            "crossing the line gave {event:?}"
        );
    }
    assert!(tracker.is_finished(), "three laps finish the race");
}

#[test]
fn the_track_has_a_sun() {
    let track = load(TRACKS[0].0);
    let light = track.light.expect("the scene has a light");
    assert!(light.direction.to_glam().y < 0.0, "the sun points down");
}

#[test]
fn the_track_has_enough_scenery_to_be_worth_drawing() {
    let track = load(TRACKS[0].0);
    assert!(track.instances.len() >= 7, "{}", track.instances.len());
    assert_eq!(
        track.colliders.len(),
        track.instances.len(),
        "every primitive in this track is solid"
    );
    let (min, max) = track.bounds().expect("bounds");
    // The play area is the barrier rectangle plus the runoff outside it.
    assert!(min.to_glam().x <= -40.0, "{min:?}");
    assert!(max.to_glam().x >= 40.0, "{max:?}");
    assert!(
        max.to_glam().y < 2.0,
        "nothing is taller than the barriers: {max:?}"
    );
}
