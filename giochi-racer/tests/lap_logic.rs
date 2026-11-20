//! A whole lap, driven by the physics car, on **every** shipped circuit.
//!
//! Phase 42 accepts "3 tracks load and the checkpoints work". Asserting that the files
//! parse is not that: a gate can be placed on the wrong side of a wall, or a corner can
//! be too tight for the car, and every geometric test still passes. So this file drives.
//!
//! # The driver
//!
//! A pure-pursuit controller along the circuit's **racing line**: `steer = −k × bearing
//! error` (positive steer is "right", which is `−yaw`), target speed falls with the error,
//! and a car that stops against a wall reverses out instead of sitting there. It is test
//! code, and it is the cheapest honest way to answer "is this circuit drivable by the
//! game's own car".
//!
//! Aiming at the gates themselves is not enough as soon as a circuit turns: the straight
//! line from one gate to the next crosses the inside of the corner, and the car drives
//! into the barrier every time. What the car follows is the circuit's **racing line**, which
//! is the `Waypoint` nodes in the scene — the same line phase 43's opponents follow. It is
//! read from the file rather than written out here, so a corner moved in the scene and not
//! in a test constant cannot leave this test passing on a circuit nobody can drive.
//!
//! # Determinism
//!
//! No wall clock, no randomness: `Car::step` gets a fixed `1/60`, so a lap that passes once
//! passes every time, on any machine.

use giochi_engine_3d::math3d::Vec3;
use giochi_racer::ai::Route;
use giochi_racer::game::physics_world;
use giochi_racer::track::lap::{LapEvent, LapTracker};
use giochi_racer::track::loading::load_track;
use giochi_racer::track::{Track, Waypoint};
use giochi_racer::vehicle::car::{Car, CarConfig};
use giochi_racer::vehicle::input::CarInput;

/// Every circuit the game ships, with the checkpoints the plan asks for.
const TRACKS: [(&str, &str, usize); 3] = [
    ("assets/tracks/track_easy.scene", "OVAL EASY", 4),
    ("assets/tracks/track_medium.scene", "FIGURE EIGHT", 6),
    ("assets/tracks/track_hard.scene", "SERPENTINE HARD", 8),
];

const STEP: f32 = 1.0 / 60.0;
/// A lap is 150 s of simulated time at most. Measured with the driver below: 19 s, 33 s
/// and 76 s, so this leaves room for a slower controller while still failing a stuck car.
const LAP_BUDGET_SECONDS: f32 = 150.0;

fn path(relative: &str) -> String {
    format!("{}/{relative}", env!("CARGO_MANIFEST_DIR"))
}

fn load(relative: &str) -> Track {
    load_track(path(relative)).unwrap_or_else(|error| panic!("{relative}: {error}"))
}

/// Wraps an angle into `-π..=π`.
fn wrap(angle: f32) -> f32 {
    let mut value = angle;
    while value > std::f32::consts::PI {
        value -= std::f32::consts::TAU;
    }
    while value < -std::f32::consts::PI {
        value += std::f32::consts::TAU;
    }
    value
}

/// Follows the racing line, like a driver who knows the circuit.
///
/// The aim point comes from [`Route`], the same geometry the opponents use, so there is one
/// definition of "where the line goes". The *speed* policy is deliberately not the
/// opponents': this controller asks for a flat 4–10 m/s and slows with the heading error,
/// which is a different question from "can the shipped corner speeds be driven". If this
/// test passes and `tests/waypoints.rs` passes, the circuit is drivable both cautiously and
/// at the pace the file asks for.
struct Pilot {
    route: Route,
    /// Seconds spent below walking pace while aiming at this point.
    stuck: f32,
    /// Seconds of reverse left to unwind a stuck car.
    recover: f32,
}

impl Pilot {
    fn new(line: &[(f32, f32)]) -> Self {
        let waypoints = line
            .iter()
            .enumerate()
            .map(|(index, (x, z))| Waypoint {
                index,
                position: Vec3::new(*x, 0.5, *z),
                speed: 10.0,
            })
            .collect();
        Self {
            route: Route::new(waypoints),
            stuck: 0.0,
            recover: 0.0,
        }
    }

    /// The input for this frame.
    fn drive(&mut self, state: &giochi_racer::vehicle::car::CarState, dt: f32) -> CarInput {
        let aim = self
            .route
            .look_ahead(state.position)
            .map(|aim| aim.point)
            .unwrap_or(state.position);
        let car = state.position.to_glam();
        // Bearing in the same convention as `CarState::yaw`: forward is `(sin, 0, cos)`.
        let bearing = (aim.x - car.x).atan2(aim.z - car.z);
        let error = wrap(bearing - state.yaw);

        // A wall is not a steering problem: no amount of lock moves a car wedged against
        // one. Reverse out, turning the nose the other way, then try again.
        if self.recover > 0.0 {
            self.recover -= dt;
            let steer = if self.recover > 0.8 { 1.0 } else { -1.0 };
            return CarInput {
                throttle: -1.0,
                steer,
                handbrake: false,
                pause: false,
            };
        }
        if state.speed_kmh < 2.0 {
            self.stuck += dt;
            if self.stuck > 0.8 {
                self.stuck = 0.0;
                self.recover = 2.0;
            }
        } else {
            self.stuck = 0.0;
        }

        // Steering is a rate, not an angle: at 30 m/s the car covers 50 m before the nose
        // has turned 90°. So the target speed falls with the error, not the throttle.
        let target_speed = 10.0 - 6.0 * (error.abs() / std::f32::consts::PI).min(1.0);
        let speed = state.speed_kmh / 3.6;
        CarInput {
            throttle: ((target_speed - speed) * 0.9).clamp(-1.0, 1.0),
            steer: (-error * 1.8).clamp(-1.0, 1.0),
            handbrake: false,
            pause: false,
        }
    }
}

#[test]
fn every_shipped_track_loads_with_the_gates_the_plan_asks_for() {
    for (file, name, checkpoints) in TRACKS {
        let track = load(file);
        assert_eq!(track.name, name, "{file}");
        assert_eq!(track.checkpoint_count(), checkpoints, "{file}");
        assert_eq!(track.checkpoints.len(), checkpoints + 1, "{file}");
        assert!(!track.instances.is_empty(), "{file} has nothing to draw");
        assert!(track.bounds().is_some(), "{file} has no extent");
        // The start line is first, and every gate is in lap order.
        for (index, gate) in track.checkpoints.iter().enumerate() {
            assert_eq!(gate.index, index, "{file}: gate {index} out of order");
        }
    }
}

#[test]
fn the_physics_car_completes_a_full_lap_on_every_shipped_track() {
    for (file, name, checkpoints) in TRACKS {
        let track = load(file);
        let mut world = physics_world(&track).expect("a physics world");
        let mut car = Car::spawn(&mut world, track.start, CarConfig::default()).expect("a car");
        let start = car.state(&world).expect("a live car");
        let mut lap = LapTracker::new(track.checkpoints.clone(), 1, start.position);

        // The line the opponents follow, from the scene: positions only, because this
        // controller predates the waypoint speeds and deliberately has none.
        let line: Vec<(f32, f32)> = track
            .waypoints
            .iter()
            .map(|waypoint| (waypoint.position.x, waypoint.position.z))
            .collect();
        assert!(line.len() >= 5, "{name}: the circuit has no racing line");
        let mut pilot = Pilot::new(&line);
        let mut state = car.state(&world).expect("a live car");
        let mut events = Vec::new();
        let frames = (LAP_BUDGET_SECONDS / STEP) as u32;

        for _ in 0..frames {
            let input = pilot.drive(&state, STEP);
            state = car.step(&mut world, input, STEP).expect("the car steps");
            events.push(lap.update(state.position, state.velocity, STEP));
            if lap.is_finished() {
                break;
            }
        }

        let remaining = lap
            .next_gate()
            .map(|gate| {
                let dx = gate.position.x - state.position.x;
                let dz = gate.position.z - state.position.z;
                (dx * dx + dz * dz).sqrt()
            })
            .unwrap_or(0.0);
        assert!(
            lap.is_finished(),
            "{name}: the car did not finish a lap in {LAP_BUDGET_SECONDS} s \
             (gate {} of {}, {remaining:.0} m from the next one)",
            lap.gates_taken(),
            checkpoints + 1
        );
        // A one-lap race ends with `RaceFinished`, not `LapComplete`: the last lap is the
        // one that ends the race, and reporting both would double-count it in a results
        // screen that listens for "a lap completed".
        assert!(
            events.iter().any(|event| matches!(
                event,
                LapEvent::LapComplete { .. } | LapEvent::RaceFinished
            )),
            "{name}: finished without reporting a completed lap"
        );
        let best = lap.best_lap().expect("a completed lap has a time");
        assert!(
            best > 5.0 && best < LAP_BUDGET_SECONDS,
            "{name}: best lap {best}"
        );
    }
}

#[test]
fn skipping_a_gate_invalidates_the_lap_but_not_the_race_clock() {
    // The plan's rule, on a real circuit: cutting the course must not count a lap.
    let track = load(TRACKS[1].0);
    let mut lap = LapTracker::new(track.checkpoints.clone(), 2, track.start.position);

    // Enter the third gate first, driving along the route, without taking the second.
    let skipped = track.checkpoints[2];
    let event = lap.update(skipped.position, skipped.direction, STEP);
    assert_eq!(
        event,
        LapEvent::LapInvalidated,
        "cutting the course was not noticed"
    );
    assert!(!lap.is_lap_valid(), "the lap stayed valid after a cut");

    // The clock kept running: `total_time` is the race, not the lap.
    assert!(lap.total_time() > 0.0);
    assert_eq!(lap.best_lap(), None, "a cut lap recorded a best time");
}
