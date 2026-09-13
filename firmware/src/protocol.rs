//! Server URL shaping: same `host:port/path` form as the laser-tag nodes,
//! then append `/frame.bin` the way the C client did.

use core::fmt::Write as _;
use heapless::String;

const SERVER_MAX: usize = 128;

/// Host, TCP port, and URL path parsed from the `server` setting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerTarget {
    pub host: String<64>,
    pub port: u16,
    pub path: String<96>,
}

/// Parse `host:port/path`, `http://host:port/path`, or `host/path` (port 80).
pub fn parse_server(raw: &str) -> Option<ServerTarget> {
    let mut s = raw.trim();
    if let Some(rest) = s.strip_prefix("http://") {
        s = rest;
    }
    if s.is_empty() || s.len() > SERVER_MAX {
        return None;
    }

    let (hostport, path) = match s.find('/') {
        Some(i) => (&s[..i], &s[i..]),
        None => (s, "/"),
    };
    if hostport.is_empty() || path.is_empty() {
        return None;
    }

    let (host, port) = split_host_port(hostport)?;
    let mut host_s = String::new();
    let mut path_s = String::new();
    host_s.push_str(host).ok()?;
    path_s.push_str(path).ok()?;
    Some(ServerTarget {
        host: host_s,
        port,
        path: path_s,
    })
}

fn split_host_port(hostport: &str) -> Option<(&str, u16)> {
    if let Some(colon) = hostport.rfind(':') {
        let host = &hostport[..colon];
        let port_s = &hostport[colon + 1..];
        if host.is_empty() {
            return None;
        }
        if !port_s.is_empty() && port_s.bytes().all(|b| b.is_ascii_digit()) {
            let port: u16 = port_s.parse().ok()?;
            return Some((host, port));
        }
    }
    Some((hostport, 80))
}

/// `http://host:port/frame.bin[?checksum=hex]`, trailing slashes stripped.
pub fn frame_url<const N: usize>(base: &str, checksum: &str) -> Option<String<N>> {
    let mut s = base.trim();
    while s.ends_with('/') {
        s = &s[..s.len() - 1];
    }
    if s.is_empty() {
        return None;
    }
    let mut out = String::new();
    if checksum.is_empty() {
        write!(out, "{s}/frame.bin").ok()?;
    } else {
        write!(out, "{s}/frame.bin?checksum={checksum}").ok()?;
    }
    Some(out)
}

/// GET target after appending `/frame.bin` to the provisioned server string.
pub fn frame_target(server: &str, checksum: &str) -> Option<ServerTarget> {
    let url = frame_url::<192>(server, checksum)?;
    parse_server(url.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expect_url(base: &str, checksum: &str, want: &str) {
        let got = frame_url::<256>(base, checksum).expect("url");
        assert_eq!(got.as_str(), want);
    }

    #[test]
    fn empty_checksum_strips_slash() {
        expect_url(
            "http://127.0.0.1:8765/",
            "",
            "http://127.0.0.1:8765/frame.bin",
        );
    }

    #[test]
    fn checksum_query() {
        expect_url(
            "http://127.0.0.1:8765",
            "abc",
            "http://127.0.0.1:8765/frame.bin?checksum=abc",
        );
    }

    #[test]
    fn lan_host() {
        expect_url(
            "http://192.168.0.251:8765/",
            "deadbeef",
            "http://192.168.0.251:8765/frame.bin?checksum=deadbeef",
        );
    }

    #[test]
    fn parse_host_port() {
        let t = parse_server("192.168.0.251:8765").unwrap();
        assert_eq!(t.host.as_str(), "192.168.0.251");
        assert_eq!(t.port, 8765);
        assert_eq!(t.path.as_str(), "/");
    }

    #[test]
    fn frame_target_appends_bin() {
        let t = frame_target("192.168.0.251:8765", "abc").unwrap();
        assert_eq!(t.host.as_str(), "192.168.0.251");
        assert_eq!(t.port, 8765);
        assert_eq!(t.path.as_str(), "/frame.bin?checksum=abc");
    }
}
