//! Body descriptions and transforms.

use crate::math3d::{Quat, Vec3};

/// What a body is driven by.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BodyKind {
    /// Moved by forces, gravity and impulses.
    #[default]
    Dynamic = 0,
    /// Never moves. Walls, floors, platforms.
    Fixed = 1,
    /// Moved by setting its position each frame. Players, lifts, moving platforms.
    KinematicPosition = 2,
}

/// A body to create: where it starts and how it behaves.
///
/// ```no_run
/// use giochi_engine_3d::math3d::Vec3;
/// use giochi_engine_3d::physics::BodyDesc;
///
/// let player = BodyDesc::dynamic(Vec3::new(0.0, 1.0, 0.0)).lock_rotations();
/// assert!(player.locked_rotations);
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BodyDesc {
    /// What drives the body.
    pub kind: BodyKind,
    /// Starting position, in world units.
    pub position: Vec3,
    /// Starting orientation.
    pub rotation: Quat,
    /// Starting linear velocity, in units per second.
    pub velocity: Vec3,
    /// Starting angular velocity, in radians per second around each axis.
    pub angular_velocity: Vec3,
    /// Whether the body may rotate at all.
    pub locked_rotations: bool,
    /// Whether continuous collision detection is on for this body.
    pub continuous_collision: bool,
    _reserved: [u32; 4],
}

impl BodyDesc {
    /// A body moved by forces and gravity.
    pub fn dynamic(position: Vec3) -> BodyDesc {
        BodyDesc {
            kind: BodyKind::Dynamic,
            position,
            rotation: Quat::IDENTITY,
            velocity: Vec3::ZERO,
            angular_velocity: Vec3::ZERO,
            locked_rotations: false,
            continuous_collision: false,
            _reserved: [0; 4],
        }
    }

    /// A body that never moves.
    pub fn static_(position: Vec3) -> BodyDesc {
        BodyDesc {
            kind: BodyKind::Fixed,
            ..BodyDesc::dynamic(position)
        }
    }

    /// A body moved by setting its position each frame.
    pub fn kinematic(position: Vec3) -> BodyDesc {
        BodyDesc {
            kind: BodyKind::KinematicPosition,
            ..BodyDesc::dynamic(position)
        }
    }

    /// Sets the starting rotation.
    pub fn with_rotation(mut self, rotation: Quat) -> BodyDesc {
        self.rotation = rotation;
        self
    }

    /// Sets the starting linear velocity.
    pub fn with_velocity(mut self, velocity: Vec3) -> BodyDesc {
        self.velocity = velocity;
        self
    }

    /// Prevents the body from rotating.
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
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BodyTransform {
    /// Position in world units.
    pub position: Vec3,
    /// Orientation.
    pub rotation: Quat,
    _reserved: [u32; 4],
}

impl BodyTransform {
    /// A transform at `position` with orientation `rotation`.
    pub fn new(position: Vec3, rotation: Quat) -> BodyTransform {
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
