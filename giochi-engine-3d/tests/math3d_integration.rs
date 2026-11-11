//! Cross-type maths checks.
//!
//! The unit tests inside each module cover that type in isolation. These cover the
//! *composition* — a transform feeding a camera feeding a projection — because that is
//! where the conventions interact and where a correct-looking pair of types still
//! produces the wrong picture.
//!
//! No GL context is needed, and none is created: everything here is a property of the
//! matrices. The GPU-side check (upload a matrix, read it back through `glGetUniformfv`)
//! belongs to phase 11, where uniforms exist; asserting it now would mean shipping a
//! helper that nothing uses.

use giochi_engine_3d::math3d::{Camera3D, Mat4, Quat, Transform3D, Vec3};

const EPSILON: f32 = 1e-5;

fn assert_close(actual: f32, expected: f32, what: &str) {
    assert!(
        (actual - expected).abs() < EPSILON,
        "{what}: got {actual}, expected {expected}"
    );
}

#[test]
fn the_depth_range_is_the_opengl_one_not_the_vulkan_one() {
    // -1..1, not 0..1. This is the assertion that caught a real bug: glam offers both
    // conventions, and the 0..1 one produces a scene that looks plausible until geometry
    // approaches the near plane. Nothing about the wrong matrix raises a GL error.
    //
    // Em glam 0.31 eram `perspective_rh` (0..1) e `perspective_rh_gl` (-1..1). Em 0.34 as
    // duas viraram submódulos de `glam::camera::rh::proj`: `vulkan`/`directx` (0..1) e
    // `opengl` (-1..1). Este teste é o que prende a troca de módulo — usar `vulkan` no
    // lugar de `opengl` não gera erro de compilação, só uma cena errada.
    let camera = Camera3D::new()
        .with_aspect(1.0)
        .with_fov_y(std::f32::consts::FRAC_PI_2)
        .with_clip_planes(1.0, 10.0);
    let view_proj = camera.view_projection().expect("valid camera");

    let near = view_proj.to_glam() * glam::Vec4::new(0.0, 0.0, -1.0, 1.0);
    let far = view_proj.to_glam() * glam::Vec4::new(0.0, 0.0, -10.0, 1.0);

    assert_close(near.z / near.w, -1.0, "near plane NDC z");
    assert_close(far.z / far.w, 1.0, "far plane NDC z");
}

#[test]
fn orthographic_also_uses_the_opengl_depth_range() {
    let matrix = Mat4::orthographic(-1.0, 1.0, -1.0, 1.0, 1.0, 10.0).expect("valid");

    let near = matrix.to_glam() * glam::Vec4::new(0.0, 0.0, -1.0, 1.0);
    let far = matrix.to_glam() * glam::Vec4::new(0.0, 0.0, -10.0, 1.0);

    assert_close(near.z / near.w, -1.0, "near plane NDC z");
    assert_close(far.z / far.w, 1.0, "far plane NDC z");
}

#[test]
fn a_model_at_the_origin_is_in_front_of_the_default_camera() {
    // The default camera looks down -Z from the origin, so a model at (0, 0, -5) is in
    // view and one at (0, 0, +5) is behind. This is the "is anything on screen" check
    // the phase 11 example will rely on.
    let camera = Camera3D::new().with_aspect(1.0);
    let view_proj = camera.view_projection().expect("valid");

    let in_front = view_proj.to_glam() * glam::Vec4::new(0.0, 0.0, -5.0, 1.0);
    assert!(in_front.w > 0.0, "in front must have w > 0");
    assert!(
        (in_front.x / in_front.w).abs() <= 1.0 && (in_front.y / in_front.w).abs() <= 1.0,
        "a point on the view axis must project inside NDC"
    );

    let behind = view_proj.to_glam() * glam::Vec4::new(0.0, 0.0, 5.0, 1.0);
    assert!(behind.w <= 0.0, "behind the camera must have w <= 0");
}

#[test]
fn a_transform_and_a_camera_compose_into_the_expected_screen_position() {
    // A square at the origin, 4 units in front of a camera that is 4 units back.
    // Its centre must land at NDC (0, 0): the middle of the screen.
    let camera = Camera3D::from_parts(
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(0.0, 0.0, -1.0),
        Vec3::UP,
    )
    .with_aspect(1.0);

    let model = Transform3D::from_translation(Vec3::new(0.0, 0.0, -4.0)).matrix();
    let clip = camera.view_projection().expect("valid").to_glam()
        * model.to_glam()
        * glam::Vec4::new(0.0, 0.0, 0.0, 1.0);

    assert_close(clip.x / clip.w, 0.0, "horizontal centre");
    assert_close(clip.y / clip.w, 0.0, "vertical centre");
    assert!(clip.w > 0.0, "in front of the camera");
}

#[test]
fn rotating_the_camera_ninety_degrees_about_y_moves_the_target_off_axis() {
    // A model straight ahead, then a camera rotated 90 degrees about +Y: the model must
    // leave the centre of the frame. If the rotation direction were inverted this would
    // still be "off centre", so the sign is asserted too.
    let model = Transform3D::from_translation(Vec3::new(0.0, 0.0, -4.0)).matrix();

    let forward = Camera3D::new().with_aspect(1.0);
    let straight = forward.view_projection().expect("valid").to_glam()
        * model.to_glam()
        * glam::Vec4::new(0.0, 0.0, 0.0, 1.0);
    assert_close(straight.x / straight.w, 0.0, "centred before the turn");

    // Turn the camera 90 degrees to the left about +Y.
    let turned = Camera3D::from_parts(
        Vec3::ZERO,
        Quat::from_axis_angle(Vec3::UP, std::f32::consts::FRAC_PI_2)
            .rotate_vec3(Vec3::new(0.0, 0.0, -1.0)),
        Vec3::UP,
    )
    .with_aspect(1.0);

    let off_axis = turned.view_projection().expect("valid").to_glam()
        * model.to_glam()
        * glam::Vec4::new(0.0, 0.0, 0.0, 1.0);

    assert!(
        (off_axis.x / off_axis.w).abs() > 0.5,
        "the model should be well off centre, got x = {}",
        off_axis.x / off_axis.w
    );
    // Turning left puts the object on the right of the frame, so NDC x is positive.
    assert!(
        off_axis.x / off_axis.w > 0.0,
        "turning the camera left must move the object right"
    );
}

#[test]
fn a_resolved_aspect_keeps_the_frame_uncropped_on_a_panel() {
    // The console case (ADR-035): the panel is 640x480 whatever was requested. The
    // camera must use the real drawable size or the scene is cropped.
    let camera = Camera3D::new().with_drawable_size(640, 480);
    assert_close(camera.aspect, 4.0 / 3.0, "aspect from the panel");

    // A point one unit up at the near-ish plane must project within NDC vertically.
    let view_proj = camera.view_projection().expect("valid");
    let clip = view_proj.to_glam() * glam::Vec4::new(0.0, 1.0, -5.0, 1.0);
    let ndc_y = clip.y / clip.w;

    assert!(
        ndc_y.abs() <= 1.0,
        "vertical NDC outside the frame: {ndc_y}"
    );
    assert!(ndc_y > 0.0, "up on the screen is up in NDC");
}

#[test]
fn the_full_chain_is_finite_for_every_reasonable_camera() {
    // A NaN anywhere in the chain blanks the frame with no GL error, so walk the whole
    // composition and check.
    for (fov, aspect, near, far) in [
        (1.0_f32, 1.0_f32, 0.1_f32, 100.0_f32),
        (std::f32::consts::FRAC_PI_2, 16.0 / 9.0, 0.01, 1000.0),
        (0.1, 0.5, 1.0, 2.0),
    ] {
        let camera = Camera3D::new()
            .with_fov_y(fov)
            .with_aspect(aspect)
            .with_clip_planes(near, far);
        let model = Transform3D::from_rotation(Quat::from_euler_angles(0.1, 0.2, 0.3)).matrix();
        let chain = camera.view_projection().expect("valid") * model;

        assert!(
            chain.is_finite(),
            "camera {fov}/{aspect}/{near}/{far} produced NaN"
        );
        assert!(model.is_finite());
        assert!(camera.is_finite());
    }
}
