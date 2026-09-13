//! EL133UF1 13.3″ Spectra 6 driver.
//!
//! Derived from el133-pico-driver / tesserae-device-pico-bin (AGPL-3.0-or-later).
//! Init values match Pimoroni inky_el133uf1.py.
//!
//! The panel is landscape-native 1600×1200. The two controllers scan a
//! 1200×1600 portrait frame split at column 600 (CS_M = GP26 left,
//! CS_S = GP16 right). `show_frame` rotates a landscape packed buffer
//! 90° CW and splits it.

use crate::epd::{Chip, Epd, SETUP_MS};
use family_frame_fw::panel::{PROWS, ROW_BYTES, pack_controller_row};

pub use family_frame_fw::panel::{BLUE, FRAME_BYTES, RED, YELLOW};

pub async fn init_panel(epd: &mut Epd) {
    epd.reset(1, 30, 30, 300).await;

    const ANTM: &[u8] = &[0x00, 0x0C, 0x0C, 0xD9, 0xDD, 0xDD, 0x15, 0x15, 0x55];
    const CMD66: &[u8] = &[0x49, 0x55, 0x13, 0x5D, 0x05, 0x10];
    const PSR: &[u8] = &[0xDF, 0x6B];
    const DCDC: &[u8] = &[0x44, 0x54, 0x00];
    const PLL: &[u8] = &[0x08];
    const CDI: &[u8] = &[0x37];
    const TCON: &[u8] = &[0x03, 0x03];
    const POFS0: &[u8] = &[0x00, 0xC0, 0x03, 0xA8];
    const POFS1: &[u8] = &[0x00, 0xC0, 0x03, 0x9A];
    const AGID: &[u8] = &[0x10];
    const PWS: &[u8] = &[0x22];
    const CCSET: &[u8] = &[0x01];
    const TRES: &[u8] = &[0x04, 0xB0, 0x03, 0x20];
    const CMDA4: &[u8] = &[0x03, 0x00, 0x01, 0x03, 0x00, 0x03, 0x00, 0x00, 0x00];
    const PWR: &[u8] = &[0x0F, 0x00, 0x28, 0x2C, 0x28, 0x38];
    const ENBUF: &[u8] = &[0x07];
    const BTSTP: &[u8] = &[0xE0, 0x20];
    const BVDDP: &[u8] = &[0x01];
    const BTSTN: &[u8] = &[0xE0, 0x20];
    const BBVDN: &[u8] = &[0x01];
    const VCOMP: &[u8] = &[0x02];

    epd.command(Chip::Master, SETUP_MS, 0x74, ANTM).await;
    epd.command(Chip::Both, SETUP_MS, 0xF0, CMD66).await;
    epd.command(Chip::Both, SETUP_MS, 0x00, PSR).await;
    epd.command(Chip::Master, SETUP_MS, 0xA5, DCDC).await;
    epd.command(Chip::Both, SETUP_MS, 0x30, PLL).await;
    epd.command(Chip::Both, SETUP_MS, 0x50, CDI).await;
    epd.command(Chip::Both, SETUP_MS, 0x60, TCON).await;
    epd.command(Chip::Master, SETUP_MS, 0x03, POFS0).await;
    epd.command(Chip::Slave, SETUP_MS, 0x03, POFS1).await;
    epd.command(Chip::Both, SETUP_MS, 0x86, AGID).await;
    epd.command(Chip::Both, SETUP_MS, 0xE3, PWS).await;
    epd.command(Chip::Both, SETUP_MS, 0xE0, CCSET).await;
    epd.command(Chip::Both, SETUP_MS, 0x61, TRES).await;
    epd.command(Chip::Master, SETUP_MS, 0xA4, CMDA4).await;
    epd.command(Chip::Master, SETUP_MS, 0x01, PWR).await;
    epd.command(Chip::Master, SETUP_MS, 0xB6, ENBUF).await;
    epd.command(Chip::Master, SETUP_MS, 0x06, BTSTP).await;
    epd.command(Chip::Master, SETUP_MS, 0xB7, BVDDP).await;
    epd.command(Chip::Master, SETUP_MS, 0x05, BTSTN).await;
    epd.command(Chip::Master, SETUP_MS, 0xB0, BBVDN).await;
    epd.command(Chip::Master, SETUP_MS, 0xB1, VCOMP).await;
}

async fn refresh(epd: &mut Epd) {
    epd.command(Chip::Both, SETUP_MS, 0x04, &[]).await;
    embassy_time::Timer::after_millis(300).await;
    epd.command(Chip::Both, SETUP_MS, 0x12, &[0x00]).await;
    epd.wait_ready(60_000).await;
    epd.command(Chip::Both, SETUP_MS, 0x02, &[0x00]).await;
    embassy_time::Timer::after_millis(300).await;
}

async fn show_pattern<F>(epd: &mut Epd, mut row: F)
where
    F: FnMut(usize, usize, &mut [u8; ROW_BYTES]),
{
    init_panel(epd).await;
    let mut buf = [0u8; ROW_BYTES];

    epd.dtm_begin(Chip::Master, SETUP_MS, 0x10).await;
    for i in 0..PROWS {
        row(i, 0, &mut buf);
        epd.dtm_write(&buf);
    }
    epd.dtm_end(Chip::Master);

    epd.dtm_begin(Chip::Slave, SETUP_MS, 0x10).await;
    for i in 0..PROWS {
        row(i, 1, &mut buf);
        epd.dtm_write(&buf);
    }
    epd.dtm_end(Chip::Slave);

    refresh(epd).await;
}

/// Rotate + split a landscape packed-4bpp frame (`FRAME_BYTES`).
pub async fn show_frame(epd: &mut Epd, frame: &[u8]) {
    show_pattern(epd, |prow, half, out| {
        pack_controller_row(frame, prow, half, out);
    })
    .await;
}

/// Solid fill of one Spectra 6 colour. Useful for cold-boot diagnostics.
pub async fn show_solid(epd: &mut Epd, colour: u8) {
    let packed = ((colour & 0x0F) << 4) | (colour & 0x0F);
    show_pattern(epd, |_prow, _half, out| {
        out.fill(packed);
    })
    .await;
}
