//! The 3D physics world: bodies, colliders, contacts, rays and the character controller.

use std::sync::Mutex;

use rapier3d::control::{CharacterAutostep, CharacterLength, KinematicCharacterController};
use rapier3d::dynamics::{
    IntegrationParameters, RigidBody, RigidBodyBuilder, RigidBodyHandle, RigidBodySet,
};
use rapier3d::geometry::{
    Capsule, ColliderBuilder, ColliderHandle, ColliderSet, CollisionEvent, ContactPair, Ray,
    RayIntersection,
};
use rapier3d::math::{Pose, Rotation, Vector};
use rapier3d::pipeline::{ActiveEvents, EventHandler, PhysicsWorld, QueryFilter};

use crate::math3d::{Quat as EngineQuat, Vec3};

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

/// One collision event of the last [`PhysicsWorld3D::step`].
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
    pub point: Vec3,
    /// The surface normal at the hit.
    pub normal: Vec3,
    /// Distance travelled along the normalised ray direction.
    pub distance: f32,
    _reserved: [u32; 4],
}

/// How far a character actually moved, and whether it is standing on something.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CharacterMove {
    /// The translation the controller allowed (input minus what a wall blocked).
    pub translation: Vec3,
    /// Whether the character is touching the ground after the move.
    pub grounded: bool,
    _reserved: [u32; 4],
}

#[derive(Debug, Clone, Copy)]
struct BodySlot {
    body: RigidBodyHandle,
    collider: Option<ColliderHandle>,
    kind: BodyKind,
    /// Capsule dimensions, for bodies created with `create_character`.
    character: Option<(f32, f32)>,
    alive: bool,
}

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
        _soft_bodies: &rapier3d::dynamics::SoftBodySet,
        _event: &rapier3d::dynamics::SoftBodyTearEvent,
    ) {
    }
}

/// A 3D physics world: gravity, bodies, colliders, contacts and characters.
#[repr(C)]
pub struct PhysicsWorld3D {
    world: PhysicsWorld,
    bodies: Vec<BodySlot>,
    live: usize,
    accumulator: f32,
    timestep: f32,
    sink: ContactSink,
    contacts: Vec<Contact>,
    queries_dirty: bool,
    _reserved: [u32; 4],
}

impl std::fmt::Debug for PhysicsWorld3D {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PhysicsWorld3D")
            .field("bodies", &self.live)
            .field("timestep", &self.timestep)
            .field("gravity", &self.world.gravity)
            .finish_non_exhaustive()
    }
}

fn vector(value: Vec3) -> Vector {
    Vector::new(value.x, value.y, value.z)
}

fn vec3(value: Vector) -> Vec3 {
    Vec3::new(value.x, value.y, value.z)
}

fn quat(value: EngineQuat) -> Rotation {
    Rotation::from_xyzw(value.x, value.y, value.z, value.w)
}

fn engine_quat(value: Rotation) -> EngineQuat {
    EngineQuat::new(value.x, value.y, value.z, value.w)
}

fn normalized(value: Vec3) -> Option<Vec3> {
    let length = value.length();
    if length.is_finite() && length > f32::EPSILON {
        Some(Vec3::from_glam(value.to_glam() / length))
    } else {
        None
    }
}

fn collider_builder(shape: ColliderShape) -> ColliderBuilder {
    match shape {
        ColliderShape::Cuboid {
            half_width,
            half_height,
            half_depth,
        } => ColliderBuilder::cuboid(half_width, half_height, half_depth),
        ColliderShape::Ball { radius } => ColliderBuilder::ball(radius),
        ColliderShape::Capsule {
            half_height,
            radius,
        } => ColliderBuilder::capsule_y(half_height, radius),
    }
}

impl PhysicsWorld3D {
    /// Creates a world with `gravity` (units per second squared) and a 60 Hz step.
    ///
    /// Earth gravity for a game whose unit is the metre is `Vec3::new(0.0, -9.81, 0.0)`.
    pub fn new(gravity: Vec3) -> Result<PhysicsWorld3D, PhysicsError> {
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
        Ok(PhysicsWorld3D {
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
    pub fn gravity(&self) -> Vec3 {
        vec3(self.world.gravity)
    }

    /// Sets the gravity applied from the next step on.
    pub fn set_gravity(&mut self, gravity: Vec3) -> Result<(), PhysicsError> {
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
    pub fn set_timestep(&mut self, seconds: f32) -> Result<(), PhysicsError> {
        if !seconds.is_finite() || seconds <= 0.0 {
            return Err(PhysicsError::InvalidDelta);
        }
        self.timestep = seconds.min(MAX_FRAME_DELTA);
        self.world.integration_parameters.dt = self.timestep;
        Ok(())
    }

    /// Advances the world by `delta` seconds and returns how many integration steps ran.
    pub fn step(&mut self, delta: f32) -> Result<u32, PhysicsError> {
        if !delta.is_finite() || delta < 0.0 {
            return Err(PhysicsError::InvalidDelta);
        }
        self.accumulator += delta.min(MAX_FRAME_DELTA);
        self.sink.clear();
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
        .pose(Pose::from_parts(vector(desc.position), quat(desc.rotation)))
        .linvel(vector(desc.velocity))
        .angvel(vector(desc.angular_velocity));
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
            character: None,
            alive: true,
        });
        self.live += 1;
        self.queries_dirty = true;
        Ok(self.bodies.len() as BodyHandle)
    }

    /// Destroys a body and its collider.
    pub fn destroy_body(&mut self, handle: BodyHandle) -> Result<(), PhysicsError> {
        let index = self.index_of(handle)?;
        let slot = self.bodies[index];
        self.world.remove_body(slot.body);
        self.bodies[index].alive = false;
        self.bodies[index].collider = None;
        self.bodies[index].character = None;
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
    /// `friction` and `restitution` are clamped into a sane range instead of rejected.
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

    /// Creates a kinematic capsule with the character controller attached.
    ///
    /// The body is kinematic (it is moved by the game, not by gravity) and its rotations
    /// are locked. Drive it with [`PhysicsWorld3D::move_character`].
    pub fn create_character(
        &mut self,
        position: Vec3,
        radius: f32,
        half_height: f32,
    ) -> Result<BodyHandle, PhysicsError> {
        let handle = self.create_body(BodyDesc::kinematic(position).lock_rotations())?;
        self.attach_collider(
            handle,
            ColliderShape::capsule(half_height, radius),
            0.4,
            0.0,
        )?;
        let index = self.index_of(handle)?;
        self.bodies[index].character = Some((radius, half_height));
        Ok(handle)
    }

    /// Moves a character by `desired`, sliding along walls and stopping at floors.
    ///
    /// Returns how much it actually moved and whether it ended up grounded — the two
    /// things a first-person controller needs (`grounded` is what a jump checks). The
    /// body's transform is updated; do not also call `move_kinematic` for the same body.
    pub fn move_character(
        &mut self,
        handle: BodyHandle,
        desired: Vec3,
        delta: f32,
    ) -> Result<CharacterMove, PhysicsError> {
        if !desired.is_finite() || !delta.is_finite() || delta < 0.0 {
            return Err(PhysicsError::InvalidDelta);
        }
        let index = self.index_of(handle)?;
        let (radius, half_height) = self.bodies[index]
            .character
            .ok_or(PhysicsError::NotACharacter)?;
        let slot = self.bodies[index];
        let body = self
            .world
            .bodies
            .get(slot.body)
            .ok_or(PhysicsError::InvalidBody)?;
        let pose = Pose::from_parts(
            vector(vec3(body.translation())),
            quat(engine_quat(*body.rotation())),
        );

        let offset = CharacterLength::Absolute(0.02);
        let controller = KinematicCharacterController {
            offset,
            slide: true,
            // Snapping is what keeps a walker from launching off every step down; the
            // step height lets it walk up a kerb without a jump.
            snap_to_ground: Some(CharacterLength::Absolute(0.3)),
            autostep: Some(CharacterAutostep {
                max_height: CharacterLength::Absolute(0.35),
                min_width: CharacterLength::Absolute(0.2),
                include_dynamic_bodies: false,
            }),
            ..KinematicCharacterController::default()
        };
        let shape = Capsule::new_y(half_height, radius);

        // The character's own capsule has to be out of the query. Rapier's own examples
        // use a collider-less body and cast the shape by hand; here the capsule *is* the
        // collider, so without this filter every cast hits the character itself and
        // `move_shape` returns a translation projected onto its own surface — a walker
        // that drifts sideways, cannot pass a doorway and slowly falls out of the world.
        let filter = match slot.collider {
            Some(collider) => QueryFilter::default().exclude_collider(collider),
            None => QueryFilter::default(),
        };
        let movement = {
            let queries = self.world.query_pipeline_with_filter(filter);
            controller.move_shape(
                delta.min(self.timestep),
                &queries,
                &shape,
                &pose,
                vector(desired),
                |_collision| {},
            )
        };

        let translation = vec3(movement.translation);
        if let Some(body) = self.world.bodies.get_mut(slot.body) {
            let next = Vec3::new(
                pose.translation.x + movement.translation.x,
                pose.translation.y + movement.translation.y,
                pose.translation.z + movement.translation.z,
            );
            body.set_next_kinematic_translation(vector(next));
            body.set_next_kinematic_rotation(quat(engine_quat(*body.rotation())));
        }
        self.queries_dirty = true;
        Ok(CharacterMove {
            translation,
            grounded: movement.grounded,
            _reserved: [0; 4],
        })
    }

    /// Whether the body has a collider.
    pub fn has_collider(&self, handle: BodyHandle) -> bool {
        self.index_of(handle)
            .map(|index| self.bodies[index].collider.is_some())
            .unwrap_or(false)
    }

    /// Where the body is.
    pub fn position(&self, handle: BodyHandle) -> Result<Vec3, PhysicsError> {
        Ok(self.transform(handle)?.position)
    }

    /// Moves the body, keeping its velocity. This is a teleport: nothing is swept.
    pub fn set_position(&mut self, handle: BodyHandle, position: Vec3) -> Result<(), PhysicsError> {
        if !position.is_finite() {
            return Err(PhysicsError::InvalidBodyDesc);
        }
        let body = self.body_mut(handle)?;
        body.set_translation(vector(position), true);
        self.queries_dirty = true;
        Ok(())
    }

    /// The body's orientation.
    pub fn rotation(&self, handle: BodyHandle) -> Result<EngineQuat, PhysicsError> {
        Ok(self.transform(handle)?.rotation)
    }

    /// Sets the body's orientation.
    pub fn set_rotation(
        &mut self,
        handle: BodyHandle,
        rotation: EngineQuat,
    ) -> Result<(), PhysicsError> {
        if !rotation.is_finite() {
            return Err(PhysicsError::InvalidBodyDesc);
        }
        let body = self.body_mut(handle)?;
        body.set_rotation(quat(rotation), true);
        Ok(())
    }

    /// The body's linear velocity, in units per second.
    pub fn velocity(&self, handle: BodyHandle) -> Result<Vec3, PhysicsError> {
        Ok(vec3(self.body(handle)?.linvel()))
    }

    /// Sets the body's linear velocity.
    pub fn set_velocity(&mut self, handle: BodyHandle, velocity: Vec3) -> Result<(), PhysicsError> {
        if !velocity.is_finite() {
            return Err(PhysicsError::InvalidBodyDesc);
        }
        let body = self.body_mut(handle)?;
        body.set_linvel(vector(velocity), true);
        Ok(())
    }

    /// The body's angular velocity, in radians per second around each axis.
    pub fn angular_velocity(&self, handle: BodyHandle) -> Result<Vec3, PhysicsError> {
        Ok(vec3(self.body(handle)?.angvel()))
    }

    /// Sets the body's angular velocity.
    pub fn set_angular_velocity(
        &mut self,
        handle: BodyHandle,
        angular_velocity: Vec3,
    ) -> Result<(), PhysicsError> {
        if !angular_velocity.is_finite() {
            return Err(PhysicsError::InvalidBodyDesc);
        }
        let body = self.body_mut(handle)?;
        body.set_angvel(vector(angular_velocity), true);
        Ok(())
    }

    /// Applies an impulse (mass times velocity) at the centre of mass.
    pub fn apply_impulse(&mut self, handle: BodyHandle, impulse: Vec3) -> Result<(), PhysicsError> {
        if !impulse.is_finite() {
            return Err(PhysicsError::InvalidBodyDesc);
        }
        let body = self.body_mut(handle)?;
        body.apply_impulse(vector(impulse), true);
        Ok(())
    }

    /// The body's whole transform, for syncing to a mesh or a camera.
    pub fn transform(&self, handle: BodyHandle) -> Result<BodyTransform, PhysicsError> {
        let body = self.body(handle)?;
        Ok(BodyTransform::new(
            vec3(body.translation()),
            engine_quat(*body.rotation()),
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
        body.set_rotation(quat(transform.rotation), true);
        self.queries_dirty = true;
        Ok(())
    }

    /// Moves a kinematic body to a transform over the next step.
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
        body.set_next_kinematic_rotation(quat(transform.rotation));
        self.queries_dirty = true;
        Ok(())
    }

    /// Calls `apply` with every live body's transform.
    pub fn sync_to_transforms(&self, apply: &mut dyn FnMut(BodyHandle, BodyTransform)) {
        for index in 0..self.bodies.len() {
            let slot = self.bodies[index];
            if !slot.alive {
                continue;
            }
            let Some(body) = self.world.bodies.get(slot.body) else {
                continue;
            };
            let transform =
                BodyTransform::new(vec3(body.translation()), engine_quat(*body.rotation()));
            apply(index as BodyHandle + 1, transform);
        }
    }

    /// Calls `sample` for every live body and teleports the ones it answers for.
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
                body.set_rotation(quat(transform.rotation), true);
                self.queries_dirty = true;
            }
        }
    }

    /// Brings the raycast acceleration structures up to date without simulating.
    ///
    /// `raycast` calls this itself when the world changed since the last step.
    pub fn refresh_queries(&mut self) {
        let dt = self.world.integration_parameters.dt;
        self.world.integration_parameters.dt = 0.0;
        self.world.step_with_events(&(), &());
        self.world.integration_parameters.dt = dt;
        self.queries_dirty = false;
    }

    /// The contacts reported by the last [`PhysicsWorld3D::step`].
    pub fn contacts(&self) -> &[Contact] {
        &self.contacts
    }

    /// Casts a ray and returns the first body it hits.
    pub fn raycast(
        &mut self,
        origin: Vec3,
        direction: Vec3,
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
            point: Vec3::new(
                origin.x + direction.x * time_of_impact,
                origin.y + direction.y * time_of_impact,
                origin.z + direction.z * time_of_impact,
            ),
            normal: vec3(normal),
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

    fn world() -> PhysicsWorld3D {
        PhysicsWorld3D::new(Vec3::new(0.0, -10.0, 0.0)).expect("world")
    }

    fn floor(world: &mut PhysicsWorld3D, top: f32) {
        let handle = world
            .create_body(BodyDesc::static_(Vec3::new(0.0, top - 0.5, 0.0)))
            .expect("floor");
        world
            .attach_collider(handle, ColliderShape::cuboid(20.0, 0.5, 20.0), 0.9, 0.0)
            .expect("collider");
    }

    #[test]
    fn a_ball_falls_and_rests_on_the_floor() {
        let mut world = world();
        floor(&mut world, 0.0);
        let ball = world
            .create_body(BodyDesc::dynamic(Vec3::new(0.0, 4.0, 0.0)))
            .expect("ball");
        world
            .attach_collider(ball, ColliderShape::ball(0.5), 0.5, 0.0)
            .expect("collider");

        for _ in 0..300 {
            world.step(FIXED_TIMESTEP).expect("step");
        }
        let position = world.position(ball).expect("position");
        assert!(
            (position.y - 0.5).abs() < 0.05,
            "ball rested at {position:?}, expected y ~ 0.5"
        );
    }

    #[test]
    fn the_accumulator_caps_the_steps_a_stall_can_run() {
        let mut world = world();
        assert_eq!(world.step(0.0).expect("step"), 0);
        assert_eq!(world.step(2.0).expect("step"), MAX_SUBSTEPS);
        assert_eq!(world.step(0.0).expect("step"), 0);
    }

    #[test]
    fn bad_input_is_rejected() {
        let mut world = world();
        assert_eq!(
            world.create_body(BodyDesc::dynamic(Vec3::new(0.0, f32::NAN, 0.0))),
            Err(PhysicsError::InvalidBodyDesc)
        );
        assert_eq!(world.step(-1.0), Err(PhysicsError::InvalidDelta));
        assert_eq!(
            world.raycast(Vec3::ZERO, Vec3::ZERO, 1.0),
            Err(PhysicsError::InvalidRay)
        );
        let body = world
            .create_body(BodyDesc::dynamic(Vec3::ZERO))
            .expect("body");
        assert_eq!(
            world.attach_collider(body, ColliderShape::ball(0.0), 0.5, 0.0),
            Err(PhysicsError::InvalidShape)
        );
        assert_eq!(
            world.move_character(body, Vec3::ZERO, FIXED_TIMESTEP),
            Err(PhysicsError::NotACharacter)
        );
    }

    #[test]
    fn a_character_walks_the_distance_it_asked_for_on_open_floor() {
        // Regression: the character's own capsule was part of the query filter, so every
        // shape cast hit the character itself and `move_shape` returned a translation
        // projected onto its own surface. The symptom was a walker that drifted, could
        // not approach a wall and eventually fell out of the world — with every other
        // test still green, because a constant direction hides the drift.
        let mut world = world();
        floor(&mut world, 0.0);
        let character = world
            .create_character(Vec3::new(0.0, 0.85, 0.0), 0.3, 0.55)
            .expect("character");

        // One warm-up cast: the first one runs before the broad phase has seen the
        // character's collider, so it cannot report `grounded` yet. A real game does not
        // care — it reads `grounded` from the second frame on — but the assertion below
        // would be about the warm-up otherwise.
        world
            .move_character(character, Vec3::ZERO, FIXED_TIMESTEP)
            .expect("warm-up move");
        world.step(FIXED_TIMESTEP).expect("step");

        // Ten frames of 0.05 units forward, on open floor, with the walker grounded.
        let mut total = 0.0f32;
        for _ in 0..10 {
            let movement = world
                .move_character(character, Vec3::new(0.05, 0.0, 0.0), FIXED_TIMESTEP)
                .expect("move");
            assert!(
                movement.grounded,
                "a character standing on the floor lost contact: {movement:?}"
            );
            total += movement.translation.x;
            // The z drift that the self-collision produced was the visible symptom.
            assert!(
                movement.translation.z.abs() < 1.0e-3,
                "moving along x drifted {:.4} along z",
                movement.translation.z
            );
            world.step(FIXED_TIMESTEP).expect("step");
        }
        assert!(
            (total - 0.5).abs() < 0.05,
            "asked to move 0.5 units forward, the controller allowed {total}"
        );
        let position = world.position(character).expect("position");
        assert!(
            (position.x - 0.5).abs() < 0.05,
            "the character ended at x = {}, expected 0.5",
            position.x
        );
        assert!(
            (position.y - 0.85).abs() < 0.02,
            "the walker sank while walking: y = {}",
            position.y
        );
    }

    #[test]
    fn a_character_walks_into_a_wall_and_stops() {
        let mut world = world();
        floor(&mut world, 0.0);
        let wall = world
            .create_body(BodyDesc::static_(Vec3::new(2.0, 2.0, 0.0)))
            .expect("wall");
        world
            .attach_collider(wall, ColliderShape::cuboid(0.5, 2.0, 5.0), 0.5, 0.0)
            .expect("collider");

        let character = world
            .create_character(Vec3::new(0.0, 0.9, 0.0), 0.3, 0.6)
            .expect("character");

        let mut grounded = false;
        for _ in 0..240 {
            // Gravity is the game's job on a kinematic body: the controller only slides
            // along what it is told to move into, so a walker with no vertical input
            // never touches the floor and never reports being grounded.
            let movement = world
                .move_character(character, Vec3::new(0.1, -0.05, 0.0), FIXED_TIMESTEP)
                .expect("move");
            grounded |= movement.grounded;
            world.step(FIXED_TIMESTEP).expect("step");
        }
        let position = world.position(character).expect("position");
        assert!(
            position.x < 1.2,
            "the character walked through the wall: {position:?}"
        );
        assert!(position.x > 0.5, "the character never moved: {position:?}");
        assert!(grounded, "the character never reported being grounded");
    }

    #[test]
    fn a_character_falls_onto_the_floor_and_stops() {
        let mut world = world();
        floor(&mut world, 0.0);
        let character = world
            .create_character(Vec3::new(0.0, 5.0, 0.0), 0.3, 0.6)
            .expect("character");
        let mut grounded = false;
        for _ in 0..180 {
            let movement = world
                .move_character(character, Vec3::new(0.0, -0.2, 0.0), FIXED_TIMESTEP)
                .expect("move");
            grounded |= movement.grounded;
            world.step(FIXED_TIMESTEP).expect("step");
        }
        let position = world.position(character).expect("position");
        assert!(
            position.y < 1.5 && position.y > 0.0,
            "the character did not land: {position:?}"
        );
        assert!(
            grounded,
            "the landed character never reported being grounded"
        );
    }

    #[test]
    fn a_ray_hits_the_nearest_body_and_contacts_are_reported() {
        let mut world = world();
        let crate_body = world
            .create_body(BodyDesc::static_(Vec3::new(3.0, 0.0, 0.0)))
            .expect("crate");
        world
            .attach_collider(crate_body, ColliderShape::cuboid(0.5, 0.5, 0.5), 0.5, 0.0)
            .expect("collider");

        let hit = world
            .raycast(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), 10.0)
            .expect("raycast")
            .expect("hit");
        assert_eq!(hit.body, crate_body);
        assert!((hit.distance - 2.5).abs() < 0.01, "hit at {}", hit.distance);

        floor(&mut world, 0.0);
        let ball = world
            .create_body(BodyDesc::dynamic(Vec3::new(0.0, 1.0, 0.0)))
            .expect("ball");
        world
            .attach_collider(ball, ColliderShape::ball(0.4), 0.5, 0.0)
            .expect("collider");
        let mut started = false;
        for _ in 0..180 {
            world.step(FIXED_TIMESTEP).expect("step");
            started |= world
                .contacts()
                .iter()
                .any(|contact| contact.kind == ContactKind::Started);
        }
        assert!(started, "the ball never reported landing");
    }

    #[test]
    fn handles_are_never_reused() {
        let mut world = world();
        let first = world
            .create_body(BodyDesc::dynamic(Vec3::ZERO))
            .expect("body");
        world.destroy_body(first).expect("destroy");
        assert_eq!(world.destroy_body(first), Err(PhysicsError::InvalidBody));
        let second = world
            .create_body(BodyDesc::dynamic(Vec3::ZERO))
            .expect("body");
        assert_ne!(first, second);
        assert_eq!(world.body_count(), 1);
    }
}
