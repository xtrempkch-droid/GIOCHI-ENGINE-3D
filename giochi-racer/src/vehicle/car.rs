//! The car: an arcade model that nobody can flip, wrapped around a Rapier body.
//!
//! # Why the model is separate from the body
//!
//! A rigid-body car is a stack of compromises: suspension rays, tyre friction curves, a
//! centre of mass that has to sit below the axles or the thing somersaults on the first
//! kerb. None of that is in scope for a low-poly arcade racer whose whole promise is
//! "press the trigger and it goes". So the *driving* is an [`ArcadeModel`] — plain
//! arithmetic over a heading and a planar velocity — and Rapier is used for what it is
//! excellent at: keeping the car out of walls and on the floor.
//!
//! The split has a second benefit that matters more than the first: the model needs no
//! GL and no physics world, so the handling is unit-tested here, not discovered by
//! playing.
//!
//! # How the two halves talk
//!
//! Each frame the body's *actual* velocity is adopted by the model, the model proposes a
//! new heading and planar velocity, and the body is told to move that way. Adopting the
//! real velocity is what makes a wall hit feel like a wall hit: the solver stops the car,
//! the model reads zero, and no amount of throttle pushes it through.
//!
//! # Why the body's rotations are locked
//!
//! The plan asks for downforce "so the car does not flip". Locking rotations is the
//! stronger version of the same guarantee — it cannot flip, at any speed, on any kerb —
//! and it removes a whole class of physics failure from a track the editor can change
//! tomorrow. The car's yaw is set directly each frame, so steering is unaffected.

use giochi_engine_3d::math3d::{Quat, Transform3D, Vec3};
use giochi_engine_3d::physics::{
    BodyDesc, BodyHandle, ColliderShape, PhysicsError, PhysicsWorld3D,
};

use crate::track::StartPosition;
use crate::vehicle::input::CarInput;

/// Metres per second in one km/h.
pub const KMH: f32 = 1.0 / 3.6;

/// Speed at which the steering reaches full authority, in m/s.
///
/// Below it the yaw rate is proportional to speed — a car creeping forward turns slowly,
/// and a stationary one does not turn at all. Above it the authority only falls, which is
/// what makes a slow car turn tighter than a fast one.
const STEER_REFERENCE_SPEED: f32 = 2.0;

/// How the car handles.
///
/// The defaults are tuned for a 60 fps arcade feel: 120 km/h top speed, a quarter turn
/// per second at low speed, and enough lateral grip to feel planted without being on
/// rails. All of it is data, because Phase 44 hands these numbers to a menu.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CarConfig {
    /// Top speed forward, in m/s. `120 km/h` by default.
    pub max_speed: f32,
    /// Top speed in reverse, in m/s.
    pub reverse_speed: f32,
    /// Forward acceleration, in m/s².
    pub accel: f32,
    /// Braking deceleration, in m/s².
    pub brake: f32,
    /// Coasting deceleration when the throttle is released, in m/s².
    pub drag: f32,
    /// Yaw rate at full lock and low speed, in rad/s.
    pub steer_rate: f32,
    /// How much of the steering is lost at top speed, `0.0..=1.0`.
    ///
    /// A car that turns as sharply at 120 km/h as at 20 is a car that spins.
    pub steer_falloff: f32,
    /// Lateral velocity damping, per second. Higher is more grip.
    pub grip: f32,
    /// Lateral damping while the handbrake is held. Lower is a bigger slide.
    pub handbrake_grip: f32,
    /// Extra deceleration while the handbrake is held, in m/s².
    pub handbrake_brake: f32,
    /// Collider half-extents: half width, half height, half length.
    pub half_extents: Vec3,
    /// Visual wheel radius, in metres. Also how high the body sits above the road.
    pub wheel_radius: f32,
}

impl Default for CarConfig {
    fn default() -> Self {
        Self {
            max_speed: 120.0 * KMH,
            reverse_speed: 30.0 * KMH,
            accel: 14.0,
            brake: 28.0,
            drag: 2.4,
            steer_rate: 1.7,
            steer_falloff: 0.55,
            grip: 9.0,
            handbrake_grip: 1.4,
            handbrake_brake: 9.0,
            half_extents: Vec3::new(0.95, 0.35, 2.0),
            wheel_radius: 0.32,
        }
    }
}

impl CarConfig {
    /// How high the body's centre sits when the car rests on flat ground.
    pub fn resting_height(&self) -> f32 {
        self.half_extents.to_glam().y.max(0.05)
    }

    /// Top speed in km/h, for the HUD.
    pub fn max_speed_kmh(&self) -> f32 {
        self.max_speed / KMH
    }
}

/// What the model wants the body to do this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CarCommand {
    /// Heading in radians about `+Y`.
    pub yaw: f32,
    /// World-space planar velocity `(x, z)`, in m/s.
    pub planar_velocity: glam::Vec2,
}

/// The handling model: heading and planar velocity, no world.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArcadeModel {
    yaw: f32,
    velocity: glam::Vec2,
    config: CarConfig,
    /// Sideways speed at the end of the last step, in m/s. The HUD's skid indicator and
    /// the phase-44 tyre smoke read this.
    lateral_speed: f32,
}

impl ArcadeModel {
    /// A stationary car facing `start.yaw`.
    pub fn new(start: StartPosition, config: CarConfig) -> Self {
        Self {
            yaw: start.yaw,
            velocity: glam::Vec2::ZERO,
            config,
            lateral_speed: 0.0,
        }
    }

    /// The tuning in use.
    pub fn config(&self) -> CarConfig {
        self.config
    }

    /// Heading in radians about `+Y`. `0` faces `+Z`, `+π/2` faces `+X`.
    pub fn yaw(&self) -> f32 {
        self.yaw
    }

    /// Unit forward vector on the ground.
    pub fn forward(&self) -> glam::Vec3 {
        glam::Vec3::new(self.yaw.sin(), 0.0, self.yaw.cos())
    }

    /// Speed along the car's own forward axis, in m/s. Negative in reverse.
    pub fn signed_speed(&self) -> f32 {
        self.velocity.dot(planar(self.forward()))
    }

    /// Speed over the ground, in m/s, always positive.
    pub fn speed(&self) -> f32 {
        self.velocity.length()
    }

    /// Speed for the HUD, in km/h.
    pub fn speed_kmh(&self) -> f32 {
        self.speed() / KMH
    }

    /// Sideways speed at the end of the last step, in m/s.
    pub fn lateral_speed(&self) -> f32 {
        self.lateral_speed
    }

    /// The simulated gear, `1..=6`, or `0` when in reverse.
    ///
    /// There is no gearbox: dividing the speed into six bands is what makes the number on
    /// the HUD move with the car, which is the only job it has.
    pub fn gear(&self) -> u8 {
        if self.signed_speed() < -0.3 {
            return 0;
        }
        let ratio = (self.speed() / self.config.max_speed).clamp(0.0, 1.0);
        ((ratio * 6.0).ceil() as u8).clamp(1, 6)
    }

    /// Replaces the model's velocity with what the physics world says the car is doing.
    ///
    /// Called once per frame, before [`ArcadeModel::step`].
    pub fn adopt_velocity(&mut self, world_velocity: Vec3) {
        let world = world_velocity.to_glam();
        if world.x.is_finite() && world.z.is_finite() {
            self.velocity = glam::Vec2::new(world.x, world.z);
        }
    }

    /// Places the model without touching its velocity, for a respawn.
    pub fn reset_heading(&mut self, yaw: f32) {
        if yaw.is_finite() {
            self.yaw = yaw;
        }
        self.velocity = glam::Vec2::ZERO;
        self.lateral_speed = 0.0;
    }

    /// Advances one frame and returns what the body should do.
    pub fn step(&mut self, input: CarInput, dt: f32) -> CarCommand {
        // A long frame (a breakpoint, a stalled window) must not teleport the car: the
        // model integrates, so it is clamped to the physics world's own frame cap.
        let dt = if dt.is_finite() {
            dt.clamp(0.0, 0.05)
        } else {
            0.0
        };

        let forward = planar(self.forward());
        let right = glam::Vec2::new(-forward.y, forward.x);
        let mut forward_speed = self.velocity.dot(forward);
        let mut lateral = self.velocity.dot(right);

        // ---- engine, brake and coast --------------------------------------
        let throttle = input.throttle.clamp(-1.0, 1.0);
        if throttle > 0.0 {
            if forward_speed < -0.1 {
                // Rolling backwards with the throttle on is a brake, not a gear change.
                forward_speed += self.config.brake * throttle * dt;
            } else {
                forward_speed += self.config.accel * throttle * dt;
            }
        } else if throttle < 0.0 {
            let amount = -throttle;
            if forward_speed > 0.1 {
                forward_speed -= self.config.brake * amount * dt;
            } else {
                // Reverse builds more slowly than forward, so the car cannot be used as
                // a rocket by tapping into reverse.
                forward_speed -= self.config.accel * 0.5 * amount * dt;
            }
        } else {
            let drop = self.config.drag * dt;
            forward_speed = if forward_speed.abs() <= drop {
                0.0
            } else {
                forward_speed - drop * forward_speed.signum()
            };
        }

        if input.handbrake {
            let drop = self.config.handbrake_brake * dt;
            forward_speed = if forward_speed.abs() <= drop {
                0.0
            } else {
                forward_speed - drop * forward_speed.signum()
            };
        }
        forward_speed = forward_speed.clamp(-self.config.reverse_speed, self.config.max_speed);

        // ---- grip ----------------------------------------------------------
        let grip = if input.handbrake {
            self.config.handbrake_grip
        } else {
            self.config.grip
        };
        // Exact exponential decay: the same slide at 30 and at 144 fps.
        lateral *= (-grip * dt).exp();
        self.lateral_speed = lateral;

        // Recompose along the **old** heading, then turn. Rotating the velocity with the
        // nose would mean the car could never slide: every degree of steering would be a
        // degree of travel. Keeping the velocity where it is and rotating the heading is
        // what turns part of the forward speed into the sideways speed that `grip` then
        // removes — and what makes the handbrake do anything at all.
        self.velocity = forward * forward_speed + right * lateral;

        // ---- steering ------------------------------------------------------
        // Authority rises from nothing at a standstill to full lock at walking pace, then
        // falls off with speed so a fast car is not twitchy.
        let speed_ratio = (forward_speed.abs() / self.config.max_speed).clamp(0.0, 1.0);
        let authority = (forward_speed.abs() / STEER_REFERENCE_SPEED).clamp(0.0, 1.0)
            * (1.0 - self.config.steer_falloff * speed_ratio);
        let steer = input.steer.clamp(-1.0, 1.0);
        // Positive steer is "right", and right is `-yaw` in this convention (see
        // `ArcadeModel::forward`). Reversing inverts it, as it does in a real car.
        self.yaw -= steer * self.config.steer_rate * authority * dt * forward_speed.signum();
        self.yaw = wrap_angle(self.yaw);

        CarCommand {
            yaw: self.yaw,
            planar_velocity: self.velocity,
        }
    }
}

/// Drops the Y and keeps `(x, z)`.
fn planar(vector: glam::Vec3) -> glam::Vec2 {
    glam::Vec2::new(vector.x, vector.z)
}

/// Wraps an angle into `-π..=π`, so a long race cannot drift into a loss of precision.
fn wrap_angle(angle: f32) -> f32 {
    if !angle.is_finite() {
        return 0.0;
    }
    let two_pi = std::f32::consts::TAU;
    let wrapped = angle.rem_euclid(two_pi);
    if wrapped > std::f32::consts::PI {
        wrapped - two_pi
    } else {
        wrapped
    }
}

/// What the car is doing, after a frame of physics.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CarState {
    /// Centre of the body, in world units.
    pub position: Vec3,
    /// Heading in radians about `+Y`.
    pub yaw: f32,
    /// Velocity in world units per second, including gravity.
    pub velocity: Vec3,
    /// Speed over the ground, for the HUD.
    pub speed_kmh: f32,
    /// The simulated gear, `0` for reverse.
    pub gear: u8,
    /// Sideways speed, in m/s. Above ~4 the car is sliding.
    pub lateral_speed: f32,
}

impl CarState {
    /// Whether the car is sliding sideways enough to leave tyre marks.
    pub fn is_sliding(&self) -> bool {
        self.lateral_speed.abs() > 4.0
    }

    /// This state's pose as a transform, for drawing the car.
    pub fn transform(&self) -> Transform3D {
        Transform3D::new(
            self.position,
            Quat::from_axis_angle(Vec3::UP, self.yaw),
            Vec3::new(1.0, 1.0, 1.0),
        )
    }
}

/// The car in the physics world.
#[derive(Debug, Clone, Copy)]
pub struct Car {
    body: BodyHandle,
    model: ArcadeModel,
}

impl Car {
    /// Creates the body (and its collider) and places it at `start`.
    ///
    /// The body is dynamic — gravity and walls are the solver's job — with rotations
    /// locked and continuous collision on: a 120 km/h box through a 1 m wall at 60 fps
    /// moves 55 cm per frame, which is enough to tunnel without CCD.
    pub fn spawn(
        world: &mut PhysicsWorld3D,
        start: StartPosition,
        config: CarConfig,
    ) -> Result<Self, PhysicsError> {
        let position = Vec3::new(
            start.position.to_glam().x,
            start.position.to_glam().y.max(config.resting_height()),
            start.position.to_glam().z,
        );
        let body = world.create_body(
            BodyDesc::dynamic(position)
                .with_rotation(Quat::from_axis_angle(Vec3::UP, start.yaw))
                .lock_rotations()
                .with_continuous_collision(),
        )?;
        let half = config.half_extents.to_glam();
        // **Frictionless**, and that matters more than it looks. The arcade model owns
        // every metre per second the car gains or loses; a collider with friction adds a
        // second, invisible braking force that is proportional to the car's weight. At
        // 0.2 against a 0.6 road it was around 5 m/s²: harmless under the 14 m/s² of
        // forward acceleration and fatal under the 7 m/s² of reverse, which is why the car
        // could reach 120 km/h forwards and only creep backwards with no steering
        // authority. With zero friction the solver only resolves *impacts*, which is
        // exactly the part of physics this game wants from it.
        world.attach_collider(
            body,
            ColliderShape::cuboid(half.x, half.y, half.z),
            0.0,
            0.0,
        )?;
        Ok(Self {
            body,
            model: ArcadeModel::new(start, config),
        })
    }

    /// The physics handle, for a caller that needs to query or move the body.
    pub fn body(&self) -> BodyHandle {
        self.body
    }

    /// The handling model, read-only: the HUD and the AI both use it.
    pub fn model(&self) -> &ArcadeModel {
        &self.model
    }

    /// The handling model, writable, for a respawn.
    pub fn model_mut(&mut self) -> &mut ArcadeModel {
        &mut self.model
    }

    /// Advances one frame: model, then body, then physics.
    ///
    /// One car, one call. A race with a field cannot use this for every car — that would
    /// step the world once per car, and four cars would run the physics at four times the
    /// frame rate. A field calls [`Car::apply`] for each car and `world.step` once; see
    /// [`crate::race::Field`].
    pub fn step(
        &mut self,
        world: &mut PhysicsWorld3D,
        input: CarInput,
        dt: f32,
    ) -> Result<CarState, PhysicsError> {
        self.apply(world, input, dt)?;
        world.step(dt)?;
        self.state(world)
    }

    /// Proposes this frame's motion to the world, without stepping it.
    ///
    /// The model integrates and the body is given the new pose and velocity; the solver has
    /// not run yet, so [`Car::state`] still reports where the car was. That split is what
    /// lets several cars share one world step: everybody proposes, the world decides once,
    /// everybody reads back.
    pub fn apply(
        &mut self,
        world: &mut PhysicsWorld3D,
        input: CarInput,
        dt: f32,
    ) -> Result<CarCommand, PhysicsError> {
        let velocity = world.velocity(self.body)?;
        self.model.adopt_velocity(velocity);

        let command = self.model.step(input, dt);

        world.set_rotation(self.body, Quat::from_axis_angle(Vec3::UP, command.yaw))?;
        // Gravity is the solver's to add; keeping the vertical speed the body already had
        // stops a car on a ramp from being pinned to the ground by its own engine.
        let gravity_speed = velocity.to_glam().y;
        world.set_velocity(
            self.body,
            Vec3::from_glam(glam::Vec3::new(
                command.planar_velocity.x,
                gravity_speed,
                command.planar_velocity.y,
            )),
        )?;
        Ok(command)
    }

    /// Where the car is now, without simulating.
    pub fn state(&self, world: &PhysicsWorld3D) -> Result<CarState, PhysicsError> {
        let position = world.position(self.body)?;
        let velocity = world.velocity(self.body)?;
        let over_ground = glam::Vec2::new(velocity.to_glam().x, velocity.to_glam().z).length();
        Ok(CarState {
            position,
            yaw: self.model.yaw(),
            velocity,
            speed_kmh: over_ground / KMH,
            gear: self.model.gear(),
            lateral_speed: self.model.lateral_speed(),
        })
    }

    /// Puts the car back on the road, stationary, facing `yaw`.
    ///
    /// `teleport` keeps the body's velocity, which is right for a checkpoint and wrong for
    /// a respawn: a car that teleports home at 90 km/h keeps driving.
    pub fn respawn(
        &mut self,
        world: &mut PhysicsWorld3D,
        position: Vec3,
        yaw: f32,
    ) -> Result<(), PhysicsError> {
        self.model.reset_heading(yaw);
        world.teleport(
            self.body,
            giochi_engine_3d::physics::BodyTransform::new(
                position,
                Quat::from_axis_angle(Vec3::UP, yaw),
            ),
        )?;
        world.set_velocity(self.body, Vec3::ZERO)?;
        world.set_angular_velocity(self.body, Vec3::ZERO)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn car(yaw: f32) -> ArcadeModel {
        ArcadeModel::new(
            StartPosition {
                position: Vec3::ZERO,
                yaw,
            },
            CarConfig::default(),
        )
    }

    fn throttle(amount: f32) -> CarInput {
        CarInput {
            throttle: amount,
            ..CarInput::NEUTRAL
        }
    }

    /// Runs the model at 60 Hz for `seconds`.
    fn hold(model: &mut ArcadeModel, input: CarInput, seconds: f32) {
        let steps = (seconds / (1.0 / 60.0)).round() as u32;
        for _ in 0..steps {
            model.step(input, 1.0 / 60.0);
        }
    }

    #[test]
    fn a_car_at_rest_does_not_move() {
        let mut model = car(0.0);
        let command = model.step(CarInput::NEUTRAL, 1.0 / 60.0);
        assert_eq!(command.planar_velocity, glam::Vec2::ZERO);
    }

    #[test]
    fn throttle_accelerates_along_the_heading() {
        let mut model = car(0.0);
        hold(&mut model, throttle(1.0), 1.0);
        let command = model.step(throttle(1.0), 1.0 / 60.0);
        assert!(command.planar_velocity.y > 10.0, "{command:?}");
        assert!(command.planar_velocity.x.abs() < 1e-4, "no sideways drift");
    }

    #[test]
    fn the_car_never_exceeds_its_top_speed() {
        let mut model = car(0.0);
        hold(&mut model, throttle(1.0), 20.0);
        assert!(model.speed() <= CarConfig::default().max_speed + 1e-3);
        assert!(
            (model.speed_kmh() - 120.0).abs() < 0.5,
            "{}",
            model.speed_kmh()
        );
    }

    #[test]
    fn releasing_the_throttle_coasts_to_a_stop() {
        let mut model = car(0.0);
        hold(&mut model, throttle(1.0), 2.0);
        assert!(model.speed() > 10.0);
        hold(&mut model, CarInput::NEUTRAL, 12.0);
        assert_eq!(model.speed(), 0.0, "drag brings it to rest, not to a crawl");
    }

    #[test]
    fn braking_stops_the_car_faster_than_coasting() {
        let mut braked = car(0.0);
        let mut coasted = car(0.0);
        hold(&mut braked, throttle(1.0), 2.0);
        hold(&mut coasted, throttle(1.0), 2.0);
        hold(&mut braked, throttle(-1.0), 0.5);
        hold(&mut coasted, CarInput::NEUTRAL, 0.5);
        assert!(braked.speed() < coasted.speed());
        assert!(
            braked.speed() >= 0.0,
            "braking does not reverse in half a second"
        );
    }

    #[test]
    fn holding_brake_from_a_standstill_reverses_to_the_reverse_limit() {
        let mut model = car(0.0);
        hold(&mut model, throttle(-1.0), 12.0);
        let speed = model.signed_speed();
        assert!(speed < -1.0, "it goes backwards: {speed}");
        assert!(
            speed >= -CarConfig::default().reverse_speed - 1e-3,
            "but not faster than the reverse limit: {speed}"
        );
        assert_eq!(model.gear(), 0, "reverse is not a gear");
    }

    #[test]
    fn steering_right_turns_the_nose_to_the_right() {
        // `+yaw` is +Z and `+X` is to the driver's left in this convention, so steering
        // right has to decrease the heading.
        let mut model = car(0.0);
        hold(&mut model, throttle(1.0), 1.0);
        let before = model.yaw();
        hold(
            &mut model,
            CarInput {
                throttle: 1.0,
                steer: 1.0,
                ..CarInput::NEUTRAL
            },
            0.5,
        );
        assert!(model.yaw() < before, "{} -> {}", before, model.yaw());
        // And the car really is heading that way, not just labelled that way.
        let command = model.step(CarInput::NEUTRAL, 0.0);
        assert!(command.planar_velocity.x < -1.0, "{command:?}");
    }

    #[test]
    fn steering_left_turns_the_nose_to_the_left() {
        let mut model = car(0.0);
        hold(&mut model, throttle(1.0), 1.0);
        hold(
            &mut model,
            CarInput {
                throttle: 1.0,
                steer: -1.0,
                ..CarInput::NEUTRAL
            },
            0.5,
        );
        let command = model.step(CarInput::NEUTRAL, 0.0);
        assert!(command.planar_velocity.x > 1.0, "{command:?}");
    }

    #[test]
    fn a_stationary_car_does_not_steer() {
        let mut model = car(0.0);
        hold(
            &mut model,
            CarInput {
                steer: 1.0,
                ..CarInput::NEUTRAL
            },
            1.0,
        );
        assert_eq!(model.yaw(), 0.0, "the wheels turn, the car does not");
    }

    #[test]
    fn steering_is_weaker_at_speed() {
        let mut slow = car(0.0);
        let mut fast = car(0.0);
        hold(&mut slow, throttle(0.2), 0.6);
        hold(&mut fast, throttle(1.0), 6.0);
        let slow_before = slow.yaw();
        let fast_before = fast.yaw();
        hold(
            &mut slow,
            CarInput {
                throttle: 0.2,
                steer: 1.0,
                ..CarInput::NEUTRAL
            },
            0.25,
        );
        hold(
            &mut fast,
            CarInput {
                throttle: 1.0,
                steer: 1.0,
                ..CarInput::NEUTRAL
            },
            0.25,
        );
        let slow_turn = (slow.yaw() - slow_before).abs();
        let fast_turn = (fast.yaw() - fast_before).abs();
        assert!(slow_turn > fast_turn, "{slow_turn} vs {fast_turn}");
    }

    #[test]
    fn the_handbrake_makes_the_car_slide_further() {
        let mut gripped = car(0.0);
        let mut sliding = car(0.0);
        for model in [&mut gripped, &mut sliding] {
            hold(model, throttle(1.0), 3.0);
        }
        // A hard right while at speed, with and without the handbrake.
        let turn = CarInput {
            throttle: 1.0,
            steer: 1.0,
            ..CarInput::NEUTRAL
        };
        hold(&mut gripped, turn, 0.5);
        hold(
            &mut sliding,
            CarInput {
                handbrake: true,
                ..turn
            },
            0.5,
        );
        assert!(
            sliding.lateral_speed().abs() > gripped.lateral_speed().abs(),
            "handbrake {} vs grip {}",
            sliding.lateral_speed(),
            gripped.lateral_speed()
        );
    }

    #[test]
    fn the_handbrake_slows_the_car_down() {
        let mut model = car(0.0);
        hold(&mut model, throttle(1.0), 2.0);
        let before = model.speed();
        hold(
            &mut model,
            CarInput {
                handbrake: true,
                ..CarInput::NEUTRAL
            },
            0.5,
        );
        assert!(
            model.speed() < before - 2.0,
            "{} -> {}",
            before,
            model.speed()
        );
    }

    #[test]
    fn the_model_is_frame_rate_independent() {
        let mut sixty = car(0.0);
        let mut one_forty_four = car(0.0);
        for _ in 0..60 {
            sixty.step(throttle(1.0), 1.0 / 60.0);
        }
        for _ in 0..144 {
            one_forty_four.step(throttle(1.0), 1.0 / 144.0);
        }
        // Both integrated one second of full throttle.
        assert!(
            (sixty.speed() - one_forty_four.speed()).abs() < 0.5,
            "{} vs {}",
            sixty.speed(),
            one_forty_four.speed()
        );
    }

    #[test]
    fn a_very_long_frame_is_clamped_instead_of_teleporting_the_car() {
        let mut model = car(0.0);
        let command = model.step(throttle(1.0), 10.0);
        // 0.05 s of acceleration, not 10 s of it.
        assert!(command.planar_velocity.length() < 1.0, "{command:?}");
    }

    #[test]
    fn a_non_finite_delta_changes_nothing() {
        let mut model = car(0.0);
        let command = model.step(throttle(1.0), f32::NAN);
        assert_eq!(command.planar_velocity, glam::Vec2::ZERO);
        assert_eq!(command.yaw, 0.0);
    }

    #[test]
    fn the_heading_is_wrapped_into_one_turn() {
        let mut model = car(0.0);
        hold(&mut model, throttle(1.0), 2.0);
        for _ in 0..400 {
            model.step(
                CarInput {
                    throttle: 1.0,
                    steer: 1.0,
                    ..CarInput::NEUTRAL
                },
                1.0 / 60.0,
            );
        }
        assert!(model.yaw().abs() <= std::f32::consts::PI, "{}", model.yaw());
    }

    #[test]
    fn adopting_the_world_velocity_makes_a_wall_stop_the_car() {
        let mut model = car(0.0);
        hold(&mut model, throttle(1.0), 3.0);
        assert!(model.speed() > 10.0);
        // The solver reports the car stopped against a wall.
        model.adopt_velocity(Vec3::ZERO);
        assert_eq!(model.speed(), 0.0);
        let command = model.step(throttle(1.0), 1.0 / 60.0);
        assert!(
            command.planar_velocity.length() < 1.0,
            "one frame of throttle after the hit is not a launch: {command:?}"
        );
    }

    #[test]
    fn a_non_finite_world_velocity_is_ignored() {
        let mut model = car(0.0);
        hold(&mut model, throttle(1.0), 1.0);
        let speed = model.speed();
        model.adopt_velocity(Vec3::new(f32::NAN, 0.0, 0.0));
        assert!((model.speed() - speed).abs() < 1e-3);
    }

    #[test]
    fn the_gear_climbs_with_speed_and_floors_at_one() {
        let config = CarConfig::default();
        let mut model = ArcadeModel::new(StartPosition::default(), config);
        assert_eq!(model.gear(), 1, "a car at rest is in first");
        hold(&mut model, throttle(1.0), 30.0);
        assert_eq!(model.gear(), 6, "at top speed it is in sixth");
    }

    #[test]
    fn the_reset_puts_the_heading_back_and_stops_the_car() {
        let mut model = car(0.0);
        hold(&mut model, throttle(1.0), 1.0);
        model.reset_heading(std::f32::consts::FRAC_PI_2);
        assert_eq!(model.speed(), 0.0);
        assert!((model.yaw() - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
    }

    #[test]
    fn a_sliding_car_reports_it() {
        let mut model = car(0.0);
        hold(&mut model, throttle(1.0), 3.0);
        hold(
            &mut model,
            CarInput {
                throttle: 1.0,
                steer: 1.0,
                handbrake: true,
                pause: false,
            },
            0.4,
        );
        let state = CarState {
            position: Vec3::ZERO,
            yaw: model.yaw(),
            velocity: Vec3::ZERO,
            speed_kmh: model.speed_kmh(),
            gear: model.gear(),
            lateral_speed: model.lateral_speed(),
        };
        assert!(state.is_sliding() || state.lateral_speed.abs() > 1.0);
    }

    #[test]
    fn the_car_faces_the_way_the_start_position_says() {
        let model = ArcadeModel::new(
            StartPosition {
                position: Vec3::ZERO,
                yaw: std::f32::consts::FRAC_PI_2,
            },
            CarConfig::default(),
        );
        let forward = model.forward();
        assert!((forward.x - 1.0).abs() < 1e-6, "facing +X: {forward:?}");
        assert!(forward.z.abs() < 1e-6);
    }

    #[test]
    fn the_resting_height_is_half_the_body() {
        let config = CarConfig::default();
        assert!((config.resting_height() - 0.35).abs() < 1e-6);
        assert!((config.max_speed_kmh() - 120.0).abs() < 1e-3);
    }
}

#[cfg(test)]
mod reverse_probe {
    use super::*;

    #[test]
    fn reversing_with_the_wheel_turned_changes_the_heading() {
        let mut model = ArcadeModel::new(StartPosition::default(), CarConfig::default());
        for _ in 0..60 {
            model.step(
                CarInput {
                    throttle: -1.0,
                    steer: 1.0,
                    ..CarInput::NEUTRAL
                },
                1.0 / 60.0,
            );
        }
        eprintln!(
            "after reversing with steer +1: yaw {:.3}, signed speed {:.3}",
            model.yaw(),
            model.signed_speed()
        );
        assert!(
            model.yaw().abs() > 0.05,
            "the nose never turned: {}",
            model.yaw()
        );
    }
}
