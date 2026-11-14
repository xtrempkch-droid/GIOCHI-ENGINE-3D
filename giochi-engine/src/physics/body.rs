//! Body descriptions and transforms.

use crate::math::Vec2;

/// What a body is driven by.
///
/// The three kinds are the ones Rapier integrates differently, named after what a game
/// does with them rather than after Rapier's `RigidBodyType` (which also has soft bodies).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BodyKind {
    /// Moved by forces, gravity and impulses.
    #[default]
    Dynamic = 0,
    /// Never moves. Walls, floors, platforms.
    Fixed = 1,
    /// Moved by setting its position each frame, and pushes dynamic bodies out of the
    /// way. Players and lifts.
    KinematicPosition = 2,
}

/// A body to create: where it starts and how it behaves.
///
/// Built with the constructors and the `with_*` methods; every field is public so the C
/// ABI can fill one in directly.
///
/// ```no_run
/// use giochi_engine::physics::BodyDesc;
/// use giochi_engine::Vec2;
///
/// let player = BodyDesc::dynamic(Vec2::new(0.0, 0.0))
///     .with_velocity(Vec2::new(0.0, 0.0))
///     .lock_rotations();
/// assert!(player.locked_rotations);
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BodyDesc {
    /// What drives the body.
    pub kind: BodyKind,
    /// Starting position, in world units.
    pub position: Vec2,
    /// Starting rotation, in radians (2D).
    pub rotation: f32,
    /// Starting linear velocity, in units per second.
    pub velocity: Vec2,
    /// Starting angular velocity, in radians per second.
    pub angular_velocity: f32,
    /// Whether the body may rotate at all.
    pub locked_rotations: bool,
    /// Whether continuous collision detection is on for this body.
    ///
    /// On for fast, small bodies (bullets) and off for everything else: CCD is a
    /// shape-cast per body per step.
    pub continuous_collision: bool,
    _reserved: [u32; 4],
}

impl BodyDesc {
    /// A body moved by forces and gravity.
    pub fn dynamic(position: Vec2) -> BodyDesc {
        BodyDesc {
            kind: BodyKind::Dynamic,
            position,
            rotation: 0.0,
            velocity: Vec2::ZERO,
            angular_velocity: 0.0,
            locked_rotations: false,
            continuous_collision: false,
            _reserved: [0; 4],
        }
    }

    /// A body that never moves.
    pub fn static_(position: Vec2) -> BodyDesc {
        BodyDesc {
            kind: BodyKind::Fixed,
            ..BodyDesc::dynamic(position)
        }
    }

    /// A body moved by setting its position each frame.
    pub fn kinematic(position: Vec2) -> BodyDesc {
        BodyDesc {
            kind: BodyKind::KinematicPosition,
            ..BodyDesc::dynamic(position)
        }
    }

    /// Sets the starting rotation, in radians.
    pub fn with_rotation(mut self, radians: f32) -> BodyDesc {
        self.rotation = radians;
        self
    }

    /// Sets the starting linear velocity.
    pub fn with_velocity(mut self, velocity: Vec2) -> BodyDesc {
        self.velocity = velocity;
        self
    }

    /// Sets the starting angular velocity, in radians per second.
    pub fn with_angular_velocity(mut self, radians_per_second: f32) -> BodyDesc {
        self.angular_velocity = radians_per_second;
        self
    }

    /// Prevents the body from rotating.
    ///
    /// What a player character wants: a capsule that tips over on every step becomes a
    /// different game.
    pub fn lock_rotations(mut self) -> BodyDesc {
        self.locked_rotations = true;
        self
    }

    /// Turns continuous collision detection on.
    pub fn with_continuous_collision(mut self) -> BodyDesc {
        self.continuous_collision = true;
        self
    }

    /// Whether every value can be handed to Rapier.
    pub(crate) fn is_valid(&self) -> bool {
        self.position.is_finite()
            && self.rotation.is_finite()
            && self.velocity.is_finite()
            && self.angular_velocity.is_finite()
    }
}

/// Where a body is, as a game thinks of it.
///
/// This is what [`PhysicsWorld2D::sync_to_transforms`] and
/// [`PhysicsWorld2D::sync_from_transforms`] exchange, and what the C ABI fills in.
///
/// [`PhysicsWorld2D::sync_to_transforms`]: super::PhysicsWorld2D::sync_to_transforms
/// [`PhysicsWorld2D::sync_from_transforms`]: super::PhysicsWorld2D::sync_from_transforms
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BodyTransform {
    /// Position in world units.
    pub position: Vec2,
    /// Rotation in radians.
    pub rotation: f32,
    _reserved: [u32; 4],
}

impl BodyTransform {
    /// A transform at `position`, unrotated.
    pub fn new(position: Vec2, rotation: f32) -> BodyTransform {
        BodyTransform {
            position,
            rotation,
            _reserved: [0; 4],
        }
    }

    /// Whether the transform can be applied to a body.
    pub fn is_finite(&self) -> bool {
        self.position.is_finite() && self.rotation.is_finite()
    }
}
