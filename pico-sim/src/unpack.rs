//! Expand a packed Spectra 6 `.bin` back to a debug PNG.
//!
//! Must match the server packer and [`firmware/PROTOCOL.md`]: headerless,
//! 1600×1200, two pixels per byte (high nibble = even column).

use anyhow::{bail, Result};
use image::{Rgba, RgbaImage};

pub const PANEL_WIDTH: u32 = 1600;
pub const PANEL_HEIGHT: u32 = 1200;
pub const PANEL_BYTES: usize = (PANEL_WIDTH as usize * PANEL_HEIGHT as usize) / 2;

const SPECTRA6: [(u8, [u8; 3]); 6] = [
    (0, [0x00, 0x00, 0x00]),
    (1, [0xff, 0xff, 0xff]),
    (2, [0xff, 0xff, 0x00]),
    (3, [0xff, 0x00, 0x00]),
    (5, [0x00, 0x00, 0xff]),
    (6, [0x00, 0xff, 0x00]),
];

pub fn unpack_preview_png(bin: &[u8]) -> Result<Vec<u8>> {
    if bin.len() != PANEL_BYTES {
        bail!(
            "packed frame must be {PANEL_BYTES} bytes, got {}",
            bin.len()
        );
    }
    let mut img = RgbaImage::new(PANEL_WIDTH, PANEL_HEIGHT);
    let mut i = 0;
    for y in 0..PANEL_HEIGHT {
        for x in (0..PANEL_WIDTH).step_by(2) {
            let byte = bin[i];
            i += 1;
            let hi = byte >> 4;
            let lo = byte & 0x0f;
            img.put_pixel(x, y, rgba_for(hi));
            img.put_pixel(x + 1, y, rgba_for(lo));
        }
    }
    let mut png = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)?;
    Ok(png)
}

fn rgba_for(idx: u8) -> Rgba<u8> {
    let rgb = SPECTRA6
        .iter()
        .find(|(i, _)| *i == idx)
        .map(|(_, rgb)| *rgb)
        .unwrap_or([0, 0, 0]);
    Rgba([rgb[0], rgb[1], rgb[2], 255])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrong_size_is_rejected() {
        assert!(unpack_preview_png(&[0u8; 10]).is_err());
    }
}
