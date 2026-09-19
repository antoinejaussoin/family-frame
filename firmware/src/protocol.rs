//! Server URL shaping: same `host:port/path` form as the laser-tag nodes,
//! then append `/api/frame.bin`. Checksum rides on `If-None-Match`; battery
//! diagnostics ride on the POST body.

use core::fmt::Write as _;
use heapless::String;

use crate::config::WAKE_AT_MAX;

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

/// `http://host:port/api/frame.bin`, trailing slashes stripped.
pub fn frame_url<const N: usize>(base: &str) -> Option<String<N>> {
    let mut s = base.trim();
    while s.ends_with('/') {
        s = &s[..s.len() - 1];
    }
    if s.is_empty() {
        return None;
    }
    let mut out = String::new();
    write!(out, "{s}/api/frame.bin").ok()?;
    Some(out)
}

/// POST target after appending `/api/frame.bin` to the provisioned server string.
pub fn frame_target(server: &str) -> Option<ServerTarget> {
    let url = frame_url::<192>(server)?;
    parse_server(url.as_str())
}

/// Token the Pico may store and echo: graphic ASCII, no form separators.
pub fn sanitize_wake_at(raw: &str) -> Option<&str> {
    let v = raw.trim().trim_matches('"');
    if v.is_empty() || v.len() > WAKE_AT_MAX {
        return None;
    }
    if !v
        .bytes()
        .all(|b| b.is_ascii_graphic() && b != b'&' && b != b'=')
    {
        return None;
    }
    Some(v)
}

/// `application/x-www-form-urlencoded` body for a Pico poll.
pub fn telemetry_form(mv: u32, pct: u16, usb: bool, wake: &str, wake_at: &str) -> String<96> {
    let mut body = String::new();
    let usb_n = if usb { 1 } else { 0 };
    let _ = write!(body, "mv={mv}&pct={pct}&usb={usb_n}&wake={wake}");
    if let Some(slot) = sanitize_wake_at(wake_at) {
        let _ = write!(body, "&wake_at={slot}");
    }
    body
}

/// Raw HTTP/1.1 POST for `/api/frame.bin` with telemetry in the body.
pub fn post_frame_request<const N: usize>(
    host: &str,
    port: u16,
    path: &str,
    checksum: &str,
    body: &str,
) -> Option<String<N>> {
    let mut req = String::new();
    if port == 80 {
        write!(req, "POST {path} HTTP/1.1\r\nHost: {host}\r\n").ok()?;
    } else {
        write!(req, "POST {path} HTTP/1.1\r\nHost: {host}:{port}\r\n").ok()?;
    }
    write!(req, "Connection: close\r\n").ok()?;
    if !checksum.is_empty() {
        write!(req, "If-None-Match: {checksum}\r\n").ok()?;
    }
    write!(
        req,
        "Content-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    )
    .ok()?;
    Some(req)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expect_url(base: &str, want: &str) {
        let got = frame_url::<256>(base).expect("url");
        assert_eq!(got.as_str(), want);
    }

    #[test]
    fn strips_trailing_slash() {
        expect_url(
            "http://127.0.0.1:8765/",
            "http://127.0.0.1:8765/api/frame.bin",
        );
    }

    #[test]
    fn lan_host() {
        expect_url(
            "http://192.168.0.251:8765/",
            "http://192.168.0.251:8765/api/frame.bin",
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
    fn frame_target_appends_bin_without_checksum() {
        let t = frame_target("192.168.0.251:8765").unwrap();
        assert_eq!(t.host.as_str(), "192.168.0.251");
        assert_eq!(t.port, 8765);
        assert_eq!(t.path.as_str(), "/api/frame.bin");
    }

    #[test]
    fn telemetry_form_encodes_fields() {
        assert_eq!(
            telemetry_form(3850, 72, false, "timer", "").as_str(),
            "mv=3850&pct=72&usb=0&wake=timer"
        );
        assert_eq!(
            telemetry_form(4200, 100, true, "cold", "").as_str(),
            "mv=4200&pct=100&usb=1&wake=cold"
        );
        assert_eq!(
            telemetry_form(3850, 72, false, "button", "").as_str(),
            "mv=3850&pct=72&usb=0&wake=button"
        );
        assert_eq!(
            telemetry_form(3850, 72, false, "timer", "2026-09-19T18:00:00Z").as_str(),
            "mv=3850&pct=72&usb=0&wake=timer&wake_at=2026-09-19T18:00:00Z"
        );
        assert_eq!(
            telemetry_form(3850, 72, false, "timer", "bad&x=1").as_str(),
            "mv=3850&pct=72&usb=0&wake=timer"
        );
    }

    #[test]
    fn post_request_keeps_checksum_on_if_none_match() {
        let body = telemetry_form(3850, 72, false, "timer", "");
        let req = post_frame_request::<384>(
            "192.168.0.251",
            8765,
            "/api/frame.bin",
            "abc123",
            body.as_str(),
        )
        .unwrap();
        let s = req.as_str();
        assert!(s.starts_with("POST /api/frame.bin HTTP/1.1\r\n"));
        assert!(s.contains("Host: 192.168.0.251:8765\r\n"));
        assert!(s.contains("If-None-Match: abc123\r\n"));
        assert!(s.contains("Content-Type: application/x-www-form-urlencoded\r\n"));
        assert!(s.contains(&format!("Content-Length: {}\r\n", body.len())));
        assert!(s.ends_with("\r\n\r\nmv=3850&pct=72&usb=0&wake=timer"));
        assert!(!s.contains("checksum="));
    }

    #[test]
    fn post_request_echoes_stored_wake_at() {
        let body = telemetry_form(3850, 72, false, "timer", "2026-09-19T18:00:00Z");
        let req = post_frame_request::<512>(
            "192.168.0.251",
            8765,
            "/api/frame.bin",
            "abc123",
            body.as_str(),
        )
        .unwrap();
        assert!(
            req.as_str()
                .ends_with("\r\n\r\nmv=3850&pct=72&usb=0&wake=timer&wake_at=2026-09-19T18:00:00Z")
        );
    }

    #[test]
    fn post_request_omits_if_none_match_when_empty() {
        let body = "mv=1&pct=0&usb=0&wake=cold";
        let req = post_frame_request::<384>("127.0.0.1", 80, "/api/frame.bin", "", body).unwrap();
        let s = req.as_str();
        assert!(s.starts_with("POST /api/frame.bin HTTP/1.1\r\nHost: 127.0.0.1\r\n"));
        assert!(!s.contains("If-None-Match"));
        assert!(s.contains(&format!("Content-Length: {}\r\n", body.len())));
    }
}
