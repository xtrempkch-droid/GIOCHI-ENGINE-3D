//! Small helpers the game modules share.

pub mod procedural_car;

use giochi_engine_3d::math3d::{Quat, Transform3D, Vec3};

/// `parent · child` for translation/rotation/scale transforms.
///
/// The engine's [`Transform3D`] is a plain value with no composition operator, and the
/// scene loader, the car's parts and the camera all need the same three lines. The scale
/// is multiplied component-wise (a scaled parent scales a child's offset *and* its size),
/// which is what the editor's hierarchy implies when you scale a group.
pub fn compose(parent: &Transform3D, child: &Transform3D) -> Transform3D {
    let parent_rotation = parent.rotation.to_glam();
    let position = parent.position.to_glam()
        + parent_rotation * (parent.scale.to_glam() * child.position.to_glam());
    let rotation = parent_rotation * child.rotation.to_glam();
    let scale = parent.scale.to_glam() * child.scale.to_glam();
    Transform3D::new(
        Vec3::from_glam(position),
        Quat::from_glam(rotation),
        Vec3::from_glam(scale),
    )
}

/// A translation, a yaw and no scale — the pose of anything that only turns about `+Y`.
pub fn pose(position: Vec3, yaw: f32) -> Transform3D {
    Transform3D::new(
        position,
        Quat::from_axis_angle(Vec3::UP, yaw),
        Vec3::new(1.0, 1.0, 1.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::FRAC_PI_2;

    #[test]
    fn composing_with_the_identity_changes_nothing() {
        let child = Transform3D::new(
            Vec3::new(1.0, 2.0, 3.0),
            Quat::from_axis_angle(Vec3::UP, 0.3),
            Vec3::new(2.0, 2.0, 2.0),
        );
        let composed = compose(&Transform3D::IDENTITY, &child);
        assert_eq!(composed.position.to_array(), child.position.to_array());
        assert_eq!(composed.scale.to_array(), child.scale.to_array());
        assert_eq!(composed.rotation, child.rotation);
    }

    #[test]
    fn a_rotated_parent_rotates_the_childs_offset() {
        // Parent turned a quarter turn about Y: a child 1 m along +Z is now 1 m along +X.
        let parent = Transform3D::new(
            Vec3::ZERO,
            Quat::from_axis_angle(Vec3::UP, FRAC_PI_2),
            Vec3::new(1.0, 1.0, 1.0),
        );
        let child = Transform3D::from_translation(Vec3::new(0.0, 0.0, 1.0));
        let composed = compose(&parent, &child);
        let position = composed.position.to_glam();
        assert!((position.x - 1.0).abs() < 1e-5, "{position:?}");
        assert!(position.z.abs() < 1e-5, "{position:?}");
    }

    #[test]
    fn a_scaled_parent_scales_the_childs_offset_and_size() {
        let parent = Transform3D::new(
            Vec3::new(10.0, 0.0, 0.0),
            Quat::IDENTITY,
            Vec3::new(2.0, 2.0, 2.0),
        );
        let child = Transform3D::new(
            Vec3::new(1.0, 0.0, 0.0),
            Quat::IDENTITY,
            Vec3::new(3.0, 3.0, 3.0),
        );
        let composed = compose(&parent, &child);
        assert_eq!(composed.position.to_array(), [12.0, 0.0, 0.0]);
        assert_eq!(composed.scale.to_array(), [6.0, 6.0, 6.0]);
    }

    #[test]
    fn composing_two_quarter_turns_is_a_half_turn() {
        let quarter = pose(Vec3::ZERO, FRAC_PI_2);
        let composed = compose(&quarter, &quarter);
        let forward = composed
            .rotation
            .rotate_vec3(Vec3::new(0.0, 0.0, 1.0))
            .to_glam();
        assert!((forward.z + 1.0).abs() < 1e-5, "{forward:?}");
    }

    #[test]
    fn a_pose_has_the_heading_it_was_given() {
        let transform = pose(Vec3::new(1.0, 2.0, 3.0), FRAC_PI_2);
        assert_eq!(transform.position.to_array(), [1.0, 2.0, 3.0]);
        assert_eq!(transform.scale.to_array(), [1.0, 1.0, 1.0]);
        let forward = transform
            .rotation
            .rotate_vec3(Vec3::new(0.0, 0.0, 1.0))
            .to_glam();
        assert!((forward.x - 1.0).abs() < 1e-5, "{forward:?}");
    }
}
