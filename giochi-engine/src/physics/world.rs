//! The 2D physics world: bodies, colliders, contacts and rays.

use std::sync::Mutex;

use crate::math::Vec2;
use rapier2d::dynamics::{
    IntegrationParameters, RigidBody, RigidBodyBuilder, RigidBodyHandle, RigidBodySet,
};
use rapier2d::geometry::{
    ColliderBuilder, ColliderHandle, ColliderSet, CollisionEvent, ContactPair, Ray, RayIntersection,
};
use rapier2d::math::{Rot2, Vector};
use rapier2d::pipeline::{ActiveEvents, EventHandler, PhysicsWorld, QueryFilter};

use super::body::{BodyDesc, BodyKind, BodyTransform};
use super::collider::ColliderShape;
use super::{BodyHandle, PhysicsError, FIXED_TIMESTEP, MAX_BODIES, MAX_FRAME_DELTA, MAX_SUBSTEPS};

/// Whether two bodies started or stopped touching.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContactKind {
    /// The two bodies began touching this frame.
    Started = 0,
    /// The two bodies stopped touching this frame.
    Stopped = 1,
}

/// One collision event of the last [`PhysicsWorld2D::step`].
///
/// Reported once per pair per state change, not per frame: a ball resting on a floor
/// produces one `Started` and no `Stopped` until it leaves. A game that wants a sound on
/// every bounce asks for the velocity, not for repeated contacts.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Contact {
    /// One of the two bodies.
    pub a: BodyHandle,
    /// The other body.
    pub b: BodyHandle,
    /// Whether they started or stopped touching.
    pub kind: ContactKind,
    _reserved: [u32; 4],
}

/// What a ray hit.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RayHit {
    /// The body the ray hit, or `0` for a collider with no body.
    pub body: BodyHandle,
    /// Where the ray hit, in world units.
    pub point: Vec2,
    /// The surface normal at the hit, pointing away from the surface.
    pub normal: Vec2,
    /// Distance travelled along the (normalised) ray direction.
    pub distance: f32,
    _reserved: [u32; 4],
}

/// A live body: the Rapier handle it was given and what it is.
#[derive(Debug, Clone, Copy)]
struct BodySlot {
    body: RigidBodyHandle,
    collider: Option<ColliderHandle>,
    kind: BodyKind,
    alive: bool,
}

/// Collects collision events during a step.
///
/// Rapier calls the handler through `&self` from the stepping thread and requires `Sync`,
/// so the buffer is behind a `Mutex`. It is uncontended (one thread, one world), the
/// vector keeps its capacity across frames, and the lock is taken per event rather than
/// held across the step.
#[derive(Debug, Default)]
struct ContactSink {
    contacts: Mutex<Vec<Contact>>,
}

impl ContactSink {
    fn drain_into(&self, out: &mut Vec<Contact>) {
        out.clear();
        if let Ok(mut list) = self.contacts.lock() {
            out.append(&mut list);
        }
    }

    fn clear(&self) {
        if let Ok(mut list) = self.contacts.lock() {
            list.clear();
        }
    }
}

impl EventHandler for ContactSink {
    fn handle_collision_event(
        &self,
        _bodies: &RigidBodySet,
        colliders: &ColliderSet,
        event: CollisionEvent,
        _contact_pair: Option<&ContactPair>,
    ) {
        let (first, second, kind) = match event {
            CollisionEvent::Started(a, b, _) => (a, b, ContactKind::Started),
            CollisionEvent::Stopped(a, b, _) => (a, b, ContactKind::Stopped),
        };
        let (Some(a), Some(b)) = (colliders.get(first), colliders.get(second)) else {
            return;
        };
        let (a, b) = (a.user_data as BodyHandle, b.user_data as BodyHandle);
        if a == 0 || b == 0 {
            return;
        }
        if let Ok(mut list) = self.contacts.lock() {
            list.push(Contact {
                a,
                b,
                kind,
                _reserved: [0; 4],
            });
        }
    }

    fn handle_contact_force_event(
        &self,
        _dt: f32,
        _bodies: &RigidBodySet,
        _colliders: &ColliderSet,
        _contact_pair: &ContactPair,
        _total_force_magnitude: f32,
    ) {
    }

    fn handle_soft_body_tear_event(
        &self,
        _soft_bodies: &rapier2d::dynamics::SoftBodySet,
        _event: &rapier2d::dynamics::SoftBodyTearEvent,
    ) {
    }
}

/// A 2D physics world: gravity, bodies, colliders and contacts.
///
/// Created with [`PhysicsWorld2D::new`], stepped once per frame with
/// [`PhysicsWorld2D::step`], then read back with
/// [`PhysicsWorld2D::sync_to_transforms`] (or the per-body accessors).
///
/// The world owns its bodies; there is no handle to a deleted body that still works, by
/// design (see the module docs).
#[repr(C)]
pub struct PhysicsWorld2D {
    world: PhysicsWorld,
    bodies: Vec<BodySlot>,
    live: usize,
    accumulator: f32,
    timestep: f32,
    sink: ContactSink,
    contacts: Vec<Contact>,
    /// Whether a body or collider changed since the last step, so the raycast
    /// acceleration structures are stale.
    queries_dirty: bool,
    _reserved: [u32; 4],
}

impl std::fmt::Debug for PhysicsWorld2D {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PhysicsWorld2D")
            .field("bodies", &self.live)
            .field("timestep", &self.timestep)
            .field("gravity", &self.world.gravity)
            .finish_non_exhaustive()
    }
}

fn vector(value: Vec2) -> Vector {
    Vector::new(value.x, value.y)
}

fn vec2(value: Vector) -> Vec2 {
    Vec2::new(value.x, value.y)
}

/// `None` for a zero-length or non-finite vector: normalising those produces `NaN`
/// positions, which is exactly what a debug ray from a mouse at the body's own centre
/// would hand Rapier.
fn normalized(value: Vec2) -> Option<Vec2> {
    let length = value.length();
    if length.is_finite() && length > f32::EPSILON {
        Some(Vec2::new(value.x / length, value.y / length))
    } else {
        None
    }
}

fn collider_builder(shape: ColliderShape) -> ColliderBuilder {
    match shape {
        ColliderShape::Box {
            half_width,
            half_height,
        } => ColliderBuilder::cuboid(half_width, half_height),
        ColliderShape::Circle { radius } => ColliderBuilder::ball(radius),
        ColliderShape::Capsule {
            half_height,
            radius,
        } => ColliderBuilder::capsule_y(half_height, radius),
    }
}

impl PhysicsWorld2D {
    /// Creates a world with `gravity` (in units per second squared) and a 60 Hz step.
    ///
    /// Earth gravity for a game whose unit is the metre is `Vec2::new(0.0, -9.81)`; a
    /// platformer usually wants something like `-30.0`, because a jump arc tuned at
    /// 9.81 looks floaty at the scale of a level.
    pub fn new(gravity: Vec2) -> Result<PhysicsWorld2D, PhysicsError> {
        if !gravity.is_finite() {
            return Err(PhysicsError::InvalidBodyDesc);
        }
        let integration_parameters = IntegrationParameters {
            dt: FIXED_TIMESTEP,
            ..IntegrationParameters::default()
        };
        let world = PhysicsWorld {
            gravity: vector(gravity),
            integration_parameters,
            ..PhysicsWorld::default()
        };
        Ok(PhysicsWorld2D {
            world,
            bodies: Vec::new(),
            live: 0,
            accumulator: 0.0,
            timestep: FIXED_TIMESTEP,
            sink: ContactSink::default(),
            contacts: Vec::new(),
            queries_dirty: false,
            _reserved: [0; 4],
        })
    }

    /// The gravity currently applied.
    pub fn gravity(&self) -> Vec2 {
        vec2(self.world.gravity)
    }

    /// Sets the gravity applied from the next step on.
    pub fn set_gravity(&mut self, gravity: Vec2) -> Result<(), PhysicsError> {
        if !gravity.is_finite() {
            return Err(PhysicsError::InvalidBodyDesc);
        }
        self.world.gravity = vector(gravity);
        Ok(())
    }

    /// The integration step, in seconds.
    pub fn timestep(&self) -> f32 {
        self.timestep
    }

    /// Sets the integration step, in seconds.
    ///
    /// A game that changes this mid-run changes how the world feels; the demo uses it to
    /// slow the simulation down with a key, which is why it is not a constant.
    pub fn set_timestep(&mut self, seconds: f32) -> Result<(), PhysicsError> {
        if !seconds.is_finite() || seconds <= 0.0 {
            return Err(PhysicsError::InvalidDelta);
        }
        self.timestep = seconds.min(MAX_FRAME_DELTA);
        self.world.integration_parameters.dt = self.timestep;
        Ok(())
    }

    /// Advances the world by `delta` seconds and returns how many integration steps ran.
    ///
    /// The accumulator is the whole point: at 60 Hz with a 60 Hz step this runs exactly
    /// one step per frame, at 30 Hz it runs two, and after a 2-second stall it runs
    /// [`MAX_SUBSTEPS`] and **drops** the rest instead of spending the next 2 seconds
    /// catching up.
    pub fn step(&mut self, delta: f32) -> Result<u32, PhysicsError> {
        if !delta.is_finite() || delta < 0.0 {
            return Err(PhysicsError::InvalidDelta);
        }
        self.accumulator += delta.min(MAX_FRAME_DELTA);
        self.sink.clear();
        // The step itself updates the broad phase, so a query after it is never stale.
        self.queries_dirty = false;

        let mut steps = 0;
        while self.accumulator >= self.timestep && steps < MAX_SUBSTEPS {
            self.accumulator -= self.timestep;
            self.world.step_with_events(&(), &self.sink);
            steps += 1;
        }
        if steps == MAX_SUBSTEPS {
            self.accumulator = 0.0;
        }
        self.sink.drain_into(&mut self.contacts);
        Ok(steps)
    }

    /// How many bodies are alive.
    pub fn body_count(&self) -> usize {
        self.live
    }

    /// Creates a body and returns its handle.
    pub fn create_body(&mut self, desc: BodyDesc) -> Result<BodyHandle, PhysicsError> {
        if !desc.is_valid() {
            return Err(PhysicsError::InvalidBodyDesc);
        }
        if self.bodies.len() >= MAX_BODIES {
            return Err(PhysicsError::BodyLimitReached);
        }
        let mut builder = match desc.kind {
            BodyKind::Dynamic => RigidBodyBuilder::dynamic(),
            BodyKind::Fixed => RigidBodyBuilder::fixed(),
            BodyKind::KinematicPosition => RigidBodyBuilder::kinematic_position_based(),
        }
        .translation(vector(desc.position))
        .rotation(desc.rotation)
        .linvel(vector(desc.velocity))
        .angvel(desc.angular_velocity);
        if desc.locked_rotations {
            builder = builder.lock_rotations();
        }
        if desc.continuous_collision {
            builder = builder.ccd_enabled(true);
        }

        let handle = self.world.insert_body(builder);
        self.bodies.push(BodySlot {
            body: handle,
            collider: None,
            kind: desc.kind,
            alive: true,
        });
        self.live += 1;
        self.queries_dirty = true;
        Ok(self.bodies.len() as BodyHandle)
    }

    /// Destroys a body and its collider.
    ///
    /// Destroying an already-destroyed body is an error, not a no-op: a double despawn
    /// means the game lost track of the handle, and that is worth catching in development.
    pub fn destroy_body(&mut self, handle: BodyHandle) -> Result<(), PhysicsError> {
        let index = self.index_of(handle)?;
        let slot = self.bodies[index];
        self.world.remove_body(slot.body);
        self.bodies[index].alive = false;
        self.bodies[index].collider = None;
        self.live -= 1;
        self.queries_dirty = true;
        Ok(())
    }

    /// Whether `handle` still names a live body.
    pub fn is_alive(&self, handle: BodyHandle) -> bool {
        self.index_of(handle).is_ok()
    }

    /// What kind of body `handle` is.
    pub fn body_kind(&self, handle: BodyHandle) -> Result<BodyKind, PhysicsError> {
        Ok(self.bodies[self.index_of(handle)?].kind)
    }

    /// Gives `handle` a collision shape.
    ///
    /// `friction` is a coefficient (`0` is ice, `1` is rough) and `restitution` is how
    /// bouncy the surface is (`0` does not bounce, `1` returns the same speed). Both are
    /// clamped into a sane range instead of rejected: they usually come from a level
    /// file, and a typo should make a slippery floor, not crash a game.
    pub fn attach_collider(
        &mut self,
        handle: BodyHandle,
        shape: ColliderShape,
        friction: f32,
        restitution: f32,
    ) -> Result<(), PhysicsError> {
        if !shape.is_valid() {
            return Err(PhysicsError::InvalidShape);
        }
        let index = self.index_of(handle)?;
        if self.bodies[index].collider.is_some() {
            return Err(PhysicsError::ColliderAlreadyAttached);
        }
        let friction = if friction.is_finite() {
            friction.clamp(0.0, 10.0)
        } else {
            0.5
        };
        let restitution = if restitution.is_finite() {
            restitution.clamp(0.0, 2.0)
        } else {
            0.0
        };
        let collider = collider_builder(shape)
            .friction(friction)
            .restitution(restitution)
            .active_events(ActiveEvents::COLLISION_EVENTS)
            .user_data(handle as u128);
        let collider = self
            .world
            .insert_collider(collider, Some(self.bodies[index].body));
        self.bodies[index].collider = Some(collider);
        self.queries_dirty = true;
        Ok(())
    }

    /// Whether the body has a collider.
    pub fn has_collider(&self, handle: BodyHandle) -> bool {
        self.index_of(handle)
            .map(|index| self.bodies[index].collider.is_some())
            .unwrap_or(false)
    }

    /// Where the body is.
    pub fn position(&self, handle: BodyHandle) -> Result<Vec2, PhysicsError> {
        Ok(self.transform(handle)?.position)
    }

    /// Moves the body, keeping its velocity.
    ///
    /// This is a teleport: no collision is swept between the old and the new position. A
    /// respawn wants this; a dash does not.
    pub fn set_position(&mut self, handle: BodyHandle, position: Vec2) -> Result<(), PhysicsError> {
        if !position.is_finite() {
            return Err(PhysicsError::InvalidBodyDesc);
        }
        let body = self.body_mut(handle)?;
        body.set_translation(vector(position), true);
        self.queries_dirty = true;
        Ok(())
    }

    /// The body's rotation, in radians.
    pub fn rotation(&self, handle: BodyHandle) -> Result<f32, PhysicsError> {
        Ok(self.transform(handle)?.rotation)
    }

    /// Sets the body's rotation, in radians.
    pub fn set_rotation(&mut self, handle: BodyHandle, radians: f32) -> Result<(), PhysicsError> {
        if !radians.is_finite() {
            return Err(PhysicsError::InvalidBodyDesc);
        }
        let body = self.body_mut(handle)?;
        body.set_rotation(Rot2::from_angle(radians), true);
        Ok(())
    }

    /// The body's linear velocity, in units per second.
    pub fn velocity(&self, handle: BodyHandle) -> Result<Vec2, PhysicsError> {
        Ok(vec2(self.body(handle)?.linvel()))
    }

    /// Sets the body's linear velocity.
    pub fn set_velocity(&mut self, handle: BodyHandle, velocity: Vec2) -> Result<(), PhysicsError> {
        if !velocity.is_finite() {
            return Err(PhysicsError::InvalidBodyDesc);
        }
        let body = self.body_mut(handle)?;
        body.set_linvel(vector(velocity), true);
        Ok(())
    }

    /// The body's angular velocity, in radians per second.
    pub fn angular_velocity(&self, handle: BodyHandle) -> Result<f32, PhysicsError> {
        Ok(self.body(handle)?.angvel())
    }

    /// Sets the body's angular velocity.
    pub fn set_angular_velocity(
        &mut self,
        handle: BodyHandle,
        radians_per_second: f32,
    ) -> Result<(), PhysicsError> {
        if !radians_per_second.is_finite() {
            return Err(PhysicsError::InvalidBodyDesc);
        }
        let body = self.body_mut(handle)?;
        body.set_angvel(radians_per_second, true);
        Ok(())
    }

    /// Applies an impulse (mass times velocity) at the centre of mass.
    ///
    /// An impulse changes velocity immediately; the same value applied as a force over a
    /// frame would not, because the frame is 1/60 of a second.
    pub fn apply_impulse(&mut self, handle: BodyHandle, impulse: Vec2) -> Result<(), PhysicsError> {
        if !impulse.is_finite() {
            return Err(PhysicsError::InvalidBodyDesc);
        }
        let body = self.body_mut(handle)?;
        body.apply_impulse(vector(impulse), true);
        Ok(())
    }

    /// The body's whole transform, for syncing to a sprite.
    pub fn transform(&self, handle: BodyHandle) -> Result<BodyTransform, PhysicsError> {
        let body = self.body(handle)?;
        Ok(BodyTransform::new(
            vec2(body.translation()),
            body.rotation().angle(),
        ))
    }

    /// Teleports the body, keeping its velocity.
    pub fn teleport(
        &mut self,
        handle: BodyHandle,
        transform: BodyTransform,
    ) -> Result<(), PhysicsError> {
        if !transform.is_finite() {
            return Err(PhysicsError::InvalidBodyDesc);
        }
        let body = self.body_mut(handle)?;
        body.set_translation(vector(transform.position), true);
        body.set_rotation(Rot2::from_angle(transform.rotation), true);
        self.queries_dirty = true;
        Ok(())
    }

    /// Moves a kinematic body to a position over the next step.
    ///
    /// Different from [`PhysicsWorld2D::set_position`]: a kinematic body that is *moved*
    /// pushes dynamic bodies and is not dragged by them, while a body that is *teleported*
    /// can end up inside one.
    pub fn move_kinematic(
        &mut self,
        handle: BodyHandle,
        transform: BodyTransform,
    ) -> Result<(), PhysicsError> {
        if !transform.is_finite() {
            return Err(PhysicsError::InvalidBodyDesc);
        }
        let body = self.body_mut(handle)?;
        body.set_next_kinematic_translation(vector(transform.position));
        body.set_next_kinematic_rotation(Rot2::from_angle(transform.rotation));
        self.queries_dirty = true;
        Ok(())
    }

    /// Calls `apply` with every live body's transform.
    ///
    /// The usual end of a frame: the world moved the bodies, now the sprites follow.
    /// Cheap because nothing is collected — the callback runs while the world is borrowed
    /// immutably, and there is no intermediate vector.
    pub fn sync_to_transforms(&self, apply: &mut dyn FnMut(BodyHandle, BodyTransform)) {
        for index in 0..self.bodies.len() {
            let slot = self.bodies[index];
            if !slot.alive {
                continue;
            }
            let Some(body) = self.world.bodies.get(slot.body) else {
                continue;
            };
            let transform = BodyTransform::new(vec2(body.translation()), body.rotation().angle());
            apply(index as BodyHandle + 1, transform);
        }
    }

    /// Calls `sample` for every live body and teleports the ones it returns a transform for.
    ///
    /// The other direction of the sync, for a respawn or a level reset: the game knows
    /// where an entity should be and the world follows. Returning `None` leaves the body
    /// alone, so the callback only has to answer for the entities it owns.
    pub fn sync_from_transforms(
        &mut self,
        sample: &mut dyn FnMut(BodyHandle) -> Option<BodyTransform>,
    ) {
        for index in 0..self.bodies.len() {
            let slot = self.bodies[index];
            if !slot.alive {
                continue;
            }
            let Some(transform) = sample(index as BodyHandle + 1) else {
                continue;
            };
            if !transform.is_finite() {
                continue;
            }
            if let Some(body) = self.world.bodies.get_mut(slot.body) {
                body.set_translation(vector(transform.position), true);
                body.set_rotation(Rot2::from_angle(transform.rotation), true);
                self.queries_dirty = true;
            }
        }
    }

    /// Brings the acceleration structures used by raycasts up to date without simulating.
    ///
    /// [`PhysicsWorld2D::raycast`] calls this itself when the world changed since the last
    /// step, so a game never has to. It is public for the case where a game wants the cost
    /// to happen at a known point rather than inside a query.
    ///
    /// The implementation is a physics step with `dt = 0`: Rapier has no separate
    /// "refresh the broad phase" entry point, and its own `detect_collisions` leaves the
    /// world in a state where the **next** step no longer moves a dynamic body (measured,
    /// see `PERF_LOG.md`). A zero-delta step updates the same structures through the
    /// supported path, moves nothing, and leaves the simulation ready for the next frame.
    pub fn refresh_queries(&mut self) {
        let dt = self.world.integration_parameters.dt;
        self.world.integration_parameters.dt = 0.0;
        self.world.step_with_events(&(), &());
        self.world.integration_parameters.dt = dt;
        self.queries_dirty = false;
    }

    /// The contacts reported by the last [`PhysicsWorld2D::step`].
    pub fn contacts(&self) -> &[Contact] {
        &self.contacts
    }

    /// Casts a ray and returns the first body it hits.
    ///
    /// `direction` is normalised for you; `max_distance` is in world units. Sensors and
    /// colliders with no body are skipped, so the result is always an entity a game can
    /// act on.
    pub fn raycast(
        &mut self,
        origin: Vec2,
        direction: Vec2,
        max_distance: f32,
    ) -> Result<Option<RayHit>, PhysicsError> {
        if self.queries_dirty {
            self.refresh_queries();
        }
        if !origin.is_finite() || !max_distance.is_finite() || max_distance < 0.0 {
            return Err(PhysicsError::InvalidRay);
        }
        let direction = normalized(direction).ok_or(PhysicsError::InvalidRay)?;
        let ray = Ray::new(vector(origin), vector(direction));
        let hit =
            self.world
                .cast_ray_and_get_normal(&ray, max_distance, true, QueryFilter::default());
        let Some((
            collider,
            RayIntersection {
                time_of_impact,
                normal,
                ..
            },
        )) = hit
        else {
            return Ok(None);
        };
        let body = self
            .world
            .colliders
            .get(collider)
            .map(|collider| collider.user_data as BodyHandle)
            .unwrap_or(0);
        Ok(Some(RayHit {
            body,
            point: origin + direction * time_of_impact,
            normal: vec2(normal),
            distance: time_of_impact,
            _reserved: [0; 4],
        }))
    }

    fn index_of(&self, handle: BodyHandle) -> Result<usize, PhysicsError> {
        let index = handle.checked_sub(1).ok_or(PhysicsError::InvalidBody)? as usize;
        match self.bodies.get(index) {
            Some(slot) if slot.alive => Ok(index),
            _ => Err(PhysicsError::InvalidBody),
        }
    }

    fn body(&self, handle: BodyHandle) -> Result<&RigidBody, PhysicsError> {
        let slot = self.bodies[self.index_of(handle)?];
        self.world
            .bodies
            .get(slot.body)
            .ok_or(PhysicsError::InvalidBody)
    }

    fn body_mut(&mut self, handle: BodyHandle) -> Result<&mut RigidBody, PhysicsError> {
        let slot = self.bodies[self.index_of(handle)?];
        self.world
            .bodies
            .get_mut(slot.body)
            .ok_or(PhysicsError::InvalidBody)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world() -> PhysicsWorld2D {
        PhysicsWorld2D::new(Vec2::new(0.0, -10.0)).expect("world")
    }

    /// A fixed floor whose top surface is at `y = top`.
    fn floor(world: &mut PhysicsWorld2D, top: f32) {
        let handle = world
            .create_body(BodyDesc::static_(Vec2::new(0.0, top - 0.5)))
            .expect("floor");
        world
            .attach_collider(handle, ColliderShape::boxed(50.0, 0.5), 0.9, 0.0)
            .expect("floor collider");
    }

    #[test]
    fn a_body_falls_and_stops_on_the_floor() {
        let mut world = world();
        floor(&mut world, 0.0);
        let ball = world
            .create_body(BodyDesc::dynamic(Vec2::new(0.0, 5.0)))
            .expect("ball");
        world
            .attach_collider(ball, ColliderShape::circle(0.5), 0.5, 0.0)
            .expect("ball collider");

        for _ in 0..300 {
            world.step(FIXED_TIMESTEP).expect("step");
        }
        let position = world.position(ball).expect("position");
        // Resting on the floor means the centre is one radius above it, within the
        // solver's penetration allowance.
        assert!(
            (position.y - 0.5).abs() < 0.05,
            "ball rested at {position:?}, expected y ~ 0.5"
        );
        assert!(
            position.x.abs() < 0.01,
            "the ball drifted sideways: {position:?}"
        );
    }

    #[test]
    fn the_accumulator_runs_the_steps_a_slow_frame_owes_and_drops_the_rest() {
        let mut world = world();
        assert_eq!(world.step(0.0).expect("step"), 0);
        assert_eq!(world.step(1.0 / 120.0).expect("step"), 0);
        assert_eq!(world.step(1.0 / 120.0).expect("step"), 1);
        // 100 steps' worth of time: only MAX_SUBSTEPS run, and the backlog is dropped.
        assert_eq!(world.step(2.0).expect("step"), MAX_SUBSTEPS);
        assert_eq!(world.step(0.0).expect("step"), 0);
    }

    #[test]
    fn handles_are_never_reused_and_dead_ones_are_errors() {
        let mut world = world();
        let first = world
            .create_body(BodyDesc::dynamic(Vec2::ZERO))
            .expect("body");
        world.destroy_body(first).expect("destroy");
        assert!(!world.is_alive(first));
        assert_eq!(world.destroy_body(first), Err(PhysicsError::InvalidBody));
        assert_eq!(world.position(first), Err(PhysicsError::InvalidBody));

        let second = world
            .create_body(BodyDesc::dynamic(Vec2::ZERO))
            .expect("body");
        assert_ne!(first, second, "a handle was reused after destroy");
        assert_eq!(world.body_count(), 1);
    }

    #[test]
    fn bad_input_is_rejected_instead_of_reaching_rapier() {
        let mut world = world();
        assert_eq!(
            world.create_body(BodyDesc::dynamic(Vec2::new(f32::NAN, 0.0))),
            Err(PhysicsError::InvalidBodyDesc)
        );
        assert_eq!(world.step(-1.0), Err(PhysicsError::InvalidDelta));
        assert_eq!(world.step(f32::NAN), Err(PhysicsError::InvalidDelta));
        assert_eq!(world.set_timestep(0.0), Err(PhysicsError::InvalidDelta));
        assert_eq!(
            world.raycast(Vec2::ZERO, Vec2::ZERO, 1.0),
            Err(PhysicsError::InvalidRay)
        );

        let body = world
            .create_body(BodyDesc::dynamic(Vec2::ZERO))
            .expect("body");
        assert_eq!(
            world.attach_collider(body, ColliderShape::circle(0.0), 0.5, 0.0),
            Err(PhysicsError::InvalidShape)
        );
        world
            .attach_collider(body, ColliderShape::circle(0.5), 0.5, 0.0)
            .expect("collider");
        assert_eq!(
            world.attach_collider(body, ColliderShape::circle(0.5), 0.5, 0.0),
            Err(PhysicsError::ColliderAlreadyAttached)
        );
    }

    #[test]
    fn a_landing_reports_one_started_contact_and_one_stopped_when_it_bounces_away() {
        let mut world = world();
        floor(&mut world, 0.0);
        let ball = world
            .create_body(BodyDesc::dynamic(Vec2::new(0.0, 3.0)))
            .expect("ball");
        world
            .attach_collider(ball, ColliderShape::circle(0.5), 0.5, 0.9)
            .expect("collider");
        let floor_body = 1u64;

        let mut started = 0;
        let mut stopped = 0;
        for _ in 0..240 {
            world.step(FIXED_TIMESTEP).expect("step");
            for contact in world.contacts() {
                let pair = (contact.a, contact.b);
                assert!(
                    pair == (ball, floor_body) || pair == (floor_body, ball),
                    "unexpected contact pair {pair:?}"
                );
                match contact.kind {
                    ContactKind::Started => started += 1,
                    ContactKind::Stopped => stopped += 1,
                }
            }
        }
        assert!(started >= 1, "a bouncing ball never reported a contact");
        assert!(stopped >= 1, "the ball never separated from the floor");
    }

    #[test]
    fn sync_moves_transforms_both_ways() {
        let mut world = world();
        let body = world
            .create_body(BodyDesc::dynamic(Vec2::new(1.0, 2.0)).with_rotation(0.5))
            .expect("body");

        let mut seen = 0;
        world.sync_to_transforms(&mut |handle, transform| {
            if handle == body {
                seen += 1;
                assert_eq!(transform.position, Vec2::new(1.0, 2.0));
                assert!((transform.rotation - 0.5).abs() < 1e-6);
            }
        });
        assert_eq!(seen, 1);

        world.sync_from_transforms(&mut |handle| {
            (handle == body).then(|| BodyTransform::new(Vec2::new(-3.0, 4.0), 0.25))
        });
        assert_eq!(
            world.position(body).expect("position"),
            Vec2::new(-3.0, 4.0)
        );
        assert!((world.rotation(body).expect("rotation") - 0.25).abs() < 1e-6);
        // A body the callback does not answer for keeps its transform.
        let other = world
            .create_body(BodyDesc::dynamic(Vec2::new(7.0, 7.0)))
            .expect("body");
        world.sync_from_transforms(&mut |handle| {
            (handle == body).then(|| BodyTransform::new(Vec2::new(-3.0, 4.0), 0.25))
        });
        assert_eq!(
            world.position(other).expect("position"),
            Vec2::new(7.0, 7.0)
        );
    }

    #[test]
    fn a_ray_hits_the_nearest_body_and_misses_when_short() {
        let mut world = world();
        let wall = world
            .create_body(BodyDesc::static_(Vec2::new(5.0, 0.0)))
            .expect("wall");
        world
            .attach_collider(wall, ColliderShape::boxed(0.5, 5.0), 0.5, 0.0)
            .expect("collider");

        let hit = world
            .raycast(Vec2::ZERO, Vec2::new(1.0, 0.0), 10.0)
            .expect("raycast")
            .expect("hit");
        assert_eq!(hit.body, wall);
        assert!(
            (hit.distance - 4.5).abs() < 0.01,
            "hit at {} expected 4.5",
            hit.distance
        );
        assert!((hit.point.x - 4.5).abs() < 0.01);
        assert!((hit.normal.x + 1.0).abs() < 0.01, "normal {:?}", hit.normal);

        assert!(world
            .raycast(Vec2::ZERO, Vec2::new(1.0, 0.0), 2.0)
            .expect("raycast")
            .is_none());
    }

    #[test]
    fn gravity_and_timestep_can_be_changed_and_are_validated() {
        let mut world = world();
        assert_eq!(world.gravity(), Vec2::new(0.0, -10.0));
        world.set_gravity(Vec2::new(1.0, -2.0)).expect("gravity");
        assert_eq!(world.gravity(), Vec2::new(1.0, -2.0));
        assert_eq!(
            world.set_gravity(Vec2::new(f32::INFINITY, 0.0)),
            Err(PhysicsError::InvalidBodyDesc)
        );
        assert_eq!(world.timestep(), FIXED_TIMESTEP);
        world.set_timestep(1.0 / 120.0).expect("timestep");
        assert_eq!(world.timestep(), 1.0 / 120.0);
    }
}
