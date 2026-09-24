//! VSYS/3 on GP43 (Pico LiPo 2 XL W / Plus 2 W).

use core::sync::atomic::{AtomicU16, AtomicU32, Ordering};
use embassy_rp::Peri;
use embassy_rp::adc::{Adc, Blocking, Channel, Config};
use embassy_rp::gpio::Pull;
use embassy_rp::peripherals::{ADC, PIN_43};

static LAST_MV: AtomicU32 = AtomicU32::new(0);
static LAST_PCT: AtomicU16 = AtomicU16::new(0);

pub struct Battery<'d> {
    adc: Adc<'d, Blocking>,
    vsys: Channel<'d>,
}

impl<'d> Battery<'d> {
    pub fn new(adc: Peri<'d, ADC>, pin_43: Peri<'d, PIN_43>) -> Self {
        Self {
            adc: Adc::new_blocking(adc, Config::default()),
            vsys: Channel::new_pin(pin_43, Pull::None),
        }
    }

    pub fn sample(&mut self) -> (u32, u16) {
        let mut sum = 0u32;
        for _ in 0..8 {
            if let Ok(raw) = self.adc.blocking_read(&mut self.vsys) {
                sum += u32::from(raw);
            }
        }
        let raw = sum / 8;
        // 12-bit ADC, 3.3 V ref, onboard /3 divider.
        let mv = raw * 3 * 3300 / 4096;
        let pct = if mv >= 4200 {
            100
        } else if mv <= 3300 {
            0
        } else {
            ((mv - 3300) * 100 / (4200 - 3300)) as u16
        };
        LAST_MV.store(mv, Ordering::Relaxed);
        LAST_PCT.store(pct, Ordering::Relaxed);
        (mv, pct)
    }
}

pub fn last() -> (u32, u16) {
    (
        LAST_MV.load(Ordering::Relaxed),
        LAST_PCT.load(Ordering::Relaxed),
    )
}
