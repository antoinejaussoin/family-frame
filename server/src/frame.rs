use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::{TimeZone, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex, RwLock};
use tracing::info;

use crate::config::{Config, FrameMode};
use crate::model::{Dashboard, FrameInfo};
use crate::pack::{self, PANEL_BYTES};
use crate::pictures::PictureStore;
use crate::screenshot;
use crate::sources;
use crate::template::Templates;

/// Reuse a previously rendered dashboard for this long when switching back
/// (Chrome raster is slow). Older than this, render a fresh one.
pub const DASHBOARD_CACHE_MAX_AGE: chrono::TimeDelta = chrono::TimeDelta::minutes(10);

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
    cfg: Arc<RwLock<Config>>,
    pictures: Arc<PictureStore>,
    templates: Templates,
    listen_port: u16,
    chrome: Mutex<Option<std::path::PathBuf>>,
    inner: Mutex<Option<Cached>>,
}

struct Cached {
    frame: Frame,
    mode_key: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct DashboardDiskMeta {
    generated_at: chrono::DateTime<Utc>,
    checksum: String,
    content_hash: String,
}

pub fn dashboard_cache_fresh(
    generated_at: chrono::DateTime<Utc>,
    now: chrono::DateTime<Utc>,
) -> bool {
    let age = now.signed_duration_since(generated_at);
    age >= chrono::TimeDelta::zero() && age <= DASHBOARD_CACHE_MAX_AGE
}

impl FrameCache {
    pub fn new(
        cfg: Arc<RwLock<Config>>,
        pictures: Arc<PictureStore>,
        listen_port: u16,
    ) -> Result<Arc<Self>> {
        let chrome = cfg
            .try_read()
            .ok()
            .and_then(|guard| screenshot::detect_chrome(&guard.chrome_path));
        Ok(Arc::new(Self {
            cfg,
            pictures,
            templates: Templates::load()?,
            listen_port,
            chrome: Mutex::new(chrome),
            inner: Mutex::new(None),
        }))
    }

    pub fn templates(&self) -> &Templates {
        &self.templates
    }

    pub fn config(&self) -> Arc<RwLock<Config>> {
        self.cfg.clone()
    }

    pub fn pictures(&self) -> Arc<PictureStore> {
        self.pictures.clone()
    }

    pub async fn snapshot_config(&self) -> Config {
        self.cfg.read().await.clone()
    }

    pub async fn invalidate(&self) {
        *self.inner.lock().await = None;
    }

    pub fn layout_hash(&self, dash: &Dashboard) -> Result<String> {
        let html = self.templates.render_dashboard(dash)?;
        let mut bytes = dash.content_bytes();
        bytes.extend_from_slice(html.as_bytes());
        bytes.extend_from_slice(crate::assets::DASHBOARD_CSS.as_bytes());
        Ok(sha256_hex(&bytes))
    }

    /// Current frame for GET (no playlist advance).
    pub async fn current(&self) -> Result<Frame> {
        self.current_inner(false, false).await
    }

    /// Current frame for Pico POST; advances picture playlist after the frame is chosen.
    pub async fn current_for_pico(&self) -> Result<Frame> {
        self.current_inner(true, false).await
    }

    /// Button wake: skip the dashboard TTL cache and rebuild from live sources.
    pub async fn current_for_pico_fresh(&self) -> Result<Frame> {
        *self.inner.lock().await = None;
        self.current_inner(true, true).await
    }

    async fn current_inner(&self, advance_after: bool, bypass_cache: bool) -> Result<Frame> {
        let cfg = self.cfg.read().await.clone();
        let mode = cfg.effective_mode();
        let mode_key = match mode {
            FrameMode::Dashboard => {
                let (interval, _) = cfg.schedule(FrameMode::Dashboard);
                format!("dashboard:{interval}")
            }
            FrameMode::Picture => {
                let id = self
                    .pictures
                    .current_id(&cfg.pictures.rotate)
                    .unwrap_or_default();
                format!("picture:{id}:{}", cfg.pictures.rotate.join(","))
            }
        };

        match mode {
            FrameMode::Dashboard => {
                let frame = self
                    .current_dashboard(&cfg, &mode_key, bypass_cache)
                    .await?;
                Ok(frame)
            }
            FrameMode::Picture => {
                let frame = self.current_picture(&cfg, &mode_key).await?;
                if advance_after {
                    self.pictures.advance(&cfg.pictures.rotate)?;
                    // Next GET/POST must re-resolve; drop cache keyed to previous photo.
                    *self.inner.lock().await = None;
                }
                Ok(frame)
            }
        }
    }

    async fn current_dashboard(
        &self,
        cfg: &Config,
        mode_key: &str,
        bypass_cache: bool,
    ) -> Result<Frame> {
        let now = Utc::now();
        if !bypass_cache {
            {
                let guard = self.inner.lock().await;
                if let Some(cached) = guard.as_ref() {
                    if cached.mode_key.starts_with("dashboard:")
                        && dashboard_cache_fresh(cached.frame.generated_at, now)
                    {
                        info!(
                            age_secs = now
                                .signed_duration_since(cached.frame.generated_at)
                                .num_seconds(),
                            checksum = %cached.frame.checksum,
                            "reusing in-memory dashboard frame"
                        );
                        return Ok(cached.frame.clone());
                    }
                }
            }
            if let Some(frame) = self.load_dashboard_disk(cfg) {
                if dashboard_cache_fresh(frame.generated_at, now) {
                    info!(
                        age_secs = now
                            .signed_duration_since(frame.generated_at)
                            .num_seconds(),
                        checksum = %frame.checksum,
                        "reusing last dashboard frame"
                    );
                    let mut guard = self.inner.lock().await;
                    *guard = Some(Cached {
                        frame: frame.clone(),
                        mode_key: mode_key.to_string(),
                    });
                    return Ok(frame);
                }
            }
        }
        let dash = sources::load_dashboard(cfg).await?;
        let content_hash = self.layout_hash(&dash)?;
        if bypass_cache {
            if let Some(frame) = self.load_dashboard_disk(cfg) {
                if frame.content_hash == content_hash {
                    info!(
                        checksum = %frame.checksum,
                        "button refresh: sources unchanged"
                    );
                    let mut guard = self.inner.lock().await;
                    *guard = Some(Cached {
                        frame: frame.clone(),
                        mode_key: mode_key.to_string(),
                    });
                    return Ok(frame);
                }
            }
        }
        let frame = self.render_dashboard(dash, content_hash).await?;
        self.save_dashboard_disk(cfg, &frame);
        let mut guard = self.inner.lock().await;
        *guard = Some(Cached {
            frame: frame.clone(),
            mode_key: mode_key.to_string(),
        });
        Ok(frame)
    }

    fn dashboard_cache_paths(
        cfg: &Config,
    ) -> (
        std::path::PathBuf,
        std::path::PathBuf,
        std::path::PathBuf,
        std::path::PathBuf,
    ) {
        let dir = cfg.config_dir.join("pictures").join(".cache");
        (
            dir.join("dashboard-last.json"),
            dir.join("dashboard-last.bin"),
            dir.join("dashboard-last.png"),
            dir.join("dashboard-last-full.png"),
        )
    }

    fn load_dashboard_disk(&self, cfg: &Config) -> Option<Frame> {
        let (meta_path, bin_path, preview_path, full_path) = Self::dashboard_cache_paths(cfg);
        let meta: DashboardDiskMeta =
            serde_json::from_slice(&std::fs::read(meta_path).ok()?).ok()?;
        let bin = std::fs::read(bin_path).ok()?;
        if bin.len() != PANEL_BYTES {
            return None;
        }
        let preview_png = std::fs::read(preview_path).ok()?;
        let png = std::fs::read(full_path).unwrap_or_else(|_| preview_png.clone());
        let today = cfg
            .tz()
            .from_utc_datetime(&Utc::now().naive_utc())
            .date_naive();
        let mut dash = Dashboard::empty(&cfg.family_name, today);
        dash.source_note = "cached-dashboard".into();
        Some(Frame {
            bin,
            png,
            preview_png,
            checksum: meta.checksum,
            content_hash: meta.content_hash,
            generated_at: meta.generated_at,
            dashboard: dash,
        })
    }

    fn save_dashboard_disk(&self, cfg: &Config, frame: &Frame) {
        let (meta_path, bin_path, preview_path, full_path) = Self::dashboard_cache_paths(cfg);
        if let Some(dir) = meta_path.parent() {
            if let Err(err) = std::fs::create_dir_all(dir) {
                tracing::warn!(%err, "could not create dashboard cache dir");
                return;
            }
        }
        let meta = DashboardDiskMeta {
            generated_at: frame.generated_at,
            checksum: frame.checksum.clone(),
            content_hash: frame.content_hash.clone(),
        };
        if let Err(err) = std::fs::write(bin_path, &frame.bin) {
            tracing::warn!(%err, "could not write dashboard cache bin");
            return;
        }
        if let Err(err) = std::fs::write(preview_path, &frame.preview_png) {
            tracing::warn!(%err, "could not write dashboard cache preview");
            return;
        }
        if let Err(err) = std::fs::write(full_path, &frame.png) {
            tracing::warn!(%err, "could not write dashboard cache screenshot");
            return;
        }
        match serde_json::to_vec_pretty(&meta) {
            Ok(bytes) => {
                if let Err(err) = std::fs::write(meta_path, bytes) {
                    tracing::warn!(%err, "could not write dashboard cache meta");
                }
            }
            Err(err) => tracing::warn!(%err, "could not serialize dashboard cache meta"),
        }
    }

    async fn current_picture(&self, cfg: &Config, mode_key: &str) -> Result<Frame> {
        {
            let guard = self.inner.lock().await;
            if let Some(cached) = guard.as_ref() {
                if cached.mode_key == mode_key {
                    return Ok(cached.frame.clone());
                }
            }
        }
        let id = self
            .pictures
            .current_id(&cfg.pictures.rotate)
            .context("picture mode has no photos in the rotation")?;
        self.pictures.ensure_cache(&id).await?;
        let bin = std::fs::read(self.pictures.bin_path(&id))
            .with_context(|| format!("reading packed photo {id}"))?;
        let preview_png = std::fs::read(self.pictures.dither_png_path(&id))
            .with_context(|| format!("reading dither preview {id}"))?;
        let checksum = sha256_hex(&bin);
        let content_hash = format!("picture:{id}:{checksum}");
        let today = cfg
            .tz()
            .from_utc_datetime(&Utc::now().naive_utc())
            .date_naive();
        let mut dash = Dashboard::empty(&cfg.family_name, today);
        dash.source_note = format!("picture:{id}");
        let frame = Frame {
            bin,
            png: preview_png.clone(),
            preview_png,
            checksum,
            content_hash,
            generated_at: Utc::now(),
            dashboard: dash,
        };
        info!(
            picture = %id,
            checksum = %frame.checksum,
            bytes = frame.bin.len(),
            "serving picture frame"
        );
        let mut guard = self.inner.lock().await;
        *guard = Some(Cached {
            frame: frame.clone(),
            mode_key: mode_key.to_string(),
        });
        Ok(frame)
    }

    async fn render_dashboard(&self, dash: Dashboard, content_hash: String) -> Result<Frame> {
        let chrome = {
            let mut guard = self.chrome.lock().await;
            if guard.is_none() {
                let cfg = self.cfg.read().await;
                *guard = screenshot::detect_chrome(&cfg.chrome_path);
            }
            guard
                .clone()
                .context("Chrome/Chromium not found — install it to rasterise /api/frame.bin, or use /preview to edit the HTML layout")?
        };
        let url = format!("http://127.0.0.1:{}/dashboard?raster=1", self.listen_port);
        let png = screenshot::capture_dashboard(&chrome, &url).await?;
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

    #[test]
    fn dashboard_cache_ttl_is_ten_minutes() {
        let now = Utc::now();
        assert!(dashboard_cache_fresh(now, now));
        assert!(dashboard_cache_fresh(
            now - chrono::TimeDelta::minutes(9),
            now
        ));
        assert!(!dashboard_cache_fresh(
            now - chrono::TimeDelta::minutes(11),
            now
        ));
        assert!(!dashboard_cache_fresh(
            now + chrono::TimeDelta::minutes(1),
            now
        ));
    }
}
