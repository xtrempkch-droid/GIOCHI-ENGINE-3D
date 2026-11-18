//! Drives the real car on the real track, with no window in the way.
//!
//! The plan's phase-41 acceptance criteria are all physics statements — "the car does not
//! fall through the floor", "it does not flip", "it slides in an arcade way" — and every
//! one of them can be asserted against the shipped `.scene` and the shipped tuning. That
//! is the point of keeping [`physics_world`] and [`Car`] free of GL: the gates check the
//! game, not a mock of it.

use giochi_engine_3d::math3d::Vec3;
use giochi_engine_3d::physics::PhysicsWorld3D;
use giochi_racer::config::GameConfig;
use giochi_racer::game::physics_world;
use giochi_racer::track::loading::load_track;
use giochi_racer::track::Track;
use giochi_racer::vehicle::car::{Car, CarConfig};
use giochi_racer::vehicle::input::CarInput;

/// The shipped track, as the game loads it.
fn track() -> Track {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/tracks/track_easy.scene"
    );
    load_track(path).expect("the shipped track loads")
}

/// Full throttle, no steering.
fn straight() -> CarInput {
    CarInput {
        throttle: 1.0,
        ..CarInput::NEUTRAL
    }
}

/// Runs `steps` frames of `input` at 60 Hz.
fn drive(
    car: &mut Car,
    world: &mut PhysicsWorld3D,
    input: CarInput,
    steps: u32,
) -> giochi_racer::vehicle::car::CarState {
    let mut state = car.state(world).expect("a live car");
    for _ in 0..steps {
        state = car.step(world, input, 1.0 / 60.0).expect("the car steps");
    }
    state
}

#[test]
fn the_car_starts_on_the_road_at_the_start_line() {
    let track = track();
    let mut world = physics_world(&track).expect("a physics world");
    let car = Car::spawn(&mut world, track.start, CarConfig::default()).expect("a car");
    let state = car.state(&world).expect("a state");
    let position = state.position.to_glam();
    // On the south straight, which runs along z = -15.
    assert!((position.z - (-15.0)).abs() < 0.5, "{position:?}");
    assert!(position.x.abs() < 0.5, "{position:?}");
    // Sitting on the road, not in it and not above it.
    assert!(position.y > 0.2 && position.y < 0.6, "{position:?}");
    assert!(state.speed_kmh < 1.0);
}

#[test]
fn the_car_drives_forward_along_its_heading() {
    let track = track();
    let mut world = physics_world(&track).expect("a physics world");
    let mut car = Car::spawn(&mut world, track.start, CarConfig::default()).expect("a car");
    let start = car.state(&world).expect("a state").position.to_glam();
    let state = drive(&mut car, &mut world, straight(), 120);
    let position = state.position.to_glam();
    // The start faces +X, so two seconds of throttle is two seconds of +X.
    assert!(
        position.x - start.x > 10.0,
        "moved {:.1} m in 2 s",
        position.x - start.x
    );
    assert!(
        (position.z - start.z).abs() < 1.0,
        "it tracked straight: {position:?}"
    );
    assert!(state.speed_kmh > 30.0, "{:.1} km/h", state.speed_kmh);
    assert!(state.gear >= 1);
}

#[test]
fn the_car_does_not_fall_through_the_road() {
    let track = track();
    let mut world = physics_world(&track).expect("a physics world");
    let mut car = Car::spawn(&mut world, track.start, CarConfig::default()).expect("a car");
    let mut lowest = f32::INFINITY;
    for _ in 0..600 {
        let state = car
            .step(&mut world, straight(), 1.0 / 60.0)
            .expect("a step");
        lowest = lowest.min(state.position.to_glam().y);
    }
    // The body's centre rests at half its height (0.35); 10 cm of slack is the solver's
    // allowed penetration, not a hole.
    assert!(lowest > 0.2, "the car dropped to y = {lowest}");
}

#[test]
fn the_car_cannot_drive_through_the_barrier() {
    let track = track();
    let mut world = physics_world(&track).expect("a physics world");
    let mut car = Car::spawn(&mut world, track.start, CarConfig::default()).expect("a car");
    // Ten seconds of full throttle into the barrier at x = 40.3, whose inner face is at
    // 40.0. The car is 4 m long, so its centre cannot pass x = 38.
    let state = drive(&mut car, &mut world, straight(), 600);
    let x = state.position.to_glam().x;
    assert!(
        x < 40.0,
        "the car reached x = {x} and went through the wall"
    );
    assert!(x > 20.0, "the car should have got to the wall: x = {x}");
}

#[test]
fn the_car_stays_upright_whatever_it_hits() {
    let track = track();
    let mut world = physics_world(&track).expect("a physics world");
    let mut car = Car::spawn(&mut world, track.start, CarConfig::default()).expect("a car");
    // Full throttle with the wheel held over, so it meets the barrier at an angle.
    let input = CarInput {
        throttle: 1.0,
        steer: 1.0,
        ..CarInput::NEUTRAL
    };
    for _ in 0..600 {
        let state = car.step(&mut world, input, 1.0 / 60.0).expect("a step");
        // The yaw is the only orientation the car has; a rolled or pitched body would
        // show up as a rotation the model never asked for.
        let forward = Vec3::new(state.yaw.sin(), 0.0, state.yaw.cos());
        assert!(forward.is_finite());
        let height = state.position.to_glam().y;
        assert!(
            height > 0.1 && height < 1.5,
            "the car left the ground plane: {height}"
        );
    }
}

/// An empty flat world, for the tests that need a straight long enough to reach top speed.
///
/// The shipped oval is 80 m across, which is not enough to accelerate to 120 km/h and
/// still have road left to brake on — the car reaches the barrier first, which is a
/// correct outcome but the wrong test.
fn open_world() -> (PhysicsWorld3D, Car) {
    use giochi_engine_3d::physics::{BodyDesc, ColliderShape};
    let mut world = PhysicsWorld3D::new(giochi_racer::game::GRAVITY).expect("a world");
    let ground = world
        .create_body(BodyDesc::static_(Vec3::new(0.0, -0.2, 0.0)))
        .expect("a ground body");
    world
        .attach_collider(ground, ColliderShape::cuboid(400.0, 0.2, 400.0), 0.6, 0.0)
        .expect("a ground collider");
    let start = giochi_racer::track::StartPosition {
        position: Vec3::new(0.0, 0.5, 0.0),
        yaw: 0.0,
    };
    let car = Car::spawn(&mut world, start, CarConfig::default()).expect("a car");
    (world, car)
}

#[test]
fn braking_from_top_speed_stops_the_car_in_a_short_distance() {
    let (mut world, mut car) = open_world();
    let fast = drive(&mut car, &mut world, straight(), 240);
    assert!(fast.speed_kmh > 90.0, "{:.1} km/h", fast.speed_kmh);
    let before = fast.position.to_glam().z;
    // 1.3 s at 28 m/s² from 33 m/s: stopped with a little to spare, and not yet into
    // reverse (holding the brake past the stop starts reversing, by design).
    let braked = drive(
        &mut car,
        &mut world,
        CarInput {
            throttle: -1.0,
            ..CarInput::NEUTRAL
        },
        78,
    );
    assert!(
        braked.speed_kmh < 10.0,
        "still doing {:.1} km/h after 1.3 s of brake",
        braked.speed_kmh
    );
    let distance = braked.position.to_glam().z - before;
    assert!(
        distance > 15.0 && distance < 30.0,
        "stopping from 120 km/h took {distance:.1} m"
    );
}

#[test]
fn a_respawn_puts_the_car_back_on_the_line_facing_the_right_way() {
    let track = track();
    let mut world = physics_world(&track).expect("a physics world");
    let mut car = Car::spawn(&mut world, track.start, CarConfig::default()).expect("a car");
    drive(&mut car, &mut world, straight(), 120);
    let moved = car.state(&world).expect("a state");
    assert!(moved.position.to_glam().x > 5.0);

    car.respawn(&mut world, track.start.position, track.start.yaw)
        .expect("a respawn");
    let back = car.state(&world).expect("a state");
    assert!(back.position.to_glam().x.abs() < 0.5, "{:?}", back.position);
    assert!(back.speed_kmh < 1.0);
    assert!(
        (back.yaw - track.start.yaw).abs() < 1e-4,
        "{} vs {}",
        back.yaw,
        track.start.yaw
    );
}

/// Steers towards `target` the way a driver would: slow down for a corner, full throttle
/// on a straight, and back out of a wall if it gets stuck.
fn aim_at(state: &giochi_racer::vehicle::car::CarState, target: Vec3, stuck: bool) -> CarInput {
    let (dx, dz) = (
        target.to_glam().x - state.position.to_glam().x,
        target.to_glam().z - state.position.to_glam().z,
    );
    // Forward is `(sin yaw, cos yaw)`, so the heading towards a point is `atan2(x, z)`.
    let desired = dx.atan2(dz);
    let mut error = desired - state.yaw;
    while error > std::f32::consts::PI {
        error -= std::f32::consts::TAU;
    }
    while error < -std::f32::consts::PI {
        error += std::f32::consts::TAU;
    }
    // The car's `steer` is positive for **right**, and right is a *decreasing* yaw in this
    // convention, so a positive heading error needs a negative steer. Reversing inverts
    // the effect, which is why the stuck branch flips the sign back.
    if stuck {
        // Reversing with the wheel turned is how a driver leaves a wall; without it the
        // car sits against the barrier with full throttle and never recovers.
        return CarInput {
            throttle: -0.8,
            steer: error.signum(),
            handbrake: false,
            pause: false,
        };
    }
    // Slower the sharper the corner: 72 km/h on a straight, 32 km/h when pointing the
    // wrong way and having to turn.
    let target_speed = if error.abs() > 0.6 {
        9.0
    } else if error.abs() > 0.25 {
        12.0
    } else {
        20.0
    };
    let speed = state.speed_kmh / 3.6;
    CarInput {
        throttle: ((target_speed - speed) / 6.0).clamp(-1.0, 1.0),
        steer: (-error * 3.5).clamp(-1.0, 1.0),
        handbrake: false,
        pause: false,
    }
}

#[test]
fn a_lap_of_the_shipped_track_can_be_completed_by_the_car() {
    use giochi_racer::track::lap::LapEvent;
    let track = track();
    let mut world = physics_world(&track).expect("a physics world");
    let mut car = Car::spawn(&mut world, track.start, CarConfig::default()).expect("a car");
    let mut lap = giochi_racer::track::lap::LapTracker::new(
        track.checkpoints.clone(),
        1,
        track.start.position,
    );

    let gates = track.checkpoints.clone();
    let mut state = car.state(&world).expect("a state");
    let mut finished = false;
    let mut slow_frames = 0u32;
    let mut reversing = 0u32;
    // Two minutes at 60 Hz: a lap of 200 m at a cautious 10 m/s is 20 s, so a car that
    // needs six times that is not driving the track, it is fighting it.
    for _ in 0..120 * 60 {
        // Aim at the current gate, and once it is close, at a point *past* it along the
        // next straight. Without the look-ahead the car aims at a point it is about to
        // pass, turns past it, and spends the corner oscillating.
        let index = lap.next_gate().map(|gate| gate.index).unwrap_or(0);
        let current = gates[index].position;
        let following = gates[(index + 1) % gates.len()].position;
        let target = {
            let to_current = current.to_glam() - state.position.to_glam();
            let distance = (to_current.x * to_current.x + to_current.z * to_current.z).sqrt();
            if distance > 22.0 {
                current
            } else {
                let along = following.to_glam() - current.to_glam();
                let length = (along.x * along.x + along.z * along.z).max(0.001);
                let step = (14.0 / length).min(1.0);
                Vec3::new(
                    current.to_glam().x + along.x * step,
                    current.to_glam().y,
                    current.to_glam().z + along.z * step,
                )
            }
        };
        // Against a wall: reverse *and keep turning* until the nose points where it has
        // to go. Ending the reverse as soon as the car moves again just drives it back
        // into the barrier, which is how the first version of this test spent two minutes
        // bouncing off the same corner.
        let stuck = reversing > 0 || (state.speed_kmh < 2.0 && slow_frames > 45);
        if stuck && reversing == 0 {
            reversing = 150;
        }
        reversing = reversing.saturating_sub(1);
        slow_frames = if state.speed_kmh < 2.0 {
            slow_frames + 1
        } else {
            0
        };
        let input = aim_at(&state, target, stuck);
        state = car.step(&mut world, input, 1.0 / 60.0).expect("a step");
        let event = lap.update(state.position, state.velocity, 1.0 / 60.0);
        if event == LapEvent::RaceFinished {
            finished = true;
            break;
        }
        assert!(event != LapEvent::LapInvalidated, "the car cut the course");
    }
    assert!(
        finished,
        "the car did not complete a lap; it is at {:?} heading {:.2} with {:.0} km/h",
        state.position.to_array(),
        state.yaw,
        state.speed_kmh
    );
}

#[test]
fn the_tuning_the_game_ships_is_the_tuning_the_plan_asks_for() {
    // 120 km/h top speed is a phase-41 acceptance number, not a suggestion.
    let config = GameConfig::default();
    assert!((config.car.max_speed_kmh() - 120.0).abs() < 0.01);
    assert!(config.car.accel > 0.0);
    assert!(
        config.car.brake > config.car.accel,
        "braking has to beat accelerating"
    );
    assert!(
        config.car.handbrake_grip < config.car.grip,
        "the handbrake has to slide"
    );
}

#[test]
fn a_locked_rotation_body_still_takes_the_rotation_it_is_given() {
    use giochi_engine_3d::math3d::Quat;
    use giochi_engine_3d::physics::{BodyDesc, ColliderShape};
    let mut world = PhysicsWorld3D::new(giochi_racer::game::GRAVITY).expect("a world");
    let body = world
        .create_body(BodyDesc::dynamic(Vec3::new(0.0, 5.0, 0.0)).lock_rotations())
        .expect("a body");
    world
        .attach_collider(body, ColliderShape::cuboid(0.5, 0.5, 0.5), 0.5, 0.0)
        .expect("a collider");
    world
        .set_rotation(
            body,
            Quat::from_axis_angle(Vec3::UP, std::f32::consts::FRAC_PI_2),
        )
        .expect("a rotation");
    let before = world.rotation(body).expect("a rotation");
    world.step(1.0 / 60.0).expect("a step");
    let after = world.rotation(body).expect("a rotation");
    eprintln!("before {before:?} after {after:?}");
    let forward = after.rotate_vec3(Vec3::new(0.0, 0.0, 1.0)).to_glam();
    assert!(
        (forward.x - 1.0).abs() < 1e-3,
        "the body ignored the rotation: forward {forward:?}"
    );
}
