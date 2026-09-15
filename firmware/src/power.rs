//! USB-host vs battery, and the onboard LEDs.
//!
//! Pico LiPo 2 XL W (PIM776):
//! - User LED: RM2 `WL_GPIO0`, software.
//! - White power LED: hardwired to 3V3; cut the rear LED trace to kill it.
//! - Red charge LED: MCP73831 `STAT`; on only while charging.
//!
//! embassy-rp forces USB VBUS-detect high, so `configured` stays true after
//! you unplug. Hosts send a SOF every 1 ms; that count freezing is the
//! disconnect signal.

use core::sync::atomic::{AtomicBool, Ordering};
use embassy_executor::Spawner;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embassy_time::Timer;

static USB_HOST: AtomicBool = AtomicBool::new(false);
static USB_HOST_CHANGED: Signal<CriticalSectionRawMutex, ()> = Signal::new();

pub fn on_usb() -> bool {
    USB_HOST.load(Ordering::Relaxed)
}

fn set_usb_host(on: bool) {
    if USB_HOST.swap(on, Ordering::Relaxed) != on {
        USB_HOST_CHANGED.signal(());
    }
}

pub async fn wait_usb_change() {
    USB_HOST_CHANGED.wait().await;
}

pub fn start(spawner: Spawner) {
    spawner.spawn(sof_watch_task().unwrap());
}

fn sof_count() -> u16 {
    // RP2350 USB.SOF_RD (datasheet 4.1.14). embassy-rp keeps `pac` crate-private.
    const USB_SOF_RD: *const u32 = 0x5011_0048 as *const u32;
    unsafe { core::ptr::read_volatile(USB_SOF_RD) as u16 & 0x07ff }
}

#[embassy_executor::task]
async fn sof_watch_task() {
    let mut last = sof_count();
    loop {
        Timer::after_millis(100).await;
        let now = sof_count();
        set_usb_host(now != last);
        last = now;
    }
}
