//! Last-sector settings record: magic, FNV-1a, length-prefixed fields.

use heapless::String;

pub const MAGIC: u32 = 0x4646_5231; // "FFR1"
pub const SSID_MAX: usize = 32;
pub const PSK_MAX: usize = 64;
pub const SERVER_MAX: usize = 128;
pub const CHECKSUM_MAX: usize = 80;
pub const RECORD: usize = 512;
pub const DEFAULT_SLEEP_S: u32 = 3600;

const SSID_OFF: usize = 8;
const PSK_OFF: usize = SSID_OFF + 1 + SSID_MAX;
const SERVER_OFF: usize = PSK_OFF + 1 + PSK_MAX;
const CHECKSUM_OFF: usize = SERVER_OFF + 1 + SERVER_MAX;
const SLEEP_OFF: usize = CHECKSUM_OFF + 1 + CHECKSUM_MAX;

/// In-RAM copy of the provisioned settings (and the flash payload).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetConfig {
    pub ssid: String<SSID_MAX>,
    pub psk: String<PSK_MAX>,
    pub server: String<SERVER_MAX>,
    pub last_checksum: String<CHECKSUM_MAX>,
    /// Last `X-Sleep-Seconds` from the server (fallback if a later poll omits it).
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

pub fn encode(cfg: &NetConfig) -> [u8; RECORD] {
    let mut buf = [0u8; RECORD];
    buf[0..4].copy_from_slice(&MAGIC.to_le_bytes());
    write_field(&mut buf, SSID_OFF, cfg.ssid.as_bytes());
    write_field(&mut buf, PSK_OFF, cfg.psk.as_bytes());
    write_field(&mut buf, SERVER_OFF, cfg.server.as_bytes());
    write_field(&mut buf, CHECKSUM_OFF, cfg.last_checksum.as_bytes());
    buf[SLEEP_OFF..SLEEP_OFF + 4].copy_from_slice(&cfg.sleep_s.to_le_bytes());
    let crc = checksum(&buf[8..]);
    buf[4..8].copy_from_slice(&crc.to_le_bytes());
    buf
}

pub fn decode(buf: &[u8; RECORD]) -> Option<NetConfig> {
    let magic = u32::from_le_bytes(buf[0..4].try_into().ok()?);
    if magic != MAGIC {
        return None;
    }
    let crc = u32::from_le_bytes(buf[4..8].try_into().ok()?);
    if crc != checksum(&buf[8..]) {
        return None;
    }
    let mut cfg = NetConfig::empty();
    cfg.ssid = read_field(buf, SSID_OFF, SSID_MAX)?;
    cfg.psk = read_field(buf, PSK_OFF, PSK_MAX)?;
    cfg.server = read_field(buf, SERVER_OFF, SERVER_MAX)?;
    cfg.last_checksum = read_field(buf, CHECKSUM_OFF, CHECKSUM_MAX)?;
    cfg.sleep_s = u32::from_le_bytes(buf[SLEEP_OFF..SLEEP_OFF + 4].try_into().ok()?);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(ssid: &str, psk: &str, server: &str, checksum: &str, sleep_s: u32) -> NetConfig {
        let mut c = NetConfig::empty();
        c.ssid.push_str(ssid).unwrap();
        c.psk.push_str(psk).unwrap();
        c.server.push_str(server).unwrap();
        c.last_checksum.push_str(checksum).unwrap();
        c.sleep_s = sleep_s;
        c
    }

    fn with_crc(mut buf: [u8; RECORD]) -> [u8; RECORD] {
        let crc = checksum(&buf[8..]);
        buf[4..8].copy_from_slice(&crc.to_le_bytes());
        buf
    }

    #[test]
    fn empty_is_not_ready_and_defaults_sleep() {
        let c = NetConfig::empty();
        assert!(!c.is_ready());
        assert_eq!(c.sleep_s, DEFAULT_SLEEP_S);
        assert_eq!(decode(&encode(&c)).unwrap(), c);
    }

    #[test]
    fn roundtrip_typical_lan() {
        let c = cfg("home", "secret", "192.168.0.251:8765", "deadbeef", 3600);
        assert!(c.is_ready());
        assert_eq!(decode(&encode(&c)).unwrap(), c);
    }

    #[test]
    fn roundtrip_max_fields_and_zero_sleep() {
        let c = cfg(
            &"s".repeat(SSID_MAX),
            &"p".repeat(PSK_MAX),
            &"h".repeat(SERVER_MAX),
            &"c".repeat(CHECKSUM_MAX),
            0,
        );
        assert_eq!(decode(&encode(&c)).unwrap(), c);
    }

    #[test]
    fn erased_flash_is_rejected() {
        assert!(decode(&[0xFF; RECORD]).is_none());
        assert!(decode(&[0; RECORD]).is_none());
    }

    #[test]
    fn bad_magic_is_rejected() {
        let mut buf = encode(&cfg("a", "", "b", "", 10));
        buf[0] ^= 1;
        assert!(decode(&buf).is_none());
    }

    #[test]
    fn bad_crc_is_rejected() {
        let mut buf = encode(&cfg("a", "", "b", "", 10));
        buf[20] ^= 1;
        assert!(decode(&buf).is_none());
    }

    #[test]
    fn field_longer_than_max_is_rejected() {
        let mut buf = encode(&NetConfig::empty());
        buf[SSID_OFF] = (SSID_MAX + 1) as u8;
        let buf = with_crc(buf);
        assert!(decode(&buf).is_none());
    }

    #[test]
    fn non_utf8_field_is_rejected() {
        let mut buf = encode(&NetConfig::empty());
        buf[SSID_OFF] = 1;
        buf[SSID_OFF + 1] = 0xFF;
        let buf = with_crc(buf);
        assert!(decode(&buf).is_none());
    }

    #[test]
    fn wifi_without_server_is_not_ready() {
        let c = cfg("home", "x", "", "", 60);
        assert!(!c.is_ready());
    }
}
