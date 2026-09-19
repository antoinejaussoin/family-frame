use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Parser;
use tracing::{error, info, warn};

mod unpack;

#[derive(Parser, Debug)]
#[command(
    name = "pico-sim",
    version = env!("FAMILY_FRAME_VERSION"),
    about = "Behave like the Pico: POST /api/frame.bin with battery diagnostics"
)]
struct Cli {
    #[arg(long, default_value = "http://127.0.0.1:8765")]
    url: String,
    /// Directory for timestamped PNGs of each new frame.
    #[arg(long)]
    out: Option<PathBuf>,
    #[arg(long, default_value_t = 3800)]
    mv: u32,
    #[arg(long, default_value_t = 55)]
    pct: u16,
    /// Pretend the board is on USB (no drain).
    #[arg(long)]
    usb: bool,
    /// Drop percent by 1 after each poll so /stats can show a slope.
    #[arg(long)]
    drain: bool,
}

/// Same fallbacks as the Pico firmware.
const FAIL_SLEEP_S: u64 = 120;
const DEFAULT_SLEEP_S: u64 = 3600;

struct Telemetry {
    mv: u32,
    pct: u16,
    usb: bool,
    drain: bool,
    first: bool,
}

impl Telemetry {
    fn body(&mut self) -> String {
        let wake = if self.first { "cold" } else { "timer" };
        self.first = false;
        let body = telemetry_form(self.mv, self.pct, self.usb, wake);
        if self.drain && !self.usb {
            self.pct = self.pct.saturating_sub(1);
            self.mv = 3300 + u32::from(self.pct) * 9;
        }
        body
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "pico_sim=info".into()),
        )
        .init();

    let cli = Cli::parse();
    run(cli).await
}

async fn run(cli: Cli) -> Result<()> {
    let out_dir = cli.out.unwrap_or_else(default_out_dir);
    std::fs::create_dir_all(&out_dir).with_context(|| format!("creating {}", out_dir.display()))?;
    info!(version = env!("FAMILY_FRAME_VERSION"), dir = %out_dir.display(), "writing timestamped frame PNGs");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .context("building HTTP client")?;
    let url = frame_url(&cli.url);
    let mut checksum = String::new();
    let mut tel = Telemetry {
        mv: cli.mv,
        pct: cli.pct.min(100),
        usb: cli.usb,
        drain: cli.drain,
        first: true,
    };
    let mut last_sleep = DEFAULT_SLEEP_S;
    loop {
        let body = tel.body();
        let (reached, server_sleep) =
            match poll_frame(&client, &url, &out_dir, &mut checksum, &body).await {
                Ok(outcome) => outcome,
                Err(err) => {
                    error!(%err, %url, "could not reach frame endpoint — retrying");
                    (false, None)
                }
            };
        if let Some(s) = server_sleep.filter(|&s| s > 0) {
            last_sleep = s;
        }
        let nap = next_nap(reached, server_sleep, last_sleep);
        info!(nap, "sleep until next poll");
        tokio::time::sleep(std::time::Duration::from_secs(nap)).await;
    }
}

fn next_nap(reached: bool, server_sleep: Option<u64>, last_sleep: u64) -> u64 {
    if !reached {
        FAIL_SLEEP_S
    } else if let Some(s) = server_sleep.filter(|&s| s > 0) {
        s
    } else if last_sleep > 0 {
        last_sleep
    } else {
        DEFAULT_SLEEP_S
    }
}

fn default_out_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out")
}

fn frame_url(base: &str) -> String {
    format!("{}/api/frame.bin", base.trim_end_matches('/'))
}

fn telemetry_form(mv: u32, pct: u16, usb: bool, wake: &str) -> String {
    format!(
        "mv={mv}&pct={pct}&usb={}&wake={wake}",
        if usb { 1 } else { 0 }
    )
}

async fn poll_frame(
    client: &reqwest::Client,
    url: &str,
    out_dir: &Path,
    checksum: &mut String,
    body: &str,
) -> Result<(bool, Option<u64>)> {
    let mut req = client
        .post(url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body.to_string());
    if !checksum.is_empty() {
        req = req.header("If-None-Match", checksum.as_str());
    }
    let resp = req.send().await.with_context(|| format!("POST {url}"))?;
    let status = resp.status();
    let sleep_s = sleep_seconds(resp.headers());
    if status.as_u16() == 204 || status.as_u16() == 304 {
        info!(checksum, %status, sleep_s, "unchanged — Pico would skip the refresh");
        Ok((true, sleep_s))
    } else if status.is_success() {
        let etag = resp
            .headers()
            .get("x-frame-checksum")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let bytes = resp.bytes().await.context("reading frame.bin body")?;
        let path = save_frame(out_dir, &etag, &bytes)?;
        *checksum = etag;
        info!(
            checksum,
            bytes = bytes.len(),
            sleep_s,
            path = %path.display(),
            "200 new frame"
        );
        Ok((true, sleep_s))
    } else {
        warn!(%status, "frame request failed");
        Ok((false, None))
    }
}

fn sleep_seconds(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    headers
        .get("x-sleep-seconds")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse().ok())
        .filter(|&s| s > 0)
}

fn save_frame(dir: &Path, checksum: &str, bin: &[u8]) -> Result<PathBuf> {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let short: String = checksum
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .take(8)
        .collect();
    let name = if short.is_empty() {
        format!("frame-{stamp}.png")
    } else {
        format!("frame-{stamp}-{short}.png")
    };
    let path = dir.join(name);
    let png = unpack::unpack_preview_png(bin).context("unpacking received frame.bin to PNG")?;
    std::fs::write(&path, png).with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use unpack::PANEL_BYTES;

    #[test]
    fn version_matches_repo_file() {
        let file = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../VERSION"));
        assert_eq!(env!("FAMILY_FRAME_VERSION"), file.trim());
    }

    #[test]
    fn frame_url_has_no_checksum_query() {
        assert_eq!(
            frame_url("http://127.0.0.1:8765/"),
            "http://127.0.0.1:8765/api/frame.bin"
        );
        assert_eq!(
            frame_url("http://127.0.0.1:8765"),
            "http://127.0.0.1:8765/api/frame.bin"
        );
    }

    #[test]
    fn telemetry_form_matches_firmware() {
        assert_eq!(
            telemetry_form(3850, 72, false, "timer"),
            "mv=3850&pct=72&usb=0&wake=timer"
        );
        assert_eq!(
            telemetry_form(3850, 72, false, "button"),
            "mv=3850&pct=72&usb=0&wake=button"
        );
    }

    #[tokio::test]
    async fn unreachable_endpoint_returns_error() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut checksum = String::new();
        let url = format!("http://{addr}/api/frame.bin");
        let err = poll_frame(
            &client,
            &url,
            dir.path(),
            &mut checksum,
            "mv=3800&pct=55&usb=0&wake=cold",
        )
        .await
        .expect_err("closed port should not succeed");
        assert!(err.to_string().contains("POST"));
        assert!(checksum.is_empty());
        assert!(dir.path().read_dir().unwrap().next().is_none());
    }

    #[test]
    fn next_nap_matches_firmware() {
        assert_eq!(next_nap(false, Some(3600), 3600), FAIL_SLEEP_S);
        assert_eq!(next_nap(true, Some(90), 3600), 90);
        assert_eq!(next_nap(true, None, 1800), 1800);
        assert_eq!(next_nap(true, Some(0), 0), DEFAULT_SLEEP_S);
        assert_eq!(next_nap(true, None, 0), DEFAULT_SLEEP_S);
    }

    #[test]
    fn sleep_seconds_parses_positive_header() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("x-sleep-seconds", "3600".parse().unwrap());
        assert_eq!(sleep_seconds(&headers), Some(3600));
        headers.insert("x-sleep-seconds", "0".parse().unwrap());
        assert_eq!(sleep_seconds(&headers), None);
        assert_eq!(sleep_seconds(&reqwest::header::HeaderMap::new()), None);
    }

    #[test]
    fn writes_timestamped_png() {
        let dir = tempfile::tempdir().unwrap();
        let bin = vec![0x11; PANEL_BYTES];
        let path = save_frame(dir.path(), "abc123def456", &bin).unwrap();
        let name = path.file_name().unwrap().to_str().unwrap();
        assert!(name.starts_with("frame-"));
        assert!(name.ends_with("-abc123de.png"));
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    }

    #[test]
    fn drain_lowers_pct() {
        let mut tel = Telemetry {
            mv: 3800,
            pct: 55,
            usb: false,
            drain: true,
            first: true,
        };
        assert_eq!(tel.body(), "mv=3800&pct=55&usb=0&wake=cold");
        assert_eq!(tel.body(), "mv=3786&pct=54&usb=0&wake=timer");
    }
}
