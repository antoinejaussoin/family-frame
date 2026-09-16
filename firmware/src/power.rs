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
//! alarm wake) unless a USB host is sending SOFs. POWMAN resets the CPUs, so
//! `main` runs again. cyw43 cannot be restarted in-place; reboot is the
//! clean way to kill the radio. A USB host keeps the core up so CDC serial
//! stays connected.
//!
//! Inky Impression A/B (GP5 / GP6, active-low) are POWMAN GPIO pwrup sources,
//! so a press wakes the chip from switched-core sleep and forces a poll. C/D
//! are Pi GPIO 25/24 — those pins are the RM2 on this board, so they cannot
//! be wake buttons.
//!
//! The RM2 `WL_REG_ON` pin is forced low *before* POWMAN runs. cyw43 still
//! owns that GPIO as an output; without the override it keeps the radio
//! powered (~50 mA) through the “sleep” hour.

use core::sync::atomic::{AtomicBool, Ordering, compiler_fence};
use embassy_executor::Spawner;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Instant, Timer};

static USB_HOST: AtomicBool = AtomicBool::new(false);
static USB_HOST_CHANGED: Signal<CriticalSectionRawMutex, ()> = Signal::new();
static WOKE_FROM_SLEEP: AtomicBool = AtomicBool::new(false);
static BUTTON_WAKE: AtomicBool = AtomicBool::new(false);

const POWMAN: u32 = 0x4010_0000;
const POWMAN_PASSWORD: u32 = 0x5AFE << 16;
const POWMAN_SET: u32 = 0x2000;
const POWMAN_CLR: u32 = 0x3000;

const OFF_VREG_CTRL: u32 = 0x04;
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
const OFF_PWRUP0: u32 = 0x8c;
const OFF_DBG_PWRCFG: u32 = 0xa4;
const OFF_SCRATCH0: u32 = 0xb0;
const OFF_CHIP_RESET: u32 = 0x2c;
const OFF_BOOT0: u32 = 0xd0;
const OFF_INTE: u32 = 0xe4;

const VREG_CTRL_UNLOCK: u32 = 1 << 13;

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
const SEQ_HW_PWRUP_SRAM: u32 = 0x3;

const PWRUP_ENABLE: u32 = 1 << 6;
const PWRUP_MODE_EDGE: u32 = 1 << 8;
const PWRUP_STATUS: u32 = 1 << 9;
const INTE_TIMER: u32 = 1 << 1;

/// Inky Impression 13.3 A / B (Pi BCM 5 / 6 through the Pico-to-Pi HAT).
const BUTTON_A_GPIO: u32 = 5;
const BUTTON_B_GPIO: u32 = 6;

/// RM2 `WL_REG_ON` (GP23). POWMAN holds it low in the low-power state.
const WL_REG_ON_GPIO: u32 = 23;
const CYW_CS_GPIO: u32 = 25;
const PSRAM_CS_GPIO: u32 = 47;
const EXT_INIT: u32 = 1 << 8;
const EXT_GPIO_DISABLE: u32 = 31;

const IO_BANK0: u32 = 0x4002_8000;
const PADS_BANK0: u32 = 0x4003_8000;
const RESETS_SET: u32 = 0x4002_2000;
const SCB_AIRCR: u32 = 0xE000_ED0C;
const NVIC_ISER: u32 = 0xE000_E100;

/// GPIO_CTRL (RP2350): OEOVER bits 15:14, OUTOVER bits 13:12, FUNCSEL=SIO.
const GPIO_OEOVER_ENABLE: u32 = 3 << 14;
const GPIO_OUTOVER_LOW: u32 = 2 << 12;
const GPIO_OUTOVER_HIGH: u32 = 3 << 12;
const GPIO_FUNCSEL_SIO: u32 = 5;
/// PADS: IE + PUE + Schmitt. ISO=0 so POWMAN still sees the pin while asleep.
const PAD_INPUT_PULLUP: u32 = (1 << 6) | (1 << 3) | (1 << 1);
const SIO_GPIO_IN: *const u32 = 0xd000_0004 as *const u32;

/// RESETS bits (RP2350): ADC, DMA, I2C1, PIO0, SPI1, TIMER0, TIMER1, USBCTRL.
const RESET_HOLD: u32 =
    (1 << 0) | (1 << 2) | (1 << 5) | (1 << 11) | (1 << 19) | (1 << 23) | (1 << 24) | (1 << 28);

const POWMAN_TIMER_IRQ: u32 = 45;

/// Low 16 bits of scratch[0] while we are in a planned POWMAN nap.
const SLEEP_MAGIC: u32 = 0xF4AE;

const LPOSC_KHZ_INT: u32 = 32;
/// 0.768 × 65536 for a 32.768 kHz LPOSC.
const LPOSC_KHZ_FRAC: u32 = 50_332;

pub fn on_usb() -> bool {
    USB_HOST.load(Ordering::Relaxed)
}

/// [`on_usb`], after the SOF debounce if the last sample was unplugged.
///
/// A cable seated during the fetch is then visible before POWMAN sleep.
pub async fn plugged_usb() -> bool {
    if !on_usb() {
        Timer::after_millis(400).await;
    }
    on_usb()
}

/// True when this boot is a wake from [`sleep_duration`] (skip cold e-ink diags).
pub fn woke_from_sleep() -> bool {
    WOKE_FROM_SLEEP.load(Ordering::Relaxed)
}

/// Current wake reason without consuming a pending button press.
pub fn wake_label() -> &'static str {
    if button_wake_pending() {
        "button"
    } else if woke_from_sleep() {
        "timer"
    } else {
        "cold"
    }
}

/// Like [`wake_label`], but a button flag is consumed so a USB stay-awake
/// loop does not keep reporting `button` after the press.
pub fn take_wake_label() -> &'static str {
    if BUTTON_WAKE.swap(false, Ordering::Relaxed) {
        "button"
    } else if woke_from_sleep() {
        "timer"
    } else {
        "cold"
    }
}

/// Put `button` back if the poll that consumed it never reached the server.
pub fn restore_button_wake(label: &str) {
    if label == "button" {
        request_button_wake();
    }
}

fn request_button_wake() {
    BUTTON_WAKE.store(true, Ordering::Relaxed);
}

fn button_wake_pending() -> bool {
    BUTTON_WAKE.load(Ordering::Relaxed)
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
    retain_button_pads();
    spawner.spawn(sof_watch_task().unwrap());
}

/// Drop the POWMAN hold on GP23 so `wifi::start` can own `WL_REG_ON`.
pub fn release_radio_hold() {
    powman_write(OFF_EXT_CTRL0, EXT_GPIO_DISABLE);
}

/// Wait out `secs` while a USB host is present.
///
/// Returns remaining time to POWMAN-sleep. Zero means a host stayed until
/// the deadline, or Inky A/B was pressed (caller should poll without sleeping).
pub async fn wait_usb_deadline(secs: u32) -> Duration {
    let secs = secs.max(1);
    let deadline = Instant::now() + Duration::from_secs(u64::from(secs));
    wait_while_usb(deadline).await;
    if button_wake_pending() || on_usb() {
        Duration::from_ticks(0)
    } else {
        deadline.saturating_duration_since(Instant::now())
    }
}

/// Power the switched-core down until `remaining` elapses. Never returns
/// when `remaining` is non-zero.
///
/// On battery this never returns: POWMAN reboot, or an AON-timer reset if
/// POWMAN refused. The radio is forced off first either way.
pub async fn sleep_duration(remaining: Duration) {
    if remaining.as_ticks() == 0 {
        return;
    }

    isolate_radio_and_psram();
    quiesce_usb();
    // RM2 internal regulators take a few ms to drop after WL_REG_ON goes low.
    Timer::after_millis(20).await;

    powman_write(OFF_SCRATCH0, SLEEP_MAGIC);
    unlock_vreg();
    disable_gpio_pwrups();
    arm_button_wakeups();
    clear_boot_vectors();
    let armed = arm_lposc_alarm_ms(remaining.as_millis().max(1));
    let waiting = armed && request_swcore_down();
    halt_for_sleep(waiting);
}

/// Stay running while a USB host is present. Poll SOF so an unplug can
/// still drop into POWMAN for the rest of the interval. Inky A/B abort the
/// wait so a plugged-in frame can still fetch on demand.
async fn wait_while_usb(deadline: Instant) {
    const SLICE: Duration = Duration::from_millis(50);
    loop {
        if buttons_pressed().await {
            request_button_wake();
            return;
        }
        if !on_usb() {
            return;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.as_ticks() == 0 {
            return;
        }
        Timer::after(remaining.min(SLICE)).await;
    }
}

fn note_wake_reason() {
    let scratch = powman_read(OFF_SCRATCH0) & 0xffff;
    powman_write(OFF_SCRATCH0, 0);
    let button = gpio_pwrup_latched();
    disable_gpio_pwrups();
    let slept = scratch == SLEEP_MAGIC || had_swcore_pd();
    WOKE_FROM_SLEEP.store(slept, Ordering::Relaxed);
    BUTTON_WAKE.store(button, Ordering::Relaxed);
}

fn had_swcore_pd() -> bool {
    powman_read(OFF_CHIP_RESET) & (1 << 25) != 0
}

const SOF_MASK: u16 = 0x07ff;
/// SOF_RD is sampled every 100 ms. A host is present on any count change
/// (`now != last`). Unplug is a freeze: 500 ms with no progress.
const SOF_DEAD_TICKS: u8 = 5;

fn sof_count() -> u16 {
    // RP2350 USB.SOF_RD (datasheet 4.1.14). embassy-rp keeps `pac` crate-private.
    const USB_SOF_RD: *const u32 = 0x5011_0048 as *const u32;
    unsafe { core::ptr::read_volatile(USB_SOF_RD) as u16 & SOF_MASK }
}

#[embassy_executor::task]
async fn sof_watch_task() {
    let mut last = sof_count();
    let mut dead_streak: u8 = 0;
    loop {
        Timer::after_millis(100).await;
        let now = sof_count();
        if now != last {
            dead_streak = 0;
            set_usb_host(true);
        } else {
            dead_streak = dead_streak.saturating_add(1).min(SOF_DEAD_TICKS);
            if dead_streak >= SOF_DEAD_TICKS {
                set_usb_host(false);
            }
        }
        last = now;
    }
}

/// Drop the device pull-up and power down the PHY so USB cannot hold POWMAN.
fn quiesce_usb() {
    const USB_SIE_CTRL: *mut u32 = 0x5011_004c as *mut u32;
    const PULLUP_EN: u32 = 1 << 16;
    const TRANSCEIVER_PD: u32 = 1 << 18;
    unsafe {
        let ctrl = core::ptr::read_volatile(USB_SIE_CTRL);
        core::ptr::write_volatile(USB_SIE_CTRL, (ctrl & !PULLUP_EN) | TRANSCEIVER_PD);
    }
}

/// Take GP23 away from cyw43 (force `WL_REG_ON` low) and hold PSRAM / RM2 CS.
fn isolate_radio_and_psram() {
    gpio_force(WL_REG_ON_GPIO, false);
    gpio_force(CYW_CS_GPIO, true);
    gpio_force(PSRAM_CS_GPIO, true);
    // POWMAN keeps GP23 low after the switched-core drops (SIO is gone then).
    powman_write(OFF_EXT_CTRL0, WL_REG_ON_GPIO | EXT_INIT);
}

fn gpio_force(gpio: u32, high: bool) {
    let outover = if high {
        GPIO_OUTOVER_HIGH
    } else {
        GPIO_OUTOVER_LOW
    };
    let ctrl = GPIO_OEOVER_ENABLE | outover | GPIO_FUNCSEL_SIO;
    unsafe {
        core::ptr::write_volatile((IO_BANK0 + 0x04 + gpio * 8) as *mut u32, ctrl);
        // ISO=0 so the pad actually drives. 4 mA, PUE if the line should idle high.
        let pad = if high { 0x18 } else { 0x10 };
        core::ptr::write_volatile((PADS_BANK0 + 0x04 + gpio * 4) as *mut u32, pad);
    }
}

fn unlock_vreg() {
    powman_set(OFF_VREG_CTRL, VREG_CTRL_UNLOCK);
}

fn disable_gpio_pwrups() {
    for i in 0..4 {
        let off = OFF_PWRUP0 + i * 4;
        powman_clr(off, PWRUP_ENABLE);
        powman_clr(off, PWRUP_STATUS);
    }
}

fn arm_button_wakeups() {
    retain_button_pads();
    arm_gpio_wakeup(0, BUTTON_A_GPIO);
    arm_gpio_wakeup(1, BUTTON_B_GPIO);
}

fn arm_gpio_wakeup(slot: u32, gpio: u32) {
    let off = OFF_PWRUP0 + slot * 4;
    // Edge, active-low (falling). Enable is a separate write so a stale level
    // cannot latch STATUS before we clear it (pico-sdk powman_enable_gpio_wakeup).
    powman_write(off, PWRUP_MODE_EDGE | (gpio & 0x3f));
    powman_clr(off, PWRUP_STATUS);
    powman_set(off, PWRUP_ENABLE);
}

fn gpio_pwrup_latched() -> bool {
    for i in 0..2 {
        let off = OFF_PWRUP0 + i * 4;
        if powman_read(off) & PWRUP_STATUS != 0 {
            return true;
        }
    }
    false
}

fn retain_button_pads() {
    for gpio in [BUTTON_A_GPIO, BUTTON_B_GPIO] {
        unsafe {
            core::ptr::write_volatile((PADS_BANK0 + 0x04 + gpio * 4) as *mut u32, PAD_INPUT_PULLUP);
        }
    }
}

fn gpio_low(gpio: u32) -> bool {
    let bits = unsafe { core::ptr::read_volatile(SIO_GPIO_IN) };
    bits & (1 << gpio) == 0
}

async fn buttons_pressed() -> bool {
    if !(gpio_low(BUTTON_A_GPIO) || gpio_low(BUTTON_B_GPIO)) {
        return false;
    }
    Timer::after_millis(20).await;
    gpio_low(BUTTON_A_GPIO) || gpio_low(BUTTON_B_GPIO)
}

fn clear_boot_vectors() {
    // 32-bit registers; no password / 16-bit mask. Reboot runs flash like a
    // cold start instead of a stale POWMAN boot vector.
    unsafe {
        for i in 0..4 {
            core::ptr::write_volatile((POWMAN + OFF_BOOT0 + i * 4) as *mut u32, 0);
        }
    }
}

fn arm_lposc_alarm_ms(delay_ms: u64) -> bool {
    // Keep LPOSC + LP VREG when the switched-core drops. SRAM banks power
    // back up on wake (HW_PWRUP_SRAM* = 0).
    let seq = (powman_read(OFF_SEQ_CFG) & !SEQ_HW_PWRUP_SRAM)
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

fn request_swcore_down() -> bool {
    for _ in 0..4 {
        powman_clr(OFF_STATE, STATE_REQ_IGNORED);
        powman_write(OFF_STATE, STATE_REQ_ALL_DOWN);
        let state = powman_read(OFF_STATE);
        if state & (STATE_REQ_IGNORED | STATE_BAD_SW_REQ) != 0 {
            disable_gpio_pwrups();
            arm_button_wakeups();
            continue;
        }
        if spin_until(|| powman_read(OFF_STATE) & STATE_WAITING != 0, 400_000) {
            return true;
        }
    }
    false
}

fn halt_for_sleep(powman_waiting: bool) -> ! {
    stop_clocked_peripherals();
    if powman_waiting {
        silence_irqs(false);
        wfi_forever();
    }
    // POWMAN did not accept the request. Radio is already off; wait for the
    // AON alarm and reboot so cyw43 is not left running for the whole nap.
    powman_set(OFF_INTE, INTE_TIMER);
    enable_irq(POWMAN_TIMER_IRQ);
    silence_irqs(true);
    loop {
        if powman_read(OFF_TIMER) & TIMER_ALARM != 0 {
            sysreset();
        }
        cortex_m::asm::dsb();
        cortex_m::asm::wfi();
    }
}

fn stop_clocked_peripherals() {
    unsafe {
        core::ptr::write_volatile(RESETS_SET as *mut u32, RESET_HOLD);
    }
}

fn wfi_forever() -> ! {
    compiler_fence(Ordering::SeqCst);
    loop {
        cortex_m::asm::dsb();
        cortex_m::asm::wfi();
    }
}

fn silence_irqs(keep_powman_timer: bool) {
    unsafe {
        core::ptr::write_volatile(0x400b_0040 as *mut u32, 0);
        core::ptr::write_volatile(0x400b_0044 as *mut u32, 0);
        core::ptr::write_volatile(0x400b_003c as *mut u32, 0x0f);
        core::ptr::write_volatile(0x5011_0090 as *mut u32, 0);
        core::ptr::write_volatile(0x5011_0040 as *mut u32, 0);

        let icer = 0xE000_E180 as *mut u32;
        let icpr = 0xE000_E280 as *mut u32;
        for i in 0..3 {
            icer.add(i).write_volatile(0xFFFF_FFFF);
            icpr.add(i).write_volatile(0xFFFF_FFFF);
        }
        if keep_powman_timer {
            enable_irq(POWMAN_TIMER_IRQ);
        }
        core::ptr::write_volatile(0xE000_E010 as *mut u32, 0);
        if !keep_powman_timer {
            cortex_m::interrupt::disable();
        }
    }
}

fn enable_irq(irq: u32) {
    let bank = irq / 32;
    let bit = irq % 32;
    unsafe {
        core::ptr::write_volatile((NVIC_ISER + bank * 4) as *mut u32, 1 << bit);
    }
}

fn sysreset() -> ! {
    compiler_fence(Ordering::SeqCst);
    unsafe {
        core::ptr::write_volatile(SCB_AIRCR as *mut u32, 0x05FA_0004);
    }
    loop {
        cortex_m::asm::nop();
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

fn powman_set(off: u32, bits: u32) {
    unsafe {
        core::ptr::write_volatile(
            (POWMAN + POWMAN_SET + off) as *mut u32,
            POWMAN_PASSWORD | (bits & 0xffff),
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
