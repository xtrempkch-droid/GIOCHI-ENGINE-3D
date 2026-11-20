//! Static track collision from the mesh itself (DOD criterion 11).
//!
//! **Ativado na Fase S3.**
//!
//! # The property
//!
//! Today the track's collision is built from **boxes** the loader synthesises from the
//! scene's node transforms (`giochi-racer/src/track/loading.rs`). A box approximation of
//! a curved barrier lets a fast car clip a corner or stop a few centimetres short of a
//! wall — small enough to look like a tuning problem, which is exactly the class of bug
//! the Stability Pack exists to remove from the Racer's backlog.
//!
//! S3 adds `physics/collider/{trimesh,convex}.rs`:
//!
//! | Body | Shape | Why |
//! |---|---|---|
//! | track, floor, walls (static, large) | trimesh | exact, cost is setup only |
//! | car, props (dynamic, small) | convex hull | cheap to update every step |
//!
//! The 64k-vertex limit and the chunking rule are part of the S3 contract; the test below
//! is what proves the rule survived implementation.

use giochi_engine_3d::math3d::Vec3;
use giochi_engine_3d::physics::{BodyDesc, ColliderShape, PhysicsWorld3D};

/// Material-free drop test: what S3 must reproduce with a trimesh floor.
///
/// Written against the box collider that exists today, so the harness and the assertion
/// ("a body dropped from 10 m ends up on the floor, not through it") are already proved.
/// S3 swaps the floor for `TrimeshCollider::from_mesh` and the assertion stays the same.
#[test]
#[ignore = "Ativado na Fase S3"]
fn a_body_dropped_from_ten_metres_rests_on_the_floor() {
    let mut world =
        PhysicsWorld3D::new(Vec3::new(0.0, -9.81, 0.0)).expect("a world with finite gravity");
    let floor = world
        .create_body(BodyDesc::static_(Vec3::new(0.0, -0.5, 0.0)))
        .expect("a static floor");
    world
        .attach_collider(floor, ColliderShape::cuboid(50.0, 0.5, 50.0), 0.6, 0.0)
        .expect("a floor collider");

    let body = world
        .create_body(BodyDesc::dynamic(Vec3::new(0.0, 10.0, 0.0)))
        .expect("a falling body");
    world
        .attach_collider(body, ColliderShape::cuboid(0.5, 0.5, 0.5), 0.6, 0.0)
        .expect("a box collider");

    for _ in 0..360 {
        world.step(1.0 / 60.0).expect("a finite delta");
    }

    let position = world.position(body).expect("a live body");
    // Resting on the floor: the box's half-height above y = 0, plus settling tolerance.
    assert!(
        position.y > 0.3 && position.y < 0.8,
        "the body is at {position:?}, not on the floor"
    );
}

#[test]
#[ignore = "Ativado na Fase S3"]
fn the_shipped_track_does_not_let_the_car_through_the_ground() {
    // Owned by `giochi-racer/tests/physics_track.rs` today, asserted here once the
    // collider is derived from the mesh: a trimesh floor must behave like the box floor.
    todo!("S3: TrimeshCollider::from_mesh on assets/tracks/track_easy.scene")
}

#[test]
#[ignore = "Ativado na Fase S3"]
fn a_convex_hull_cube_collides_like_a_cuboid() {
    todo!("S3: ConvexCollider::from_mesh vs ColliderShape::cuboid, same resting height")
}

#[test]
#[ignore = "Ativado na Fase S3"]
fn a_mesh_above_the_vertex_limit_is_split_into_chunks() {
    todo!("S3: the 64k-vertex limit documented in docs/STABILITY_DOD.md #11")
}
