//! Transform ↔ physics synchronisation, in the frame it happens (DOD criteria 8 and 9).
//!
//! **Ativado na Fase S2.**
//!
//! # The two directions, and why order matters
//!
//! ```text
//! sync_from_transforms(scene)   // editor edit or teleport enters physics
//! world.step(dt)                // physics advances
//! sync_to_transforms(scene)     // physics leaves towards the renderer
//! ```
//!
//! The interesting property is not that both functions exist — they do — but that the
//! value written by one is what the other reads **without an extra step**. A one-frame
//! lag is invisible in a unit test and obvious in the editor when a dragged object
//! passes through a wall before the collider catches up.
//!
//! # Camera jitter, turned into a number
//!
//! "Sem jitter perceptível" cannot be asserted. The DOD replaces it with the property
//! that actually produces jitter: with a target moving at constant velocity, the distance
//! the camera moves per frame must converge monotonically and never change sign. An
//! exponential follow that is evaluated with a `dt`-independent blend passes; one that
//! uses a fixed blend per frame fails at 30 fps.

use giochi_engine_3d::camera::FollowCamera;
use giochi_engine_3d::math3d::{Quat, Vec3};
use giochi_engine_3d::physics::{BodyDesc, BodyTransform, ColliderShape, PhysicsWorld3D};

fn world() -> PhysicsWorld3D {
    let mut world =
        PhysicsWorld3D::new(Vec3::new(0.0, -9.81, 0.0)).expect("a world with finite gravity");
    let floor = world
        .create_body(BodyDesc::static_(Vec3::new(0.0, -0.5, 0.0)))
        .expect("a static floor");
    world
        .attach_collider(floor, ColliderShape::cuboid(50.0, 0.5, 50.0), 0.6, 0.0)
        .expect("a floor collider");
    world
}

/// A transform with the identity rotation.
fn at(x: f32, y: f32, z: f32) -> BodyTransform {
    BodyTransform::new(Vec3::new(x, y, z), Quat::IDENTITY)
}

#[test]
#[ignore = "Ativado na Fase S2"]
fn a_scene_edit_is_visible_to_physics_in_the_same_frame() {
    let mut world = world();
    let body = world
        .create_body(BodyDesc::dynamic(Vec3::new(0.0, 5.0, 0.0)))
        .expect("a dynamic body");
    world
        .attach_collider(body, ColliderShape::cuboid(0.5, 0.5, 0.5), 0.6, 0.0)
        .expect("a collider");

    // What the editor does when a node is dragged: write the transform into the world.
    world.sync_from_transforms(&mut |handle| {
        if handle == body {
            Some(at(3.0, 5.0, -2.0))
        } else {
            None
        }
    });

    // No `step` in between: the next read must already be the edited value.
    let position = world.position(body).expect("a live body");
    assert!(
        (position.to_glam() - Vec3::new(3.0, 5.0, -2.0).to_glam()).length() < 1e-6,
        "{position:?}"
    );

    // And a raycast in the same frame sees it there, not at the old place.
    let hit = world
        .raycast(Vec3::new(3.0, 9.0, -2.0), Vec3::new(0.0, -1.0, 0.0), 10.0)
        .expect("a valid ray");
    assert!(hit.is_some(), "the edited body is not where the scene says");
}

#[test]
#[ignore = "Ativado na Fase S2"]
fn physics_reaches_the_scene_after_the_step_without_a_second_frame() {
    let mut world = world();
    let body = world
        .create_body(BodyDesc::dynamic(Vec3::new(0.0, 4.0, 0.0)))
        .expect("a dynamic body");
    world
        .attach_collider(body, ColliderShape::cuboid(0.5, 0.5, 0.5), 0.6, 0.0)
        .expect("a collider");

    let before = world.position(body).expect("a live body");
    world.step(1.0 / 60.0).expect("a finite delta");

    // The scene is written from the step that just ran, not from the previous one.
    let mut written = None;
    world.sync_to_transforms(&mut |handle, transform| {
        if handle == body {
            written = Some(transform);
        }
    });
    let written = written.expect("the body was reported");
    let read_back = world.position(body).expect("a live body");
    assert!(
        (written.position.to_glam() - read_back.to_glam()).length() < 1e-6,
        "scene {written:?} disagrees with physics {read_back:?}"
    );
    assert!(
        written.position.y < before.y,
        "gravity did not move the body"
    );
}

#[test]
#[ignore = "Ativado na Fase S2"]
fn only_the_bodies_the_scene_answers_for_are_moved() {
    let mut world = world();
    let moved = world
        .create_body(BodyDesc::dynamic(Vec3::new(0.0, 5.0, 0.0)))
        .expect("a dynamic body");
    let untouched = world
        .create_body(BodyDesc::dynamic(Vec3::new(8.0, 5.0, 0.0)))
        .expect("a second body");
    for handle in [moved, untouched] {
        world
            .attach_collider(handle, ColliderShape::cuboid(0.5, 0.5, 0.5), 0.6, 0.0)
            .expect("a collider");
    }

    world.sync_from_transforms(&mut |handle| (handle == moved).then(|| at(1.0, 5.0, 1.0)));

    let a = world.position(moved).expect("a live body");
    let b = world.position(untouched).expect("a live body");
    assert!((a.to_glam() - Vec3::new(1.0, 5.0, 1.0).to_glam()).length() < 1e-6);
    assert!((b.to_glam() - Vec3::new(8.0, 5.0, 0.0).to_glam()).length() < 1e-6);
}

/// Runs a [`FollowCamera`] behind a target moving at `speed` for `frames` frames at
/// `fps`, and returns the distance the eye moved on each frame after the first.
fn camera_steps(fps: u32, frames: usize) -> Vec<f32> {
    let mut camera = FollowCamera::new();
    let dt = 1.0 / fps as f32;
    let speed = 12.0;
    let velocity = Vec3::new(0.0, 0.0, -speed);

    let mut previous: Option<Vec3> = None;
    let mut steps: Vec<f32> = Vec::with_capacity(frames);
    for frame in 0..frames {
        let target = Vec3::new(0.0, 0.5, -speed * dt * frame as f32);
        camera.update(target, velocity, dt);
        let eye = camera.eye();
        if let Some(last) = previous {
            steps.push((eye.to_glam() - last.to_glam()).length());
        }
        previous = Some(eye);
    }
    steps
}

#[test]
#[ignore = "Ativado na Fase S2"]
fn the_chase_camera_converges_without_oscillating_at_thirty_and_sixty_fps() {
    for fps in [30, 60] {
        let steps = camera_steps(fps, 90);
        assert!(!steps.is_empty());

        // After the first few frames the eye is locked to the target's speed, so the
        // per-frame distance must settle instead of bouncing between two values.
        let tail = &steps[steps.len() / 2..];
        let mean = tail.iter().sum::<f32>() / tail.len() as f32;
        for (index, step) in tail.iter().enumerate() {
            assert!(
                (step - mean).abs() < mean * 0.25,
                "{fps} fps, frame {index}: moved {step}, mean {mean}"
            );
        }
    }
}

#[test]
#[ignore = "Ativado na Fase S2"]
fn the_camera_moves_the_same_distance_per_second_at_thirty_and_sixty_fps() {
    // Framerate independence for the *camera*, which is what "sem jitter" means when a
    // frame hitches: after the same simulated time, the eye must be in the same place.
    let mut thirty = FollowCamera::new();
    let mut sixty = FollowCamera::new();
    let speed = 12.0;
    let velocity = Vec3::new(0.0, 0.0, -speed);
    let target = Vec3::new(0.0, 0.5, -60.0);

    for _ in 0..60 {
        thirty.update(target, velocity, 1.0 / 30.0);
    }
    for _ in 0..120 {
        sixty.update(target, velocity, 1.0 / 60.0);
    }

    let distance = (thirty.eye().to_glam() - sixty.eye().to_glam()).length();
    assert!(
        distance < 0.5,
        "the cameras diverged by {distance} after 2 s"
    );
}
