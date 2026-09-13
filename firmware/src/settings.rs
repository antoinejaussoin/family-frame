//! Provisioned Wi-Fi and server target: RAM copy plus last flash sector.
//!
//! Same USB-CLI + last-sector layout as the laser-tag temperature / IR nodes,
//! with extra fields for the last frame checksum and the poll interval.

use embassy_rp::flash::{Blocking, ERASE_SIZE, Error as FlashError, Flash};
use embassy_rp::peripherals::FLASH;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_sync::signal::Signal;
use heapless::String;

pub use family_frame_fw::protocol::parse_server;

/// Pico LiPo 2 XL W onboard flash size.
pub const FLASH_SIZE: usize = 16 * 1024 * 1024;
/// Last 4 KiB sector, well above the firmware image.
const CONFIG_OFFSET: u32 = (FLASH_SIZE - ERASE_SIZE) as u32;

const MAGIC: u32 = 0x4646_5231; // "FFR1"
const SSID_MAX: usize = 32;
const PSK_MAX: usize = 64;
const SERVER_MAX: usize = 128;
const CHECKSUM_MAX: usize = 80;
const RECORD: usize = 512;

pub const DEFAULT_SLEEP_S: u32 = 3600;

pub type ConfigFlash = Flash<'static, FLASH, Blocking, FLASH_SIZE>;
pub type SharedFlash = Mutex<CriticalSectionRawMutex, ConfigFlash>;

static SETTINGS: Mutex<CriticalSectionRawMutex, NetConfig> = Mutex::new(NetConfig::empty());
static JOIN: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// In-RAM copy of the provisioned settings.
#[derive(Clone)]
pub struct NetConfig {
    pub ssid: String<SSID_MAX>,
    pub psk: String<PSK_MAX>,
    pub server: String<SERVER_MAX>,
    pub last_checksum: String<CHECKSUM_MAX>,
    pub sleep_s: u32,
}

impl NetConfig {
    pub const fn empty() -> Self {
        Self {
            ssid: String::new(),
            psk: String::new(),
            server: String::new(),
            last_checksum: String::new(),
            sleep_s: DEFAULT_SLEEP_S,
        }
    }

    pub fn is_ready(&self) -> bool {
        !self.ssid.is_empty() && !self.server.is_empty()
    }
}

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

fn encode(cfg: &NetConfig) -> [u8; RECORD] {
    let mut buf = [0u8; RECORD];
    buf[0..4].copy_from_slice(&MAGIC.to_le_bytes());
    write_field(&mut buf, 8, cfg.ssid.as_bytes());
    write_field(&mut buf, 8 + 1 + SSID_MAX, cfg.psk.as_bytes());
    write_field(
        &mut buf,
        8 + 1 + SSID_MAX + 1 + PSK_MAX,
        cfg.server.as_bytes(),
    );
    write_field(
        &mut buf,
        8 + 1 + SSID_MAX + 1 + PSK_MAX + 1 + SERVER_MAX,
        cfg.last_checksum.as_bytes(),
    );
    let sleep_off = 8 + 1 + SSID_MAX + 1 + PSK_MAX + 1 + SERVER_MAX + 1 + CHECKSUM_MAX;
    buf[sleep_off..sleep_off + 4].copy_from_slice(&cfg.sleep_s.to_le_bytes());
    let crc = checksum(&buf[8..]);
    buf[4..8].copy_from_slice(&crc.to_le_bytes());
    buf
}

fn decode(buf: &[u8; RECORD]) -> Option<NetConfig> {
    let magic = u32::from_le_bytes(buf[0..4].try_into().ok()?);
    if magic != MAGIC {
        return None;
    }
    let crc = u32::from_le_bytes(buf[4..8].try_into().ok()?);
    if crc != checksum(&buf[8..]) {
        return None;
    }
    let mut cfg = NetConfig::empty();
    cfg.ssid = read_field(buf, 8, SSID_MAX)?;
    cfg.psk = read_field(buf, 8 + 1 + SSID_MAX, PSK_MAX)?;
    cfg.server = read_field(buf, 8 + 1 + SSID_MAX + 1 + PSK_MAX, SERVER_MAX)?;
    cfg.last_checksum = read_field(
        buf,
        8 + 1 + SSID_MAX + 1 + PSK_MAX + 1 + SERVER_MAX,
        CHECKSUM_MAX,
    )?;
    let sleep_off = 8 + 1 + SSID_MAX + 1 + PSK_MAX + 1 + SERVER_MAX + 1 + CHECKSUM_MAX;
    cfg.sleep_s = u32::from_le_bytes(buf[sleep_off..sleep_off + 4].try_into().ok()?);
    Some(cfg)
}

fn write_field(buf: &mut [u8], offset: usize, bytes: &[u8]) {
    buf[offset] = bytes.len() as u8;
    buf[offset + 1..offset + 1 + bytes.len()].copy_from_slice(bytes);
}

fn read_field<const N: usize>(buf: &[u8], offset: usize, max: usize) -> Option<String<N>> {
    let len = buf[offset] as usize;
    if len > max || offset + 1 + len > buf.len() {
        return None;
    }
    let s = core::str::from_utf8(&buf[offset + 1..offset + 1 + len]).ok()?;
    let mut out = String::new();
    out.push_str(s).ok()?;
    Some(out)
}

fn checksum(data: &[u8]) -> u32 {
    let mut h = 2166136261u32;
    for b in data {
        h ^= u32::from(*b);
        h = h.wrapping_mul(16777619);
    }
    h
}
