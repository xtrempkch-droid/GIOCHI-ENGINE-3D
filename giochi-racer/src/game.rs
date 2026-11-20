//! One race: the track, the car, the camera, the clock and the HUD.
//!
//! Everything that needs a GL context happens in [`RaceGame::new`], once. The frame is
//! then two calls — [`RaceGame::step`] (input in, simulation out) and
//! [`RaceGame::render`] (draw) — which is what lets the game loop in `main` stay short
//! enough to read and what will let phase 44's scene stack swap races without rebuilding
//! a renderer.
//!
//! # The order of a frame
//!
//! 1. The car proposes and the physics world decides ([`Car::step`]).
//! 2. The lap tracker is told where the car ended up, not where it wanted to go. A car
//!    stopped by a wall does not score the gate behind the wall.
//! 3. The camera trails the car's *new* pose, so the view never lags a frame behind the
//!    simulation.

use giochi_engine_3d::math3d::{Color3D, Vec3};
use giochi_engine_3d::physics::{BodyDesc, PhysicsWorld3D};
use giochi_engine_3d::render3d::{MeshHandle, ShaderHandle};
use giochi_engine_3d::shader::BuiltinShader;
use giochi_engine_3d::ui::GlesHud;
use giochi_engine_3d::{RenderContext3D, Result3D};

use crate::assets;
use crate::config::GameConfig;
use crate::race::{Field, Opponent, Standing};
use crate::track::lap::{LapEvent, LapTracker};
use crate::track::loading::load_track;
use crate::track::{DrawCall, Track};
use crate::utils::procedural_car::{self, CarMeshes};
use crate::utils::{compose, pose};
use crate::vehicle::camera::ChaseCamera;
use crate::vehicle::car::CarState;
use crate::vehicle::hud::{self, HudState};
use crate::vehicle::input::CarInput;

/// Gravity for a game whose unit is the metre.
pub const GRAVITY: Vec3 = Vec3::new(0.0, -9.81, 0.0);

/// The sky.
pub const SKY: Color3D = Color3D::new(0.42, 0.62, 0.86, 1.0);

/// How long a lap message stays on screen, in seconds.
pub const MESSAGE_SECONDS: f32 = 2.5;

/// How the race is going, for a caller that wants to act on it (phase 44's results scene).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RacePhase {
    /// The car is on the road and the clock is running.
    Racing,
    /// Every lap is done. The car can still be driven; the clock has stopped.
    Finished,
}

/// A race in progress, with everything the simulation needs and everything GL does.
///
/// The simulation itself is [`Field`], which has no GL in it and can therefore be driven by
/// a test. This type owns the track's meshes, the camera, the HUD and the message banner;
/// the state it reports is the field's.
pub struct RaceGame {
    config: GameConfig,
    track: Track,
    /// The race itself: world, cars, opponents, clocks, classification. No GL in it.
    field: Field,
    camera: ChaseCamera,
    hud: GlesHud,
    shader: ShaderHandle,
    car_meshes: CarMeshes,
    /// Every mesh the track uploaded, freed by [`RaceGame::release`].
    track_meshes: Vec<MeshHandle>,
    track_draws: Vec<DrawCall>,
    wheel_spin: f32,
    /// Each opponent's wheel rotation, in the field's order.
    opponent_wheel_spin: Vec<f32>,
    /// The last lap event, kept for one HUD message.
    last_event: Option<LapEvent>,
    /// Seconds the current message has been on screen.
    message_age: f32,
    /// The best lap already on record for this track, if any, so the results overlay can
    /// say "NEW BEST" without the game owning a settings file (phase 44 does).
    previous_best: Option<f32>,
}

impl RaceGame {
    /// Loads `config.track` and builds everything that needs GL.
    pub fn new(render: &mut RenderContext3D, config: GameConfig) -> anyhow::Result<Self> {
        let path = assets::resolve(&config.track);
        let track = load_track(&path).map_err(|error| {
            anyhow::anyhow!(
                "{error}\n(the game looks for assets next to the executable, then the working \
                 directory, then the checkout; the tracks are described in \
                 `giochi-racer/docs/TRACK_SPECS.md`)"
            )
        })?;
        log::info!(
            "track '{}': {} drawables, {} colliders, {} gates",
            track.name,
            track.instances.len(),
            track.colliders.len(),
            track.checkpoints.len()
        );

        let asset_root = assets::asset_root();
        let uploaded = track.upload(render, &asset_root)?;
        let shader = render.create_builtin_shader(BuiltinShader::Phong)?;
        render.set_clear_color(SKY);
        if let Some(light) = uploaded.light {
            render.set_light(light);
        }

        let field = Field::new(&track, config.laps, config.car, physics_world(&track)?)?;
        log::info!(
            "grid: player on pole, {} opponents ({})",
            field.opponents().len(),
            field
                .opponents()
                .iter()
                .map(|opponent| format!("{} {:.0}%", opponent.name(), opponent.skill() * 100.0))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let opponent_wheel_spin = vec![0.0; field.opponents().len()];
        let camera = ChaseCamera::new(config.camera);
        let car_meshes = CarMeshes::create(render, &config.car)?;
        let hud = GlesHud::new(render.glow())?;

        Ok(Self {
            config,
            track,
            field,
            camera,
            hud,
            shader,
            car_meshes,
            track_meshes: uploaded.handles,
            track_draws: uploaded.draws,
            wheel_spin: 0.0,
            opponent_wheel_spin,
            last_event: None,
            message_age: MESSAGE_SECONDS,
            previous_best: None,
        })
    }

    /// The track being raced.
    pub fn track(&self) -> &Track {
        &self.track
    }

    /// The lap tracker, for the HUD and for a results screen.
    pub fn lap(&self) -> &LapTracker {
        self.field.player_lap()
    }

    /// Where the car is and what it is doing.
    pub fn car_state(&self) -> CarState {
        self.field.player_state()
    }

    /// The physics world, for a debug overlay.
    pub fn world(&self) -> &PhysicsWorld3D {
        self.field.world()
    }

    /// The race itself, without the GL around it.
    pub fn field(&self) -> &Field {
        &self.field
    }

    /// Whether the race is over.
    pub fn phase(&self) -> RacePhase {
        if self.field.player_lap().is_finished() {
            RacePhase::Finished
        } else {
            RacePhase::Racing
        }
    }

    /// Advances one frame of simulation.
    ///
    /// The race is stepped by [`Field`]; what is left here is the part that needs a frame
    /// rather than a simulation: the message banner, the wheels' rotation and the camera.
    pub fn step(&mut self, input: CarInput, dt: f32) {
        let dt = dt.clamp(0.0, 0.1);
        self.field.step(input, dt);
        let car_state = self.field.player_state();

        // Wheel rotation comes from distance travelled, so the wheels turn at the rate the
        // speed implies rather than at a constant.
        let radius = self.config.car.wheel_radius.max(0.05);
        self.wheel_spin -= car_state.speed_kmh / 3.6 / radius * dt;
        for (spin, opponent) in self
            .opponent_wheel_spin
            .iter_mut()
            .zip(self.field.opponents())
        {
            *spin -= opponent.state().speed_kmh / 3.6 / radius * dt;
        }

        if let Some(event) = self.field.take_event() {
            match event {
                LapEvent::LapComplete { lap, time, best } => {
                    log::info!(
                        "lap {lap} in {time:.3}s{}",
                        if best { " (best)" } else { "" }
                    );
                }
                LapEvent::LapInvalidated => log::info!("lap invalidated"),
                LapEvent::RaceFinished => log::info!(
                    "race finished in {:.3}s, best lap {:?}",
                    self.field.player_lap().total_time(),
                    self.field.player_lap().best_lap()
                ),
                LapEvent::Checkpoint(index) => log::debug!("checkpoint {index}"),
                LapEvent::None => {}
            }
            self.last_event = Some(event);
            self.message_age = 0.0;
        }
        self.message_age += dt;

        let forward = Vec3::new(car_state.yaw.sin(), 0.0, car_state.yaw.cos());
        let ratio = self.speed_ratio();
        self.camera.update(car_state.position, forward, ratio, dt);
    }

    /// The classification, leader first.
    pub fn standings(&self) -> &[Standing] {
        self.field.standings()
    }

    /// The opponents, for a test that wants to look at them.
    pub fn opponents(&self) -> &[Opponent] {
        self.field.opponents()
    }

    /// Where the player finished, `1`-based, or `0` before the first step.
    pub fn player_position(&self) -> usize {
        self.field.player_position()
    }

    /// Metres between the player and the car in front, or `None` when the player leads.
    pub fn gap_ahead(&self) -> Option<f32> {
        self.field.gap_ahead()
    }

    /// Draws the frame. The caller has already called `RenderContext3D::begin_frame`.
    pub fn render(
        &mut self,
        render: &mut RenderContext3D,
        screen: (u32, u32),
        dt: f32,
    ) -> Result3D<()> {
        let camera = self
            .camera
            .to_camera3d(screen.0, screen.1, self.speed_ratio());

        for call in &self.track_draws {
            render.draw_mesh_material(
                call.mesh,
                self.shader,
                &call.transform,
                &camera,
                &call.material,
            )?;
        }

        // The opponents are drawn before the player, so that on the frame where two cars
        // overlap the player's is the one on top.
        for (spin, opponent) in self.opponent_wheel_spin.iter().zip(self.field.opponents()) {
            let state = opponent.state();
            let car_pose = pose(state.position, state.yaw);
            for part in procedural_car::parts_with_paint(&self.config.car, *spin, opponent.color())
            {
                let transform = compose(&car_pose, &part.transform);
                let mesh = self.car_meshes.handle(part.shape);
                render.draw_mesh_colored(mesh, self.shader, &transform, &camera, part.color)?;
            }
        }

        let car_state = self.field.player_state();
        let car_pose = pose(car_state.position, car_state.yaw);
        for part in procedural_car::parts(&self.config.car, self.wheel_spin) {
            let transform = compose(&car_pose, &part.transform);
            let mesh = self.car_meshes.handle(part.shape);
            render.draw_mesh_colored(mesh, self.shader, &transform, &camera, part.color)?;
        }

        let state = self.hud_state();
        hud::draw(&mut self.hud, render, screen, dt, &state)?;
        Ok(())
    }

    /// What the HUD should show right now.
    pub fn hud_state(&self) -> HudState {
        let car_state = self.field.player_state();
        let lap = self.field.player_lap();
        HudState {
            position: self.field.player_position(),
            field_size: self.field.field_size(),
            gap_ahead: self.field.gap_ahead(),
            track: self.track.name.clone(),
            speed_kmh: car_state.speed_kmh,
            gear: car_state.gear,
            lap: lap.current_lap(),
            total_laps: lap.total_laps(),
            lap_time: lap.lap_time(),
            total_time: lap.total_time(),
            best_lap: lap.best_lap(),
            event: if self.message_age < MESSAGE_SECONDS {
                self.last_event
            } else {
                None
            },
            finished: lap.is_finished(),
            new_best: lap.is_finished()
                && lap
                    .best_lap()
                    .is_some_and(|best| self.previous_best.is_none_or(|old| best < old)),
        }
    }

    /// The best lap already on record for this track.
    ///
    /// Phase 42 keeps the record in memory; phase 44 writes it to
    /// `~/.config/giochi-racer/scores.toml`.
    pub fn previous_best(&self) -> Option<f32> {
        self.previous_best
    }

    /// Records the time to beat before the race starts.
    pub fn set_previous_best(&mut self, best: Option<f32>) {
        self.previous_best = best;
    }

    /// Starts the race again on the same track: clock, laps and car back to the line.
    ///
    /// What `R` does after the results overlay. Before the race is over, `R` only puts the
    /// car back on the road ([`RaceGame::respawn_car`]) — a key that erased a bad lap mid
    /// race would be a cheat, not a feature.
    pub fn restart(&mut self) {
        self.field.restart();
        self.last_event = None;
        self.message_age = 0.0;
        self.wheel_spin = 0.0;
        self.opponent_wheel_spin
            .iter_mut()
            .for_each(|spin| *spin = 0.0);
        let state = self.field.player_state();
        self.camera.snap(
            state.position,
            Vec3::new(state.yaw.sin(), 0.0, state.yaw.cos()),
            0.0,
        );
    }

    /// The chase camera, for a test that wants to look at the frame.
    pub fn camera(&self) -> &ChaseCamera {
        &self.camera
    }

    /// The HUD painter, for a caller that wants to add its own overlay.
    pub fn hud_mut(&mut self) -> &mut GlesHud {
        &mut self.hud
    }

    /// Puts the car back on the start line, stationary, with the clock untouched.
    ///
    /// What `R` will do in phase 44. The lap tracker is not reset: a respawn is not a
    /// restart, and resetting it here would make the key a way to erase a bad lap.
    pub fn respawn_car(&mut self) {
        self.field.respawn_player();
        let state = self.field.player_state();
        self.camera.snap(
            state.position,
            Vec3::new(state.yaw.sin(), 0.0, state.yaw.cos()),
            0.0,
        );
    }

    /// Frees every GL object the race owns.
    ///
    /// Called by `main` while the context is still current. Skipping it is not a crash —
    /// the context is destroyed next anyway — but it is what a scene change in phase 44
    /// will use, so it is written and exercised now rather than then.
    pub fn release(mut self, render: &mut RenderContext3D) {
        for handle in std::mem::take(&mut self.track_meshes) {
            render.destroy_mesh(handle);
        }
        self.car_meshes.destroy(render);
        render.destroy_shader(self.shader);
        self.track_draws.clear();
    }

    fn speed_ratio(&self) -> f32 {
        self.field.player_state().speed_kmh / self.config.car.max_speed_kmh()
    }
}

/// Builds the physics world a track implies: one static body per collider.
///
/// Public because `tests/physics_track.rs` drives the real car on the real track with no
/// GL context, which is the only way "the car does not fall through the floor" and "the
/// car does not tunnel through the barrier" can be asserted on CI.
pub fn physics_world(
    track: &Track,
) -> Result<PhysicsWorld3D, giochi_engine_3d::physics::PhysicsError> {
    let mut world = PhysicsWorld3D::new(GRAVITY)?;
    for collider in &track.colliders {
        let body = world
            .create_body(BodyDesc::static_(collider.position).with_rotation(collider.rotation))?;
        // Walls are walls: high friction would make the car stick to them, and any
        // restitution would bounce a 120 km/h car back down the straight.
        world.attach_collider(body, collider.shape, 0.6, 0.0)?;
    }
    Ok(world)
}

#[cfg(test)]
mod tests {
    use super::*;
    use giochi_engine_3d::math3d::Transform3D;

    #[test]
    fn gravity_points_down_at_earth_g() {
        assert_eq!(GRAVITY.to_array(), [0.0, -9.81, 0.0]);
    }

    #[test]
    fn the_sky_is_blue() {
        const { assert!(SKY.b > SKY.r && SKY.b > SKY.g) };
    }

    #[test]
    fn a_pose_puts_the_car_where_the_physics_left_it() {
        let transform = pose(Vec3::new(1.0, 0.5, 2.0), 0.0);
        assert_eq!(transform.position.to_array(), [1.0, 0.5, 2.0]);
    }

    #[test]
    fn a_car_part_composes_with_the_car_pose() {
        let base = pose(Vec3::new(0.0, 0.0, 0.0), std::f32::consts::FRAC_PI_2);
        let child = Transform3D::from_translation(Vec3::new(0.0, 0.0, 1.0));
        let world = compose(&base, &child);
        // Turned a quarter turn, "one metre forward" is one metre along +X.
        assert!((world.position.to_glam().x - 1.0).abs() < 1e-5);
    }
}
