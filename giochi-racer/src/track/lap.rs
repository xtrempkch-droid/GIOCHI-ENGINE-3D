//! Counting laps, and refusing to count a lap the car did not earn.
//!
//! A lap is the ordered sequence `finish → 1 → 2 → … → n-1 → finish`. The rules this
//! module enforces, and why each one is there:
//!
//! * **A gate counts on entry, not while you are inside it.** Otherwise a car parked on
//!   the line would finish a lap per frame.
//! * **The car has to be moving along the gate.** A checkpoint is a disc with a direction,
//!   so reversing back over the line — which is what you do after a spin — does not score
//!   it. Without this the cheapest lap in the world is "drive back and forth over the
//!   start line".
//! * **A gate out of order invalidates the lap, it does not stop the race.** The lap
//!   timer keeps running, the lap number does not move, and the next crossing of the
//!   finish line starts a clean lap. Cutting the course costs you the lap, not the run.
//! * **The timer is the race clock.** `total_time` never resets; `lap_time` does.
//!
//! Everything here is pure arithmetic over positions, which is what makes the whole
//! scoring system testable without a window, a car or a track file.

use giochi_engine_3d::math3d::Vec3;

use super::Checkpoint;

/// What just happened to the lap.
///
/// Not `Eq`: the lap times inside [`LapEvent::LapComplete`] are `f32`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LapEvent {
    /// Nothing.
    None,
    /// The car entered gate `index` in the right order.
    Checkpoint(usize),
    /// A lap was completed and counted.
    LapComplete {
        /// The lap number that was just completed, `1`-based.
        lap: u32,
        /// How long it took, in seconds.
        time: f32,
        /// Whether it beat the previous best.
        best: bool,
    },
    /// The car cut the course or crossed the line backwards; the lap will not count.
    LapInvalidated,
    /// The last lap was completed. The race is over.
    RaceFinished,
}

/// A race in progress.
#[derive(Debug, Clone, PartialEq)]
pub struct LapTracker {
    gates: Vec<Checkpoint>,
    total_laps: u32,
    /// Index of the gate the car is expected to enter next.
    next: usize,
    /// Per gate: was the car inside it on the previous update?
    inside: Vec<bool>,
    /// Whether the current lap has hit every gate in order.
    lap_valid: bool,
    /// Checkpoints taken this lap, not counting the start/finish line.
    taken: usize,
    current_lap: u32,
    lap_time: f32,
    total_time: f32,
    last_lap: Option<f32>,
    best_lap: Option<f32>,
    finished: bool,
}

impl LapTracker {
    /// Starts a race on `gates` over `total_laps` laps, with the car at `start`.
    ///
    /// `start` matters: the car spawns **on** the finish line, and a tracker that did not
    /// know that would count a lap on the first frame.
    pub fn new(gates: Vec<Checkpoint>, total_laps: u32, start: Vec3) -> Self {
        let inside = gates.iter().map(|gate| gate.is_inside(start)).collect();
        let next = if gates.len() > 1 { 1 } else { 0 };
        Self {
            gates,
            total_laps: total_laps.max(1),
            next,
            inside,
            lap_valid: true,
            taken: 0,
            current_lap: 1,
            lap_time: 0.0,
            total_time: 0.0,
            last_lap: None,
            best_lap: None,
            finished: false,
        }
    }

    /// The gates, in lap order.
    pub fn gates(&self) -> &[Checkpoint] {
        &self.gates
    }

    /// The lap being driven, `1`-based.
    pub fn current_lap(&self) -> u32 {
        self.current_lap
    }

    /// How many laps finish the race.
    pub fn total_laps(&self) -> u32 {
        self.total_laps
    }

    /// Seconds since this lap started.
    pub fn lap_time(&self) -> f32 {
        self.lap_time
    }

    /// Seconds since the race started.
    pub fn total_time(&self) -> f32 {
        self.total_time
    }

    /// The last completed lap, if there is one.
    pub fn last_lap(&self) -> Option<f32> {
        self.last_lap
    }

    /// The fastest completed lap, if there is one.
    pub fn best_lap(&self) -> Option<f32> {
        self.best_lap
    }

    /// Whether the race is over.
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Whether the current lap is still clean.
    pub fn is_lap_valid(&self) -> bool {
        self.lap_valid
    }

    /// The gate the car should head for next, if the track has one.
    pub fn next_gate(&self) -> Option<&Checkpoint> {
        self.gates.get(self.next)
    }

    /// Checkpoints taken this lap, for a HUD "2/4" readout.
    ///
    /// The start/finish line is not counted: it is crossed at the start of every lap, so
    /// counting it would make the first readout of a race "1 of 5".
    pub fn gates_taken(&self) -> usize {
        self.taken
    }

    /// How many checkpoints a lap has, for the other half of the readout.
    pub fn checkpoint_count(&self) -> usize {
        self.gates.len().saturating_sub(1)
    }

    /// Advances the clock and scores any gate the car has just driven into.
    ///
    /// `velocity` only has to be a direction: the test is `dot(velocity, gate.direction)
    /// > 0`, so its magnitude is irrelevant.
    pub fn update(&mut self, position: Vec3, velocity: Vec3, dt: f32) -> LapEvent {
        if self.finished {
            return LapEvent::None;
        }
        let dt = dt.max(0.0);
        self.lap_time += dt;
        self.total_time += dt;

        // Every gate's "was inside" flag is updated every frame, and at most one gate
        // scores. Both halves matter: returning early would leave the flags of the gates
        // the loop never reached stale, and a car that scores gate 1 on one frame and
        // gate 3 on the next would then find gate 3 "already inside" and skip it — which
        // is how a clean lap turned into an invalid one.
        let mut event = LapEvent::None;
        for index in 0..self.gates.len() {
            let gate = self.gates[index];
            let inside = gate.is_inside(position);
            let was_inside = std::mem::replace(&mut self.inside[index], inside);
            if event != LapEvent::None || !inside || was_inside {
                continue;
            }
            let along = dot_horizontal(velocity, gate.direction);
            if along <= 0.0 {
                // Reversing through a gate never scores it, and never invalidates either:
                // a spin should cost time, not the whole lap.
                continue;
            }
            event = self.enter(index);
        }
        event
    }

    fn enter(&mut self, index: usize) -> LapEvent {
        if index != self.next {
            // Out of order. Whether the car skipped a gate or drove the circuit
            // backwards, the lap cannot count now; the finish line is what starts the
            // next one.
            self.lap_valid = false;
            self.taken = 0;
            self.next = 0;
            return LapEvent::LapInvalidated;
        }

        if index != 0 {
            // A gate other than the finish advances the sequence, wrapping to the finish
            // line after the last checkpoint. Without the wrap the lap would end at a
            // `next` of `len`, which no gate can ever satisfy, and every lap would be
            // invalidated at the line.
            self.taken += 1;
            self.next = if index + 1 >= self.gates.len() {
                0
            } else {
                index + 1
            };
            return LapEvent::Checkpoint(index);
        }

        // The finish line, in order: close the lap.
        let time = self.lap_time;
        let valid = self.lap_valid;
        self.lap_time = 0.0;
        self.lap_valid = true;
        self.taken = 0;
        self.next = if self.gates.len() > 1 { 1 } else { 0 };
        // The car is inside the finish gate right now; remembering that is what keeps the
        // same crossing from scoring twice on the next frame.
        if let Some(first) = self.inside.first_mut() {
            *first = true;
        }

        if !valid {
            return LapEvent::LapInvalidated;
        }

        self.last_lap = Some(time);
        let best = self.best_lap.is_none_or(|best| time < best);
        if best {
            self.best_lap = Some(time);
        }
        let lap = self.current_lap;
        if self.current_lap >= self.total_laps {
            self.finished = true;
            return LapEvent::RaceFinished;
        }
        self.current_lap += 1;
        LapEvent::LapComplete { lap, time, best }
    }
}

/// `dot` of two vectors projected onto the ground plane.
fn dot_horizontal(a: Vec3, b: Vec3) -> f32 {
    let (a, b) = (a.to_glam(), b.to_glam());
    a.x * b.x + a.z * b.z
}

#[cfg(test)]
mod tests {
    use super::*;
    use giochi_engine_3d::math3d::Vec3;

    /// A square circuit: finish at the origin, up to `z = 40`, across to `x = 40`, back
    /// down, and home along `-X`.
    ///
    /// Gate `0` sits at the origin, so a car "at the start" is inside it and a car at
    /// `z = 10` has left it.
    fn square() -> Vec<Checkpoint> {
        vec![
            Checkpoint {
                index: 0,
                position: Vec3::new(0.0, 0.0, 0.0),
                radius: 5.0,
                // Arriving at the line from gate 3, which is at `x = 40`.
                direction: Vec3::new(-1.0, 0.0, 0.0),
            },
            Checkpoint {
                index: 1,
                position: Vec3::new(0.0, 0.0, 40.0),
                radius: 5.0,
                direction: Vec3::new(0.0, 0.0, 1.0),
            },
            Checkpoint {
                index: 2,
                position: Vec3::new(40.0, 0.0, 40.0),
                radius: 5.0,
                direction: Vec3::new(1.0, 0.0, 0.0),
            },
            Checkpoint {
                index: 3,
                position: Vec3::new(40.0, 0.0, 0.0),
                radius: 5.0,
                direction: Vec3::new(0.0, 0.0, -1.0),
            },
        ]
    }

    const FORWARD: Vec3 = Vec3::new(0.0, 0.0, 1.0);

    fn tracker() -> LapTracker {
        LapTracker::new(square(), 3, Vec3::new(0.0, 0.5, 0.0))
    }

    /// The velocity that scores `gate`: along the gate's own arrival direction.
    fn arriving(tracker: &LapTracker, gate: usize) -> Vec3 {
        tracker.gates()[gate].direction
    }

    /// Drives the whole circuit once, without the clock mattering.
    fn drive_a_lap(tracker: &mut LapTracker) -> LapEvent {
        let mut last = LapEvent::None;
        for gate in [1usize, 2, 3, 0] {
            let position = tracker.gates()[gate].position;
            let velocity = arriving(tracker, gate);
            last = tracker.update(position, velocity, 1.0);
        }
        last
    }

    #[test]
    fn standing_on_the_finish_line_does_not_count_a_lap() {
        let mut tracker = tracker();
        // The car spawns inside gate 0. Fifty frames later it is still there.
        for _ in 0..50 {
            assert_eq!(
                tracker.update(Vec3::new(0.0, 0.5, 0.0), FORWARD, 1.0 / 60.0),
                LapEvent::None
            );
        }
        assert_eq!(tracker.current_lap(), 1);
    }

    #[test]
    fn the_clock_runs_while_the_car_sits_still() {
        let mut tracker = tracker();
        tracker.update(Vec3::new(0.0, 0.5, 0.0), FORWARD, 0.5);
        assert!((tracker.lap_time() - 0.5).abs() < 1e-6);
        assert!((tracker.total_time() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn a_clean_lap_counts_once_and_restarts_the_lap_clock() {
        let mut tracker = tracker();
        let event = drive_a_lap(&mut tracker);
        assert_eq!(
            event,
            LapEvent::LapComplete {
                lap: 1,
                time: 4.0,
                best: true
            }
        );
        assert_eq!(tracker.current_lap(), 2);
        assert_eq!(tracker.last_lap(), Some(4.0));
        assert_eq!(tracker.best_lap(), Some(4.0));
        assert!((tracker.lap_time() - 0.0).abs() < 1e-6);
        assert!(
            (tracker.total_time() - 4.0).abs() < 1e-6,
            "the race clock keeps running"
        );
    }

    #[test]
    fn a_better_lap_updates_the_best_and_a_worse_one_does_not() {
        let mut tracker = tracker();
        drive_a_lap(&mut tracker);
        // Second lap: slower.
        for _ in 0..10 {
            tracker.update(Vec3::new(0.0, 0.5, 50.0), FORWARD, 1.0);
        }
        drive_a_lap(&mut tracker);
        assert_eq!(tracker.best_lap(), Some(4.0));
        assert!(tracker.last_lap().unwrap() > 4.0);

        // Third lap: faster, and it is the last one.
        for gate in [1usize, 2, 3] {
            let position = tracker.gates()[gate].position;
            let velocity = arriving(&tracker, gate);
            tracker.update(position, velocity, 0.25);
        }
        let finish = tracker.gates()[0].position;
        let event = tracker.update(finish, arriving(&tracker, 0), 0.25);
        assert_eq!(event, LapEvent::RaceFinished, "three laps finish the race");
        assert!(tracker.is_finished());
        assert!(tracker.best_lap().unwrap() < 4.0);
    }

    #[test]
    fn the_race_stops_scoring_after_the_finish() {
        let mut tracker = tracker();
        drive_a_lap(&mut tracker);
        drive_a_lap(&mut tracker);
        let finish = tracker.gates()[0].position;
        for _ in 0..10 {
            tracker.update(finish, FORWARD, 0.0);
        }
        drive_a_lap(&mut tracker);
        assert!(tracker.is_finished());
        assert_eq!(
            tracker.update(finish, FORWARD, 1.0),
            LapEvent::None,
            "a finished race ignores further crossings"
        );
    }

    #[test]
    fn skipping_a_gate_invalidates_the_lap_without_stopping_the_clock() {
        let mut tracker = tracker();
        // Straight from the start to gate 2, skipping gate 1.
        let gate2 = tracker.gates()[2].position;
        let before = tracker.total_time();
        let event = tracker.update(gate2, arriving(&tracker, 2), 0.5);
        assert_eq!(event, LapEvent::LapInvalidated);
        assert!(!tracker.is_lap_valid());
        assert!(tracker.total_time() > before);
        assert_eq!(tracker.current_lap(), 1, "an invalid lap does not count");
    }

    #[test]
    fn an_invalidated_lap_can_be_restarted_by_crossing_the_line() {
        let mut tracker = tracker();
        let gate2 = tracker.gates()[2].position;
        let arrival = arriving(&tracker, 2);
        tracker.update(gate2, arrival, 0.5);
        // Leave the line, then cross it: the lap restarts clean instead of counting.
        tracker.update(Vec3::new(0.0, 0.5, 20.0), FORWARD, 1.0);
        let finish = tracker.gates()[0].position;
        let event = tracker.update(finish, arriving(&tracker, 0), 1.0);
        assert_eq!(event, LapEvent::LapInvalidated, "the lap was not earned");
        assert_eq!(tracker.current_lap(), 1, "and it did not count");
        assert!(tracker.is_lap_valid(), "but the next one starts clean");
        assert_eq!(tracker.next_gate().map(|gate| gate.index), Some(1));
    }

    #[test]
    fn crossing_the_line_backwards_does_not_count() {
        let mut tracker = tracker();
        // Complete the gates but arrive at the line going the wrong way.
        for gate in [1usize, 2, 3] {
            let position = tracker.gates()[gate].position;
            let velocity = arriving(&tracker, gate);
            tracker.update(position, velocity, 1.0);
        }
        // Complete the gates but arrive at the line going the wrong way.
        let finish = tracker.gates()[0].position;
        let arrival = arriving(&tracker, 0);
        let backwards = Vec3::new(-arrival.x, -arrival.y, -arrival.z);
        let event = tracker.update(finish, backwards, 1.0);
        assert_eq!(event, LapEvent::None, "a reversed crossing is not a lap");
        assert_eq!(tracker.current_lap(), 1);
    }

    #[test]
    fn driving_back_over_a_checkpoint_does_not_invalidate_the_lap() {
        let mut tracker = tracker();
        let gate1 = tracker.gates()[1].position;
        let arrival = arriving(&tracker, 1);
        assert_eq!(tracker.update(gate1, arrival, 1.0), LapEvent::Checkpoint(1));
        // Leave, spin, come back the wrong way: no event, lap still clean.
        tracker.update(Vec3::new(0.0, 0.5, 30.0), FORWARD, 1.0);
        let backwards_velocity = Vec3::new(-arrival.x, -arrival.y, -arrival.z);
        let backwards = tracker.update(gate1, backwards_velocity, 1.0);
        assert_eq!(backwards, LapEvent::None);
        assert!(tracker.is_lap_valid());
    }

    #[test]
    fn a_track_with_only_a_finish_line_counts_laps_by_crossing_it() {
        let gates = vec![Checkpoint {
            index: 0,
            position: Vec3::new(0.0, 0.0, 0.0),
            radius: 5.0,
            direction: Vec3::new(0.0, 0.0, -1.0),
        }];
        let mut tracker = LapTracker::new(gates, 2, Vec3::new(0.0, 0.5, 0.0));
        tracker.update(Vec3::new(0.0, 0.5, 20.0), FORWARD, 1.0);
        let event = tracker.update(Vec3::new(0.0, 0.5, 0.0), Vec3::new(0.0, 0.0, -1.0), 1.0);
        assert_eq!(
            event,
            LapEvent::LapComplete {
                lap: 1,
                time: 2.0,
                best: true
            }
        );
    }

    #[test]
    fn one_lap_is_enough_if_the_race_asks_for_one() {
        let mut tracker = LapTracker::new(square(), 1, Vec3::new(0.0, 0.5, 0.0));
        assert_eq!(drive_a_lap(&mut tracker), LapEvent::RaceFinished);
        assert!(tracker.is_finished());
    }

    #[test]
    fn a_zero_lap_race_is_treated_as_one_lap() {
        let tracker = LapTracker::new(square(), 0, Vec3::new(0.0, 0.5, 0.0));
        assert_eq!(tracker.total_laps(), 1);
    }

    #[test]
    fn gates_taken_counts_the_checkpoints_crossed_this_lap() {
        let mut tracker = tracker();
        assert_eq!(
            tracker.gates_taken(),
            0,
            "the start line is not a checkpoint"
        );
        assert_eq!(tracker.checkpoint_count(), 3);
        let gate1 = tracker.gates()[1].position;
        let arrival = arriving(&tracker, 1);
        tracker.update(gate1, arrival, 0.0);
        assert_eq!(tracker.gates_taken(), 1);
        let finish = tracker.gates()[0].position;
        tracker.update(Vec3::new(0.0, 0.5, 20.0), FORWARD, 0.0);
        let arrival = arriving(&tracker, 0);
        tracker.update(finish, arrival, 0.0);
        // The lap was invalid (gates 2 and 3 were skipped), so the count restarts at 0.
        assert_eq!(tracker.gates_taken(), 0);
    }

    #[test]
    fn a_clean_lap_takes_every_checkpoint_and_counts_them() {
        let mut tracker = tracker();
        for gate in [1usize, 2, 3] {
            let position = tracker.gates()[gate].position;
            let velocity = arriving(&tracker, gate);
            tracker.update(position, velocity, 0.0);
        }
        assert_eq!(tracker.gates_taken(), 3);
        let finish = tracker.gates()[0].position;
        tracker.update(finish, arriving(&tracker, 0), 0.0);
        assert_eq!(tracker.gates_taken(), 0, "a new lap starts with none");
    }

    #[test]
    fn the_next_gate_is_the_one_the_driver_should_aim_for() {
        let mut tracker = tracker();
        assert_eq!(tracker.next_gate().map(|gate| gate.index), Some(1));
        let gate1 = tracker.gates()[1].position;
        let arrival = arriving(&tracker, 1);
        tracker.update(gate1, arrival, 0.0);
        assert_eq!(tracker.next_gate().map(|gate| gate.index), Some(2));
    }

    #[test]
    fn a_negative_delta_does_not_rewind_the_clock() {
        let mut tracker = tracker();
        tracker.update(Vec3::new(0.0, 0.5, 0.0), FORWARD, 1.0);
        tracker.update(Vec3::new(0.0, 0.5, 0.0), FORWARD, -5.0);
        assert!((tracker.total_time() - 1.0).abs() < 1e-6);
    }
}
