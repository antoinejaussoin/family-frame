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
    #[cfg(feature = "oled-debug")]
    temp: Option<Channel<'d>>,
}

impl<'d> Battery<'d> {
    pub fn new(adc: Peri<'d, ADC>, pin_43: Peri<'d, PIN_43>) -> Self {
        Self {
            adc: Adc::new_blocking(adc, Config::default()),
            vsys: Channel::new_pin(pin_43, Pull::None),
            #[cfg(feature = "oled-debug")]
            temp: None,
        }
    }

    #[cfg(feature = "oled-debug")]
    pub fn with_chip_temp(
        mut self,
        sensor: Peri<'d, embassy_rp::peripherals::ADC_TEMP_SENSOR>,
    ) -> Self {
        self.temp = Some(Channel::new_temp_sensor(sensor));
        self
    }

    /// On-die temperature in tenths of a degree C, or `None` if unused.
    #[cfg(feature = "oled-debug")]
    pub fn sample_chip_tenths(&mut self) -> Option<i16> {
        let ch = self.temp.as_mut()?;
        let raw = self.adc.blocking_read(ch).ok()?;
        let celsius = 27.0 - (raw as f32 * 3.3 / 4096.0 - 0.706) / 0.001721;
        Some((celsius * 10.0) as i16)
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
