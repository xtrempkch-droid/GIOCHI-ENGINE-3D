//! An opponent: the same `Car` as the player, driven by a [`Pilot`].
//!
//! This is the thin half — a car, and the controller that steers it. The controller itself
//! is [`Pilot`], which takes a [`CarState`] and has no car of its own, so that a test can
//! drive the player's car with the same code that drives the opponents
//! (`tests/race_field.rs`), and so that "does the controller steer at a corner?" can be
//! answered without a physics world at all.
//!
//! # Why the opponents are cars and not a script
//!
//! The plan allows "opponent on rails": move it along the waypoints instead of simulating
//! it. That was rejected. An opponent that ignores the walls looks wrong the first time it
//! overlaps one, and the whole point of phase 41's car is that the physics is trustworthy.
//! The opponents drive the **same** `Car` as the player, with the same colliders and the
//! same arcade model; only the input source differs.

use giochi_engine_3d::math3d::Vec3;
use giochi_engine_3d::physics::{PhysicsError, PhysicsWorld3D};

use crate::ai::pilot::Pilot;
use crate::ai::waypoint::Route;
use crate::vehicle::car::{Car, CarCommand, CarState};
use crate::vehicle::input::CarInput;

/// A car with a driver in it.
#[derive(Debug)]
pub struct AiDriver {
    car: Car,
    pilot: Pilot,
}

impl AiDriver {
    /// Puts `car` on `route` at `skill`, which is clamped by
    /// [`clamp_skill`](crate::ai::clamp_skill).
    ///
    /// `wander_phase` should differ between cars; any finite value works.
    pub fn new(car: Car, route: Route, skill: f32, wander_phase: f32) -> Self {
        Self {
            car,
            pilot: Pilot::new(route, skill, wander_phase),
        }
    }

    /// The same driver, preferring a lane `metres` to the right of the line.
    pub fn with_lane(mut self, metres: f32) -> Self {
        self.pilot = self.pilot.with_lane(metres);
        self
    }

    /// The car, for drawing and for its state.
    pub fn car(&self) -> &Car {
        &self.car
    }

    /// The car, mutably, for a rescue.
    pub fn car_mut(&mut self) -> &mut Car {
        &mut self.car
    }

    /// The controller, for a test that wants to look at it.
    pub fn pilot(&self) -> &Pilot {
        &self.pilot
    }

    /// The line being followed.
    pub fn route(&self) -> &Route {
        self.pilot.route()
    }

    /// This opponent's pace multiplier.
    pub fn skill(&self) -> f32 {
        self.pilot.skill()
    }

    /// The lane this opponent prefers, in metres, positive right of the line.
    pub fn lane(&self) -> f32 {
        self.pilot.lane()
    }

    /// Whether the driver is currently reversing out of a barrier.
    pub fn is_recovering(&self) -> bool {
        self.pilot.is_recovering()
    }

    /// The car's current state.
    pub fn state(&self, world: &PhysicsWorld3D) -> Option<CarState> {
        self.car.state(world).ok()
    }

    /// The input this frame, given where the car is.
    pub fn drive(&mut self, world: &PhysicsWorld3D, dt: f32) -> CarInput {
        match self.car.state(world) {
            Ok(state) => self.pilot.drive(&state, dt),
            Err(error) => {
                log::error!("opponent: no state: {error}");
                CarInput::NEUTRAL
            }
        }
    }

    /// Drives the car and proposes this frame to the world, without stepping it.
    ///
    /// A field steps the world once for all of its cars, so it calls this for each car and
    /// then steps the world itself — see [`crate::race::Field`].
    pub fn apply(
        &mut self,
        world: &mut PhysicsWorld3D,
        dt: f32,
    ) -> Result<CarCommand, PhysicsError> {
        let input = self.drive(world, dt);
        self.car.apply(world, input, dt)
    }

    /// Drives and steps in one call: the input this frame, applied, then physics.
    ///
    /// For one car. A field must not use this for every car — that would step the world
    /// once per car.
    pub fn step(&mut self, world: &mut PhysicsWorld3D, dt: f32) -> Result<CarState, PhysicsError> {
        let input = self.drive(world, dt);
        self.car.step(world, input, dt)
    }

    /// Whether this driver has been stuck enough times to need putting back on the line.
    ///
    /// The caller decides *where*, because only the race knows the current waypoint and a
    /// safe place to drop a car.
    pub fn needs_rescue(&self) -> bool {
        self.pilot.needs_rescue()
    }

    /// Where a rescue would put the car: the waypoint it is heading for, facing the one
    /// after it.
    ///
    /// Asked of the driver rather than worked out by the caller because the driver is the
    /// only thing that knows which segment it is stuck on. `None` for an empty line, in
    /// which case there is nowhere to be put back.
    pub fn rescue_spot(&self) -> Option<(Vec3, f32)> {
        let waypoints = self.pilot.route().waypoints();
        if waypoints.is_empty() {
            return None;
        }
        let index = self.pilot.route().next_waypoint().unwrap_or(0);
        let here = waypoints[index].position;
        let next = waypoints[(index + 1) % waypoints.len()].position;
        let dx = next.x - here.x;
        let dz = next.z - here.z;
        // Facing along the line: `CarState::yaw` has forward at `(sin, 0, cos)`.
        let yaw = if dx.hypot(dz) < 1e-3 {
            0.0
        } else {
            dx.atan2(dz)
        };
        Some((here, yaw))
    }

    /// Puts the car on `position` facing `yaw` and forgets the recovery history.
    ///
    /// The line is rewound too: after a rescue the car is not on the segment it was on, and
    /// a cursor left behind would send it round the lap backwards.
    pub fn rescue(&mut self, world: &mut PhysicsWorld3D, position: Vec3, yaw: f32) {
        if let Err(error) = self.car.respawn(world, position, yaw) {
            log::warn!("AI rescue failed: {error}");
            return;
        }
        self.pilot.after_rescue();
    }

    /// Rewinds the controller without moving the car, for a restart on the grid.
    pub fn restart(&mut self) {
        self.pilot.restart();
    }
}
