//! The ball-balance physics, with no renderer anywhere near it.
//!
//! Kept in the library rather than in the example for one reason: the mapping between the
//! stick and the direction the ball rolls is a thing that can be *wrong*, and being wrong
//! there makes the game unplayable in a way that is easy to miss on a machine where nobody
//! is holding the stick. Tests pin the direction, the slope, the edge and the reset.
//!
//! The whole simulation is 2D on the platform's own surface. Gravity is projected into the
//! platform's tangent basis once per step and integrated there, so tilting the board never
//! moves the ball sideways by accident the way re-projecting a world-space position onto a
//! moving plane would.

use crate::math3d::{Quat, Vec3};

/// Tuning for [`BalanceGame`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BalanceConfig {
    /// Length of one side of the square platform, world units.
    pub platform_size: f32,
    /// Radius of the ball.
    pub ball_radius: f32,
    /// Tilt at full stick deflection, radians.
    pub max_tilt: f32,
    /// Acceleration due to gravity, m/s².
    pub gravity: f32,
    /// How far past the edge the ball must travel before it counts as fallen.
    pub fall_margin: f32,
    /// Velocity retained per step. 1.0 is frictionless.
    pub damping: f32,
    /// Seconds the fall animation plays before the ball respawns.
    pub fall_duration: f32,
    /// Physics step. Input is sub-stepped to this, so a slow frame does not change the
    /// result and the game behaves the same at 30 and at 60 fps.
    pub fixed_step: f32,
}

impl Default for BalanceConfig {
    fn default() -> Self {
        Self {
            platform_size: 6.5,
            ball_radius: 0.55,
            // 22 degrees. Steep enough that a held stick commits the ball, shallow enough
            // that small corrections are usable.
            max_tilt: 0.38,
            gravity: 9.81,
            fall_margin: 0.05,
            // Deliberately high: a ball with a lot of momentum is one you have to correct
            // early, which is what makes it a usable latency test.
            damping: 0.995,
            fall_duration: 0.75,
            fixed_step: 1.0 / 120.0,
        }
    }
}

impl BalanceConfig {
    /// Half the platform's side length.
    pub fn half_extent(&self) -> f32 {
        self.platform_size * 0.5
    }
}

/// Where the ball is and how it is moving, in platform-local coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ball {
    x: f32,
    z: f32,
    vx: f32,
    vz: f32,
    spin: Quat,
}

impl Ball {
    /// A ball resting at the middle of the platform.
    pub fn at_centre() -> Self {
        Self {
            x: 0.0,
            z: 0.0,
            vx: 0.0,
            vz: 0.0,
            spin: Quat::IDENTITY,
        }
    }

    /// Position on the platform surface, in platform-local units.
    pub fn position(&self) -> (f32, f32) {
        (self.x, self.z)
    }

    /// Velocity along the platform surface, local units per second.
    pub fn velocity(&self) -> (f32, f32) {
        (self.vx, self.vz)
    }

    /// Accumulated rolling rotation, for drawing only.
    pub fn spin(&self) -> Quat {
        self.spin
    }

    fn step(&mut self, tilt: Quat, config: &BalanceConfig, delta: f32) {
        let gravity = Vec3::new(0.0, -config.gravity, 0.0);
        let tangent_x = tilt.rotate_vec3(Vec3::new(1.0, 0.0, 0.0));
        let tangent_z = tilt.rotate_vec3(Vec3::new(0.0, 0.0, 1.0));

        // A solid sphere rolling without slipping accelerates at 5/7 of the tangential
        // component of gravity. Using the full value makes it feel like ice.
        const ROLLING: f32 = 5.0 / 7.0;
        self.vx += ROLLING * dot(gravity, tangent_x) * delta;
        self.vz += ROLLING * dot(gravity, tangent_z) * delta;

        // Per-second damping, so the feel does not change with the step size.
        let retained = config.damping.powf(delta * 60.0);
        self.vx *= retained;
        self.vz *= retained;

        self.x += self.vx * delta;
        self.z += self.vz * delta;

        self.accumulate_spin(config, delta);
    }

    fn accumulate_spin(&mut self, config: &BalanceConfig, delta: f32) {
        let speed = (self.vx * self.vx + self.vz * self.vz).sqrt();
        if speed < 1e-4 {
            return;
        }

        let axis = Vec3::new(self.vz, 0.0, -self.vx).normalized();
        let angle = speed * delta / config.ball_radius;
        self.spin = Quat::from_axis_angle(axis, angle) * self.spin;
    }

    /// Whether the ball has rolled off the platform.
    pub fn has_fallen(&self, config: &BalanceConfig) -> bool {
        let limit = config.half_extent() + config.fall_margin;
        self.x.abs() > limit || self.z.abs() > limit
    }

    /// `0.0` at the centre, `1.0` at the rim. Used to warn the player.
    pub fn edge_proximity(&self, config: &BalanceConfig) -> f32 {
        (self.x.abs().max(self.z.abs()) / config.half_extent()).clamp(0.0, 1.0)
    }

    /// Centre of the ball in world space, resting on the tilted platform.
    ///
    /// `drop` lowers it for the fall animation.
    pub fn world_position(&self, tilt: Quat, drop: f32, config: &BalanceConfig) -> Vec3 {
        let tangent_x = tilt.rotate_vec3(Vec3::new(1.0, 0.0, 0.0));
        let tangent_z = tilt.rotate_vec3(Vec3::new(0.0, 0.0, 1.0));
        let normal = tilt.rotate_vec3(Vec3::UP);

        Vec3::new(
            tangent_x.x * self.x + tangent_z.x * self.z + normal.x * config.ball_radius,
            tangent_x.y * self.x + tangent_z.y * self.z + normal.y * config.ball_radius - drop,
            tangent_x.z * self.x + tangent_z.z * self.z + normal.z * config.ball_radius,
        )
    }
}

fn dot(a: Vec3, b: Vec3) -> f32 {
    a.x * b.x + a.y * b.y + a.z * b.z
}

/// The tilt a stick position asks for.
///
/// `stick_x` is positive to the right, `stick_z` positive toward the viewer. Both move the
/// ball in their own direction, which is the mapping a player assumes without being told.
///
/// The signs are not symmetric and that is the point: a rotation about +Z by a positive
/// angle raises the +X side, so rolling right needs the *negative* angle, while a rotation
/// about +X by a positive angle already lowers the +Z side. Getting this wrong inverts one
/// axis and the game feels like it is fighting you.
pub fn tilt_from_input(stick_x: f32, stick_z: f32, config: &BalanceConfig) -> Quat {
    let roll = Quat::from_axis_angle(Vec3::new(0.0, 0.0, 1.0), -stick_x * config.max_tilt);
    let pitch = Quat::from_axis_angle(Vec3::new(1.0, 0.0, 0.0), stick_z * config.max_tilt);
    roll * pitch
}

/// State of one round.
#[derive(Debug, Clone)]
pub struct BalanceGame {
    config: BalanceConfig,
    ball: Ball,
    tilt: Quat,
    accumulator: f32,
    fall_timer: f32,
    falls: u32,
    survived: f32,
    best: f32,
}

impl BalanceGame {
    /// Starts a round with the ball at the centre.
    pub fn new(config: BalanceConfig) -> Self {
        Self {
            config,
            ball: Ball::at_centre(),
            tilt: Quat::IDENTITY,
            accumulator: 0.0,
            fall_timer: 0.0,
            falls: 0,
            survived: 0.0,
            best: 0.0,
        }
    }

    /// The tuning in use.
    pub fn config(&self) -> &BalanceConfig {
        &self.config
    }

    /// Advances the round by one frame.
    ///
    /// `stick` is the raw input, `delta` the frame time. The input is integrated in fixed
    /// steps, so a frame that took twice as long produces the same result as two normal
    /// frames rather than a different one.
    pub fn update(&mut self, stick: (f32, f32), delta: f32) {
        if self.fall_timer > 0.0 {
            self.fall_timer -= delta;
            if self.fall_timer <= 0.0 {
                self.respawn();
            }
            return;
        }

        self.tilt = tilt_from_input(stick.0, stick.1, &self.config);

        // A long frame must not become one enormous physics step; it becomes many small
        // ones. The clamp is the safety valve for a frame that took absurdly long, such as
        // a console that was suspended.
        self.accumulator = (self.accumulator + delta).min(0.25);
        while self.accumulator >= self.config.fixed_step {
            self.ball
                .step(self.tilt, &self.config, self.config.fixed_step);
            self.accumulator -= self.config.fixed_step;
            self.survived += self.config.fixed_step;
        }

        if self.ball.has_fallen(&self.config) {
            self.falls += 1;
            if self.survived > self.best {
                self.best = self.survived;
            }
            self.fall_timer = self.config.fall_duration;
        }
    }

    /// Puts the ball back at the centre and starts timing again.
    pub fn respawn(&mut self) {
        self.ball = Ball::at_centre();
        self.accumulator = 0.0;
        self.fall_timer = 0.0;
        self.survived = 0.0;
    }

    /// The ball.
    pub fn ball(&self) -> &Ball {
        &self.ball
    }

    /// Current tilt of the platform.
    pub fn tilt(&self) -> Quat {
        self.tilt
    }

    /// Whether the fall animation is playing.
    pub fn is_falling(&self) -> bool {
        self.fall_timer > 0.0
    }

    /// How far below the surface to draw the ball. Rises quadratically so the fall
    /// accelerates instead of dropping at a constant speed.
    pub fn fall_drop(&self) -> f32 {
        if self.fall_timer <= 0.0 {
            return 0.0;
        }
        let progress = 1.0 - (self.fall_timer / self.config.fall_duration);
        progress * progress * 6.0
    }

    /// How many times the ball has fallen.
    pub fn falls(&self) -> u32 {
        self.falls
    }

    /// Seconds the current round has lasted.
    pub fn survived(&self) -> f32 {
        self.survived
    }

    /// Longest round so far.
    pub fn best(&self) -> f32 {
        self.best.max(self.survived)
    }

    /// A deterministic input script, for CI and for measuring without a human.
    ///
    /// Left, right, near, far, one second each, repeating. Every direction appears, so a
    /// run through the whole script visits all four quadrants of the platform.
    pub fn scripted_input(frame: u64) -> (f32, f32) {
        match (frame / 60) % 4 {
            0 => (1.0, 0.0),
            1 => (-1.0, 0.0),
            2 => (0.0, 1.0),
            _ => (0.0, -1.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> BalanceConfig {
        BalanceConfig::default()
    }

    /// Runs `seconds` of simulation at 60 fps with a constant stick position.
    fn run(game: &mut BalanceGame, stick: (f32, f32), seconds: f32) {
        let frames = (seconds * 60.0).round() as u32;
        for _ in 0..frames {
            game.update(stick, 1.0 / 60.0);
        }
    }

    #[test]
    fn a_centred_stick_leaves_the_ball_alone() {
        let mut game = BalanceGame::new(config());
        run(&mut game, (0.0, 0.0), 2.0);

        let (x, z) = game.ball().position();
        assert!(x.abs() < 1e-6, "ball drifted along x: {x}");
        assert!(z.abs() < 1e-6, "ball drifted along z: {z}");
        assert!(!game.is_falling());
        assert_eq!(game.falls(), 0);
    }

    #[test]
    fn pushing_right_rolls_the_ball_right() {
        let mut game = BalanceGame::new(config());
        run(&mut game, (1.0, 0.0), 0.5);

        let (x, _) = game.ball().position();
        assert!(x > 0.1, "ball should have rolled toward +x, it is at {x}");
    }

    #[test]
    fn pushing_left_rolls_the_ball_left() {
        let mut game = BalanceGame::new(config());
        run(&mut game, (-1.0, 0.0), 0.5);

        let (x, _) = game.ball().position();
        assert!(x < -0.1, "ball should have rolled toward -x, it is at {x}");
    }

    #[test]
    fn pushing_away_rolls_the_ball_toward_the_viewer() {
        let mut game = BalanceGame::new(config());
        run(&mut game, (0.0, 1.0), 0.5);

        let (_, z) = game.ball().position();
        assert!(z > 0.1, "ball should have rolled toward +z, it is at {z}");
    }

    #[test]
    fn the_two_axes_are_independent() {
        let mut right = BalanceGame::new(config());
        let mut near = BalanceGame::new(config());
        run(&mut right, (1.0, 0.0), 0.5);
        run(&mut near, (0.0, 1.0), 0.5);

        let (rx, rz) = right.ball().position();
        let (nzx, nz) = near.ball().position();

        assert!(
            rz.abs() < 1e-6,
            "pushing right moved the ball along z: {rz}"
        );
        assert!(
            nzx.abs() < 1e-6,
            "pushing near moved the ball along x: {nzx}"
        );
        assert!(
            (rx - nz).abs() < 1e-4,
            "the axes should be symmetric: {rx} vs {nz}"
        );
    }

    #[test]
    fn the_ball_accelerates_rather_than_jumping() {
        let mut game = BalanceGame::new(config());

        run(&mut game, (1.0, 0.0), 0.25);
        let early = game.ball().position().0;
        run(&mut game, (1.0, 0.0), 0.25);
        let late = game.ball().position().0 - early;

        assert!(
            late > early,
            "the second quarter second ({late}) should cover more ground than the first ({early})"
        );
    }

    #[test]
    fn a_ball_past_the_edge_falls_and_the_round_restarts() {
        let mut config = config();
        // A tiny platform, so the ball reaches the edge quickly and the test stays fast.
        config.platform_size = 0.4;
        let mut game = BalanceGame::new(config);

        run(&mut game, (1.0, 0.0), 0.5);
        assert_eq!(game.falls(), 1, "the ball should have fallen");
        assert!(game.is_falling(), "the fall animation should be playing");

        // Let the animation run with the stick centred, so nothing moves the ball once it
        // respawns and "did it go back to the middle" is a clean question.
        let fall = config_fall(game.config());
        run(&mut game, (0.0, 0.0), fall);
        let (x, z) = game.ball().position();
        assert!(
            x.abs() < 1e-6 && z.abs() < 1e-6,
            "ball did not respawn: ({x}, {z})"
        );
        assert!(!game.is_falling());
    }

    fn config_fall(config: &BalanceConfig) -> f32 {
        config.fall_duration + 0.05
    }

    #[test]
    fn the_best_round_survives_a_fall() {
        let mut config = config();
        config.platform_size = 0.4;
        let mut game = BalanceGame::new(config);

        run(&mut game, (1.0, 0.0), 0.5);
        assert_eq!(game.falls(), 1);
        let best = game.best();
        assert!(best > 0.0, "the round that ended should be remembered");

        // The round the player is in now is shorter, so the best stays put.
        assert!(game.survived() < best || game.is_falling());
    }

    #[test]
    fn respawning_clears_the_round_but_not_the_score() {
        let mut game = BalanceGame::new(config());
        run(&mut game, (1.0, 0.0), 0.3);
        assert!(game.survived() > 0.0);

        game.respawn();
        let (x, z) = game.ball().position();
        assert!(x.abs() < 1e-6 && z.abs() < 1e-6);
        assert_eq!(game.survived(), 0.0);
    }

    #[test]
    fn a_slow_frame_is_not_a_bigger_step() {
        // One frame of 1/30 s must land in the same place as two frames of 1/60 s.
        let mut slow = BalanceGame::new(config());
        let mut fast = BalanceGame::new(config());

        for _ in 0..10 {
            slow.update((1.0, 0.0), 1.0 / 30.0);
            fast.update((1.0, 0.0), 1.0 / 60.0);
            fast.update((1.0, 0.0), 1.0 / 60.0);
        }

        let (sx, _) = slow.ball().position();
        let (fx, _) = fast.ball().position();
        assert!((sx - fx).abs() < 1e-4, "slow {sx} vs fast {fx}");
    }

    #[test]
    fn the_edge_warning_tracks_the_ball() {
        let config = config();
        let mut game = BalanceGame::new(config);
        assert_eq!(game.ball().edge_proximity(&config), 0.0);

        run(&mut game, (1.0, 0.0), 0.6);
        let proximity = game.ball().edge_proximity(&config);
        assert!(proximity > 0.0 && proximity <= 1.0, "proximity {proximity}");
    }

    #[test]
    fn the_ball_sits_on_the_surface_not_through_it() {
        let config = config();
        let mut game = BalanceGame::new(config);
        run(&mut game, (0.6, -0.3), 0.4);

        let position = game.ball().world_position(game.tilt(), 0.0, &config);
        // Distance from the plane along its normal must be the radius.
        let normal = game.tilt().rotate_vec3(Vec3::UP);
        let distance = dot(position, normal);
        assert!(
            (distance - config.ball_radius).abs() < 1e-4,
            "ball centre is {distance} from the plane, expected {}",
            config.ball_radius
        );
    }

    #[test]
    fn the_scripted_input_covers_every_direction() {
        let mut seen = (false, false, false, false);
        for frame in 0..240 {
            let (x, z) = BalanceGame::scripted_input(frame);
            if x > 0.0 {
                seen.0 = true;
            }
            if x < 0.0 {
                seen.1 = true;
            }
            if z > 0.0 {
                seen.2 = true;
            }
            if z < 0.0 {
                seen.3 = true;
            }
        }
        assert!(
            seen.0 && seen.1 && seen.2 && seen.3,
            "script missed a direction: {seen:?}"
        );
    }
}
