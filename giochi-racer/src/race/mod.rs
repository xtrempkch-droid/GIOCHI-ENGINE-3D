//! The starting grid, and the standings that come out of it.
//!
//! # Why the grid is a function and not four numbers in the scene
//!
//! A grid slot is derived from the start line: so far back along the direction the car
//! faces, so far left or right. The author already placed the start line; asking them to
//! place four more nodes would mean four more things that can disagree with it.
//!
//! # Why the standings are approximate on purpose
//!
//! A precise time gap needs a split time at every gate for every car. The lap tracker keeps
//! one clock, not a history, and inventing a history to print a number on a HUD is a lot of
//! machinery for a readout nobody can check. Instead progress is measured in metres along
//! the *line*: gates passed times the average gate spacing, minus the distance still to go
//! to the next gate. That is exact enough to order the field and honest enough to print as
//! "12 m ahead" rather than as a time.

pub mod field;

pub use field::{Field, Opponent, OPPONENTS, PLAYER_NAME};

use giochi_engine_3d::math3d::Vec3;

use crate::track::lap::LapTracker;
use crate::track::{StartPosition, Track};

/// How far apart the rows of the grid are, in metres.
///
/// A car is 4 m long, so 6.5 m leaves 2.5 m of daylight between the nose of one row and the
/// tail of the next. Any tighter and the solver spends the first second of the race pushing
/// four cars apart instead of racing them.
pub const GRID_ROW_SPACING: f32 = 6.5;

/// How far either side of the centre line a grid slot sits, in metres.
///
/// Half a road width is 5 m and a car is 1.9 m wide, so 1.8 m keeps both front wheels on
/// the tarmac with room to spare.
pub const GRID_LATERAL: f32 = 1.8;

/// How many cars a race puts on the grid, the player included.
pub const GRID_SLOTS: usize = 4;

/// The `slots` starting positions of a grid on `track`, pole first.
///
/// Slot 0 is on the start line itself; the rest go back in pairs, left then right, so the
/// player starts with both front wheels on the line and nobody alongside until they are
/// passed. A grid that put the player at the back would put three opponents between the
/// camera and the first corner, which is where a race is most likely to be lost through no
/// fault of the driver.
pub fn starting_grid(track: &Track, slots: usize) -> Vec<StartPosition> {
    let start = track.start;
    let forward = Vec3::new(start.yaw.sin(), 0.0, start.yaw.cos());
    // `forward` turned a quarter turn about `+Y`. Both are unit vectors on the ground.
    let right = Vec3::new(start.yaw.cos(), 0.0, -start.yaw.sin());

    (0..slots)
        .map(|slot| {
            let row = (slot / 2) as f32;
            let side = if slot % 2 == 0 { -1.0 } else { 1.0 };
            let back = row * GRID_ROW_SPACING;
            let across = side * GRID_LATERAL;
            StartPosition {
                position: Vec3::new(
                    start.position.x - forward.x * back + right.x * across,
                    start.position.y,
                    start.position.z - forward.z * back + right.z * across,
                ),
                yaw: start.yaw,
            }
        })
        .collect()
}

/// How far round the race a car is, in metres along the line.
///
/// `gate_spacing` is the lap length divided by the number of gates; see the module comment
/// for why an average is good enough.
pub fn progress_metres(lap: &LapTracker, position: Vec3, gate_spacing: f32) -> f32 {
    let passed =
        lap.current_lap().saturating_sub(1) as usize * lap.gates().len() + lap.gates_taken();
    let remaining = lap.next_gate().map_or(0.0, |gate| {
        let dx = gate.position.x - position.x;
        let dz = gate.position.z - position.z;
        (dx * dx + dz * dz).sqrt()
    });
    passed as f32 * gate_spacing - remaining
}

/// One line of the classification.
#[derive(Debug, Clone, PartialEq)]
pub struct Standing {
    /// Who is where: the player's name, or an opponent's.
    pub name: String,
    /// `1` for the leader.
    pub position: usize,
    /// Metres along the race: the ranking key, and what the gap is measured from.
    pub progress: f32,
    /// The lap being driven, `1`-based.
    pub lap: u32,
    /// Whether this car has finished every lap.
    pub finished: bool,
}

/// One car's contribution to the classification, before it is ordered.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// Who it is.
    pub name: String,
    /// Metres along the race.
    pub progress: f32,
    /// The lap being driven, `1`-based.
    pub lap: u32,
    /// Whether every lap is done.
    pub finished: bool,
    /// Seconds since the race started; only compared between finished cars.
    pub elapsed: f32,
}

/// Orders the field, leader first.
///
/// Finished cars come first and, among themselves, in the order they finished (a smaller
/// elapsed time). Everyone else is ordered by progress. Ties keep their input order, which
/// is the grid, so the classification never flickers between two stationary cars.
pub fn classify(entries: &[Entry]) -> Vec<Standing> {
    let mut order: Vec<usize> = (0..entries.len()).collect();
    order.sort_by(|left, right| {
        let a = &entries[*left];
        let b = &entries[*right];
        match (a.finished, b.finished) {
            (true, true) => a.elapsed.total_cmp(&b.elapsed),
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            (false, false) => b.progress.total_cmp(&a.progress),
        }
        .then(left.cmp(right))
    });
    order
        .into_iter()
        .enumerate()
        .map(|(index, source)| {
            let entry = &entries[source];
            Standing {
                name: entry.name.clone(),
                position: index + 1,
                progress: entry.progress,
                lap: entry.lap,
                finished: entry.finished,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track() -> Track {
        Track {
            name: "grid".to_owned(),
            start: StartPosition {
                position: Vec3::new(10.0, 0.5, -20.0),
                // Facing `+X`.
                yaw: std::f32::consts::FRAC_PI_2,
            },
            checkpoints: Vec::new(),
            waypoints: Vec::new(),
            instances: Vec::new(),
            colliders: Vec::new(),
            light: None,
        }
    }

    #[test]
    fn the_first_slot_is_beside_the_start_line() {
        let grid = starting_grid(&track(), GRID_SLOTS);
        assert_eq!(grid.len(), GRID_SLOTS);
        // The pole slot is a lane to one side of the line and not a metre back.
        assert!(
            (grid[0].position.x - 10.0).abs() < 1e-4,
            "{:?}",
            grid[0].position
        );
        assert!((grid[0].position.z + 20.0 - GRID_LATERAL).abs() < 1e-4);
        assert!((grid[0].position.y - 0.5).abs() < 1e-6);
        assert_eq!(grid[0].yaw, std::f32::consts::FRAC_PI_2);
    }

    #[test]
    fn the_grid_is_two_by_two_behind_the_line() {
        let grid = starting_grid(&track(), GRID_SLOTS);
        // Facing `+X`, so "back" is `-X`.
        assert!((grid[1].position.x - 10.0).abs() < 1e-4);
        assert!((grid[2].position.x - (10.0 - GRID_ROW_SPACING)).abs() < 1e-4);
        assert!((grid[3].position.x - (10.0 - GRID_ROW_SPACING)).abs() < 1e-4);
        // Both slots of a row are a lane either side of the centre line.
        assert!(
            ((grid[0].position.z - grid[1].position.z).abs() - 2.0 * GRID_LATERAL).abs() < 1e-4
        );
        assert!(
            ((grid[2].position.z - grid[3].position.z).abs() - 2.0 * GRID_LATERAL).abs() < 1e-4
        );
    }

    #[test]
    fn no_two_slots_are_close_enough_to_touch() {
        let grid = starting_grid(&track(), GRID_SLOTS);
        for left in 0..grid.len() {
            for right in left + 1..grid.len() {
                let dx = grid[left].position.x - grid[right].position.x;
                let dz = grid[left].position.z - grid[right].position.z;
                let gap = (dx * dx + dz * dz).sqrt();
                // A car is 4 m long and 1.9 m wide; under 3 m apart is bodies overlapping
                // at spawn, which the solver answers by launching them.
                assert!(gap > 3.0, "slots {left} and {right} are {gap:.2} m apart");
            }
        }
    }

    #[test]
    fn a_grid_of_one_is_the_start_line() {
        let grid = starting_grid(&track(), 1);
        assert_eq!(grid.len(), 1);
        assert!((grid[0].position.x - 10.0).abs() < 1e-4);
    }

    fn entry(name: &str, progress: f32, lap: u32, finished: bool, elapsed: f32) -> Entry {
        Entry {
            name: name.to_owned(),
            progress,
            lap,
            finished,
            elapsed,
        }
    }

    #[test]
    fn the_furthest_round_the_race_is_first() {
        let field = classify(&[
            entry("player", 100.0, 1, false, 0.0),
            entry("rival", 240.0, 2, false, 0.0),
        ]);
        assert_eq!(field[0].name, "rival");
        assert_eq!(field[0].position, 1);
        assert_eq!(field[1].position, 2);
    }

    #[test]
    fn a_finished_car_is_ahead_of_one_still_racing() {
        let field = classify(&[
            entry("racing", 900.0, 3, false, 0.0),
            entry("finished", 10.0, 1, true, 40.0),
        ]);
        assert_eq!(field[0].name, "finished");
    }

    #[test]
    fn finished_cars_are_ordered_by_their_time() {
        let field = classify(&[
            entry("slow", 0.0, 3, true, 61.0),
            entry("quick", 0.0, 3, true, 58.0),
        ]);
        assert_eq!(field[0].name, "quick");
        assert_eq!(field[1].name, "slow");
    }

    #[test]
    fn a_tie_keeps_the_grid_order() {
        let field = classify(&[
            entry("pole", 50.0, 1, false, 0.0),
            entry("row1", 50.0, 1, false, 0.0),
        ]);
        assert_eq!(field[0].name, "pole");
    }
}
