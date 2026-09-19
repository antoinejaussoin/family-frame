//! Streaming HTTP/1.1 POST for `/api/frame.bin` into PSRAM.

use embassy_net::dns::DnsQueryType;
use embassy_net::tcp::TcpSocket;
use embassy_net::{IpEndpoint, Stack};
use embassy_time::Duration;
use embedded_io_async::Write;
use family_frame_fw::config::WAKE_AT_MAX;
use family_frame_fw::headers::{ParsedResponse, ResponseReader};
use family_frame_fw::protocol::{frame_target, post_frame_request, telemetry_form};
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

pub async fn get_frame(
    stack: Stack<'static>,
    dest: &mut [u8],
) -> (FrameResult, String<80>, Option<u32>, String<WAKE_AT_MAX>) {
    fn fail() -> (FrameResult, String<80>, Option<u32>, String<WAKE_AT_MAX>) {
        (FrameResult::Err, String::new(), None, String::new())
    }
    if dest.len() < FRAME_BYTES {
        FrameStatus::Fail.store();
        return fail();
    }
    if !crate::wifi::is_up() || !stack.is_config_up() {
        FrameStatus::Fail.store();
        return fail();
    }

    let cfg = settings::snapshot().await;
    let Some(target) = frame_target(cfg.server.as_str()) else {
        FrameStatus::Fail.store();
        return fail();
    };

    let ips = match stack.dns_query(target.host.as_str(), DnsQueryType::A).await {
        Ok(v) if !v.is_empty() => v,
        _ => {
            FrameStatus::Fail.store();
            return fail();
        }
    };
    let ip = ips[0];

    let (mv, pct) = crate::battery::last();
    let wake = crate::power::take_wake_label();
    let body = telemetry_form(mv, pct, crate::power::on_usb(), wake, cfg.wake_at.as_str());
    let Some(req) = post_frame_request::<512>(
        target.host.as_str(),
        target.port,
        target.path.as_str(),
        cfg.last_checksum.as_str(),
        body.as_str(),
    ) else {
        crate::power::restore_button_wake(wake);
        FrameStatus::Fail.store();
        return fail();
    };

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
        Ok(Some(parsed)) => match parsed.status {
            204 | 304 => {
                FrameStatus::NotModified.store();
                (
                    FrameResult::NotModified,
                    parsed.checksum,
                    parsed.sleep_s,
                    parsed.wake_at,
                )
            }
            200 if parsed.body_len == FRAME_BYTES => {
                FrameStatus::Ok.store();
                (
                    FrameResult::Ok,
                    parsed.checksum,
                    parsed.sleep_s,
                    parsed.wake_at,
                )
            }
            _ => {
                FrameStatus::Fail.store();
                (
                    FrameResult::Err,
                    parsed.checksum,
                    parsed.sleep_s,
                    parsed.wake_at,
                )
            }
        },
        _ => {
            crate::power::restore_button_wake(wake);
            FrameStatus::Fail.store();
            fail()
        }
    }
}

async fn read_response(socket: &mut TcpSocket<'_>, dest: &mut [u8]) -> Option<ParsedResponse> {
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
