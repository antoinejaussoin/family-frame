use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use chrono::Utc;
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;
use tracing::info;

use crate::config::Config;
use crate::model::{Dashboard, FrameInfo};
use crate::pack;
use crate::screenshot;
use crate::sources;
use crate::template::Templates;

#[derive(Clone)]
pub struct Frame {
    pub bin: Vec<u8>,
    pub png: Vec<u8>,
    pub preview_png: Vec<u8>,
    pub checksum: String,
    pub content_hash: String,
    pub generated_at: chrono::DateTime<Utc>,
    pub dashboard: Dashboard,
}

impl Frame {
    pub fn info(&self) -> FrameInfo {
        FrameInfo {
            checksum: self.checksum.clone(),
            bytes: self.bin.len(),
            generated_at: self.generated_at,
            content_hash: self.content_hash.clone(),
            source_note: self.dashboard.source_note.clone(),
        }
    }
}

pub struct FrameCache {
    cfg: Config,
    templates: Templates,
    listen_port: u16,
    chrome: Option<std::path::PathBuf>,
    inner: Mutex<Option<Cached>>,
}

struct Cached {
    frame: Frame,
    rendered_at: Instant,
}

impl FrameCache {
    pub fn new(cfg: Config, listen_port: u16) -> Result<Arc<Self>> {
        let chrome = screenshot::detect_chrome(&cfg.chrome_path);
        Ok(Arc::new(Self {
            cfg,
            templates: Templates::load()?,
            listen_port,
            chrome,
            inner: Mutex::new(None),
        }))
    }

    pub fn templates(&self) -> &Templates {
        &self.templates
    }

    pub fn config(&self) -> &Config {
        &self.cfg
    }

    pub fn layout_hash(&self, dash: &Dashboard) -> Result<String> {
        let html = self.templates.render_dashboard(dash)?;
        let mut bytes = dash.content_bytes();
        bytes.extend_from_slice(html.as_bytes());
        bytes.extend_from_slice(crate::assets::DASHBOARD_CSS.as_bytes());
        Ok(sha256_hex(&bytes))
    }

    pub async fn current(&self) -> Result<Frame> {
        let dash = sources::load_dashboard(&self.cfg).await?;
        let content_hash = self.layout_hash(&dash)?;
        {
            let guard = self.inner.lock().await;
            if let Some(cached) = guard.as_ref() {
                let fresh = cached.rendered_at.elapsed()
                    < std::time::Duration::from_secs(self.cfg.refresh_minutes * 60);
                if cached.frame.content_hash == content_hash && fresh {
                    return Ok(cached.frame.clone());
                }
                if cached.frame.content_hash == content_hash {
                    return Ok(cached.frame.clone());
                }
            }
        }
        let frame = self.render(dash, content_hash).await?;
        let mut guard = self.inner.lock().await;
        *guard = Some(Cached {
            frame: frame.clone(),
            rendered_at: Instant::now(),
        });
        Ok(frame)
    }

    async fn render(&self, dash: Dashboard, content_hash: String) -> Result<Frame> {
        let chrome = self
            .chrome
            .as_ref()
            .context("Chrome/Chromium not found — install it to rasterise /frame.bin, or use /preview to edit the HTML layout")?;
        let url = format!("http://127.0.0.1:{}/dashboard?raster=1", self.listen_port);
        let png = screenshot::capture_dashboard(chrome, &url).await?;
        let png = ensure_panel_size(&png)?;
        let bin = pack::pack_png_to_spectra6(&png)?;
        let preview_png = pack::unpack_preview_png(&bin)?;
        let checksum = sha256_hex(&bin);
        info!(
            checksum = %checksum,
            bytes = bin.len(),
            "rendered Spectra 6 frame"
        );
        #[cfg(debug_assertions)]
        dump_debug_images(&png, &preview_png);
        Ok(Frame {
            bin,
            png,
            preview_png,
            checksum,
            content_hash,
            generated_at: Utc::now(),
            dashboard: dash,
        })
    }
}

#[cfg(debug_assertions)]
fn dump_debug_images(png: &[u8], preview_png: &[u8]) {
    let dir = crate::config::asset_root().join("out");
    if let Err(err) = std::fs::create_dir_all(&dir) {
        tracing::warn!(%err, "could not create debug image dir");
        return;
    }
    let chrome = dir.join("frame.png");
    let dither = dir.join("frame-dither.png");
    if let Err(err) = std::fs::write(&chrome, png) {
        tracing::warn!(%err, path = %chrome.display(), "could not write debug screenshot");
        return;
    }
    if let Err(err) = std::fs::write(&dither, preview_png) {
        tracing::warn!(%err, path = %dither.display(), "could not write debug dither");
        return;
    }
    info!(
        chrome = %chrome.display(),
        dither = %dither.display(),
        "wrote debug frame images"
    );
}

fn ensure_panel_size(png: &[u8]) -> Result<Vec<u8>> {
    let img = image::load_from_memory(png)?.to_rgba8();
    if img.width() == pack::PANEL_WIDTH && img.height() == pack::PANEL_HEIGHT {
        return Ok(png.to_vec());
    }
    let resized = image::imageops::resize(
        &img,
        pack::PANEL_WIDTH,
        pack::PANEL_HEIGHT,
        image::imageops::FilterType::Triangle,
    );
    let mut out = Vec::new();
    resized.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)?;
    Ok(out)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

pub fn checksum_matches(frame: &Frame, offered: Option<&str>) -> bool {
    let Some(offered) = offered.map(str::trim).filter(|s| !s.is_empty()) else {
        return false;
    };
    let offered = offered.trim_matches('"');
    offered.eq_ignore_ascii_case(&frame.checksum)
        || offered.eq_ignore_ascii_case(&frame.content_hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Dashboard;
    use crate::pack::PANEL_BYTES;
    use chrono::NaiveDate;

    #[test]
    fn etag_matches_quoted_and_bare() {
        let dash = Dashboard::empty("Family", NaiveDate::from_ymd_opt(2026, 9, 12).unwrap());
        let frame = Frame {
            bin: vec![0; PANEL_BYTES],
            png: vec![],
            preview_png: vec![],
            checksum: "abc123".into(),
            content_hash: "fff".into(),
            generated_at: Utc::now(),
            dashboard: dash,
        };
        assert!(checksum_matches(&frame, Some("abc123")));
        assert!(checksum_matches(&frame, Some("\"abc123\"")));
        assert!(checksum_matches(&frame, Some("ABC123")));
        assert!(!checksum_matches(&frame, Some("nope")));
        assert!(!checksum_matches(&frame, None));
    }
}
