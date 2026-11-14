//! Acceptance tests for the physics world: a stack that stays up, contacts that name the
//! right bodies, a ray that sees what was just created, and a step that does not depend
//! on the frame rate.
//!
//! This binary does not touch SDL, so it may hold several tests.

use giochi_engine::physics::{BodyDesc, ColliderShape, ContactKind, PhysicsWorld2D};
use giochi_engine::Vec2;

const STEP: f32 = 1.0 / 60.0;

fn floor(world: &mut PhysicsWorld2D) -> u64 {
    let handle = world
        .create_body(BodyDesc::static_(Vec2::new(0.0, -0.5)))
        .expect("floor body");
    world
        .attach_collider(handle, ColliderShape::boxed(40.0, 0.5), 0.9, 0.0)
        .expect("floor collider");
    handle
}

fn add_box(world: &mut PhysicsWorld2D, x: f32, y: f32) -> u64 {
    let handle = world
        .create_body(BodyDesc::dynamic(Vec2::new(x, y)))
        .expect("box body");
    world
        .attach_collider(handle, ColliderShape::boxed(0.5, 0.5), 0.7, 0.0)
        .expect("box collider");
    handle
}

#[test]
fn a_tower_of_boxes_settles_and_stays_settled() {
    let mut world = PhysicsWorld2D::new(Vec2::new(0.0, -20.0)).expect("world");
    floor(&mut world);
    let mut boxes = Vec::new();
    for row in 0..6u32 {
        boxes.push(add_box(&mut world, 0.0, 0.6 + row as f32 * 1.02));
    }

    // 2 seconds to fall and settle, then 2 more in which nothing should move.
    for _ in 0..120 {
        world.step(STEP).expect("step");
    }
    let settled: Vec<Vec2> = boxes
        .iter()
        .map(|handle| world.position(*handle).expect("position"))
        .collect();

    for _ in 0..120 {
        world.step(STEP).expect("step");
    }
    for (index, handle) in boxes.iter().enumerate() {
        let now = world.position(*handle).expect("position");
        let drift = now.distance(settled[index]);
        let speed = world.velocity(*handle).expect("velocity").length();
        assert!(
            drift < 0.02 && speed < 0.2,
            "box {index} is not settled: moved {drift} units, speed {speed}"
        );
        assert!(now.y > 0.3, "box {index} fell through the floor: {now:?}");
    }

    // The stack must not have collapsed into one column of overlapping boxes: each box
    // should still be above the previous one.
    for pair in boxes.windows(2) {
        let lower = world.position(pair[0]).expect("position");
        let upper = world.position(pair[1]).expect("position");
        assert!(
            upper.y > lower.y + 0.5,
            "the tower collapsed: {lower:?} then {upper:?}"
        );
    }
}

#[test]
fn a_slow_frame_runs_more_steps_but_not_unboundedly() {
    let mut world = PhysicsWorld2D::new(Vec2::new(0.0, -10.0)).expect("world");
    let body = add_box(&mut world, 0.0, 10.0);

    // One frame at 30 Hz is two 60 Hz integration steps.
    assert_eq!(world.step(STEP * 2.0).expect("step"), 2);
    // A one-second stall must not simulate one second in one call.
    assert_eq!(
        world.step(1.0).expect("step"),
        giochi_engine::physics::MAX_SUBSTEPS
    );
    // And the backlog is dropped rather than paid off later: the next 60 Hz frame runs
    // one step, not five.
    assert_eq!(world.step(STEP).expect("step"), 1);
    assert!(world.position(body).expect("position").y < 10.0);

    // A different frame rate produces a different path, which is the point of the fixed
    // step: the *simulation* is always 60 Hz.
    world.set_timestep(STEP * 2.0).expect("timestep");
    assert_eq!(world.timestep(), STEP * 2.0);
}

#[test]
fn contacts_name_the_bodies_that_touched() {
    let mut world = PhysicsWorld2D::new(Vec2::new(0.0, -20.0)).expect("world");
    let ground = floor(&mut world);
    let ball = world
        .create_body(BodyDesc::dynamic(Vec2::new(0.0, 3.0)).with_velocity(Vec2::new(0.0, -8.0)))
        .expect("ball");
    world
        .attach_collider(ball, ColliderShape::circle(0.5), 0.2, 0.0)
        .expect("collider");

    let mut started = Vec::new();
    for _ in 0..180 {
        world.step(STEP).expect("step");
        for contact in world.contacts() {
            if contact.kind == ContactKind::Started {
                let mut pair = [contact.a, contact.b];
                pair.sort_unstable();
                started.push(pair);
            }
        }
    }
    let mut expected = [ground, ball];
    expected.sort_unstable();
    assert!(
        started.contains(&expected),
        "the ball's landing was never reported: {started:?}"
    );
    // No phantom contacts from the floor colliding with itself.
    assert!(started.iter().all(|pair| pair[0] != pair[1]));
}

#[test]
fn a_raycast_sees_a_body_created_in_the_same_frame() {
    let mut world = PhysicsWorld2D::new(Vec2::ZERO).expect("world");
    let wall = world
        .create_body(BodyDesc::static_(Vec2::new(2.0, 0.0)))
        .expect("wall");
    world
        .attach_collider(wall, ColliderShape::boxed(0.5, 3.0), 0.5, 0.0)
        .expect("collider");

    let hit = world
        .raycast(Vec2::ZERO, Vec2::new(1.0, 0.0), 5.0)
        .expect("raycast")
        .expect("the wall was created before the ray was cast");
    assert_eq!(hit.body, wall);
    assert!((hit.distance - 1.5).abs() < 0.01, "hit at {}", hit.distance);

    // A teleport followed by a ray sees the new position, not the old one.
    world
        .set_position(wall, Vec2::new(2.0, 10.0))
        .expect("teleport");
    assert!(
        world
            .raycast(Vec2::ZERO, Vec2::new(1.0, 0.0), 5.0)
            .expect("raycast")
            .is_none(),
        "the ray still hit the wall where it used to be"
    );
}

#[test]
fn destroying_a_body_removes_its_collider_from_the_world() {
    let mut world = PhysicsWorld2D::new(Vec2::ZERO).expect("world");
    let wall = world
        .create_body(BodyDesc::static_(Vec2::new(2.0, 0.0)))
        .expect("wall");
    world
        .attach_collider(wall, ColliderShape::boxed(0.5, 3.0), 0.5, 0.0)
        .expect("collider");
    assert!(world
        .raycast(Vec2::ZERO, Vec2::new(1.0, 0.0), 5.0)
        .expect("raycast")
        .is_some());

    world.destroy_body(wall).expect("destroy");
    assert!(
        world
            .raycast(Vec2::ZERO, Vec2::new(1.0, 0.0), 5.0)
            .expect("raycast")
            .is_none(),
        "the destroyed body's collider is still in the world"
    );
    assert_eq!(world.body_count(), 0);
}
