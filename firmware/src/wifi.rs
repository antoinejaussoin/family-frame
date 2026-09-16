//! CYW43 Wi-Fi, DHCP stack, and join loop.
//!
//! Same Embassy / PIO wiring as the laser-tag temperature and IR-capture nodes.

use core::fmt::Write as _;
use core::sync::atomic::{AtomicU8, AtomicU16, Ordering};
use cyw43::{JoinAuth, JoinError, JoinOptions, PowerManagementMode};
use cyw43_pio::{PioSpi, RM2_CLOCK_DIVIDER};
use embassy_executor::Spawner;
use embassy_futures::select::{Either, select};
use embassy_net::{Config, Stack, StackResources};
use embassy_rp::Peri;
use embassy_rp::clocks::RoscRng;
use embassy_rp::dma;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::peripherals::{DMA_CH0, PIN_23, PIN_24, PIN_25, PIN_29, PIO0};
use embassy_rp::pio::Pio;
use embassy_time::{Duration, Timer, with_timeout};
use static_cell::StaticCell;

use heapless::String;

use crate::board::Irqs;
use crate::settings;

static WIFI_STATUS: AtomicU8 = AtomicU8::new(WifiStatus::Setup as u8);
static FRAME_STATUS: AtomicU8 = AtomicU8::new(FrameStatus::None as u8);
/// Last IPv4 octet, or [`NO_IP`] when DHCP has not given us an address.
static LAST_OCTET: AtomicU16 = AtomicU16::new(NO_IP);
const NO_IP: u16 = 0xFFFF;

/// `join()` either errors quickly or hangs waiting for a handshake event.
const JOIN_TIMEOUT: Duration = Duration::from_secs(12);
/// cyw43 0.7 can return `Ok` from `join` before it reports link-up.
const LINK_TIMEOUT: Duration = Duration::from_secs(5);
const DHCP_TIMEOUT: Duration = Duration::from_secs(20);

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum WifiStatus {
    Setup = 0,
    Joining = 1,
    Up = 2,
    Fail = 3,
}

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FrameStatus {
    None = 0,
    Ok = 1,
    NotModified = 2,
    Fail = 3,
}

impl WifiStatus {
    fn store(self) {
        WIFI_STATUS.store(self as u8, Ordering::Relaxed);
    }

    fn load() -> Self {
        match WIFI_STATUS.load(Ordering::Relaxed) {
            1 => Self::Joining,
            2 => Self::Up,
            3 => Self::Fail,
            _ => Self::Setup,
        }
    }
}

impl FrameStatus {
    pub fn store(self) {
        FRAME_STATUS.store(self as u8, Ordering::Relaxed);
    }

    fn load() -> Self {
        match FRAME_STATUS.load(Ordering::Relaxed) {
            1 => Self::Ok,
            2 => Self::NotModified,
            3 => Self::Fail,
            _ => Self::None,
        }
    }
}

pub fn is_up() -> bool {
    WifiStatus::load() == WifiStatus::Up
}

pub fn status_line() -> String<21> {
    let mut s = String::new();
    match WifiStatus::load() {
        WifiStatus::Joining => {
            let _ = s.push_str("wifi: joining");
        }
        WifiStatus::Up => {
            let frame = match FrameStatus::load() {
                FrameStatus::Ok => "200",
                FrameStatus::NotModified => "204",
                FrameStatus::Fail => "fail",
                FrameStatus::None => "",
            };
            match (last_octet(), frame) {
                (Some(n), "") => {
                    let _ = write!(s, "wifi .{n}");
                }
                (Some(n), f) => {
                    let _ = write!(s, "wifi .{n} {f}");
                }
                (None, "") => {
                    let _ = s.push_str("wifi ok");
                }
                (None, f) => {
                    let _ = write!(s, "wifi {f}");
                }
            }
        }
        WifiStatus::Fail => {
            let _ = s.push_str("wifi fail");
        }
        WifiStatus::Setup => {
            let _ = s.push_str("USB: wifi/save");
        }
    }
    s
}

fn last_octet() -> Option<u8> {
    let v = LAST_OCTET.load(Ordering::Relaxed);
    (v <= 255).then_some(v as u8)
}

fn remember_ip(stack: Stack<'_>) {
    match stack.config_v4() {
        Some(cfg) => {
            LAST_OCTET.store(
                u16::from(cfg.address.address().octets()[3]),
                Ordering::Relaxed,
            );
        }
        None => forget_ip(),
    }
}

fn forget_ip() {
    LAST_OCTET.store(NO_IP, Ordering::Relaxed);
}

pub async fn start(
    spawner: Spawner,
    pin_pwr: Peri<'static, PIN_23>,
    pin_cs: Peri<'static, PIN_25>,
    pin_dio: Peri<'static, PIN_24>,
    pin_clk: Peri<'static, PIN_29>,
    pio0: Peri<'static, PIO0>,
    dma_ch0: Peri<'static, DMA_CH0>,
) -> Stack<'static> {
    let fw = cyw43::aligned_bytes!("../cyw43-firmware/43439A0.bin");
    let clm = cyw43::aligned_bytes!("../cyw43-firmware/43439A0_clm.bin");
    let nvram = cyw43::aligned_bytes!("../cyw43-firmware/nvram_rp2040.bin");

    let pwr = Output::new(pin_pwr, Level::Low);
    let cs = Output::new(pin_cs, Level::High);
    let mut pio = Pio::new(pio0, Irqs);
    let spi = PioSpi::new(
        &mut pio.common,
        pio.sm0,
        RM2_CLOCK_DIVIDER,
        pio.irq0,
        cs,
        pin_dio,
        pin_clk,
        dma::Channel::new(dma_ch0, Irqs),
    );

    static STATE: StaticCell<cyw43::State> = StaticCell::new();
    let (net_device, mut control, runner) =
        cyw43::new(STATE.init(cyw43::State::new()), pwr, spi, fw, nvram).await;
    spawner.spawn(cyw43_task(runner).unwrap());
    control.init(clm.as_ref()).await;
    control.gpio_set(0, false).await;
    // PowerSave during associate drops handshake events on this chip.
    control
        .set_power_management(PowerManagementMode::None)
        .await;

    let mut rng = RoscRng;
    static RESOURCES: StaticCell<StackResources<8>> = StaticCell::new();
    let (stack, net_runner) = embassy_net::new(
        net_device,
        Config::dhcpv4(Default::default()),
        RESOURCES.init(StackResources::new()),
        rng.next_u64(),
    );
    spawner.spawn(net_task(net_runner).unwrap());
    spawner.spawn(wifi_task(control, stack).unwrap());
    stack
}

#[embassy_executor::task]
async fn cyw43_task(
    runner: cyw43::Runner<'static, cyw43::SpiBus<Output<'static>, PioSpi<'static, PIO0, 0>>>,
) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static, cyw43::NetDriver<'static>>) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn wifi_task(mut control: cyw43::Control<'static>, stack: Stack<'static>) -> ! {
    let mut backoff_s = 1u64;
    loop {
        apply_user_led(&mut control).await;
        let cfg = settings::snapshot().await;
        if cfg.ssid.is_empty() {
            forget_ip();
            WifiStatus::Setup.store();
            wait_while_updating_led(&mut control, settings::wait_rejoin()).await;
            backoff_s = 1;
            continue;
        }

        WifiStatus::Joining.store();
        FrameStatus::None.store();

        if associate(&mut control, stack, &cfg).await {
            remember_ip(stack);
            backoff_s = 1;
            WifiStatus::Up.store();
            control
                .set_power_management(PowerManagementMode::PowerSave)
                .await;
            wait_while_updating_led(&mut control, async {
                match select(settings::wait_rejoin(), stack.wait_link_down()).await {
                    Either::First(()) | Either::Second(()) => {}
                }
            })
            .await;
            forget_ip();
            WifiStatus::Joining.store();
            continue;
        }

        forget_ip();

        wait_while_updating_led(&mut control, async {
            match select(settings::wait_rejoin(), Timer::after_secs(backoff_s)).await {
                Either::First(()) | Either::Second(()) => {}
            }
        })
        .await;
        backoff_s = (backoff_s.saturating_mul(2)).min(8);
    }
}

/// User LED on only while a USB host is sending SOFs. Off on battery.
async fn apply_user_led(control: &mut cyw43::Control<'_>) {
    control.gpio_set(0, crate::power::on_usb()).await;
}

async fn wait_while_updating_led<F, T>(control: &mut cyw43::Control<'_>, fut: F) -> T
where
    F: core::future::Future<Output = T>,
{
    let mut fut = core::pin::pin!(fut);
    loop {
        apply_user_led(control).await;
        match select(fut.as_mut(), crate::power::wait_usb_change()).await {
            Either::First(v) => return v,
            Either::Second(()) => {}
        }
    }
}

/// Associate, wait until the driver reports link-up, then DHCP.
///
/// Default `JoinOptions` use WPA2+WPA3/SAE. On a WPA2 AP that handshake
/// often fails (or `join` returns `Ok` while link stays down). WPA2-only
/// first, WPA2+WPA3 only if the AP rejects WPA2. Power management stays
/// off until DHCP succeeds so assoc/4-way events are not missed.
async fn associate(
    control: &mut cyw43::Control<'static>,
    stack: Stack<'static>,
    cfg: &settings::NetConfig,
) -> bool {
    control
        .set_power_management(PowerManagementMode::None)
        .await;
    control.leave().await;
    let _ = with_timeout(Duration::from_millis(500), stack.wait_link_down()).await;
    Timer::after_millis(250).await;

    if !join_ssid(control, cfg).await {
        control.leave().await;
        return false;
    }

    if with_timeout(LINK_TIMEOUT, stack.wait_link_up())
        .await
        .is_err()
    {
        control.leave().await;
        return false;
    }

    if with_timeout(DHCP_TIMEOUT, stack.wait_config_up())
        .await
        .is_err()
    {
        control.leave().await;
        false
    } else {
        true
    }
}

async fn join_ssid(control: &mut cyw43::Control<'static>, cfg: &settings::NetConfig) -> bool {
    let ssid = cfg.ssid.as_str();
    if cfg.psk.is_empty() {
        return join_once(control, ssid, JoinOptions::new_open()).await;
    }

    let mut wpa2 = JoinOptions::new(cfg.psk.as_bytes());
    wpa2.auth = JoinAuth::Wpa2;
    match with_timeout(JOIN_TIMEOUT, control.join(ssid, wpa2)).await {
        Ok(Ok(())) => true,
        Ok(Err(JoinError::AuthenticationFailure)) => {
            control.leave().await;
            Timer::after_millis(250).await;
            let mut mixed = JoinOptions::new(cfg.psk.as_bytes());
            mixed.auth = JoinAuth::Wpa2Wpa3;
            join_once(control, ssid, mixed).await
        }
        _ => false,
    }
}

async fn join_once(
    control: &mut cyw43::Control<'static>,
    ssid: &str,
    options: JoinOptions<'_>,
) -> bool {
    matches!(
        with_timeout(JOIN_TIMEOUT, control.join(ssid, options)).await,
        Ok(Ok(()))
    )
}
