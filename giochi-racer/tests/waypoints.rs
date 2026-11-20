//! The racing line, and the opponent that follows it, on every shipped circuit.
//!
//! Phase 43 accepts "three opponents race the player". That claim is only worth anything if
//! the opponents can actually get round: a line that misses a gate never completes a lap, a
//! corner speed that is too high puts the car in the barrier, and a rescue that fires every
//! few seconds is not racing. So this file drives, three laps at a time, and measures.
//!
//! # Why the data and the controller are checked together
//!
//! The speeds in the scenes are computed by
//! [`scripts/generate-waypoints.py`](../scripts/generate-waypoints.py) from the car's own
//! handling constants. If those constants change and the script is not re-run, the circuit
//! becomes quietly slower -- or quietly undrivable. Driving the shipped file on every commit
//! is what turns that from a bug report into a failing test.
//!
//! # Determinism
//!
//! Fixed `1/60` step, no wall clock, no randomness: the wander is a sine of the driver's own
//! simulated time. A lap that passes here passes on the device.

use giochi_engine_3d::math3d::Vec3;
use giochi_racer::ai::{AiDriver, Route};
use giochi_racer::game::physics_world;
use giochi_racer::track::lap::LapTracker;
use giochi_racer::track::loading::load_track;
use giochi_racer::track::Track;
use giochi_racer::vehicle::car::{Car, CarConfig};

/// Every circuit the game ships, with its checkpoint count and how long a lap takes an
/// opponent of `skill = 1.0`. The times are measured, not guessed: they are here so that a
/// circuit that suddenly takes twice as long fails loudly.
const TRACKS: [(&str, &str, usize, f32); 3] = [
    ("assets/tracks/track_easy.scene", "OVAL EASY", 4, 20.0),
    ("assets/tracks/track_medium.scene", "FIGURE EIGHT", 6, 35.0),
    ("assets/tracks/track_hard.scene", "SERPENTINE HARD", 8, 78.0),
];

const LAPS: u32 = 3;
const STEP: f32 = 1.0 / 60.0;

fn path(relative: &str) -> String {
    format!("{}/{relative}", env!("CARGO_MANIFEST_DIR"))
}

fn load(relative: &str) -> Track {
    load_track(path(relative)).unwrap_or_else(|error| panic!("{relative}: {error}"))
}

/// Distance from `point` to the closed polyline through `line`, ignoring height.
fn distance_to_line(point: Vec3, line: &[Vec3]) -> f32 {
    let car = glam::Vec2::new(point.x, point.z);
    let mut best = f32::INFINITY;
    for index in 0..line.len() {
        let from = glam::Vec2::new(line[index].x, line[index].z);
        let to = glam::Vec2::new(
            line[(index + 1) % line.len()].x,
            line[(index + 1) % line.len()].z,
        );
        let along = to - from;
        let squared = along.length_squared();
        let t = if squared < 1e-9 {
            0.0
        } else {
            ((car - from).dot(along) / squared).clamp(0.0, 1.0)
        };
        best = best.min((car - (from + along * t)).length());
    }
    best
}

#[test]
fn every_shipped_circuit_has_a_racing_line_a_car_could_follow() {
    for (file, name, _, _) in TRACKS {
        let track = load(file);
        let line = &track.waypoints;
        assert!(
            line.len() >= 5,
            "{name}: {} waypoints is not a line",
            line.len()
        );
        for (index, waypoint) in line.iter().enumerate() {
            assert_eq!(
                waypoint.index, index,
                "{name}: waypoint {index} out of order"
            );
            assert!(
                waypoint.position.is_finite(),
                "{name}: waypoint {index} is not a number"
            );
            assert!(
                waypoint.speed.is_finite() && waypoint.speed > 3.0,
                "{name}: waypoint {index} asks for {} m/s",
                waypoint.speed
            );
            assert!(
                waypoint.speed <= CarConfig::default().max_speed + 0.1,
                "{name}: waypoint {index} asks for {} m/s, faster than the car",
                waypoint.speed
            );
        }

        // The line starts where the car starts, so the grid is on it.
        let start = line[0].position;
        let offset = ((start.x - track.start.position.x).powi(2)
            + (start.z - track.start.position.z).powi(2))
        .sqrt();
        assert!(
            offset < 15.0,
            "{name}: the line starts {offset:.1} m from the start line"
        );

        // Spacing sanity: a gap wider than anything on the circuit means a corner is
        // missing, and a gap near zero means a duplicated node the author did not intend.
        for index in 0..line.len() {
            let here = line[index].position;
            let next = line[(index + 1) % line.len()].position;
            let gap = ((next.x - here.x).powi(2) + (next.z - here.z).powi(2)).sqrt();
            assert!(
                (2.0..=110.0).contains(&gap),
                "{name}: waypoint {index} to {} is {gap:.1} m apart",
                (index + 1) % line.len()
            );
        }

        // Every gate has to be on the line, or the opponents drive past it and never
        // complete a lap. The gate is a disc, so the line has to pass through the disc.
        for gate in &track.checkpoints {
            let gap = distance_to_line(gate.position, &points(line));
            assert!(
                gap < gate.radius,
                "{name}: gate {} is {gap:.1} m from the line, outside its {:.0} m disc",
                gate.index,
                gate.radius
            );
        }
    }
}

fn points(line: &[giochi_racer::track::Waypoint]) -> Vec<Vec3> {
    line.iter().map(|waypoint| waypoint.position).collect()
}

#[test]
fn an_opponent_completes_three_laps_on_every_circuit() {
    for (file, name, checkpoints, expected_lap) in TRACKS {
        let track = load(file);
        let mut world = physics_world(&track).expect("a physics world");
        let car = Car::spawn(&mut world, track.start, CarConfig::default()).expect("a car");
        let mut driver = AiDriver::new(car, Route::from_track(&track), 1.0, 0.0);
        let mut lap = LapTracker::new(track.checkpoints.clone(), LAPS, track.start.position);

        let line = points(&track.waypoints);
        let mut worst_excursion: f32 = 0.0;
        let mut top_kmh: f32 = 0.0;
        // Three laps of the slowest circuit at the measured pace, with room for a bad one.
        let budget = (expected_lap * LAPS as f32 * 3.0).max(60.0);
        let frames = (budget / STEP) as u32;
        let mut used = 0;

        for frame in 0..frames {
            let state = driver.step(&mut world, STEP).expect("the car steps");
            lap.update(state.position, state.velocity, STEP);
            top_kmh = top_kmh.max(state.speed_kmh);
            worst_excursion = worst_excursion.max(distance_to_line(state.position, &line));
            assert!(
                !driver.needs_rescue(),
                "{name}: the opponent needed a rescue after {:.1} s (gate {} of {})",
                frame as f32 * STEP,
                lap.gates_taken(),
                checkpoints + 1
            );
            if lap.is_finished() {
                used = frame;
                break;
            }
        }

        assert!(
            lap.is_finished(),
            "{name}: the opponent did not finish {LAPS} laps in {budget:.0} s \
             (gate {} of {})",
            lap.gates_taken(),
            checkpoints + 1
        );
        let average = lap.total_time() / LAPS as f32;
        assert!(
            (expected_lap * 0.5..=expected_lap * 2.0).contains(&average),
            "{name}: a lap took {average:.1} s, expected around {expected_lap} s"
        );
        // A car that spends the race on the grass is not racing. The road is 10 m wide and
        // the line is its centre, so 9 m is generous and still fails a car in the scenery.
        assert!(
            worst_excursion < 9.0,
            "{name}: the opponent strayed {worst_excursion:.1} m from the line"
        );
        eprintln!(
            "{name}: {:.1} s for {LAPS} laps ({average:.1} s each), top {top_kmh:.0} km/h, \
             worst {worst_excursion:.1} m off the line, {} frames",
            used as f32 * STEP,
            used + 1
        );
    }
}
