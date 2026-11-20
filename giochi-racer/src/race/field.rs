//! The race simulation, with no GL in it.
//!
//! Everything phase 43 adds — the field, the opponents, the classification — lives here
//! rather than in [`RaceGame`](crate::game::RaceGame), because a race that can only be
//! created with a GL context can only be tested with a GL context. `main`'s loop, the
//! results screen and the tests all drive this; [`RaceGame`](crate::game::RaceGame) adds
//! the track's meshes, the camera and the HUD on top.
//!
//! # One world step per frame
//!
//! [`Car::step`] steps the physics world. That is right for one car and wrong for four: it
//! would run the world at four times the frame rate, and every car would be integrating a
//! different amount of simulated time from the one on screen. This was measured, not
//! assumed — the first four-car field drove through the barriers at 4× speed within five
//! seconds. So a frame here is three passes:
//!
//! 1. **Everybody proposes.** Each car's model integrates its input and writes the result
//!    to its body ([`Car::apply`]). Nothing has moved yet.
//! 2. **The world decides, once.** One `PhysicsWorld3D::step` for the whole field.
//! 3. **Everybody reads back.** Each car's pose after the solver ran, then the lap trackers,
//!    the rescues and the classification.

use giochi_engine_3d::math3d::{Color3D, Vec3};
use giochi_engine_3d::physics::PhysicsWorld3D;

use crate::ai::{AiDriver, Route};
use crate::track::lap::{LapEvent, LapTracker};
use crate::track::{Checkpoint, StartPosition, Track};
use crate::vehicle::car::{Car, CarConfig, CarState};
use crate::vehicle::input::CarInput;

use super::{classify, progress_metres, starting_grid, Entry, Standing, GRID_SLOTS};

/// The name the player is classified under.
pub const PLAYER_NAME: &str = "Voce";

/// Who the opponents are, and how they drive.
///
/// Each entry is a name, a skill, the lane the car prefers to hold in metres, and its paint.
/// The skills multiply the line's target speed; `1.0` is exactly the pace the circuit author
/// asked for, which is already close to the fastest the car can hold each corner. The lanes
/// are what stop the field from being three cars on one line: without them the quickest
/// opponent runs into the back of the one in front and the race becomes a queue.
pub const OPPONENTS: [(&str, f32, f32, Color3D); 3] = [
    ("Vermelho", 1.0, -1.6, Color3D::new(0.92, 0.30, 0.16, 1.0)),
    ("Azul", 0.93, 0.0, Color3D::new(0.20, 0.42, 0.90, 1.0)),
    ("Verde", 0.86, 1.6, Color3D::new(0.22, 0.72, 0.36, 1.0)),
];

/// A car the player is racing against.
///
/// The same [`Car`] as the player's, driven by an [`AiDriver`] instead of a gamepad, with a
/// [`LapTracker`] of its own so it is scored by the same gates and the same rules.
pub struct Opponent {
    name: &'static str,
    color: Color3D,
    driver: AiDriver,
    lap: LapTracker,
    state: CarState,
}

impl Opponent {
    /// Their name, for the classification and the results screen.
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Their paint.
    pub fn color(&self) -> Color3D {
        self.color
    }

    /// Their laps and clock.
    pub fn lap(&self) -> &LapTracker {
        &self.lap
    }

    /// Where they are, as of the last step.
    pub fn state(&self) -> CarState {
        self.state
    }

    /// How fast they are willing to go, `1.0` being the circuit's authored pace.
    pub fn skill(&self) -> f32 {
        self.driver.skill()
    }

    /// The lane they prefer to hold, in metres, positive right of the line.
    pub fn lane(&self) -> f32 {
        self.driver.lane()
    }

    /// Their car, for a test that wants to look at the body.
    pub fn car(&self) -> &Car {
        self.driver.car()
    }
}

/// The whole race: the world, the player, the field, the clocks and the classification.
pub struct Field {
    world: PhysicsWorld3D,
    player: Car,
    player_state: CarState,
    player_lap: LapTracker,
    opponents: Vec<Opponent>,
    /// Where everybody starts, so a restart is the same grid as the first race.
    grid: Vec<StartPosition>,
    /// The gates, kept for building a fresh tracker on a restart.
    gates: Vec<Checkpoint>,
    laps: u32,
    /// The tuning everybody on the grid shares, for a restart that does not fall through
    /// the floor.
    car_config: CarConfig,
    /// Average metres between gates, for turning gate progress into metres.
    gate_spacing: f32,
    standings: Vec<Standing>,
    /// The player's last lap event, waiting to be shown for a moment.
    last_event: Option<LapEvent>,
}

impl Field {
    /// Puts the whole field on the grid of `track` and starts the clocks.
    ///
    /// A circuit with no waypoints has no line for the opponents to follow, so it stays the
    /// time trial it was and [`Field::opponents`] is empty. That is a deliberate fallback,
    /// not an error: the editor can make a track before it makes a racing line.
    pub fn new(
        track: &Track,
        laps: u32,
        config: CarConfig,
        world: PhysicsWorld3D,
    ) -> Result<Self, giochi_engine_3d::physics::PhysicsError> {
        let mut world = world;
        let grid = starting_grid(track, GRID_SLOTS);
        let player = Car::spawn(&mut world, grid[0], config)?;
        let player_state = player.state(&world)?;
        let player_lap = LapTracker::new(track.checkpoints.clone(), laps, player_state.position);

        let mut opponents = Vec::new();
        let line = Route::from_track(track);
        if !line.is_empty() {
            for (index, (name, skill, lane, color)) in OPPONENTS.iter().enumerate() {
                let Some(start) = grid.get(index + 1).copied() else {
                    break;
                };
                let car = Car::spawn(&mut world, start, config)?;
                let state = car.state(&world)?;
                opponents.push(Opponent {
                    name,
                    color: *color,
                    // A different wander phase per car, so three opponents of equal skill do
                    // not trace one another's line.
                    driver: AiDriver::new(car, line.clone(), *skill, index as f32 * 1.7 + 0.4)
                        .with_lane(*lane),
                    lap: LapTracker::new(track.checkpoints.clone(), laps, state.position),
                    state,
                });
            }
        }

        let mut field = Self {
            world,
            player,
            player_state,
            player_lap,
            opponents,
            grid,
            gates: track.checkpoints.clone(),
            laps: laps.max(1),
            car_config: config,
            gate_spacing: gate_spacing(track),
            standings: Vec::new(),
            last_event: None,
        };
        field.update_standings();
        Ok(field)
    }

    /// Advances the whole race one frame.
    pub fn step(&mut self, input: CarInput, dt: f32) {
        let dt = dt.clamp(0.0, 0.1);

        // 1. Everybody proposes.
        if let Err(error) = self.player.apply(&mut self.world, input, dt) {
            log::error!("physics: {error}");
        }
        for opponent in &mut self.opponents {
            if let Err(error) = opponent.driver.apply(&mut self.world, dt) {
                log::error!("opponent {}: {error}", opponent.name);
            }
        }

        // 2. The world decides, once.
        if let Err(error) = self.world.step(dt) {
            log::error!("physics: {error}");
        }

        // 3. Everybody reads back.
        match self.player.state(&self.world) {
            Ok(state) => self.player_state = state,
            // A failed read means the body is gone, which is a bug; keeping the last good
            // pose keeps the frame drawable while the log says what happened.
            Err(error) => log::error!("physics: {error}"),
        }
        for opponent in &mut self.opponents {
            match opponent.driver.state(&self.world) {
                Some(state) => opponent.state = state,
                None => log::error!("opponent {}: no state", opponent.name),
            }
        }

        // Clocks and gates.
        let event =
            self.player_lap
                .update(self.player_state.position, self.player_state.velocity, dt);
        if event != LapEvent::None {
            self.last_event = Some(event);
        }
        for opponent in &mut self.opponents {
            opponent
                .lap
                .update(opponent.state.position, opponent.state.velocity, dt);
        }

        // A car that has given up on a barrier is put back on the line rather than left
        // there: a race with a car parked in the fence is not a race.
        for opponent in &mut self.opponents {
            if !opponent.driver.needs_rescue() {
                continue;
            }
            if let Some((position, yaw)) = opponent.driver.rescue_spot() {
                log::warn!(
                    "opponent {} rescued onto the line on lap {}",
                    opponent.name,
                    opponent.lap.current_lap()
                );
                opponent.driver.rescue(&mut self.world, position, yaw);
                if let Some(state) = opponent.driver.state(&self.world) {
                    opponent.state = state;
                }
            }
        }

        self.update_standings();
    }

    /// The physics world, for a caller that needs to query a body.
    pub fn world(&self) -> &PhysicsWorld3D {
        &self.world
    }

    /// The player's car.
    pub fn player(&self) -> &Car {
        &self.player
    }

    /// The player's pose after the last step.
    pub fn player_state(&self) -> CarState {
        self.player_state
    }

    /// The player's laps and clock.
    pub fn player_lap(&self) -> &LapTracker {
        &self.player_lap
    }

    /// The field, in grid order.
    pub fn opponents(&self) -> &[Opponent] {
        &self.opponents
    }

    /// Takes the player's last lap event, if there is one waiting.
    ///
    /// Taken rather than borrowed so that the banner is shown once: a HUD that re-reads the
    /// same event every frame would show "LAP 2" for the rest of the race.
    pub fn take_event(&mut self) -> Option<LapEvent> {
        self.last_event.take()
    }

    /// The classification, leader first.
    pub fn standings(&self) -> &[Standing] {
        &self.standings
    }

    /// Where the player is, `1`-based.
    pub fn player_position(&self) -> usize {
        self.standings
            .iter()
            .find(|standing| standing.name == PLAYER_NAME)
            .map_or(0, |standing| standing.position)
    }

    /// Metres between the player and the car in front, or `None` when the player leads.
    ///
    /// A distance, not a time: see the module comment for why the game does not pretend to
    /// know a clock it is not keeping.
    pub fn gap_ahead(&self) -> Option<f32> {
        let position = self.player_position();
        if position <= 1 {
            return None;
        }
        let ahead = self.standings.get(position - 2)?;
        let mine = self
            .standings
            .iter()
            .find(|standing| standing.name == PLAYER_NAME)?;
        Some((ahead.progress - mine.progress).max(0.0))
    }

    /// How many cars are racing, the player included.
    pub fn field_size(&self) -> usize {
        self.opponents.len() + 1
    }

    /// Starts the race again: clocks, laps and every car back on the same grid.
    pub fn restart(&mut self) {
        self.player_lap = LapTracker::new(self.gates.clone(), self.laps, self.grid[0].position);
        if let Err(error) =
            self.player
                .respawn(&mut self.world, self.grid[0].position, self.grid[0].yaw)
        {
            log::error!("restart: {error}");
            return;
        }
        if let Ok(state) = self.player.state(&self.world) {
            self.player_state = state;
        }
        for (index, opponent) in self.opponents.iter_mut().enumerate() {
            let Some(start) = self.grid.get(index + 1).copied() else {
                break;
            };
            if let Err(error) =
                opponent
                    .driver
                    .car_mut()
                    .respawn(&mut self.world, start.position, start.yaw)
            {
                log::error!("restart {}: {error}", opponent.name);
                continue;
            }
            opponent.driver.restart();
            opponent.lap = LapTracker::new(self.gates.clone(), self.laps, start.position);
            if let Some(state) = opponent.driver.state(&self.world) {
                opponent.state = state;
            }
        }
        self.update_standings();
    }

    /// Puts the player back on the start line, stationary, with the clock untouched.
    ///
    /// A respawn is not a restart: resetting the lap tracker here would make the key a way
    /// to erase a bad lap.
    pub fn respawn_player(&mut self) {
        let start = self.grid[0];
        let position = Vec3::new(
            start.position.x,
            start.position.y.max(self.car_config.resting_height()),
            start.position.z,
        );
        if let Err(error) = self.player.respawn(&mut self.world, position, start.yaw) {
            log::error!("respawn: {error}");
            return;
        }
        if let Ok(state) = self.player.state(&self.world) {
            self.player_state = state;
        }
    }

    /// Recomputes the classification from where everyone is now.
    fn update_standings(&mut self) {
        let mut entries: Vec<Entry> = Vec::with_capacity(self.field_size());
        entries.push(Entry {
            name: PLAYER_NAME.to_owned(),
            progress: progress_metres(
                &self.player_lap,
                self.player_state.position,
                self.gate_spacing,
            ),
            lap: self.player_lap.current_lap(),
            finished: self.player_lap.is_finished(),
            elapsed: self.player_lap.total_time(),
        });
        for opponent in &self.opponents {
            entries.push(Entry {
                name: opponent.name.to_owned(),
                progress: progress_metres(
                    &opponent.lap,
                    opponent.state.position,
                    self.gate_spacing,
                ),
                lap: opponent.lap.current_lap(),
                finished: opponent.lap.is_finished(),
                elapsed: opponent.lap.total_time(),
            });
        }
        self.standings = classify(&entries);
    }
}

/// Average metres between gates on `track`, for turning gate progress into metres.
///
/// A circuit with no waypoints has no line length; one metre keeps a progress number from
/// collapsing to zero rather than pretending to know a distance it does not have.
fn gate_spacing(track: &Track) -> f32 {
    let gates = track.checkpoints.len().max(1) as f32;
    if track.waypoints.is_empty() {
        return 1.0;
    }
    let length: f32 = track
        .waypoints
        .iter()
        .enumerate()
        .map(|(index, waypoint)| {
            let next = &track.waypoints[(index + 1) % track.waypoints.len()];
            let dx = next.position.x - waypoint.position.x;
            let dz = next.position.z - waypoint.position.z;
            (dx * dx + dz * dz).sqrt()
        })
        .sum();
    if length > 1.0 {
        length / gates
    } else {
        1.0
    }
}
