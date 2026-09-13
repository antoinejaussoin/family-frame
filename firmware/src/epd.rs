//! SPI/GPIO transport for the Inky Impression on a Hard Stuff Pico-to-Pi HAT.
//!
//! Derived from el133-pico-driver (AGPL-3.0-or-later).
//!
//! The HAT swaps clock and data (meter-verified; do not trust the vendor PDF):
//! SCLK GP10, MOSI GP11, DC GP22, RST GP27, BUSY GP17 (LOW = busy).

use embassy_rp::gpio::{Input, Output};
use embassy_rp::peripherals::SPI1;
use embassy_rp::spi::{Blocking, Spi};
use embassy_time::Timer;

pub const SETUP_MS: u64 = 300;

#[derive(Clone, Copy)]
pub enum Chip {
    Master,
    Slave,
    Both,
}

pub struct Epd {
    spi: Spi<'static, SPI1, Blocking>,
    dc: Output<'static>,
    rst: Output<'static>,
    busy: Input<'static>,
    cs_m: Output<'static>,
    cs_s: Output<'static>,
}

impl Epd {
    pub fn new(
        spi: Spi<'static, SPI1, Blocking>,
        dc: Output<'static>,
        rst: Output<'static>,
        busy: Input<'static>,
        cs_m: Output<'static>,
        cs_s: Output<'static>,
    ) -> Self {
        Self {
            spi,
            dc,
            rst,
            busy,
            cs_m,
            cs_s,
        }
    }

    pub async fn reset(&mut self, pulses: u8, low_ms: u64, high_ms: u64, settle_ms: u64) {
        for _ in 0..pulses {
            self.rst.set_low();
            Timer::after_millis(low_ms).await;
            self.rst.set_high();
            Timer::after_millis(high_ms).await;
        }
        Timer::after_millis(settle_ms).await;
    }

    pub async fn wait_ready(&mut self, timeout_ms: u64) {
        Timer::after_millis(2000).await;
        let mut ms = 2000u64;
        while self.busy.is_low() && ms < timeout_ms {
            Timer::after_millis(100).await;
            ms += 100;
        }
    }

    fn cs_assert(&mut self, chip: Chip) {
        match chip {
            Chip::Master => self.cs_m.set_low(),
            Chip::Slave => self.cs_s.set_low(),
            Chip::Both => {
                self.cs_m.set_low();
                self.cs_s.set_low();
            }
        }
    }

    fn cs_release(&mut self, chip: Chip) {
        match chip {
            Chip::Master => self.cs_m.set_high(),
            Chip::Slave => self.cs_s.set_high(),
            Chip::Both => {
                self.cs_m.set_high();
                self.cs_s.set_high();
            }
        }
    }

    pub async fn command(&mut self, chip: Chip, setup_ms: u64, cmd: u8, data: &[u8]) {
        self.cs_assert(chip);
        self.dc.set_low();
        Timer::after_millis(setup_ms).await;
        let _ = self.spi.blocking_write(&[cmd]);
        if !data.is_empty() {
            self.dc.set_high();
            let _ = self.spi.blocking_write(data);
        }
        self.cs_release(chip);
    }

    pub async fn dtm_begin(&mut self, chip: Chip, setup_ms: u64, dtm_cmd: u8) {
        self.cs_assert(chip);
        self.dc.set_low();
        Timer::after_millis(setup_ms).await;
        let _ = self.spi.blocking_write(&[dtm_cmd]);
        self.dc.set_high();
    }

    pub fn dtm_write(&mut self, data: &[u8]) {
        let _ = self.spi.blocking_write(data);
    }

    pub fn dtm_end(&mut self, chip: Chip) {
        self.cs_release(chip);
    }
}
