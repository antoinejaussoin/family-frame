//! Pico LiPo 2 XL W: USB-provisioned Wi-Fi, GET /frame.bin, Inky Impression 13.3″.

#![no_std]
#![no_main]

mod battery;
mod board;
mod cli;
mod el133;
mod epd;
mod http;
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

#[unsafe(link_section = ".start_block")]
#[used]
static IMAGE_DEF: ImageDef = ImageDef::secure_exe();

#[unsafe(link_section = ".bi_entries")]
#[used]
static PICOTOOL_ENTRIES: [embassy_rp::binary_info::EntryAddr; 4] = [
    embassy_rp::binary_info::rp_program_name!(c"family-frame"),
    embassy_rp::binary_info::rp_cargo_version!(),
    embassy_rp::binary_info::rp_program_description!(
        c"Pico LiPo 2 XL W family frame: GET /frame.bin + Inky 13.3"
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

    let stack = wifi::start(
        spawner, p.PIN_23, p.PIN_25, p.PIN_24, p.PIN_29, p.PIO0, p.DMA_CH0,
    )
    .await;
    cli::start(spawner, p.USB, flash);

    let mut bat = battery::Battery::new(p.ADC, p.PIN_43);

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
            if cold && !painted_diag {
                el133::show_solid(&mut epd, YELLOW).await;
                painted_diag = true;
                cold = false;
            }
            Timer::after_secs(2).await;
            continue;
        }

        let waited = Instant::now();
        while !wifi::is_up() && waited.elapsed() < Duration::from_secs(35) {
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
        Timer::after_secs(u64::from(nap)).await;
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
) -> bool {
    let _ = bat.sample();
    let Some(frame) = frame else {
        if paint_diag {
            el133::show_solid(epd, RED).await;
        }
        return false;
    };

    if !wifi::is_up() {
        if paint_diag {
            el133::show_solid(epd, RED).await;
        }
        return false;
    }

    let (status, _got, etag) = http::get_frame(stack, frame).await;
    compiler_fence(Ordering::SeqCst);

    match status {
        FrameResult::NotModified => true,
        FrameResult::Ok => {
            el133::show_frame(epd, frame).await;
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
