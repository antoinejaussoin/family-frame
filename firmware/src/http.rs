//! Streaming HTTP/1.1 GET for `/frame.bin` into PSRAM.

use core::fmt::Write as _;
use embassy_net::dns::DnsQueryType;
use embassy_net::tcp::TcpSocket;
use embassy_net::{IpEndpoint, Stack};
use embassy_time::Duration;
use embedded_io_async::Write;
use family_frame_fw::headers::ResponseReader;
use family_frame_fw::protocol::frame_target;
use heapless::String;

use crate::el133::FRAME_BYTES;
use crate::settings;
use crate::wifi::FrameStatus;

const HTTP_TIMEOUT_S: u64 = 90;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FrameResult {
    Ok,
    NotModified,
    Err,
}

pub async fn get_frame(stack: Stack<'static>, dest: &mut [u8]) -> (FrameResult, usize, String<80>) {
    let empty = String::new();
    if dest.len() < FRAME_BYTES {
        FrameStatus::Fail.store();
        return (FrameResult::Err, 0, empty);
    }
    if !crate::wifi::is_up() || !stack.is_config_up() {
        FrameStatus::Fail.store();
        return (FrameResult::Err, 0, empty);
    }

    let cfg = settings::snapshot().await;
    let Some(target) = frame_target(cfg.server.as_str(), cfg.last_checksum.as_str()) else {
        FrameStatus::Fail.store();
        return (FrameResult::Err, 0, empty);
    };

    let ips = match stack.dns_query(target.host.as_str(), DnsQueryType::A).await {
        Ok(v) if !v.is_empty() => v,
        _ => {
            FrameStatus::Fail.store();
            return (FrameResult::Err, 0, empty);
        }
    };
    let ip = ips[0];

    let mut req: String<384> = String::new();
    if target.port == 80 {
        let _ = write!(
            req,
            "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n",
            target.path, target.host
        );
    } else {
        let _ = write!(
            req,
            "GET {} HTTP/1.1\r\nHost: {}:{}\r\nConnection: close\r\n",
            target.path, target.host, target.port
        );
    }
    if !cfg.last_checksum.is_empty() {
        let _ = write!(req, "If-None-Match: {}\r\n", cfg.last_checksum);
    }
    let _ = req.push_str("\r\n");

    let mut rx = [0u8; 4096];
    let mut tx = [0u8; 512];
    let mut socket = TcpSocket::new(stack, &mut rx, &mut tx);
    socket.set_timeout(Some(Duration::from_secs(HTTP_TIMEOUT_S)));

    let fetched = embassy_time::with_timeout(Duration::from_secs(HTTP_TIMEOUT_S), async {
        socket
            .connect(IpEndpoint::new(ip, target.port))
            .await
            .ok()?;
        socket.write_all(req.as_bytes()).await.ok()?;
        read_response(&mut socket, dest).await
    })
    .await;

    match fetched {
        Ok(Some((status, body, etag))) => match status {
            304 => {
                FrameStatus::NotModified.store();
                (FrameResult::NotModified, body, etag)
            }
            200 if body == FRAME_BYTES => {
                FrameStatus::Ok.store();
                (FrameResult::Ok, body, etag)
            }
            _ => {
                FrameStatus::Fail.store();
                (FrameResult::Err, body, etag)
            }
        },
        _ => {
            FrameStatus::Fail.store();
            (FrameResult::Err, 0, empty)
        }
    }
}

async fn read_response(
    socket: &mut TcpSocket<'_>,
    dest: &mut [u8],
) -> Option<(u16, usize, String<80>)> {
    let mut reader = ResponseReader::new();
    let mut chunk = [0u8; 1024];

    loop {
        let n = socket.read(&mut chunk).await.ok()?;
        if n == 0 {
            break;
        }
        if !reader.feed(&chunk[..n], dest) {
            return None;
        }
    }

    reader.finish()
}
