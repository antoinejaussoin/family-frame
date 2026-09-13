use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Parser;
use tracing::{error, info, warn};

mod unpack;

#[derive(Parser, Debug)]
#[command(
    name = "pico-sim",
    about = "Behave like the Pico: poll /frame.bin with the last checksum"
)]
struct Cli {
    #[arg(long, default_value = "http://127.0.0.1:8765")]
    url: String,
    #[arg(long, default_value_t = 5)]
    interval_secs: u64,
    /// Directory for timestamped PNGs of each new frame.
    #[arg(long)]
    out: Option<PathBuf>,
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
    run(cli.url, cli.interval_secs, cli.out).await
}

async fn run(base: String, interval_secs: u64, out: Option<PathBuf>) -> Result<()> {
    let out_dir = out.unwrap_or_else(default_out_dir);
    std::fs::create_dir_all(&out_dir).with_context(|| format!("creating {}", out_dir.display()))?;
    info!(dir = %out_dir.display(), "writing timestamped frame PNGs");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .context("building HTTP client")?;
    let mut checksum = String::new();
    loop {
        let url = frame_url(&base, &checksum);
        if let Err(err) = poll_frame(&client, &url, &out_dir, &mut checksum).await {
            error!(%err, %url, "could not reach frame endpoint — retrying");
        }
        tokio::time::sleep(std::time::Duration::from_secs(interval_secs)).await;
    }
}

fn default_out_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out")
}

fn frame_url(base: &str, checksum: &str) -> String {
    format!(
        "{}/frame.bin{}",
        base.trim_end_matches('/'),
        if checksum.is_empty() {
            String::new()
        } else {
            format!("?checksum={checksum}")
        }
    )
}

async fn poll_frame(
    client: &reqwest::Client,
    url: &str,
    out_dir: &Path,
    checksum: &mut String,
) -> Result<()> {
    let resp = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    let status = resp.status();
    if status.as_u16() == 304 {
        info!(checksum, "304 not modified — Pico would skip the refresh");
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
            path = %path.display(),
            "200 new frame"
        );
    } else {
        warn!(%status, "frame request failed");
    }
    Ok(())
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
    fn frame_url_omits_empty_checksum() {
        assert_eq!(
            frame_url("http://127.0.0.1:8765/", ""),
            "http://127.0.0.1:8765/frame.bin"
        );
        assert_eq!(
            frame_url("http://127.0.0.1:8765", "abc"),
            "http://127.0.0.1:8765/frame.bin?checksum=abc"
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
        let url = format!("http://{addr}/frame.bin");
        let err = poll_frame(&client, &url, dir.path(), &mut checksum)
            .await
            .expect_err("closed port should not succeed");
        assert!(err.to_string().contains("GET"));
        assert!(checksum.is_empty());
        assert!(dir.path().read_dir().unwrap().next().is_none());
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
}
