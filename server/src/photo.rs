//! Photo pipeline: cover-crop to 4:3, Spectra 6 dither, pack.
//!
//! Live recipe ([`PHOTO_DITHER_MODE`] × [`PHOTO_PUNCH`]) was picked from
//! `examples/dither_matrix` (see `examples/README.md`). Around that:
//!
//! - **OpenDisplay** (`epaper-dithering`): measured `SPECTRA_7_3_6COLOR_V2`,
//!   `tone: auto` + `gamut: auto` for photos; optional mild exposure/sat/shadows.
//! - **aitjcize / GooDisplay**: match against *measured* panel RGB, not neon
//!   primaries; mild unsharp only — no hand-tuned sky remaps.
//!
//! Dashboard HTML keeps its own Floyd–Steinberg path in [`crate::pack`].

use anyhow::{bail, Context, Result};
use epaper_dithering_core::enums::{DitherMode, GamutCompression, ToneCompression};
use epaper_dithering_core::measured_palettes::SPECTRA_7_3_6COLOR_V2;
use epaper_dithering_core::palettes::ColorScheme;
use epaper_dithering_core::types::ImageBuffer;
use epaper_dithering_core::{dither_with_canonical, DitherConfig};
use image::{imageops, Rgba, RgbaImage};

use crate::pack::{PANEL_BYTES, PANEL_HEIGHT, PANEL_WIDTH};

/// Bump when the dither recipe changes so cached `.bin` / preview files regenerate.
pub const DITHER_VERSION: u32 = 23;

/// Error-diffusion kernel for photos. Re-run `examples/dither_matrix` to compare.
pub const PHOTO_DITHER_MODE: DitherMode = DitherMode::Burkes;

/// Shared sRGB contrast (pivot 0.5) and OKLab saturation. `1.0` is identity.
pub const PHOTO_PUNCH: f64 = 0.90;

/// Crate palette index → Tesserae Spectra 6 nibble.
/// Order is BWY R B G (crate indices 0..5); wire nibbles skip 4.
const CRATE_TO_NIBBLE: [u8; 6] = [0, 1, 2, 3, 5, 6];

/// Measured SPECTRA_7_3_6COLOR_V2 RGB, keyed by Tesserae nibble (for UI preview).
const MEASURED_RGB: [(u8, [u8; 3]); 6] = [
    (0, [31, 24, 41]),    // black
    (1, [168, 180, 182]), // white
    (2, [180, 173, 0]),   // yellow
    (3, [113, 24, 19]),   // red
    (5, [36, 70, 139]),   // blue
    (6, [50, 84, 60]),    // green
];

/// Ideal Spectra primaries — blended into LCD previews only.
/// Measured white (~175) looks mid-grey on a monitor; the panel is brighter in
/// reflected light, so a mild lift keeps the UI honest without neon inks.
const IDEAL_RGB: [(u8, [u8; 3]); 6] = [
    (0, [0, 0, 0]),
    (1, [255, 255, 255]),
    (2, [255, 255, 0]),
    (3, [255, 0, 0]),
    (5, [0, 0, 255]),
    (6, [0, 255, 0]),
];

/// Blend measured → ideal for on-screen preview (0 = measured, 1 = neon ideal).
const PREVIEW_LIFT: f32 = 0.45;

/// Cover-crop to 4:3 (panel aspect), Lanczos3 to panel size, mild unsharp.
pub fn prepare_panel_rgba(img: &RgbaImage) -> RgbaImage {
    let cropped = cover_crop_4_3(img);
    let sized = if cropped.width() == PANEL_WIDTH && cropped.height() == PANEL_HEIGHT {
        cropped
    } else {
        imageops::resize(
            &cropped,
            PANEL_WIDTH,
            PANEL_HEIGHT,
            imageops::FilterType::Lanczos3,
        )
    };
    // GooDisplay: mild unsharp before dither. No tone remaps — OpenDisplay’s
    // exposure/shadows/tone/gamut knobs own that job.
    unsharp_rgba(&sized, 0.6)
}

/// Scale so the image covers the panel aspect, then center-crop.
pub fn cover_crop_4_3(img: &RgbaImage) -> RgbaImage {
    let w = img.width() as f64;
    let h = img.height() as f64;
    let target = PANEL_WIDTH as f64 / PANEL_HEIGHT as f64; // 4/3
    let (crop_w, crop_h) = if w / h > target {
        let crop_h = h;
        let crop_w = h * target;
        (crop_w, crop_h)
    } else {
        let crop_w = w;
        let crop_h = w / target;
        (crop_w, crop_h)
    };
    let x = ((w - crop_w) / 2.0).round().max(0.0) as u32;
    let y = ((h - crop_h) / 2.0).round().max(0.0) as u32;
    let cw = crop_w.round().clamp(1.0, w) as u32;
    let ch = crop_h.round().clamp(1.0, h) as u32;
    let cw = cw.min(img.width().saturating_sub(x));
    let ch = ch.min(img.height().saturating_sub(y));
    imageops::crop_imm(img, x, y, cw, ch).to_image()
}

/// Simple unsharp mask via blur + amount (amount 1.0 ≈ mild).
fn unsharp_rgba(img: &RgbaImage, amount: f32) -> RgbaImage {
    let blurred = imageops::blur(img, 1.0);
    let mut out = RgbaImage::new(img.width(), img.height());
    for (x, y, px) in img.enumerate_pixels() {
        let Rgba([r, g, b, a]) = *px;
        let Rgba([br, bg, bb, _]) = *blurred.get_pixel(x, y);
        let sharpen = |c: u8, bc: u8| -> u8 {
            let v = c as f32 + amount * (c as f32 - bc as f32);
            v.round().clamp(0.0, 255.0) as u8
        };
        out.put_pixel(
            x,
            y,
            Rgba([sharpen(r, br), sharpen(g, bg), sharpen(b, bb), a]),
        );
    }
    out
}

/// sRGB contrast around mid-grey. `1.0` is identity.
pub fn apply_contrast_srgb(img: &RgbaImage, contrast: f32) -> RgbaImage {
    if (contrast - 1.0).abs() < 1e-6 {
        return img.clone();
    }
    let mut out = img.clone();
    for px in out.pixels_mut() {
        for c in 0..3 {
            let v = px.0[c] as f32 / 255.0;
            px.0[c] = (((v - 0.5) * contrast + 0.5).clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        }
    }
    out
}

/// Dither a panel-sized RGBA image and pack to Spectra 6 nibbles.
pub fn dither_pack_rgba(img: &RgbaImage) -> Result<(Vec<u8>, Vec<u8>)> {
    if img.width() != PANEL_WIDTH || img.height() != PANEL_HEIGHT {
        bail!(
            "photo must be {PANEL_WIDTH}x{PANEL_HEIGHT}, got {}x{}",
            img.width(),
            img.height()
        );
    }
    let contrasted = apply_contrast_srgb(img, PHOTO_PUNCH as f32);
    let rgb = rgba_to_rgb_flat(&contrasted);
    let buf = ImageBuffer::new(&rgb, PANEL_WIDTH as usize);

    // OpenDisplay photo path: measured palette + auto tone/gamut.
    // Punch < 1.0 keeps Spectra blue/green from taking over skies and sand.
    let config = DitherConfig {
        mode: PHOTO_DITHER_MODE,
        serpentine: true,
        exposure: 1.05,
        saturation: PHOTO_PUNCH,
        shadows: 0.15,
        highlights: 0.3,
        tone: ToneCompression::Auto,
        gamut: GamutCompression::Auto,
    };
    let canonical = ColorScheme::Bwgbry;
    let indices = dither_with_canonical(&buf, &SPECTRA_7_3_6COLOR_V2, canonical, config);
    if indices.len() != (PANEL_WIDTH * PANEL_HEIGHT) as usize {
        bail!(
            "dither returned {} indices, expected {}",
            indices.len(),
            PANEL_WIDTH * PANEL_HEIGHT
        );
    }
    let nibbles: Vec<u8> = indices
        .iter()
        .map(|&i| {
            let n = CRATE_TO_NIBBLE.get(i as usize).copied().unwrap_or(0);
            // Spectra green is a common “olive sand / muddy shadow” attractor on
            // family photos (see Spectra 6 dither threads). Hokku’s hue-aware
            // gate avoids it; we don’t have that LUT, so map green → black for
            // photos only. Dashboard path keeps all six inks.
            if n == 6 {
                0
            } else {
                n
            }
        })
        .collect();
    let bin = pack_nibbles(&nibbles);
    if bin.len() != PANEL_BYTES {
        bail!(
            "packed photo must be {PANEL_BYTES} bytes, got {}",
            bin.len()
        );
    }
    let preview = unpack_measured_preview_png(&bin)?;
    Ok((bin, preview))
}

/// Decode bytes → prepare → dither → pack.
pub fn process_photo_bytes(bytes: &[u8]) -> Result<(Vec<u8>, Vec<u8>, RgbaImage)> {
    let img = image::load_from_memory(bytes)
        .context("decoding uploaded photo")?
        .to_rgba8();
    let panel = prepare_panel_rgba(&img);
    let (bin, preview) = dither_pack_rgba(&panel)?;
    Ok((bin, preview, panel))
}

/// Expand packed nibbles to a PNG using measured panel colours (better UI preview).
pub fn unpack_measured_preview_png(bin: &[u8]) -> Result<Vec<u8>> {
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
            img.put_pixel(x, y, measured_rgba(byte >> 4));
            img.put_pixel(x + 1, y, measured_rgba(byte & 0x0f));
        }
    }
    let mut png = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)?;
    Ok(png)
}

fn measured_rgba(idx: u8) -> Rgba<u8> {
    // Pure black on LCD — measured black is a purple-grey.
    if idx == 0 {
        return Rgba([0, 0, 0, 255]);
    }
    let measured = MEASURED_RGB
        .iter()
        .find(|(i, _)| *i == idx)
        .map(|(_, rgb)| *rgb)
        .unwrap_or([0, 0, 0]);
    let ideal = IDEAL_RGB
        .iter()
        .find(|(i, _)| *i == idx)
        .map(|(_, rgb)| *rgb)
        .unwrap_or(measured);
    let t = PREVIEW_LIFT;
    let blend = |m: u8, i: u8| -> u8 {
        ((m as f32) * (1.0 - t) + (i as f32) * t)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    Rgba([
        blend(measured[0], ideal[0]),
        blend(measured[1], ideal[1]),
        blend(measured[2], ideal[2]),
        255,
    ])
}

fn rgba_to_rgb_flat(img: &RgbaImage) -> Vec<u8> {
    let mut out = Vec::with_capacity((img.width() * img.height() * 3) as usize);
    for px in img.pixels() {
        let Rgba([r, g, b, a]) = *px;
        if a < 128 {
            out.extend_from_slice(&[255, 255, 255]);
        } else {
            out.extend_from_slice(&[r, g, b]);
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

/// Build a small JPEG thumbnail for the library grid.
pub fn make_thumb_jpeg(img: &RgbaImage, max_edge: u32) -> Result<Vec<u8>> {
    let (w, h) = (img.width(), img.height());
    let scale = max_edge as f64 / w.max(h) as f64;
    let tw = ((w as f64) * scale).round().max(1.0) as u32;
    let th = ((h as f64) * scale).round().max(1.0) as u32;
    let thumb = imageops::resize(img, tw, th, imageops::FilterType::Triangle);
    let rgb = image::DynamicImage::ImageRgba8(thumb).to_rgb8();
    let mut out = Vec::new();
    rgb.write_to(
        &mut std::io::Cursor::new(&mut out),
        image::ImageFormat::Jpeg,
    )?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_to_nibble_matches_tesserae() {
        assert_eq!(CRATE_TO_NIBBLE, [0, 1, 2, 3, 5, 6]);
    }

    #[test]
    fn cover_crop_trims_too_wide() {
        let img = RgbaImage::from_pixel(2000, 1200, Rgba([10, 20, 30, 255]));
        let cropped = cover_crop_4_3(&img);
        assert_eq!(cropped.width(), 1600);
        assert_eq!(cropped.height(), 1200);
    }

    #[test]
    fn cover_crop_trims_too_tall_landscape() {
        let img = RgbaImage::from_pixel(1600, 1400, Rgba([10, 20, 30, 255]));
        let cropped = cover_crop_4_3(&img);
        let ratio = cropped.width() as f64 / cropped.height() as f64;
        assert!((ratio - 4.0 / 3.0).abs() < 0.01, "ratio={ratio}");
    }

    #[test]
    fn prepare_yields_panel_size() {
        let img = RgbaImage::from_pixel(800, 600, Rgba([200, 100, 50, 255]));
        let panel = prepare_panel_rgba(&img);
        assert_eq!(panel.width(), PANEL_WIDTH);
        assert_eq!(panel.height(), PANEL_HEIGHT);
    }

    #[test]
    fn dither_pack_exact_size() {
        let img = RgbaImage::from_pixel(PANEL_WIDTH, PANEL_HEIGHT, Rgba([180, 120, 80, 255]));
        let (bin, preview) = dither_pack_rgba(&img).unwrap();
        assert_eq!(bin.len(), PANEL_BYTES);
        assert!(!preview.is_empty());
        for byte in &bin {
            let hi = byte >> 4;
            let lo = byte & 0x0f;
            assert!(matches!(hi, 0 | 1 | 2 | 3 | 5 | 6), "hi={hi}");
            assert!(matches!(lo, 0 | 1 | 2 | 3 | 5 | 6), "lo={lo}");
        }
    }

    #[test]
    fn thumb_jpeg_encodes_from_rgba() {
        let img = RgbaImage::from_pixel(200, 150, Rgba([10, 20, 30, 255]));
        let jpeg = make_thumb_jpeg(&img, 80).unwrap();
        assert!(jpeg.len() > 50);
        assert_eq!(&jpeg[..2], &[0xff, 0xd8]);
    }

    #[test]
    fn contrast_one_is_identity() {
        let img = RgbaImage::from_pixel(4, 4, Rgba([40, 90, 180, 255]));
        assert_eq!(apply_contrast_srgb(&img, 1.0), img);
    }

    #[test]
    fn measured_preview_uses_panel_white_not_pure() {
        let img = RgbaImage::from_pixel(PANEL_WIDTH, PANEL_HEIGHT, Rgba([255, 255, 255, 255]));
        let (bin, preview) = dither_pack_rgba(&img).unwrap();
        assert_eq!(bin.len(), PANEL_BYTES);
        let preview_img = image::load_from_memory(&preview).unwrap().to_rgba8();
        let Rgba([r, g, b, _]) = *preview_img.get_pixel(0, 0);
        // Lifted preview white sits between measured (~175) and pure 255.
        assert!(
            r > 190 && r < 255,
            "lifted preview white expected, got {r},{g},{b}"
        );
    }
}
