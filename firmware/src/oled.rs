//! 0.96″ I²C OLED status panel for the `family-frame-oled` debug binary.
//!
//! Same Pi Hut 128×64 module as the laser-tag temperature-display node.
//! That listing says SSD1306; the working laser-tag firmware talks to it as
//! SH1106. Switch [`OledConfig::ssd1306_128x64`](oled_i2c::OledConfig::ssd1306_128x64)
//! below if the yellow band is readable and the blue area is noise.
//!
//! Wiring (USB-C end of the XL W, Pico numbering, **right** side):
//! OLED `VCC` → `3V3` (pin 36), `GND` → `GND` (pin 23), `SDA` → **GP18**
//! (pin 24), `SCL` → **GP19** (pin 25). Do not use GP16/GP17: those stay
//! wired for the Inky HAT.

use core::fmt::Write as _;
use embassy_rp::i2c::{Blocking, I2c};
use embassy_rp::peripherals::I2C1;
use embassy_rp::psram::Psram;
use embedded_graphics::mono_font::ascii::FONT_6X10;
use embedded_graphics::mono_font::{MonoTextStyle, MonoTextStyleBuilder};
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::text::{Baseline, Text};
use heapless::String;
use oled_i2c::{Oled, OledConfig};

use crate::battery::Battery;
use crate::el133::FRAME_BYTES;
use crate::wifi;

pub type OledI2c = I2c<'static, I2C1, Blocking>;

const I2C_ADDR: u8 = 0x3C;
const LINE_CHARS: usize = 21;

pub struct DebugOled {
    screen: Option<Screen>,
    psram: String<20>,
}

struct Screen {
    display: Oled<OledI2c>,
    style: MonoTextStyle<'static, BinaryColor>,
}

impl DebugOled {
    pub fn start(i2c: OledI2c, psram: Option<&Psram<'static>>) -> Self {
        Self {
            screen: Screen::new(i2c),
            psram: psram_label(psram),
        }
    }

    pub fn off(&mut self) {
        if let Some(screen) = self.screen.as_mut() {
            let _ = screen.display.display_off();
        }
    }

    pub fn paint(&mut self, _bat: &mut Battery<'_>, extra: &str) {
        let Some(screen) = self.screen.as_mut() else {
            return;
        };
        let _ = screen.display.display_on();
        let (mv, pct) = crate::battery::last();
        let status = wifi::status_line();
        screen.show(self.psram.as_str(), mv, pct, status.as_str(), extra);
    }
}

impl Screen {
    fn new(i2c: OledI2c) -> Option<Self> {
        Some(Self {
            display: Oled::new(
                i2c,
                I2C_ADDR,
                OledConfig::sh1106_128x64().with_column_offset(0),
            )
            .ok()?,
            style: MonoTextStyleBuilder::new()
                .font(&FONT_6X10)
                .text_color(BinaryColor::On)
                .build(),
        })
    }

    fn show(
        &mut self,
        psram: &str,
        mv: u32,
        pct: u16,
        status: &str,
        extra: &str,
    ) {
        self.display.clear_buffer();
        line(&mut self.display, self.style, 0, "family-frame OLED");
        line(&mut self.display, self.style, 1, psram);
        line(&mut self.display, self.style, 2, &bat_line(mv, pct));
        line(&mut self.display, self.style, 3, &lposc_line());
        line(&mut self.display, self.style, 4, status);
        line(&mut self.display, self.style, 5, extra);
        self.display.flush().ok();
    }
}

fn line(
    display: &mut Oled<OledI2c>,
    style: MonoTextStyle<'static, BinaryColor>,
    row: i32,
    text: &str,
) {
    let mut buf = [0u8; LINE_CHARS];
    let n = text.len().min(LINE_CHARS);
    buf[..n].copy_from_slice(&text.as_bytes()[..n]);
    let text = core::str::from_utf8(&buf[..n]).unwrap_or("");
    let _ = Text::with_baseline(text, Point::new(0, row * 10), style, Baseline::Top).draw(display);
}

fn psram_label(psram: Option<&Psram<'static>>) -> String<20> {
    let mut s = String::new();
    match psram {
        Some(p) if p.size() >= FRAME_BYTES => {
            let mb = p.size() / (1024 * 1024);
            let _ = write!(s, "psram {mb}MB ok");
        }
        Some(_) => {
            let _ = s.push_str("psram too small");
        }
        None => {
            let _ = s.push_str("psram FAIL");
        }
    }
    s
}

fn bat_line(mv: u32, pct: u16) -> String<20> {
    let mut s = String::new();
    if mv == 0 {
        let _ = s.push_str("bat --");
    } else {
        let _ = write!(s, "bat {mv}mV {pct}%");
    }
    s
}

fn lposc_line() -> String<21> {
    let (hz, slow_tenths, src) = crate::power::lposc_status();
    let mut s = String::new();
    let sign = if slow_tenths < 0 { '-' } else { '+' };
    let abs = slow_tenths.unsigned_abs();
    let _ = write!(s, "{src} {sign}{}.{}% {hz}", abs / 10, abs % 10);
    s
}
