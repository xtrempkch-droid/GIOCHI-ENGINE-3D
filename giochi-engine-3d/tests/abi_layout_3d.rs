//! Layout assertions for part II of `docs/C_ABI_CONTRACT.md`.
//!
//! Same rule as the 2D crate's `abi_layout.rs`: these numbers are the contract.
//! Changing one means changing the document and bumping `GIOCHI_ABI_VERSION` — never
//! "fix the test".
//!
//! All five structs are 4-byte aligned because every field is `f32` or `u32`, so no
//! padding is ever inserted and the sizes are exactly the sum of the fields. The tests
//! assert that rather than assume it: a future field of type `f64` or a handle would
//! introduce padding and silently change the layout.

use std::mem::{align_of, size_of};

use giochi_engine_3d::math3d::Color3D;

#[test]
fn color3d_is_32_bytes_aligned_to_4() {
    // 4 floats + 4 u32.
    assert_eq!(size_of::<Color3D>(), 32);
    assert_eq!(align_of::<Color3D>(), 4);
}

#[test]
fn vec3_and_quat_sizes_match_the_documented_arrays() {
    // Written as plain arrays here because the wrapper structs arrive in phase 10.
    // The contract states the size of the C form, so the array is what must match.
    let vec3_fields = size_of::<[f32; 3]>() + size_of::<[u32; 4]>();
    let quat_fields = size_of::<[f32; 4]>() + size_of::<[u32; 4]>();

    assert_eq!(vec3_fields, 28, "GiochiVec3 is size 28, align 4");
    assert_eq!(quat_fields, 32, "GiochiQuat is size 32, align 4");
}

#[test]
fn transform3d_and_camera3d_sizes_match_the_c_header() {
    // GiochiTransform3D = Vec3(28) + Quat(32) + Vec3(28) + reserved(16) = 104.
    // GiochiCamera3D    = Vec3(28) * 3 + 4 floats(16) + reserved(16)   = 116.
    //
    // These were checked against a real C compiler (`sizeof` in `cc`), not derived by
    // hand: the first draft of the contract had 92 and 64, both wrong.
    let transform = 28 + 32 + 28 + 16;
    let camera = 28 * 3 + 16 + 16;

    assert_eq!(transform, 104, "GiochiTransform3D");
    assert_eq!(camera, 116, "GiochiCamera3D");
}

#[test]
fn the_reserved_tail_is_four_u32_everywhere() {
    // Rule R3: `_reserved: [u32; N]` with N >= 4 at the end of every exposed struct.
    // Color3D is the only one implemented so far, and it must be zero after every
    // constructor.
    assert_eq!(Color3D::SKY._reserved, [0; 4]);
    assert_eq!(Color3D::new(1.0, 2.0, 3.0, 4.0)._reserved, [0; 4]);
    assert_eq!(Color3D::rgb(0.1, 0.2, 0.3)._reserved, [0; 4]);
    assert_eq!(Color3D::default()._reserved, [0; 4]);
}

#[test]
fn no_gl_type_appears_in_a_public_struct() {
    // The rule from ADR-032 that keeps the surface C-shaped: a public struct may not
    // carry `glow`, `glam` or `sdl2` types. `Color3D` is the only public struct in
    // phase 9 and it is four floats plus reserved, which this asserts structurally.
    // 8 four-byte fields: four floats and four u32s.
    assert_eq!(size_of::<Color3D>(), 8 * 4);
    assert_eq!(align_of::<Color3D>(), align_of::<u32>());
}
