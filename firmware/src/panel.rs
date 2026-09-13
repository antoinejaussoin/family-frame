//! Landscape packed-4bpp → EL133 portrait halves (90° CW, split at col 600).

pub const WIDTH: usize = 1600;
pub const HEIGHT: usize = 1200;
pub const FRAME_BYTES: usize = WIDTH * HEIGHT / 2;
pub const ROW_BYTES: usize = 300;
pub const PROWS: usize = 1600;
pub const LANDSCAPE_ROW_BYTES: usize = WIDTH / 2;

pub const BLACK: u8 = 0x0;
pub const WHITE: u8 = 0x1;
pub const YELLOW: u8 = 0x2;
pub const RED: u8 = 0x3;
pub const BLUE: u8 = 0x5;
pub const GREEN: u8 = 0x6;

/// One controller scan line after the 90° CW rotate/split.
///
/// Landscape `(x, y)` → portrait `(1199 - y, x)`. Master (`half == 0`) is
/// portrait columns 0–599 (landscape `y` 1199…600); slave is 600–1199
/// (`y` 599…0). `prow` is the landscape `x`. Each output byte is two
/// portrait columns: high nibble even, low nibble odd.
pub fn pack_controller_row(frame: &[u8], prow: usize, half: usize, out: &mut [u8; ROW_BYTES]) {
    debug_assert_eq!(frame.len(), FRAME_BYTES);
    debug_assert!(prow < PROWS);
    debug_assert!(half <= 1);
    let shift = if prow & 1 == 1 { 0 } else { 4 };
    let col = prow >> 1;
    let base = if half == 0 { 1199 } else { 599 };
    for k in 0..ROW_BYTES {
        let hb = frame[(base - 2 * k) * LANDSCAPE_ROW_BYTES + col];
        let lb = frame[(base - 1 - 2 * k) * LANDSCAPE_ROW_BYTES + col];
        out[k] = (((hb >> shift) & 0x0F) << 4) | ((lb >> shift) & 0x0F);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set_pixel(frame: &mut [u8], x: usize, y: usize, colour: u8) {
        let i = y * LANDSCAPE_ROW_BYTES + x / 2;
        if x % 2 == 0 {
            frame[i] = (frame[i] & 0x0F) | ((colour & 0x0F) << 4);
        } else {
            frame[i] = (frame[i] & 0xF0) | (colour & 0x0F);
        }
    }

    fn nibble(out: &[u8; ROW_BYTES], k: usize, high: bool) -> u8 {
        if high { out[k] >> 4 } else { out[k] & 0x0F }
    }

    fn packed(frame: &[u8], prow: usize, half: usize) -> [u8; ROW_BYTES] {
        let mut out = [0u8; ROW_BYTES];
        pack_controller_row(frame, prow, half, &mut out);
        out
    }

    fn hits(frame: &[u8], colour: u8) -> Vec<(usize, usize, usize, bool)> {
        let mut found = Vec::new();
        for half in 0..2 {
            for prow in 0..PROWS {
                let out = packed(frame, prow, half);
                for k in 0..ROW_BYTES {
                    if nibble(&out, k, true) == colour {
                        found.push((half, prow, k, true));
                    }
                    if nibble(&out, k, false) == colour {
                        found.push((half, prow, k, false));
                    }
                }
            }
        }
        found
    }

    #[test]
    fn master_top_left_is_landscape_bottom_left() {
        let mut frame = vec![0u8; FRAME_BYTES];
        set_pixel(&mut frame, 0, 1199, RED);
        assert_eq!(hits(&frame, RED), [(0, 0, 0, true)]);
    }

    #[test]
    fn master_odd_x_uses_low_nibble_of_the_landscape_byte() {
        let mut frame = vec![0u8; FRAME_BYTES];
        set_pixel(&mut frame, 1, 1198, BLUE);
        assert_eq!(hits(&frame, BLUE), [(0, 1, 0, false)]);
    }

    #[test]
    fn slave_starts_at_landscape_y_599() {
        let mut frame = vec![0u8; FRAME_BYTES];
        set_pixel(&mut frame, 0, 599, GREEN);
        assert_eq!(hits(&frame, GREEN), [(1, 0, 0, true)]);
    }

    #[test]
    fn split_does_not_leak_across_halves() {
        let mut frame = vec![0u8; FRAME_BYTES];
        set_pixel(&mut frame, 4, 600, YELLOW);
        set_pixel(&mut frame, 4, 599, RED);
        assert_eq!(hits(&frame, YELLOW), [(0, 4, 299, false)]);
        assert_eq!(hits(&frame, RED), [(1, 4, 0, true)]);
    }

    #[test]
    fn landscape_bottom_right_is_master_last_prow() {
        let mut frame = vec![0u8; FRAME_BYTES];
        set_pixel(&mut frame, 1599, 1199, WHITE);
        assert_eq!(hits(&frame, WHITE), [(0, 1599, 0, true)]);
    }

    #[test]
    fn landscape_top_right_is_slave_last_byte() {
        let mut frame = vec![0x11u8; FRAME_BYTES];
        set_pixel(&mut frame, 1599, 0, BLACK);
        assert_eq!(hits(&frame, BLACK), [(1, 1599, 299, false)]);
    }
}
