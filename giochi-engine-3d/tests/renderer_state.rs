//! The renderer's GL state cache, if and only if the profiler asks for one
//! (DOD criterion 7, S5b decision).
//!
//! **Ativado na Fase S5.**
//!
//! # Why this file exists before the optimisation does
//!
//! S5 is allowed to conclude "the renderer is not a hotspot, nothing was optimised". What
//! it is *not* allowed to do is optimise without a measurement. This test is the half of
//! the decision that can be written in advance: whatever `render3d::renderer::state`
//! ends up doing, the property is
//!
//! > two consecutive draws with the same program and texture must issue one bind, not two
//!
//! and the state cache must be **flushed at `begin_frame`**, because egui, debug draw and
//! any host code can change GL state outside the renderer. A cache that trusts its own
//! memory across a frame boundary is the classic silent-corruption bug this criterion
//! exists to prevent.
//!
//! # Measuring binds
//!
//! `glBindTexture` is not interceptable from safe Rust. S5c therefore exposes a
//! `debug_bind_counts()` (test-only or feature-gated) with `(program_binds, texture_binds)`
//! counters incremented at the single place the renderer binds. Counting at the call site
//! is what makes the test possible without a mock GL implementation.

use giochi_engine_3d::render3d::Material;
use giochi_engine_3d::shader::BuiltinShader;

#[test]
#[ignore = "Ativado na Fase S5"]
fn the_state_cache_starts_each_frame_from_a_known_state() {
    todo!("S5c: renderer::begin_frame resets last_program/last_texture (only if S5b says hotspot)")
}

#[test]
#[ignore = "Ativado na Fase S5"]
fn two_draws_with_the_same_texture_bind_it_once() {
    todo!("S5c: debug_bind_counts() == (1, 1) for two textured draws sharing program and texture")
}

#[test]
#[ignore = "Ativado na Fase S5"]
fn alternating_textures_bind_each_time() {
    // The cache must not be "sticky": A, B, A is three binds, not two.
    todo!("S5c: A B A texture sequence issues 3 binds")
}

#[test]
#[ignore = "Ativado na Fase S5"]
fn a_draw_after_external_state_change_is_not_skipped() {
    // Simulates the egui/debug-draw case: the cache is invalidated at `begin_frame`, so
    // the first draw of the next frame re-binds regardless of what the last frame did.
    todo!("S5c: begin_frame invalidates; the first draw of the next frame binds again")
}

#[test]
#[ignore = "Ativado na Fase S5"]
fn the_public_renderer_api_did_not_change() {
    // S5c splits `renderer.rs` into modules behind `legacy.rs`; the compatibility surface
    // is what these items name. If this test stops compiling after the split, the split
    // broke a consumer — which is the one thing a "no behaviour change" refactor cannot do.
    let _: fn() -> BuiltinShader = || BuiltinShader::Flat;
    let _ = BuiltinShader::Textured;
    let _ = Material::DEFAULT;
    // Measured 2026-11-20: two `Color3D` (32 bytes each), five `f32`, a `bool` and the
    // reserved tail. If this number moves, the C ABI layout test moves with it.
    assert_eq!(
        std::mem::size_of::<Material>(),
        96,
        "Material grew; the C ABI layout test would fail too"
    );
}
