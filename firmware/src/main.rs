//! Pico LiPo 2 XL W: USB-provisioned Wi-Fi, GET /frame.bin, Inky Impression 13.3″.

#![no_std]
#![no_main]

mod battery;
mod board;
mod cli;
mod el133;
mod epd;
mod http;
#[cfg(feature = "oled-debug")]
mod oled;
mod settings;
mod wifi;

use core::sync::atomic::{Ordering, compiler_fence};
use embassy_executor::Spawner;
use embassy_rp::block::ImageDef;
use embassy_rp::flash::Flash;
use embassy_rp::gpio::{Input, Level, Output, Pull};
use embassy_rp::psram::{Config as PsramConfig, Psram, VerificationType};
use embassy_rp::qmi_cs1::QmiCs1;
use embassy_rp::spi::{Config as SpiConfig, Spi};
use embassy_sync::mutex::Mutex;
use embassy_time::{Duration, Instant, Timer};
use panic_halt as _;
use static_cell::StaticCell;

use crate::el133::{BLUE, FRAME_BYTES, RED, YELLOW};
use crate::epd::Epd;
use crate::http::FrameResult;
use crate::settings::{ConfigFlash, SharedFlash};

const FAIL_SLEEP_S: u32 = 120;
const AWAKE_POLL_S: u32 = 60;
/// Long enough for a missed handshake plus one retry (join + link + DHCP).
const WIFI_WAIT_S: u64 = 90;

#[unsafe(link_section = ".start_block")]
#[used]
static IMAGE_DEF: ImageDef = ImageDef::secure_exe();

#[unsafe(link_section = ".bi_entries")]
#[used]
static PICOTOOL_ENTRIES: [embassy_rp::binary_info::EntryAddr; 4] = [
    #[cfg(not(feature = "oled-debug"))]
    embassy_rp::binary_info::rp_program_name!(c"family-frame"),
    #[cfg(feature = "oled-debug")]
    embassy_rp::binary_info::rp_program_name!(c"family-frame-oled"),
    embassy_rp::binary_info::rp_cargo_version!(),
    #[cfg(not(feature = "oled-debug"))]
    embassy_rp::binary_info::rp_program_description!(
        c"Pico LiPo 2 XL W family frame: GET /frame.bin + Inky 13.3"
    ),
    #[cfg(feature = "oled-debug")]
    embassy_rp::binary_info::rp_program_description!(
        c"family-frame OLED debug: same client + SSD1306/SH1106 status"
    ),
    embassy_rp::binary_info::rp_program_build_attribute!(),
];

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    let flash: ConfigFlash = Flash::new_blocking(p.FLASH);
    static FLASH: StaticCell<SharedFlash> = StaticCell::new();
    let flash = FLASH.init(Mutex::new(flash));
    settings::replace(settings::load_flash(&mut *flash.lock().await)).await;

    let _psram = init_psram(p.QMI_CS1, p.PIN_47);
    let frame_ptr = psram_ptr(_psram.as_ref());

    let mut bat = battery::Battery::new(p.ADC, p.PIN_43);
    #[cfg(feature = "oled-debug")]
    {
        bat = bat.with_chip_temp(p.ADC_TEMP_SENSOR);
    }
    let mut ui = DebugUi::new(
        #[cfg(feature = "oled-debug")]
        p.I2C1,
        #[cfg(feature = "oled-debug")]
        p.PIN_19,
        #[cfg(feature = "oled-debug")]
        p.PIN_18,
        #[cfg(feature = "oled-debug")]
        _psram.as_ref(),
    );
    let _ = bat.sample();
    ui.paint(&mut bat, "booting radio");

    let stack = wifi::start(
        spawner, p.PIN_23, p.PIN_25, p.PIN_24, p.PIN_29, p.PIO0, p.DMA_CH0,
    )
    .await;
    cli::start(spawner, p.USB, flash);
    ui.paint(&mut bat, "radio up");

    let mut spi_cfg = SpiConfig::default();
    spi_cfg.frequency = 4_000_000;
    let spi = Spi::new_blocking_txonly(p.SPI1, p.PIN_10, p.PIN_11, spi_cfg);
    let mut epd = Epd::new(
        spi,
        Output::new(p.PIN_22, Level::Low),
        Output::new(p.PIN_27, Level::High),
        Input::new(p.PIN_17, Pull::Up),
        Output::new(p.PIN_26, Level::High),
        Output::new(p.PIN_16, Level::High),
    );

    let mut cold = true;
    let mut painted_diag = false;
    let mut fails: u32 = 0;

    loop {
        let cfg = settings::snapshot().await;
        if !cfg.is_ready() {
            let _ = bat.sample();
            ui.paint(&mut bat, "need wifi/save");
            if cold && !painted_diag {
                el133::show_solid(&mut epd, YELLOW).await;
                painted_diag = true;
                cold = false;
            }
            Timer::after_secs(2).await;
            continue;
        }

        let waited = Instant::now();
        while !wifi::is_up() && waited.elapsed() < Duration::from_secs(WIFI_WAIT_S) {
            ui.paint(&mut bat, "joining wifi");
            Timer::after_millis(200).await;
        }

        let frame =
            frame_ptr.map(|ptr| unsafe { core::slice::from_raw_parts_mut(ptr, FRAME_BYTES) });
        let reached = run_cycle(
            stack,
            &mut epd,
            &mut bat,
            frame,
            cold && !painted_diag,
            flash,
            &mut ui,
        )
        .await;
        cold = false;
        if reached {
            fails = 0;
        } else {
            fails = fails.saturating_add(1);
        }

        let cfg = settings::snapshot().await;
        let nap = if cfg.sleep_s == 0 {
            AWAKE_POLL_S
        } else if fails > 0 && cfg.sleep_s > FAIL_SLEEP_S {
            FAIL_SLEEP_S
        } else {
            cfg.sleep_s
        };
        nap_with_ui(&mut ui, &mut bat, nap).await;
    }
}

fn init_psram(
    qmi_cs1: embassy_rp::Peri<'static, embassy_rp::peripherals::QMI_CS1>,
    pin_47: embassy_rp::Peri<'static, embassy_rp::peripherals::PIN_47>,
) -> Option<Psram<'static>> {
    let qmi = QmiCs1::new(qmi_cs1, pin_47);
    let mut cfg = PsramConfig::aps6404l();
    cfg.clock_hz = embassy_rp::clocks::clk_sys_freq();
    cfg.verification_type = VerificationType::Aps6404l;
    Psram::new(qmi, cfg).ok()
}

fn psram_ptr(psram: Option<&Psram<'static>>) -> Option<*mut u8> {
    let psram = psram?;
    if psram.size() < FRAME_BYTES {
        return None;
    }
    Some(psram.base_address())
}

async fn run_cycle(
    stack: embassy_net::Stack<'static>,
    epd: &mut Epd,
    bat: &mut battery::Battery<'_>,
    frame: Option<&'static mut [u8]>,
    paint_diag: bool,
    flash: &'static SharedFlash,
    ui: &mut DebugUi,
) -> bool {
    let _ = bat.sample();
    let Some(frame) = frame else {
        ui.paint(bat, "no PSRAM");
        if paint_diag {
            el133::show_solid(epd, RED).await;
        }
        return false;
    };

    if !wifi::is_up() {
        ui.paint(bat, "wifi fail");
        if paint_diag {
            el133::show_solid(epd, RED).await;
        }
        return false;
    }

    ui.paint(bat, "GET /frame.bin");
    let (status, _got, etag) = http::get_frame(stack, frame).await;
    compiler_fence(Ordering::SeqCst);

    match status {
        FrameResult::NotModified => {
            ui.paint(bat, "frame 304 skip");
            true
        }
        FrameResult::Ok => {
            ui.paint(bat, "frame 200 eink");
            el133::show_frame(epd, frame).await;
            ui.paint(bat, "frame 200 done");
            if !etag.is_empty() {
                let cfg = settings::snapshot().await;
                if !eq_ignore_ascii_case(etag.as_str(), cfg.last_checksum.as_str()) {
                    settings::update(|c| {
                        c.last_checksum.clear();
                        let _ = c.last_checksum.push_str(etag.as_str());
                    })
                    .await;
                    let cfg = settings::snapshot().await;
                    let mut flash = flash.lock().await;
                    let _ = settings::save_flash(&mut flash, &cfg);
                }
            }
            true
        }
        FrameResult::Err => {
            ui.paint(bat, "frame GET fail");
            let cfg = settings::snapshot().await;
            if paint_diag && cfg.last_checksum.is_empty() {
                el133::show_solid(epd, BLUE).await;
            }
            false
        }
    }
}

fn eq_ignore_ascii_case(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

struct DebugUi {
    #[cfg(feature = "oled-debug")]
    oled: oled::DebugOled,
}

impl DebugUi {
    fn new(
        #[cfg(feature = "oled-debug")] i2c1: embassy_rp::Peri<'static, embassy_rp::peripherals::I2C1>,
        #[cfg(feature = "oled-debug")] scl: embassy_rp::Peri<'static, embassy_rp::peripherals::PIN_19>,
        #[cfg(feature = "oled-debug")] sda: embassy_rp::Peri<'static, embassy_rp::peripherals::PIN_18>,
        #[cfg(feature = "oled-debug")] psram: Option<&embassy_rp::psram::Psram<'static>>,
    ) -> Self {
        #[cfg(feature = "oled-debug")]
        {
            let mut i2c_config = embassy_rp::i2c::Config::default();
            i2c_config.frequency = 100_000;
            let i2c = embassy_rp::i2c::I2c::new_blocking(i2c1, scl, sda, i2c_config);
            Self {
                oled: oled::DebugOled::start(i2c, psram),
            }
        }
        #[cfg(not(feature = "oled-debug"))]
        Self {}
    }

    fn paint(&mut self, bat: &mut battery::Battery<'_>, extra: &str) {
        #[cfg(feature = "oled-debug")]
        self.oled.paint(bat, extra);
        #[cfg(not(feature = "oled-debug"))]
        let _ = (bat, extra);
    }
}

async fn nap_with_ui(ui: &mut DebugUi, bat: &mut battery::Battery<'_>, nap: u32) {
    #[cfg(feature = "oled-debug")]
    {
        use core::fmt::Write as _;
        let mut left = nap;
        while left > 0 {
            let mut extra = heapless::String::<20>::new();
            let _ = write!(extra, "nap {left}s");
            let _ = bat.sample();
            ui.paint(bat, extra.as_str());
            let chunk = left.min(2);
            Timer::after_secs(u64::from(chunk)).await;
            left -= chunk;
        }
    }
    #[cfg(not(feature = "oled-debug"))]
    {
        let _ = ui;
        let _ = bat;
        Timer::after_secs(u64::from(nap)).await;
    }
}
