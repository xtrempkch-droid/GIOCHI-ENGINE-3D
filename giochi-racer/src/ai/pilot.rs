//! The controller: pure pursuit along a [`Route`], from a [`CarState`] to a [`CarInput`].
//!
//! This is the half of an opponent that has no car in it, and it is public because the
//! player's car in `tests/race_field.rs` needs one: a field test with a player who cannot
//! steer is a field test with three opponents on an empty road, which is exactly the case
//! that was already covered.
//!
//! # Skill, and what it does not mean
//!
//! `skill` multiplies the line's target speed (`1.0` = exactly what the circuit author asked
//! for). It does **not** touch the steering gain, the grip or the physics: an opponent with
//! `skill = 1.1` is not a better driver, it is a car willing to carry more speed into the
//! same corner — which is the one difference that makes a race feel raced without making it
//! unfair in a way the player cannot see.
//!
//! # The unstick rule, and why it is bounded
//!
//! A car wedged against a barrier is not a steering problem: no amount of lock moves it. The
//! controller reverses for a moment, turning the nose the other way, then tries again — the
//! same recovery `tests/lap_logic.rs` uses. That is bounded in time, and after
//! [`Pilot::MAX_RECOVERIES`] failed attempts the pilot stops trying and says so, because a
//! race has to finish even if one car cannot.

use giochi_engine_3d::math3d::Vec3;

use crate::ai::waypoint::Route;
use crate::vehicle::car::{CarState, KMH};
use crate::vehicle::input::CarInput;

/// Keeps a skill factor inside a range a car can actually drive.
///
/// Below `0.5` the car crawls and the race is a solo time trial; above `1.2` the line's
/// corner speeds become untenable and it spends the race in the barrier. A non-finite value
/// falls back to the authored pace rather than poisoning the arithmetic for the rest of the
/// race.
pub fn clamp_skill(skill: f32) -> f32 {
    if skill.is_finite() {
        skill.clamp(0.5, 1.2)
    } else {
        1.0
    }
}

/// Wraps an angle into `-π..=π`.
pub fn wrap(angle: f32) -> f32 {
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

/// A driver without a car: where to aim, and how hard to press.
#[derive(Debug, Clone)]
pub struct Pilot {
    route: Route,
    /// Multiplies the line's target speed. `1.0` is the authored pace.
    skill: f32,
    /// A fixed per-driver offset, so two cars of equal skill do not trace the same line.
    wander_phase: f32,
    /// Metres to one side of the line, positive right of it. This driver's lane.
    lane: f32,
    /// Seconds this driver has been racing, for the wander.
    elapsed: f32,
    /// Seconds spent below walking pace while aiming at this point.
    stuck: f32,
    /// Seconds of reverse left to unwind a stuck car.
    recover: f32,
    /// Failed reversals since the car last moved properly, for the bounded rescue.
    recoveries: u32,
}

impl Pilot {
    /// How many failed reversals before the pilot asks for a rescue.
    pub const MAX_RECOVERIES: u32 = 3;

    /// How long the car may be below walking pace before reversing, in seconds.
    pub const STUCK_SECONDS: f32 = 0.8;

    /// How long a reversal lasts, in seconds.
    pub const RECOVER_SECONDS: f32 = 2.0;

    /// How fast the car must be going for a reversal to count as having worked, in km/h.
    pub const RECOVERED_KMH: f32 = 20.0;

    /// How much of the target speed the heading error may take away, `0.0..=1.0`.
    ///
    /// At a right-angle error the driver wants about half the straight-line speed. Any more
    /// and the car brakes to a stop in every corner; any less and it arrives too fast to
    /// turn. Measured on the three shipped circuits.
    pub const ERROR_PENALTY: f32 = 0.55;

    /// How hard the pilot corrects heading: `steer = -error * gain`.
    pub const STEER_GAIN: f32 = 1.8;

    /// How hard the pilot corrects speed: `throttle = (target - speed) * gain`.
    pub const THROTTLE_GAIN: f32 = 1.4;

    /// The size of the per-driver wobble, as a fraction of the target speed.
    pub const WANDER: f32 = 0.03;

    /// How far to one side of the line a driver may prefer to sit, in metres.
    ///
    /// The road is 10 m wide and the car 1.9 m, so 2.5 m still leaves 2.5 m to the barrier
    /// when the car is exactly on its lane and the line is exactly in the middle.
    pub const LANE_LIMIT: f32 = 2.5;

    /// A pilot on `route`, at `skill`, wobbling on `wander_phase`, holding no lane.
    pub fn new(route: Route, skill: f32, wander_phase: f32) -> Self {
        Self {
            route,
            skill: clamp_skill(skill),
            wander_phase: if wander_phase.is_finite() {
                wander_phase
            } else {
                0.0
            },
            lane: 0.0,
            elapsed: 0.0,
            stuck: 0.0,
            recover: 0.0,
            recoveries: 0,
        }
    }

    /// The same pilot, preferring a lane `metres` to the right of the line.
    ///
    /// Three opponents aiming at one line converge on it and shunt each other; three
    /// opponents each a lane and a half apart race each other. The lane is a *preference*:
    /// it offsets the aim point perpendicular to the line, so the car still steers by the
    /// same pure pursuit and still takes the corner it can see.
    pub fn with_lane(mut self, metres: f32) -> Self {
        self.lane = if metres.is_finite() {
            metres.clamp(-Self::LANE_LIMIT, Self::LANE_LIMIT)
        } else {
            0.0
        };
        self
    }

    /// The line being followed.
    pub fn route(&self) -> &Route {
        &self.route
    }

    /// The line, for a rescue that has to rewind it.
    pub fn route_mut(&mut self) -> &mut Route {
        &mut self.route
    }

    /// This pilot's pace multiplier.
    pub fn skill(&self) -> f32 {
        self.skill
    }

    /// This pilot's lane offset, in metres, positive to the right of the line.
    pub fn lane(&self) -> f32 {
        self.lane
    }

    /// Whether the pilot is currently reversing out of a barrier.
    pub fn is_recovering(&self) -> bool {
        self.recover > 0.0
    }

    /// Whether this pilot has been stuck enough times to need putting back on the line.
    pub fn needs_rescue(&self) -> bool {
        self.recoveries >= Self::MAX_RECOVERIES
    }

    /// Puts the counters back to the start of a race.
    pub fn restart(&mut self) {
        self.route.reset();
        self.elapsed = 0.0;
        self.stuck = 0.0;
        self.recover = 0.0;
        self.recoveries = 0;
    }

    /// Forgets the recovery history, after the car has been put back on the line.
    pub fn after_rescue(&mut self) {
        self.restart();
    }

    /// The input for this frame, given where the car is and how fast it is going.
    ///
    /// The caller steps the world; this produces exactly the [`CarInput`] a player would.
    pub fn drive(&mut self, state: &CarState, dt: f32) -> CarInput {
        let dt = if dt.is_finite() {
            dt.clamp(0.0, 0.05)
        } else {
            0.0
        };
        self.elapsed += dt;

        let Some(aim) = self.route.look_ahead(state.position) else {
            return CarInput::NEUTRAL;
        };

        // The aim point, moved into this driver's lane. Perpendicular to the *line*, so on
        // a straight the car tracks parallel to it rather than aiming askew.
        let aim_point = Vec3::new(
            aim.point.x + aim.heading.z * self.lane,
            aim.point.y,
            aim.point.z - aim.heading.x * self.lane,
        );
        let car = state.position.to_glam();
        let bearing = (aim_point.to_glam().x - car.x).atan2(aim_point.to_glam().z - car.z);
        let error = wrap(bearing - state.yaw);

        // A wall is not a steering problem. Reverse out, turning the nose the other way.
        if self.recover > 0.0 {
            self.recover -= dt;
            let steer = if self.recover > Self::RECOVER_SECONDS * 0.6 {
                1.0
            } else {
                -1.0
            };
            return CarInput {
                throttle: -1.0,
                steer,
                handbrake: false,
                pause: false,
            };
        }
        if state.speed_kmh < 2.0 {
            self.stuck += dt;
            if self.stuck > Self::STUCK_SECONDS {
                self.stuck = 0.0;
                self.recover = Self::RECOVER_SECONDS;
                self.recoveries += 1;
            }
        } else {
            self.stuck = 0.0;
            // Moving again: the reversal worked, so the next one gets its full budget.
            if self.recoveries > 0 && state.speed_kmh > Self::RECOVERED_KMH {
                self.recoveries = 0;
            }
        }

        // Steering is a rate, not an angle, so the target speed is what pays for the turn.
        let wander = (self.wander_phase + self.elapsed * 0.7).sin() * Self::WANDER;
        let target = aim.speed
            * self.skill
            * (1.0 - Self::ERROR_PENALTY * (error.abs() / std::f32::consts::PI).min(1.0))
            * (1.0 + wander);
        let speed = state.speed_kmh * KMH;

        CarInput {
            throttle: ((target - speed) * Self::THROTTLE_GAIN).clamp(-1.0, 1.0),
            steer: (-error * Self::STEER_GAIN).clamp(-1.0, 1.0),
            handbrake: false,
            pause: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::Waypoint;

    fn waypoint(index: usize, x: f32, z: f32, speed: f32) -> Waypoint {
        Waypoint {
            index,
            position: Vec3::new(x, 0.5, z),
            speed,
        }
    }

    /// A 100 m square, driven anticlockwise from the south-west corner, at 20 m/s.
    fn square() -> Route {
        Route::new(vec![
            waypoint(0, 0.0, 0.0, 20.0),
            waypoint(1, 100.0, 0.0, 20.0),
            waypoint(2, 100.0, 100.0, 20.0),
            waypoint(3, 0.0, 100.0, 20.0),
        ])
    }

    fn state(x: f32, z: f32, yaw: f32, speed_kmh: f32) -> CarState {
        CarState {
            position: Vec3::new(x, 0.5, z),
            yaw,
            velocity: Vec3::new(
                yaw.sin() * speed_kmh * KMH,
                0.0,
                yaw.cos() * speed_kmh * KMH,
            ),
            speed_kmh,
            gear: 2,
            lateral_speed: 0.0,
        }
    }

    #[test]
    fn skill_is_clamped_to_a_drivable_range() {
        assert!((clamp_skill(0.1) - 0.5).abs() < 1e-6);
        assert!((clamp_skill(9.0) - 1.2).abs() < 1e-6);
        assert!((clamp_skill(0.85) - 0.85).abs() < 1e-6);
        assert!((clamp_skill(f32::NAN) - 1.0).abs() < 1e-6);
        assert!((clamp_skill(f32::INFINITY) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn wrap_keeps_angles_in_one_turn() {
        for angle in [-7.0, -3.5, 3.5, 7.0] {
            assert!(wrap(angle).abs() <= std::f32::consts::PI + 1e-5, "{angle}");
        }
        assert!(wrap(std::f32::consts::TAU).abs() < 1e-5);
        assert!(wrap(f32::NAN).abs() < 1e-5);
    }

    #[test]
    fn wrap_of_half_a_turn_stays_half_a_turn() {
        // The bearing error of a car pointing the wrong way is half a turn, and wrapping
        // must not turn that into "no error at all".
        for angle in [std::f32::consts::PI, -std::f32::consts::PI] {
            assert!((wrap(angle).abs() - std::f32::consts::PI).abs() < 1e-5);
        }
    }

    #[test]
    fn a_lane_is_clamped_inside_the_road() {
        assert!((Pilot::new(square(), 1.0, 0.0).with_lane(9.0).lane() - 2.5).abs() < 1e-6);
        assert!((Pilot::new(square(), 1.0, 0.0).with_lane(-9.0).lane() + 2.5).abs() < 1e-6);
        assert_eq!(
            Pilot::new(square(), 1.0, 0.0).with_lane(f32::NAN).lane(),
            0.0
        );
    }

    #[test]
    fn a_car_pointing_at_the_aim_point_steers_straight() {
        let mut pilot = Pilot::new(square(), 1.0, 0.0);
        // On the first leg facing `+X`, which is where the line goes.
        let input = pilot.drive(
            &state(10.0, 0.0, std::f32::consts::FRAC_PI_2, 40.0),
            1.0 / 60.0,
        );
        assert!(input.steer.abs() < 0.05, "steer {}", input.steer);
        assert!(input.throttle > 0.5, "throttle {}", input.throttle);
    }

    #[test]
    fn a_car_pointing_at_the_barrier_steers_away_from_it() {
        let mut pilot = Pilot::new(square(), 1.0, 0.0);
        // Facing `+Z` while the line runs `+X`: a quarter turn to the driver's left.
        let input = pilot.drive(&state(10.0, 0.0, 0.0, 40.0), 1.0 / 60.0);
        assert!(input.steer.abs() > 0.5, "steer {}", input.steer);
    }

    #[test]
    fn a_car_too_fast_for_the_corner_brakes() {
        let mut pilot = Pilot::new(square(), 1.0, 0.0);
        // A quarter turn to make, at 120 km/h: the error penalty takes the 20 m/s target
        // down to about 14, so the pilot has to slow down rather than press on.
        let input = pilot.drive(&state(10.0, 0.0, 0.0, 120.0), 1.0 / 60.0);
        assert!(
            input.throttle < 0.0,
            "the car should be slowing for the turn, throttle {}",
            input.throttle
        );
    }

    #[test]
    fn a_car_that_has_stopped_reverses_out() {
        let mut pilot = Pilot::new(square(), 1.0, 0.0);
        let stopped = state(10.0, 0.0, std::f32::consts::FRAC_PI_2, 0.0);
        let mut saw_reverse = false;
        for _ in 0..120 {
            let input = pilot.drive(&stopped, 1.0 / 60.0);
            if input.throttle < 0.0 {
                saw_reverse = true;
                break;
            }
        }
        assert!(saw_reverse, "a stopped car never reversed");
        assert!(pilot.is_recovering(), "the pilot is not reversing");
    }

    #[test]
    fn a_pilot_that_cannot_unstick_asks_for_a_rescue() {
        let mut pilot = Pilot::new(square(), 1.0, 0.0);
        let stopped = state(10.0, 0.0, std::f32::consts::FRAC_PI_2, 0.0);
        // Twenty seconds of being stuck: far more than the three reversals it takes.
        for _ in 0..1200 {
            pilot.drive(&stopped, 1.0 / 60.0);
        }
        assert!(pilot.needs_rescue(), "the pilot never asked for a rescue");
    }

    #[test]
    fn a_rescue_forgets_the_history() {
        let mut pilot = Pilot::new(square(), 1.0, 0.0);
        let stopped = state(10.0, 0.0, std::f32::consts::FRAC_PI_2, 0.0);
        for _ in 0..1200 {
            pilot.drive(&stopped, 1.0 / 60.0);
        }
        assert!(pilot.needs_rescue());
        pilot.after_rescue();
        assert!(!pilot.needs_rescue());
        assert_eq!(pilot.route().cursor(), 0);
    }

    #[test]
    fn two_lanes_aim_at_two_different_points() {
        let mut left = Pilot::new(square(), 1.0, 0.0).with_lane(-2.0);
        let mut right = Pilot::new(square(), 1.0, 0.0).with_lane(2.0);
        let car = state(10.0, 0.0, std::f32::consts::FRAC_PI_2, 20.0);
        // The line runs `+X` at `z = 0`, so each lane asks for a slightly different heading.
        let difference =
            (left.drive(&car, 1.0 / 60.0).steer - right.drive(&car, 1.0 / 60.0).steer).abs();
        assert!(difference > 0.05, "the lanes aimed at the same place");
    }
}
