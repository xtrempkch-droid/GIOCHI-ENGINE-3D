//! RGBA color.

/// An 8-bit-per-channel RGBA color.
///
/// # C ABI
///
/// ```c
/// typedef struct GiochiColor {
///     uint8_t  r, g, b, a;
///     uint32_t _reserved[4];
/// } GiochiColor;
/// ```
///
/// Size: 20 bytes, alignment: 4 bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    /// Red channel.
    pub r: u8,
    /// Green channel.
    pub g: u8,
    /// Blue channel.
    pub b: u8,
    /// Alpha channel: `0` fully transparent, `255` fully opaque.
    pub a: u8,
    /// Reserved for future ABI-compatible fields. Must be zero.
    pub _reserved: [u32; 4],
}

impl Color {
    /// Opaque black.
    pub const BLACK: Color = Color::rgb(0, 0, 0);
    /// Opaque white.
    pub const WHITE: Color = Color::rgb(255, 255, 255);
    /// Fully transparent.
    pub const TRANSPARENT: Color = Color::new(0, 0, 0, 0);

    /// Creates a color from its four channels.
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self {
            r,
            g,
            b,
            a,
            _reserved: [0; 4],
        }
    }

    /// Creates an opaque color.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::new(r, g, b, 255)
    }
}

impl Default for Color {
    fn default() -> Self {
        Self::BLACK
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb_is_opaque() {
        let color = Color::rgb(1, 2, 3);
        assert_eq!((color.r, color.g, color.b, color.a), (1, 2, 3, 255));
    }

    #[test]
    fn default_color_is_opaque_black() {
        assert_eq!(Color::default(), Color::BLACK);
    }

    #[test]
    fn reserved_tail_is_zero_initialised() {
        assert_eq!(Color::WHITE._reserved, [0; 4]);
    }
}
