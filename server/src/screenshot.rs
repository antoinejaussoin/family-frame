use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use tokio::process::Command;
use tracing::info;

pub fn detect_chrome(configured: &str) -> Option<PathBuf> {
    if !configured.trim().is_empty() {
        let p = PathBuf::from(configured.trim());
        if p.exists() {
            return Some(p);
        }
    }
    for name in [
        "google-chrome",
        "google-chrome-stable",
        "chromium",
        "chromium-browser",
        "chrome",
    ] {
        if let Ok(out) = std::process::Command::new("which").arg(name).output() {
            if out.status.success() {
                let line = String::from_utf8_lossy(&out.stdout);
                let path = line.trim();
                if !path.is_empty() {
                    return Some(PathBuf::from(path));
                }
            }
        }
    }
    // macOS installs Chrome as an .app; it is not on PATH.
    for path in [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Google Chrome Beta.app/Contents/MacOS/Google Chrome Beta",
        "/Applications/Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
    ] {
        let p = PathBuf::from(path);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

pub async fn capture_dashboard(chrome: &std::path::Path, url: &str) -> Result<Vec<u8>> {
    let dir = tempfile::tempdir().context("temp dir for screenshot")?;
    let png_path = dir.path().join("dashboard.png");
    let user_data = dir.path().join("chrome-profile");
    std::fs::create_dir_all(&user_data)?;

    info!(%url, chrome = %chrome.display(), "capturing dashboard");
    // --single-process / --no-zygote SIGSEGV current Chrome on macOS.
    let mut child = Command::new(chrome)
        .arg("--headless=new")
        .arg("--disable-gpu")
        .arg("--no-sandbox")
        .arg("--disable-dev-shm-usage")
        .arg("--no-first-run")
        .arg("--disable-background-networking")
        .arg("--disable-component-update")
        .arg("--disable-extensions")
        .arg("--hide-scrollbars")
        .arg("--disable-lcd-text")
        .arg("--disable-font-subpixel-positioning")
        .arg("--font-render-hinting=full")
        .arg("--force-device-scale-factor=1")
        .arg("--default-background-color=FFFFFFFF")
        .arg("--window-size=1600,1200")
        .arg("--virtual-time-budget=8000")
        .arg(format!("--user-data-dir={}", user_data.display()))
        .arg(format!("--screenshot={}", png_path.display()))
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("spawning {}", chrome.display()))?;

    let mut stderr = child.stderr.take().expect("piped stderr");
    let stderr_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        let _ = tokio::io::AsyncReadExt::read_to_end(&mut stderr, &mut buf).await;
        buf
    });

    let deadline = Instant::now() + Duration::from_secs(25);
    loop {
        if png_path.exists() {
            if let Ok(meta) = std::fs::metadata(&png_path) {
                if meta.len() > 8_000 {
                    let _ = child.start_kill();
                    let _ = child.wait().await;
                    return Ok(std::fs::read(&png_path)?);
                }
            }
        }
        if let Some(status) = child.try_wait()? {
            if png_path.exists() {
                return Ok(std::fs::read(&png_path)?);
            }
            bail!(
                "chrome exited with {status} and wrote no screenshot{}",
                chrome_stderr_suffix(stderr_task.await.unwrap_or_default())
            );
        }
        if Instant::now() > deadline {
            let _ = child.start_kill();
            let _ = child.wait().await;
            if png_path.exists() {
                return Ok(std::fs::read(&png_path)?);
            }
            bail!(
                "chrome timed out writing {}{}",
                png_path.display(),
                chrome_stderr_suffix(stderr_task.await.unwrap_or_default())
            );
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

fn chrome_stderr_suffix(bytes: Vec<u8>) -> String {
    let log = String::from_utf8_lossy(&bytes);
    let t = log.trim();
    if t.is_empty() {
        return String::new();
    }
    let t = if t.len() > 400 {
        &t[t.len() - 400..]
    } else {
        t
    };
    format!("; chrome stderr: {t}")
}
