//! HTTP/1.1 response framing for `/api/frame.bin` (status, checksum, sleep, body).

use heapless::String;

use crate::config::WAKE_AT_MAX;

pub const HEADER_MAX: usize = 2048;

/// Status line plus the headers the Pico actually uses.
#[derive(Clone, Debug)]
pub struct ParsedResponse {
    pub status: u16,
    pub body_len: usize,
    pub checksum: String<80>,
    pub sleep_s: Option<u32>,
    pub wake_at: String<WAKE_AT_MAX>,
}

/// Incremental TCP → header / body split. Binary bodies are not parsed as text.
pub struct ResponseReader {
    hdr: [u8; HEADER_MAX],
    hdr_len: usize,
    hdr_end: Option<usize>,
    body_len: usize,
    overflow: bool,
}

impl ResponseReader {
    pub fn new() -> Self {
        Self {
            hdr: [0u8; HEADER_MAX],
            hdr_len: 0,
            hdr_end: None,
            body_len: 0,
            overflow: false,
        }
    }

    /// Consume the next socket chunk into `dest`. `false` = overflow or huge headers.
    pub fn feed(&mut self, mut data: &[u8], dest: &mut [u8]) -> bool {
        if self.overflow {
            return false;
        }
        if self.hdr_end.is_none() {
            let room = self.hdr.len().saturating_sub(self.hdr_len);
            let take = data.len().min(room);
            self.hdr[self.hdr_len..self.hdr_len + take].copy_from_slice(&data[..take]);
            self.hdr_len += take;
            if let Some(end) = find_header_end(&self.hdr[..self.hdr_len]) {
                self.hdr_end = Some(end);
                let extra = end..self.hdr_len;
                let extra_len = extra.len();
                if self.body_len + extra_len <= dest.len() {
                    dest[self.body_len..self.body_len + extra_len]
                        .copy_from_slice(&self.hdr[extra]);
                    self.body_len += extra_len;
                } else {
                    self.overflow = true;
                    return false;
                }
                data = &data[take..];
            } else if self.hdr_len >= self.hdr.len() {
                return false;
            } else {
                return true;
            }
        }
        if !data.is_empty() {
            if self.body_len + data.len() <= dest.len() {
                dest[self.body_len..self.body_len + data.len()].copy_from_slice(data);
                self.body_len += data.len();
            } else {
                self.overflow = true;
                return false;
            }
        }
        true
    }

    pub fn finish(&self) -> Option<ParsedResponse> {
        if self.overflow {
            return None;
        }
        let end = self.hdr_end?;
        let headers = core::str::from_utf8(&self.hdr[..end]).ok()?;
        Some(ParsedResponse {
            status: parse_status(headers)?,
            body_len: self.body_len,
            checksum: copy_checksum(headers),
            sleep_s: copy_sleep_seconds(headers),
            wake_at: copy_wake_at(headers),
        })
    }
}

/// One-shot helper for tests and complete-in-one-buffer responses.
pub fn parse_response(raw: &[u8], dest: &mut [u8]) -> Option<ParsedResponse> {
    let mut reader = ResponseReader::new();
    if !reader.feed(raw, dest) {
        return None;
    }
    reader.finish()
}

pub fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|i| i + 4)
}

pub fn parse_status(headers: &str) -> Option<u16> {
    let line = headers.lines().next()?;
    let mut parts = line.split_whitespace();
    let _ = parts.next()?;
    parts.next()?.parse().ok()
}

pub fn copy_checksum(headers: &str) -> String<80> {
    let mut out = String::new();
    let value = header_value(headers, "x-frame-checksum")
        .or_else(|| header_value(headers, "etag"))
        .unwrap_or("");
    let v = value.trim().trim_matches('"');
    let _ = out.push_str(v);
    out
}

pub fn copy_sleep_seconds(headers: &str) -> Option<u32> {
    header_value(headers, "x-sleep-seconds")?
        .trim()
        .parse()
        .ok()
        .filter(|&s| s > 0)
}

/// Opaque schedule slot from `X-Wake-At`. Empty if missing or not safe to echo.
pub fn copy_wake_at(headers: &str) -> String<WAKE_AT_MAX> {
    let mut out = String::new();
    if let Some(v) = header_value(headers, "x-wake-at").and_then(crate::protocol::sanitize_wake_at)
    {
        let _ = out.push_str(v);
    }
    out
}

pub fn header_value<'a>(headers: &'a str, name: &str) -> Option<&'a str> {
    for line in headers.lines() {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        if k.eq_ignore_ascii_case(name) {
            return Some(v.trim());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(status: &str, extra_headers: &str, body: &[u8]) -> Vec<u8> {
        let mut raw = format!("HTTP/1.1 {status}\r\n{extra_headers}\r\n").into_bytes();
        raw.extend_from_slice(body);
        raw
    }

    #[test]
    fn status_200_and_304() {
        assert_eq!(parse_status("HTTP/1.1 200 OK\r\n"), Some(200));
        assert_eq!(parse_status("HTTP/1.0 304 Not Modified\r\n"), Some(304));
        assert_eq!(parse_status("HTTP/1.1 204 No Content\r\n"), Some(204));
        assert_eq!(parse_status("broken"), None);
    }

    #[test]
    fn empty_204_body() {
        let raw = response("204 No Content", "X-Frame-Checksum: abc\r\n", b"");
        let mut dest = [0u8; 8];
        let parsed = parse_response(&raw, &mut dest).expect("parse");
        assert_eq!(parsed.status, 204);
        assert_eq!(parsed.body_len, 0);
        assert_eq!(parsed.checksum.as_str(), "abc");
        assert_eq!(parsed.sleep_s, None);
    }

    #[test]
    fn checksum_prefers_x_frame_over_quoted_etag() {
        let headers = "HTTP/1.1 200 OK\r\n\
             ETag: \"old\"\r\n\
             X-Frame-Checksum: deadbeef\r\n\r\n";
        assert_eq!(copy_checksum(headers).as_str(), "deadbeef");
    }

    #[test]
    fn checksum_falls_back_to_etag_and_strips_quotes() {
        let headers = "HTTP/1.1 200 OK\r\nETag: \"abc123\"\r\n\r\n";
        assert_eq!(copy_checksum(headers).as_str(), "abc123");
    }

    #[test]
    fn header_names_are_case_insensitive() {
        let headers = "HTTP/1.1 200 OK\r\nx-frame-checksum: AbC\r\n\r\n";
        assert_eq!(copy_checksum(headers).as_str(), "AbC");
        assert_eq!(header_value(headers, "X-Frame-Checksum"), Some("AbC"));
    }

    #[test]
    fn binary_body_after_headers_is_not_utf8_parsed() {
        let body = [
            0xFF, 0x00, 0xFE, b'\n', b'E', b'T', b'a', b'g', b':', b' ', b'x',
        ];
        let raw = response("200 OK", "X-Frame-Checksum: good\r\n", &body);
        let mut dest = [0u8; 32];
        let parsed = parse_response(&raw, &mut dest).expect("parse");
        assert_eq!(parsed.status, 200);
        assert_eq!(parsed.body_len, body.len());
        assert_eq!(&dest[..parsed.body_len], &body);
        assert_eq!(parsed.checksum.as_str(), "good");
        assert_eq!(parsed.sleep_s, None);
    }

    #[test]
    fn headers_split_across_chunks() {
        let mut dest = [0u8; 8];
        let mut r = ResponseReader::new();
        assert!(r.feed(b"HTTP/1.1 200 OK\r\nX-Frame-", &mut dest));
        assert!(r.finish().is_none());
        assert!(r.feed(b"Checksum: ab\r\nX-Sleep-Seconds: 90\r\n\r\nXY", &mut dest));
        let parsed = r.finish().expect("complete");
        assert_eq!(parsed.status, 200);
        assert_eq!(parsed.body_len, 2);
        assert_eq!(&dest[..2], b"XY");
        assert_eq!(parsed.checksum.as_str(), "ab");
        assert_eq!(parsed.sleep_s, Some(90));
    }

    #[test]
    fn sleep_seconds_is_case_insensitive_and_ignores_zero() {
        assert_eq!(
            copy_sleep_seconds("HTTP/1.1 204 No Content\r\nX-Sleep-Seconds: 3600\r\n\r\n"),
            Some(3600)
        );
        assert_eq!(
            copy_sleep_seconds("HTTP/1.1 200 OK\r\nx-sleep-seconds: 1\r\n\r\n"),
            Some(1)
        );
        assert_eq!(
            copy_sleep_seconds("HTTP/1.1 200 OK\r\nX-Sleep-Seconds: 0\r\n\r\n"),
            None
        );
        assert_eq!(
            copy_sleep_seconds("HTTP/1.1 200 OK\r\nX-Sleep-Seconds: nope\r\n\r\n"),
            None
        );
        assert_eq!(copy_sleep_seconds("HTTP/1.1 200 OK\r\n\r\n"), None);
    }

    #[test]
    fn wake_at_is_case_insensitive_and_rejects_junk() {
        assert_eq!(
            copy_wake_at("HTTP/1.1 204 No Content\r\nX-Wake-At: 2026-09-19T18:00:00Z\r\n\r\n")
                .as_str(),
            "2026-09-19T18:00:00Z"
        );
        assert_eq!(
            copy_wake_at("HTTP/1.1 200 OK\r\nx-wake-at: \"2026-09-19T18:00:00Z\"\r\n\r\n").as_str(),
            "2026-09-19T18:00:00Z"
        );
        assert!(copy_wake_at("HTTP/1.1 200 OK\r\nX-Wake-At: bad&x=1\r\n\r\n").is_empty());
        assert!(copy_wake_at("HTTP/1.1 200 OK\r\n\r\n").is_empty());
    }

    #[test]
    fn body_overflow_is_rejected() {
        let raw = response("200 OK", "", b"12345");
        let mut dest = [0u8; 4];
        assert!(parse_response(&raw, &mut dest).is_none());
    }

    #[test]
    fn incomplete_headers_fail() {
        let mut dest = [0u8; 8];
        assert!(parse_response(b"HTTP/1.1 200 OK\r\nETag: x\r\n", &mut dest).is_none());
    }

    #[test]
    fn headers_larger_than_buffer_fail() {
        let mut huge = b"HTTP/1.1 200 OK\r\n".to_vec();
        while huge.len() < HEADER_MAX + 8 {
            huge.extend_from_slice(b"X-Pad: yyyyyyyyyyyyyyyy\r\n");
        }
        huge.extend_from_slice(b"\r\n");
        let mut dest = [0u8; 8];
        assert!(parse_response(&huge, &mut dest).is_none());
    }
}
