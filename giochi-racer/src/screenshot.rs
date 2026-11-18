//! Turning a captured frame into a PNG-free file a person can look at.
//!
//! BMP rather than PNG for the same reason the engine's own captures use it: no
//! dependency, no encoder, and every image viewer in existence opens it. The game only
//! writes one when `--quadro` is passed, which is what the gates use to prove a frame was
//! actually drawn rather than merely simulated.

use std::io;
use std::path::Path;

/// Bytes in a 32-bit BMP header.
const HEADER: usize = 54;

/// Writes `rgba` (four bytes per pixel, top row first) as a 32-bit BMP.
pub fn write_bmp(path: impl AsRef<Path>, width: u32, height: u32, rgba: &[u8]) -> io::Result<()> {
    let expected = width as usize * height as usize * 4;
    if rgba.len() != expected {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "capture has {} bytes; a {width}x{height} RGBA frame has {expected}",
                rgba.len()
            ),
        ));
    }
    std::fs::write(path.as_ref(), encode_bmp(width, height, rgba))
}

/// The BMP file, as bytes, with the RGBA reordered to the file's BGRA order.
pub fn encode_bmp(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let mut pixels = Vec::with_capacity(rgba.len());
    for pixel in rgba.as_chunks::<4>().0 {
        pixels.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
    }

    let mut bmp = Vec::with_capacity(HEADER + pixels.len());
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&((HEADER + pixels.len()) as u32).to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&(HEADER as u32).to_le_bytes());
    bmp.extend_from_slice(&40u32.to_le_bytes());
    bmp.extend_from_slice(&(width as i32).to_le_bytes());
    bmp.extend_from_slice(&(height as i32).to_le_bytes());
    bmp.extend_from_slice(&1u16.to_le_bytes());
    bmp.extend_from_slice(&32u16.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&(pixels.len() as u32).to_le_bytes());
    bmp.extend_from_slice(&2835i32.to_le_bytes());
    bmp.extend_from_slice(&2835i32.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&pixels);
    bmp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_header_describes_the_picture() {
        let rgba = vec![0u8; 2 * 2 * 4];
        let bmp = encode_bmp(2, 2, &rgba);
        assert_eq!(&bmp[0..2], b"BM");
        assert_eq!(bmp.len(), HEADER + 16);
        // File size, then the offset of the pixel data.
        assert_eq!(
            u32::from_le_bytes([bmp[2], bmp[3], bmp[4], bmp[5]]) as usize,
            bmp.len()
        );
        assert_eq!(
            u32::from_le_bytes([bmp[10], bmp[11], bmp[12], bmp[13]]) as usize,
            HEADER
        );
        // 32 bits per pixel, bottom-up, uncompressed.
        assert_eq!(u16::from_le_bytes([bmp[28], bmp[29]]), 32);
        assert_eq!(i32::from_le_bytes([bmp[18], bmp[19], bmp[20], bmp[21]]), 2);
        assert_eq!(i32::from_le_bytes([bmp[22], bmp[23], bmp[24], bmp[25]]), 2);
    }

    #[test]
    fn red_and_blue_swap_as_bmp_requires() {
        // One pixel: R=10 G=20 B=30 A=40 becomes B G R A.
        let bmp = encode_bmp(1, 1, &[10, 20, 30, 40]);
        assert_eq!(&bmp[HEADER..], &[30, 20, 10, 40]);
    }

    #[test]
    fn a_capture_of_the_wrong_size_is_refused_instead_of_written_short() {
        let error = write_bmp("/tmp/giochi-racer-should-not-exist.bmp", 4, 4, &[0u8; 8])
            .expect_err("a 4x4 frame needs 64 bytes");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(!Path::new("/tmp/giochi-racer-should-not-exist.bmp").exists());
    }
}
