//! World-to-screen camera.
//!
//! The camera defines which part of world space is visible. The transform is
//! deliberately simple (translate + uniform zoom around the viewport centre) so
//! it maps directly onto SDL2's destination-rectangle drawing:
//!
//! ```text
//! screen = viewport.center + (world - camera.position) * zoom
//! ```
//!
//! A `zoom` of `1.0` leaves sizes untouched; `2.0` magnifies twice. Screen Y
//! grows downwards, so the transform is applied identically to both axes.

use crate::math::{Rect, Vec2};

/// A 2D camera.
///
/// # C ABI
///
/// ```c
/// typedef struct GiochiCamera {
///     float       x, y, zoom;
///     GiochiRect  viewport;
///     uint32_t    _reserved[4];
/// } GiochiCamera;
/// ```
///
/// Size: 60 bytes, alignment: 4 bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// World X coordinate at the centre of the viewport.
    pub x: f32,
    /// World Y coordinate at the centre of the viewport.
    pub y: f32,
    /// Uniform magnification factor; `1.0` is one world unit per pixel.
    pub zoom: f32,
    /// Screen-space rectangle the camera draws into.
    pub viewport: Rect,
    /// Reserved for future ABI-compatible fields. Must be zero.
    pub _reserved: [u32; 4],
}

impl Camera {
    /// Creates a camera centred on the world origin with `zoom = 1.0`.
    pub const fn new(viewport: Rect) -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            zoom: 1.0,
            viewport,
            _reserved: [0; 4],
        }
    }

    /// Converts a world-space point to screen space.
    ///
    /// Implemented in phase 2.
    pub fn world_to_screen(&self, _world: Vec2) -> Vec2 {
        todo!("fase 2: transformar mundo -> tela")
    }
}
