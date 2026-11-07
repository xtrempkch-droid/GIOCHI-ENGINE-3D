//! Floating-point RGBA colour.
//!
//! The 2D engine's [`Color`](giochi_engine::math::Color) is `u8`-based because
//! `sdl2::render` wants bytes. GL takes floats in `0.0..=1.0`, and converting at every
//! call site invites off-by-255 mistakes, so the 3D path carries its own type.

/// Linear-agnostic float RGBA colour, one `f32` per channel.
///
/// # C ABI
///
/// ```c
/// typedef struct GiochiColor3D {
///     float r, g, b, a;
///     uint32_t _reserved[4];
/// } GiochiColor3D;
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color3D {
    /// Red, `0.0..=1.0`.
    pub r: f32,
    /// Green, `0.0..=1.0`.
    pub g: f32,
    /// Blue, `0.0..=1.0`.
    pub b: f32,
    /// Alpha: `0.0` transparent, `1.0` opaque.
    pub a: f32,
    /// Reserved for future ABI-compatible fields. Must be zero.
    pub _reserved: [u32; 4],
}

impl Color3D {
    /// Creates a colour from four channels.
    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self {
            r,
            g,
            b,
            a,
            _reserved: [0; 4],
        }
    }

    /// Creates an opaque colour.
    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self::new(r, g, b, 1.0)
    }

    /// Black.
    pub const BLACK: Self = Self::rgb(0.0, 0.0, 0.0);
    /// White.
    pub const WHITE: Self = Self::rgb(1.0, 1.0, 1.0);
    /// Opaque sky blue, the phase 9 demo clear colour.
    pub const SKY: Self = Self::rgb(0.35, 0.55, 0.85);

    /// Returns the channels as `[r, g, b, a]`, the shape GL wants.
    pub const fn to_array(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }

    /// Returns the same colour with a different alpha.
    pub const fn with_alpha(mut self, a: f32) -> Self {
        self.a = a;
        self
    }

    /// Clamps every channel into `0.0..=1.0`.
    ///
    /// GL accepts out-of-range floats but a driver may clamp differently from another,
    /// so the renderer clamps to keep behaviour identical across targets.
    pub fn clamped(self) -> Self {
        let fix = |v: f32| {
            if v.is_nan() {
                0.0
            } else {
                v.clamp(0.0, 1.0)
            }
        };
        Self::new(fix(self.r), fix(self.g), fix(self.b), fix(self.a))
    }
}

impl Default for Color3D {
    fn default() -> Self {
        Self::BLACK
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructors_leave_reserved_zeroed() {
        assert_eq!(Color3D::rgb(0.1, 0.2, 0.3)._reserved, [0; 4]);
        assert_eq!(Color3D::new(0.1, 0.2, 0.3, 0.4)._reserved, [0; 4]);
    }

    #[test]
    fn to_array_follows_gl_order() {
        let color = Color3D::new(0.1, 0.2, 0.3, 0.4);
        assert_eq!(color.to_array(), [0.1, 0.2, 0.3, 0.4]);
    }

    #[test]
    fn clamping_handles_nan_and_out_of_range() {
        let wild = Color3D::new(-2.0, 5.0, f32::NAN, 0.5).clamped();

        assert_eq!(wild.r, 0.0);
        assert_eq!(wild.g, 1.0);
        assert_eq!(wild.b, 0.0, "NaN must not propagate into GL");
        assert_eq!(wild.a, 0.5);
    }

    #[test]
    fn with_alpha_preserves_rgb() {
        let color = Color3D::SKY.with_alpha(0.25);

        assert_eq!((color.r, color.g, color.b), (0.35, 0.55, 0.85));
        assert_eq!(color.a, 0.25);
    }

    #[test]
    fn the_layout_is_c_compatible() {
        // 4 floats + 4 u32, no padding surprises: 32 bytes, 4-byte alignment.
        assert_eq!(core::mem::size_of::<Color3D>(), 32);
        assert_eq!(core::mem::align_of::<Color3D>(), 4);
    }
}
