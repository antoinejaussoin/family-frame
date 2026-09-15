//! USB-host vs battery, onboard LEDs, and POWMAN sleep between polls.
//!
//! Pico LiPo 2 XL W (PIM776):
//! - User LED: RM2 `WL_GPIO0`, software.
//! - White power LED: hardwired to 3V3; cut the rear LED trace to kill it.
//! - Red charge LED: MCP73831 `STAT`; on only while charging.
//!
//! embassy-rp forces USB VBUS-detect high, so `configured` stays true after
//! you unplug. Hosts send a SOF every 1 ms; that count freezing is the
//! disconnect signal.
//!
//! Between `/frame.bin` polls the switched-core is powered down (AON LPOSC
//! alarm wake). That resets the CPUs, so `main` runs again. cyw43 cannot be
//! restarted in-place; reboot is the clean way to kill the radio.

use core::sync::atomic::{AtomicBool, Ordering, compiler_fence};
use embassy_executor::Spawner;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embassy_time::Timer;

static USB_HOST: AtomicBool = AtomicBool::new(false);
static USB_HOST_CHANGED: Signal<CriticalSectionRawMutex, ()> = Signal::new();
static WOKE_FROM_SLEEP: AtomicBool = AtomicBool::new(false);

const POWMAN: u32 = 0x4010_0000;
const POWMAN_PASSWORD: u32 = 0x5AFE << 16;
const POWMAN_CLR: u32 = 0x3000;

const OFF_SEQ_CFG: u32 = 0x34;
const OFF_STATE: u32 = 0x38;
const OFF_EXT_CTRL0: u32 = 0x44;
const OFF_LPOSC_FREQ_INT: u32 = 0x50;
const OFF_LPOSC_FREQ_FRAC: u32 = 0x54;
const OFF_SET_TIME_15: u32 = 0x6c;
const OFF_READ_TIME_UPPER: u32 = 0x70;
const OFF_READ_TIME_LOWER: u32 = 0x74;
const OFF_ALARM_15: u32 = 0x84;
const OFF_TIMER: u32 = 0x88;
const OFF_DBG_PWRCFG: u32 = 0xa4;
const OFF_SCRATCH0: u32 = 0xb0;
const OFF_CHIP_RESET: u32 = 0x2c;

const TIMER_RUN: u32 = 1 << 1;
const TIMER_ALARM_ENAB: u32 = 1 << 4;
const TIMER_PWRUP_ON_ALARM: u32 = 1 << 5;
const TIMER_ALARM: u32 = 1 << 6;
const TIMER_USE_LPOSC: u32 = 1 << 8;
const TIMER_USING_LPOSC: u32 = 1 << 17;

const STATE_REQ_ALL_DOWN: u32 = 0x0f << 4;
const STATE_REQ_IGNORED: u32 = 1 << 8;
const STATE_BAD_SW_REQ: u32 = 1 << 10;
const STATE_WAITING: u32 = 1 << 12;

const SEQ_USE_VREG_LP: u32 = 1 << 4;
const SEQ_USE_VREG_HP: u32 = 1 << 5;
const SEQ_USE_BOD_LP: u32 = 1 << 6;
const SEQ_USE_BOD_HP: u32 = 1 << 7;
const SEQ_RUN_LPOSC_IN_LP: u32 = 1 << 8;

/// RM2 `WL_REG_ON` (GP23). POWMAN holds it low in the low-power state.
const WL_REG_ON_GPIO: u32 = 23;
const EXT_INIT: u32 = 1 << 8;
const EXT_GPIO_DISABLE: u32 = 31;

/// Low 16 bits of scratch[0] while we are in a planned POWMAN nap.
const SLEEP_MAGIC: u32 = 0xF4AE;

const LPOSC_KHZ_INT: u32 = 32;
/// 0.768 × 65536 for a 32.768 kHz LPOSC.
const LPOSC_KHZ_FRAC: u32 = 50_332;

pub fn on_usb() -> bool {
    USB_HOST.load(Ordering::Relaxed)
}

/// True when this boot is a wake from [`sleep_secs`] (skip cold e-ink diags).
pub fn woke_from_sleep() -> bool {
    WOKE_FROM_SLEEP.load(Ordering::Relaxed)
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
    note_wake_reason();
    spawner.spawn(sof_watch_task().unwrap());
}

/// Drop the POWMAN hold on GP23 so `wifi::start` can own `WL_REG_ON`.
pub fn release_radio_hold() {
    powman_write(OFF_EXT_CTRL0, EXT_GPIO_DISABLE);
}

/// OLED off, radio off, switched-core powered down until `secs` have passed.
///
/// Wake is a full `main` restart. Returns only if POWMAN refused the request
/// (then we busy-wait with Embassy so the poll loop still runs).
pub async fn sleep_secs(secs: u32) {
    let secs = secs.max(1);
    powman_write(OFF_SCRATCH0, SLEEP_MAGIC);
    if arm_lposc_alarm_ms(u64::from(secs) * 1000) && request_swcore_down() {
        // Radio comes down only after POWMAN has accepted the request, so a
        // refused sleep still has a working cyw43 for the Embassy fallback.
        hold_radio_off();
        Timer::after_millis(30).await;
        halt_for_powerdown();
    }
    powman_write(OFF_SCRATCH0, 0);
    Timer::after_secs(u64::from(secs)).await;
}

fn note_wake_reason() {
    let scratch = powman_read(OFF_SCRATCH0) & 0xffff;
    powman_write(OFF_SCRATCH0, 0);
    let slept = scratch == SLEEP_MAGIC || had_swcore_pd();
    WOKE_FROM_SLEEP.store(slept, Ordering::Relaxed);
}

fn had_swcore_pd() -> bool {
    powman_read(OFF_CHIP_RESET) & (1 << 25) != 0
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

fn arm_lposc_alarm_ms(delay_ms: u64) -> bool {
    // Keep LPOSC + LP VREG when the switched-core drops.
    let seq = powman_read(OFF_SEQ_CFG)
        | SEQ_USE_VREG_LP
        | SEQ_USE_VREG_HP
        | SEQ_USE_BOD_LP
        | SEQ_USE_BOD_HP
        | SEQ_RUN_LPOSC_IN_LP;
    powman_write(OFF_SEQ_CFG, seq);
    // Debugger pwrupreq otherwise blocks sleep after a probe session.
    powman_write(OFF_DBG_PWRCFG, 1);

    let mut timer = powman_read(OFF_TIMER) & 0xffff;
    let running = timer & TIMER_RUN != 0;
    if running {
        timer &= !TIMER_RUN;
        powman_write(OFF_TIMER, timer);
    }
    powman_write(OFF_LPOSC_FREQ_INT, LPOSC_KHZ_INT);
    powman_write(OFF_LPOSC_FREQ_FRAC, LPOSC_KHZ_FRAC);
    if !running {
        write_u64_words(OFF_SET_TIME_15, 0);
    }
    timer = (timer | TIMER_USE_LPOSC | TIMER_RUN) & !TIMER_ALARM_ENAB & !TIMER_ALARM;
    powman_write(OFF_TIMER, timer);
    if !spin_until(|| powman_read(OFF_TIMER) & TIMER_USING_LPOSC != 0, 200_000) {
        return false;
    }

    timer = powman_read(OFF_TIMER) & 0xffff;
    powman_write(OFF_TIMER, timer & !TIMER_ALARM_ENAB);
    let alarm = now_ms().saturating_add(delay_ms.max(1));
    write_u64_words(OFF_ALARM_15, alarm);
    timer = powman_read(OFF_TIMER) & 0xffff;
    powman_write(OFF_TIMER, timer | TIMER_ALARM);
    timer = powman_read(OFF_TIMER) & 0xffff;
    powman_write(
        OFF_TIMER,
        (timer | TIMER_ALARM_ENAB | TIMER_PWRUP_ON_ALARM | TIMER_USE_LPOSC | TIMER_RUN)
            & !TIMER_ALARM,
    );
    true
}

fn hold_radio_off() {
    // init=1, init_state=0, lp_entry=0, lp_exit=0, gpio 23.
    powman_write(OFF_EXT_CTRL0, WL_REG_ON_GPIO | EXT_INIT);
}

fn request_swcore_down() -> bool {
    powman_clr(OFF_STATE, STATE_REQ_IGNORED);
    powman_write(OFF_STATE, STATE_REQ_ALL_DOWN);
    let state = powman_read(OFF_STATE);
    if state & (STATE_REQ_IGNORED | STATE_BAD_SW_REQ) != 0 {
        return false;
    }
    spin_until(|| powman_read(OFF_STATE) & STATE_WAITING != 0, 400_000)
}

fn halt_for_powerdown() -> ! {
    silence_irqs();
    compiler_fence(Ordering::SeqCst);
    loop {
        cortex_m::asm::dsb();
        cortex_m::asm::wfi();
    }
}

fn silence_irqs() {
    // Stop sources that would bounce WFI before POWMAN sees a halted CPU.
    unsafe {
        core::ptr::write_volatile(0x400b_0040 as *mut u32, 0);
        core::ptr::write_volatile(0x400b_0044 as *mut u32, 0);
        core::ptr::write_volatile(0x400b_003c as *mut u32, 0x0f);
        core::ptr::write_volatile(0x5011_0090 as *mut u32, 0);
        core::ptr::write_volatile(0x5011_0040 as *mut u32, 0);
        // RESETS.RESET SET: ADC, DMA, I2C1, PIO0, SPI1, USBCTRL.
        const RESETS_SET: *mut u32 = 0x4002_2000 as *mut u32;
        const HOLD: u32 = (1 << 0) | (1 << 2) | (1 << 5) | (1 << 11) | (1 << 19) | (1 << 28);
        core::ptr::write_volatile(RESETS_SET, HOLD);

        let icer = 0xE000_E180 as *mut u32;
        let icpr = 0xE000_E280 as *mut u32;
        for i in 0..3 {
            icer.add(i).write_volatile(0xFFFF_FFFF);
            icpr.add(i).write_volatile(0xFFFF_FFFF);
        }
        // SysTick CSR disable (embassy uses TIMER0, but don't leave this armed).
        core::ptr::write_volatile(0xE000_E010 as *mut u32, 0);
        cortex_m::interrupt::disable();
    }
}

fn now_ms() -> u64 {
    loop {
        let upper1 = powman_read(OFF_READ_TIME_UPPER);
        let lower = powman_read(OFF_READ_TIME_LOWER);
        let upper2 = powman_read(OFF_READ_TIME_UPPER);
        if upper1 == upper2 {
            return ((upper1 as u64) << 32) | u64::from(lower);
        }
    }
}

fn write_u64_words(off_15: u32, value: u64) {
    powman_write(off_15, (value & 0xffff) as u32);
    powman_write(off_15 - 4, ((value >> 16) & 0xffff) as u32);
    powman_write(off_15 - 8, ((value >> 32) & 0xffff) as u32);
    powman_write(off_15 - 12, ((value >> 48) & 0xffff) as u32);
}

fn powman_read(off: u32) -> u32 {
    unsafe { core::ptr::read_volatile((POWMAN + off) as *const u32) }
}

fn powman_write(off: u32, value: u32) {
    unsafe {
        core::ptr::write_volatile(
            (POWMAN + off) as *mut u32,
            POWMAN_PASSWORD | (value & 0xffff),
        );
    }
}

fn powman_clr(off: u32, bits: u32) {
    unsafe {
        core::ptr::write_volatile(
            (POWMAN + POWMAN_CLR + off) as *mut u32,
            POWMAN_PASSWORD | (bits & 0xffff),
        );
    }
}

fn spin_until(mut pred: impl FnMut() -> bool, tries: u32) -> bool {
    for _ in 0..tries {
        if pred() {
            return true;
        }
        cortex_m::asm::nop();
    }
    false
}
