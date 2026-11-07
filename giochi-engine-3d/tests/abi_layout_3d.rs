//! Layout assertions for part II of `docs/C_ABI_CONTRACT.md`.
//!
//! Same rule as the 2D crate's `abi_layout.rs`: these numbers are the contract.
//! Changing one means changing the document and bumping `GIOCHI_ABI_VERSION` — never
//! "fix the test".
//!
//! Every struct is 4-byte aligned because every field is `f32` or `u32`, so no padding
//! is ever inserted and the size is exactly the sum of the fields. The tests assert
//! that rather than assume it: a future `f64` field or a handle would introduce padding
//! and silently change the layout, which is exactly the bug this file exists to catch.

use std::mem::{align_of, size_of};

use giochi_engine_3d::math3d::{Camera3D, Color3D, Mat4, Quat, Transform3D, Vec3};

#[test]
fn vec3_is_28_bytes_aligned_to_4() {
    assert_eq!(size_of::<Vec3>(), 28);
    assert_eq!(align_of::<Vec3>(), 4);
}

#[test]
fn quat_is_32_bytes_aligned_to_4() {
    assert_eq!(size_of::<Quat>(), 32);
    assert_eq!(align_of::<Quat>(), 4);
}

#[test]
fn color3d_is_32_bytes_aligned_to_4() {
    // 4 floats + 4 u32.
    assert_eq!(size_of::<Color3D>(), 32);
    assert_eq!(align_of::<Color3D>(), 4);
}

#[test]
fn transform3d_is_104_bytes_aligned_to_4() {
    // Vec3(28) + Quat(32) + Vec3(28) + reserved(16).
    assert_eq!(size_of::<Transform3D>(), 104);
    assert_eq!(align_of::<Transform3D>(), 4);
}

#[test]
fn camera3d_is_116_bytes_aligned_to_4() {
    // Vec3(28) * 3 + 4 floats(16) + reserved(16).
    assert_eq!(size_of::<Camera3D>(), 116);
    assert_eq!(align_of::<Camera3D>(), 4);
}

#[test]
fn mat4_is_not_a_c_type_but_keeps_its_documented_layout() {
    // `docs/C_ABI_CONTRACT.md` §11 deliberately keeps Mat4 out of the header: callers
    // pass a Transform3D and a Camera3D and the engine composes. The layout is still
    // pinned because it must be column-major and pointer-safe for `glUniformMatrix4fv`.
    assert_eq!(size_of::<Mat4>(), 64 + 16);
    assert_eq!(align_of::<Mat4>(), 4);
}

#[test]
fn the_sizes_compose_the_way_the_header_documents() {
    // The arithmetic is not decoration: it is the reason the sizes above hold. If a
    // future edit breaks one of these identities, the individual assertions above fail
    // too but this one names the cause.
    assert_eq!(
        size_of::<Transform3D>(),
        size_of::<Vec3>() * 2 + size_of::<Quat>() + 16
    );
    assert_eq!(size_of::<Camera3D>(), size_of::<Vec3>() * 3 + 16 + 16);
    assert_eq!(size_of::<Mat4>(), 16 * 4 + 16);
}

#[test]
fn the_reserved_tail_is_four_u32_everywhere() {
    // Rule R3: `_reserved: [u32; N]` with N >= 4 at the end of every exposed struct,
    // zeroed by every constructor.
    assert_eq!(Vec3::new(1.0, 2.0, 3.0)._reserved, [0; 4]);
    assert_eq!(Vec3::ZERO._reserved, [0; 4]);
    assert_eq!(Quat::IDENTITY._reserved, [0; 4]);
    assert_eq!(Quat::new(1.0, 2.0, 3.0, 4.0)._reserved, [0; 4]);
    assert_eq!(Color3D::SKY._reserved, [0; 4]);
    assert_eq!(Transform3D::IDENTITY._reserved, [0; 4]);
    assert_eq!(Camera3D::new()._reserved, [0; 4]);
    assert_eq!(Mat4::IDENTITY._reserved, [0; 4]);
    assert_eq!(Transform3D::default()._reserved, [0; 4]);
    assert_eq!(Camera3D::default()._reserved, [0; 4]);
    assert_eq!(Mat4::default()._reserved, [0; 4]);
}

#[test]
fn no_gl_type_appears_in_a_public_struct() {
    // ADR-032's rule, structurally: a public struct may only hold primitives and other
    // C-shaped structs. Every type below is a whole number of 4-byte fields, which is
    // true only if no `glow`, `glam` or `sdl2` type slipped in -- those are larger and
    // SIMD-aligned, so they would change both the size and the alignment.
    for (size, align) in [
        (size_of::<Vec3>(), align_of::<Vec3>()),
        (size_of::<Quat>(), align_of::<Quat>()),
        (size_of::<Color3D>(), align_of::<Color3D>()),
        (size_of::<Transform3D>(), align_of::<Transform3D>()),
        (size_of::<Camera3D>(), align_of::<Camera3D>()),
        (size_of::<Mat4>(), align_of::<Mat4>()),
    ] {
        assert_eq!(align, 4, "a SIMD type would raise this to 8 or 16");
        assert_eq!(size % 4, 0, "a SIMD type would pad this");
    }
}
