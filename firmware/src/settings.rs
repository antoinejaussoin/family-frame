//! Provisioned Wi-Fi and server target: RAM copy plus last flash sector.
//!
//! Same USB-CLI + last-sector layout as the laser-tag temperature / IR nodes,
//! with extra fields for the last frame checksum and the last server sleep.

use embassy_rp::flash::{Blocking, ERASE_SIZE, Error as FlashError, Flash};
use embassy_rp::peripherals::FLASH;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_sync::signal::Signal;
use family_frame_fw::config::{RECORD, decode, encode};

pub use family_frame_fw::config::NetConfig;
pub use family_frame_fw::protocol::parse_server;

/// Pico LiPo 2 XL W onboard flash size.
pub const FLASH_SIZE: usize = 16 * 1024 * 1024;
/// Last 4 KiB sector, well above the firmware image.
const CONFIG_OFFSET: u32 = (FLASH_SIZE - ERASE_SIZE) as u32;

pub type ConfigFlash = Flash<'static, FLASH, Blocking, FLASH_SIZE>;
pub type SharedFlash = Mutex<CriticalSectionRawMutex, ConfigFlash>;

static SETTINGS: Mutex<CriticalSectionRawMutex, NetConfig> = Mutex::new(NetConfig::empty());
static JOIN: Signal<CriticalSectionRawMutex, ()> = Signal::new();

pub async fn snapshot() -> NetConfig {
    SETTINGS.lock().await.clone()
}

pub async fn replace(cfg: NetConfig) {
    *SETTINGS.lock().await = cfg;
}

pub async fn update(f: impl FnOnce(&mut NetConfig)) {
    f(&mut *SETTINGS.lock().await);
}

pub fn request_rejoin() {
    JOIN.signal(());
}

pub async fn wait_rejoin() {
    JOIN.wait().await;
}

pub fn load_flash(flash: &mut ConfigFlash) -> NetConfig {
    let mut buf = [0u8; RECORD];
    if flash.blocking_read(CONFIG_OFFSET, &mut buf).is_err() {
        return NetConfig::empty();
    }
    decode(&buf).unwrap_or_else(NetConfig::empty)
}

pub fn save_flash(flash: &mut ConfigFlash, cfg: &NetConfig) -> Result<(), FlashError> {
    let buf = encode(cfg);
    flash.blocking_erase(CONFIG_OFFSET, CONFIG_OFFSET + ERASE_SIZE as u32)?;
    flash.blocking_write(CONFIG_OFFSET, &buf)
}

pub fn erase_flash(flash: &mut ConfigFlash) -> Result<(), FlashError> {
    flash.blocking_erase(CONFIG_OFFSET, CONFIG_OFFSET + ERASE_SIZE as u32)
}
