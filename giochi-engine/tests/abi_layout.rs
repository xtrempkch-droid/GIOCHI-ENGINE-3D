//! Layout assertions for every `#[repr(C)]` type in the C ABI contract.
//!
//! These tests lock the byte layout documented in `docs/C_ABI_CONTRACT.md`.
//! Changing a size or alignment here means changing the contract and bumping
//! `GIOCHI_ABI_VERSION` — never "fix the test".

use std::mem::{align_of, size_of};

use giochi_engine::core::engine::{EngineConfig, ENGINE_TITLE_CAPACITY};
use giochi_engine::core::time::Time;
use giochi_engine::math::{Color, Rect, Vec2};
use giochi_engine::platform::window::WindowConfig;
use giochi_engine::render::{Camera, Sprite};

#[test]
fn vec2_is_24_bytes_aligned_to_4() {
    assert_eq!(size_of::<Vec2>(), 24);
    assert_eq!(align_of::<Vec2>(), 4);
}

#[test]
fn rect_is_32_bytes_aligned_to_4() {
    assert_eq!(size_of::<Rect>(), 32);
    assert_eq!(align_of::<Rect>(), 4);
}

#[test]
fn color_is_20_bytes_aligned_to_4() {
    assert_eq!(size_of::<Color>(), 20);
    assert_eq!(align_of::<Color>(), 4);
}

#[test]
fn sprite_is_56_bytes_aligned_to_8() {
    assert_eq!(size_of::<Sprite>(), 56);
    assert_eq!(align_of::<Sprite>(), 8);
}

#[test]
fn camera_is_60_bytes_aligned_to_4() {
    assert_eq!(size_of::<Camera>(), 60);
    assert_eq!(align_of::<Camera>(), 4);
}

#[test]
fn time_is_32_bytes_aligned_to_8() {
    assert_eq!(size_of::<Time>(), 32);
    assert_eq!(align_of::<Time>(), 8);
}

#[test]
fn engine_config_is_112_bytes_aligned_to_4() {
    assert_eq!(ENGINE_TITLE_CAPACITY, 64);
    assert_eq!(size_of::<EngineConfig>(), 112);
    assert_eq!(align_of::<EngineConfig>(), 4);
}

#[test]
fn window_config_is_44_bytes_aligned_to_4() {
    assert_eq!(size_of::<WindowConfig>(), 44);
    assert_eq!(align_of::<WindowConfig>(), 4);
}

#[test]
fn constructors_zero_the_reserved_tail() {
    assert_eq!(Vec2::new(1.0, 2.0)._reserved, [0; 4]);
    assert_eq!(Rect::new(1.0, 2.0, 3.0, 4.0)._reserved, [0; 4]);
    assert_eq!(Color::new(1, 2, 3, 4)._reserved, [0; 4]);
    assert_eq!(Sprite::new(1, 2.0, 3.0, 4.0, 5.0)._reserved, [0; 8]);
    assert_eq!(Camera::new(Rect::ZERO)._reserved, [0; 4]);
    assert_eq!(Time::new()._reserved, [0; 4]);
    assert_eq!(EngineConfig::default()._reserved, [0; 8]);
    assert_eq!(WindowConfig::default()._reserved, [0; 8]);
}

#[test]
fn rect_edges_are_derived_from_position_and_size() {
    let rect = Rect::new(10.0, 20.0, 30.0, 40.0);

    assert_eq!(rect.right(), 40.0);
    assert_eq!(rect.bottom(), 60.0);
    assert!(!rect.is_empty());
    assert!(Rect::ZERO.is_empty());
}
