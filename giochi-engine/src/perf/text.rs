//! A tiny bitmap font used by the debug overlay.
//!
//! The overlay cannot rely on a TTF file: the production image is a minimal Buildroot
//! rootfs and shipping a font blob would add a licence question to a debug aid. The
//! glyphs below are authored as 5x7 bitmaps, upper-cased, and rasterised into an RGBA
//! buffer that becomes one texture per refresh. That keeps the overlay to one draw call
//! on every backend, including `kmsdrm`, and needs no font package at all (ADR-042).

/// Glyph width in font pixels.
const GLYPH_WIDTH: u32 = 5;
/// Glyph height in font pixels.
const GLYPH_HEIGHT: u32 = 7;
/// Advance between glyphs in font pixels (one column of spacing).
const ADVANCE: u32 = GLYPH_WIDTH + 1;
/// Line height in font pixels (one row of spacing).
const LINE_HEIGHT: u32 = GLYPH_HEIGHT + 1;
/// Padding around the text block in font pixels.
const PADDING: u32 = 3;

/// A rasterised block of text ready to be uploaded as an RGBA texture.
pub(crate) struct RasterText {
    pub(crate) width: u32,
    pub(crate) height: u32,
    /// Row-major RGBA8 pixels, `width * height * 4` bytes.
    pub(crate) pixels: Vec<u8>,
}

/// Rasterises `lines` at `scale` into an opaque RGBA buffer.
///
/// `scale` is clamped to `1..=4`. Returns `None` for an empty line list, which the
/// caller treats as "nothing to draw".
pub(crate) fn rasterize(
    lines: &[String],
    scale: u32,
    background: [u8; 4],
    foreground: [u8; 4],
) -> Option<RasterText> {
    let scale = scale.clamp(1, 4);
    let columns = lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);
    if lines.is_empty() || columns == 0 {
        return None;
    }

    let padding = PADDING * scale;
    let width = columns as u32 * ADVANCE * scale + padding * 2;
    let height = lines.len() as u32 * LINE_HEIGHT * scale + padding * 2;
    let mut pixels = vec![0u8; width as usize * height as usize * 4];
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.copy_from_slice(&background);
    }

    for (line_index, line) in lines.iter().enumerate() {
        let top = padding + line_index as u32 * LINE_HEIGHT * scale;
        for (column_index, character) in line.chars().enumerate() {
            let Some(rows) = glyph(character) else {
                continue;
            };
            let left = padding + column_index as u32 * ADVANCE * scale;
            for (row_index, bits) in rows.iter().enumerate() {
                for column in 0..GLYPH_WIDTH {
                    if bits & (1 << (GLYPH_WIDTH - 1 - column)) == 0 {
                        continue;
                    }
                    fill_block(
                        &mut pixels,
                        width,
                        left + column * scale,
                        top + row_index as u32 * scale,
                        scale,
                        foreground,
                    );
                }
            }
        }
    }

    Some(RasterText {
        width,
        height,
        pixels,
    })
}

fn fill_block(pixels: &mut [u8], width: u32, left: u32, top: u32, scale: u32, colour: [u8; 4]) {
    for y in top..top + scale {
        for x in left..left + scale {
            let offset = (y as usize * width as usize + x as usize) * 4;
            pixels[offset..offset + 4].copy_from_slice(&colour);
        }
    }
}

/// One glyph, top row first, leftmost bit of each row is the left column.
///
/// Lowercase input is upper-cased; characters without a glyph render as a space so a
/// new label can never produce a garbled overlay.
fn glyph(character: char) -> Option<[u8; 7]> {
    let rows = match character.to_ascii_uppercase() {
        'A' => [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'B' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
        'C' => [
            0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110,
        ],
        'D' => [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
        'E' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        'F' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'G' => [
            0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01110,
        ],
        'H' => [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'I' => [
            0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        'J' => [
            0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100,
        ],
        'K' => [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
        'L' => [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
        'M' => [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
        'N' => [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
        'O' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'P' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'Q' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ],
        'R' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
        'S' => [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        'T' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'U' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'V' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
        'W' => [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010,
        ],
        'X' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
        'Y' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'Z' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
        '0' => [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
        '3' => [
            0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110,
        ],
        '4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110,
        ],
        '6' => [
            0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100,
        ],
        '.' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b01100, 0b01100,
        ],
        ',' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b01100, 0b01100, 0b01000,
        ],
        ':' => [
            0b00000, 0b01100, 0b01100, 0b00000, 0b01100, 0b01100, 0b00000,
        ],
        '-' => [
            0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000,
        ],
        '/' => [
            0b00001, 0b00010, 0b00010, 0b00100, 0b01000, 0b01000, 0b10000,
        ],
        '%' => [
            0b11001, 0b11010, 0b00010, 0b00100, 0b01000, 0b01011, 0b10011,
        ],
        '(' => [
            0b00010, 0b00100, 0b01000, 0b01000, 0b01000, 0b00100, 0b00010,
        ],
        ')' => [
            0b01000, 0b00100, 0b00010, 0b00010, 0b00010, 0b00100, 0b01000,
        ],
        '+' => [
            0b00000, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0b00000,
        ],
        '=' => [
            0b00000, 0b00000, 0b11111, 0b00000, 0b11111, 0b00000, 0b00000,
        ],
        '<' => [
            0b00010, 0b00100, 0b01000, 0b10000, 0b01000, 0b00100, 0b00010,
        ],
        '>' => [
            0b01000, 0b00100, 0b00010, 0b00001, 0b00010, 0b00100, 0b01000,
        ],
        '_' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b11111,
        ],
        '?' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b00000, 0b00100,
        ],
        ' ' => [0; 7],
        _ => return None,
    };
    Some(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLACK: [u8; 4] = [0, 0, 0, 255];
    const WHITE: [u8; 4] = [255, 255, 255, 255];

    fn foreground_pixels(text: &RasterText) -> usize {
        text.pixels
            .chunks_exact(4)
            .filter(|pixel| pixel[0] == 255)
            .count()
    }

    #[test]
    fn dimensions_follow_the_glyph_grid() {
        let text = rasterize(&["AB".to_string()], 1, BLACK, WHITE).expect("text");

        // 2 glyphs * (5 + 1) + 2 * (3 px padding)
        assert_eq!(text.width, 18);
        // 1 line * (7 + 1) + 2 * (3 px padding)
        assert_eq!(text.height, 14);
        assert_eq!(text.pixels.len(), 18 * 14 * 4);
        assert!(foreground_pixels(&text) > 0);
    }

    #[test]
    fn scale_multiplies_the_grid() {
        let one = rasterize(&["A".to_string()], 1, BLACK, WHITE).expect("text");
        let two = rasterize(&["A".to_string()], 2, BLACK, WHITE).expect("text");

        assert_eq!(two.width, one.width * 2);
        assert_eq!(two.height, one.height * 2);
        assert_eq!(foreground_pixels(&two), foreground_pixels(&one) * 4);
    }

    #[test]
    fn lowercase_is_upper_cased() {
        let lower = rasterize(&["fps".to_string()], 1, BLACK, WHITE).expect("text");
        let upper = rasterize(&["FPS".to_string()], 1, BLACK, WHITE).expect("text");

        assert_eq!(lower.pixels, upper.pixels);
    }

    #[test]
    fn unknown_characters_render_as_spaces() {
        let unknown = rasterize(&["\u{1}".to_string()], 1, BLACK, WHITE).expect("text");
        let space = rasterize(&[" ".to_string()], 1, BLACK, WHITE).expect("text");

        assert_eq!(unknown.pixels, space.pixels);
        assert_eq!(foreground_pixels(&unknown), 0);
    }

    #[test]
    fn an_empty_line_list_has_nothing_to_draw() {
        assert!(rasterize(&[], 2, BLACK, WHITE).is_none());
        assert!(rasterize(&[String::new()], 2, BLACK, WHITE).is_none());
    }
}
