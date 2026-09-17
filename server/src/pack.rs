//! Pack a 1600×1200 PNG into the Inky / Tesserae Spectra 6 `.bin` wire format.
//!
//! Headerless, exactly `1600 * 1200 / 2` bytes, scanline order, two pixels
//! per byte: high nibble = even column, low nibble = odd column.
//!
//! Palette (nibble values):
//! `0` black, `1` white, `2` yellow, `3` red, `5` blue, `6` green.

use anyhow::{bail, Result};
use image::{Rgba, RgbaImage};

pub const PANEL_WIDTH: u32 = 1600;
pub const PANEL_HEIGHT: u32 = 1200;
pub const PANEL_BYTES: usize = (PANEL_WIDTH as usize * PANEL_HEIGHT as usize) / 2;

/// Spectra 6 indices and the sRGB primaries the Inky firmware expects.
pub const SPECTRA6: [(u8, [u8; 3]); 6] = [
    (0, [0x00, 0x00, 0x00]),
    (1, [0xff, 0xff, 0xff]),
    (2, [0xff, 0xff, 0x00]),
    (3, [0xff, 0x00, 0x00]),
    (5, [0x00, 0x00, 0xff]),
    (6, [0x00, 0xff, 0x00]),
];

pub fn pack_png_to_spectra6(png: &[u8]) -> Result<Vec<u8>> {
    let img = image::load_from_memory(png)?.to_rgba8();
    pack_rgba(&img)
}

pub fn pack_rgba(img: &RgbaImage) -> Result<Vec<u8>> {
    if img.width() != PANEL_WIDTH || img.height() != PANEL_HEIGHT {
        bail!(
            "frame must be {PANEL_WIDTH}x{PANEL_HEIGHT}, got {}x{}",
            img.width(),
            img.height()
        );
    }
    let indexed = dither_floyd_steinberg(img);
    Ok(pack_nibbles(&indexed))
}

fn color_dist2(r: i32, g: i32, b: i32, rgb: [u8; 3]) -> i32 {
    let dr = r - rgb[0] as i32;
    let dg = g - rgb[1] as i32;
    let db = b - rgb[2] as i32;
    dr * dr + dg * dg + db * db
}

fn nearest_index(r: i32, g: i32, b: i32) -> (u8, [u8; 3]) {
    let mut best = SPECTRA6[0];
    let mut best_d = i32::MAX;
    for &(idx, rgb) in &SPECTRA6 {
        let d = color_dist2(r, g, b, rgb);
        if d < best_d {
            best_d = d;
            best = (idx, rgb);
        }
    }
    best
}

/// Squared distance from `p` to the RGB segment between two palette colours.
fn dist_to_segment2(p: [i32; 3], a: [u8; 3], b: [u8; 3]) -> i32 {
    let ab = [
        b[0] as i32 - a[0] as i32,
        b[1] as i32 - a[1] as i32,
        b[2] as i32 - a[2] as i32,
    ];
    let ap = [p[0] - a[0] as i32, p[1] - a[1] as i32, p[2] - a[2] as i32];
    let ab2 = ab[0] * ab[0] + ab[1] * ab[1] + ab[2] * ab[2];
    if ab2 == 0 {
        return ap[0] * ap[0] + ap[1] * ap[1] + ap[2] * ap[2];
    }
    let dot = ap[0] * ab[0] + ap[1] * ab[1] + ap[2] * ab[2];
    let t = dot.clamp(0, ab2);
    let closest = [
        a[0] as i32 + ab[0] * t / ab2,
        a[1] as i32 + ab[1] * t / ab2,
        a[2] as i32 + ab[2] * t / ab2,
    ];
    let d0 = p[0] - closest[0];
    let d1 = p[1] - closest[1];
    let d2 = p[2] - closest[2];
    d0 * d0 + d1 * d1 + d2 * d2
}

/// Black or white — the paper and the usual ink. Anti-aliased type is a
/// blend with one of these; chromatic mixes (orange = yellow+red) are not.
fn is_black_or_white(rgb: [u8; 3]) -> bool {
    rgb == [0x00, 0x00, 0x00] || rgb == [0xff, 0xff, 0xff]
}

/// Chrome font/SVG anti-aliasing is a blend of two Spectra colours.
/// Greys against paper must snap with no error diffusion, or letters grow
/// a halo. Blends of two inks (sun orange, etc.) are left to Floyd–Steinberg
/// so a real colour in the screenshot dithers at 1px after raster.
fn is_antialiased_edge(r: i32, g: i32, b: i32) -> bool {
    const EDGE_DIST2: i32 = 48 * 48;
    let p = [r, g, b];
    for (i, &(_, a)) in SPECTRA6.iter().enumerate() {
        if color_dist2(r, g, b, a) <= EDGE_DIST2 {
            return true;
        }
        for &(_, c) in SPECTRA6.iter().skip(i + 1) {
            if !is_black_or_white(a) && !is_black_or_white(c) {
                continue;
            }
            if dist_to_segment2(p, a, c) <= EDGE_DIST2 {
                return true;
            }
        }
    }
    false
}

/// Floyd–Steinberg dither to the six panel colours.
/// Anti-aliased UI edges snap to the palette so type stays crisp.
fn dither_floyd_steinberg(img: &RgbaImage) -> Vec<u8> {
    let w = img.width() as usize;
    let h = img.height() as usize;
    let mut work: Vec<[i32; 3]> = img
        .pixels()
        .map(|px| {
            let Rgba([r, g, b, _]) = *px;
            [r as i32, g as i32, b as i32]
        })
        .collect();
    let mut out = vec![0u8; w * h];

    for y in 0..h {
        let left_to_right = y % 2 == 0;
        let xs: Box<dyn Iterator<Item = usize>> = if left_to_right {
            Box::new(0..w)
        } else {
            Box::new((0..w).rev())
        };
        for x in xs {
            let i = y * w + x;
            let Rgba([sr, sg, sb, _]) = img[(x as u32, y as u32)];
            let snap = is_antialiased_edge(sr as i32, sg as i32, sb as i32);
            let (idx, rgb) = if snap {
                nearest_index(sr as i32, sg as i32, sb as i32)
            } else {
                let [r, g, b] = work[i];
                nearest_index(r, g, b)
            };
            out[i] = idx;
            if snap {
                continue;
            }
            let [r, g, b] = work[i];
            let err = [r - rgb[0] as i32, g - rgb[1] as i32, b - rgb[2] as i32];
            let mut spread = |wx: isize, wy: isize, num: i32, den: i32| {
                let nx = x as isize + wx;
                let ny = y as isize + wy;
                if nx < 0 || ny < 0 || nx >= w as isize || ny >= h as isize {
                    return;
                }
                let j = ny as usize * w + nx as usize;
                for c in 0..3 {
                    work[j][c] += err[c] * num / den;
                }
            };
            if left_to_right {
                spread(1, 0, 7, 16);
                spread(-1, 1, 3, 16);
                spread(0, 1, 5, 16);
                spread(1, 1, 1, 16);
            } else {
                spread(-1, 0, 7, 16);
                spread(1, 1, 3, 16);
                spread(0, 1, 5, 16);
                spread(-1, 1, 1, 16);
            }
        }
    }
    out
}

fn pack_nibbles(indexed: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(PANEL_BYTES);
    for pair in indexed.chunks_exact(2) {
        out.push((pair[0] << 4) | (pair[1] & 0x0f));
    }
    out
}

/// Debug PNG: expand packed nibbles back to the six RGB primaries.
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

#[allow(dead_code)]
pub fn solid_png(r: u8, g: u8, b: u8) -> Result<Vec<u8>> {
    let img = RgbaImage::from_pixel(PANEL_WIDTH, PANEL_HEIGHT, Rgba([r, g, b, 255]));
    let mut png = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)?;
    Ok(png)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_size_is_960000() {
        let png = solid_png(255, 255, 255).unwrap();
        let bin = pack_png_to_spectra6(&png).unwrap();
        assert_eq!(bin.len(), PANEL_BYTES);
        assert!(bin.iter().all(|&b| b == 0x11), "white should pack as 0x11");
    }

    #[test]
    fn black_and_red_nibbles() {
        let mut img = RgbaImage::from_pixel(PANEL_WIDTH, PANEL_HEIGHT, Rgba([0, 0, 0, 255]));
        img.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        img.put_pixel(1, 0, Rgba([0, 0, 0, 255]));
        let bin = pack_rgba(&img).unwrap();
        assert_eq!(bin[0] >> 4, 3, "even column red");
        assert_eq!(bin[0] & 0x0f, 0, "odd column black");
    }

    #[test]
    fn wrong_size_is_rejected() {
        let img = RgbaImage::new(10, 10);
        assert!(pack_rgba(&img).is_err());
    }

    fn nibble_at(bin: &[u8], x: u32, y: u32) -> u8 {
        let i = (y * PANEL_WIDTH + x) as usize;
        let byte = bin[i / 2];
        if i % 2 == 0 {
            byte >> 4
        } else {
            byte & 0x0f
        }
    }

    #[test]
    fn antialiased_gray_does_not_speckle() {
        let mut img = RgbaImage::from_pixel(PANEL_WIDTH, PANEL_HEIGHT, Rgba([255, 255, 255, 255]));
        img.put_pixel(20, 20, Rgba([80, 80, 80, 255]));
        img.put_pixel(20, 21, Rgba([160, 160, 160, 255]));
        img.put_pixel(21, 20, Rgba([210, 180, 160, 255]));
        let bin = pack_rgba(&img).unwrap();
        assert_eq!(nibble_at(&bin, 20, 20), 0, "dark gray snaps to black");
        assert_eq!(nibble_at(&bin, 20, 21), 1, "light gray snaps to white");
        assert_eq!(
            nibble_at(&bin, 21, 20),
            1,
            "LCD fringe snaps without a new colour"
        );
        for (x, y) in [
            (19, 20),
            (22, 20),
            (20, 19),
            (20, 22),
            (19, 21),
            (21, 21),
            (22, 21),
        ] {
            assert_eq!(nibble_at(&bin, x, y), 1, "halo at {x},{y}");
        }
    }

    #[test]
    fn orange_fill_dithers_yellow_and_red() {
        let mut img = RgbaImage::from_pixel(PANEL_WIDTH, PANEL_HEIGHT, Rgba([255, 255, 255, 255]));
        for y in 200..280 {
            for x in 200..280 {
                img.put_pixel(x, y, Rgba([255, 136, 0, 255]));
            }
        }
        let bin = pack_rgba(&img).unwrap();
        let mut yellow = 0;
        let mut red = 0;
        for y in 210..270 {
            for x in 210..270 {
                match nibble_at(&bin, x, y) {
                    2 => yellow += 1,
                    3 => red += 1,
                    other => panic!("unexpected nibble {other} inside orange fill"),
                }
            }
        }
        assert!(
            yellow > 800 && red > 800,
            "orange should dither to both inks, yellow={yellow} red={red}"
        );
    }
}
