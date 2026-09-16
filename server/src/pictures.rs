//! On-disk photo library under `{config_dir}/pictures/`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::info;
use uuid::Uuid;

use crate::photo::{self, DITHER_VERSION};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PictureMeta {
    pub id: String,
    pub filename: String,
    pub uploaded_at: String,
    #[serde(default)]
    pub dither_version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct LibraryFile {
    pictures: Vec<PictureMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct RotateState {
    /// Index into the current `rotate` playlist (advanced on Pico POST).
    index: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct PictureListItem {
    pub id: String,
    pub filename: String,
    pub uploaded_at: String,
    pub in_rotate: bool,
    pub thumb_url: String,
    pub dither_url: String,
    pub original_url: String,
}

pub struct PictureStore {
    root: PathBuf,
    lock: Mutex<()>,
}

impl PictureStore {
    pub fn open(config_dir: &Path) -> Result<Arc<Self>> {
        let root = config_dir.join("pictures");
        std::fs::create_dir_all(root.join(".cache"))
            .with_context(|| format!("creating {}", root.display()))?;
        Ok(Arc::new(Self {
            root,
            lock: Mutex::new(()),
        }))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn library_path(&self) -> PathBuf {
        self.root.join("library.json")
    }

    fn state_path(&self) -> PathBuf {
        self.root.join(".state.json")
    }

    fn cache_dir(&self) -> PathBuf {
        self.root.join(".cache")
    }

    fn load_library(&self) -> Result<LibraryFile> {
        let path = self.library_path();
        if !path.exists() {
            return Ok(LibraryFile::default());
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_str(&text).context("parsing library.json")
    }

    fn save_library(&self, lib: &LibraryFile) -> Result<()> {
        let path = self.library_path();
        let text = serde_json::to_string_pretty(lib)?;
        std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))
    }

    fn load_state(&self) -> Result<RotateState> {
        let path = self.state_path();
        if !path.exists() {
            return Ok(RotateState::default());
        }
        let text = std::fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&text).unwrap_or_default())
    }

    fn save_state(&self, state: &RotateState) -> Result<()> {
        let path = self.state_path();
        std::fs::write(&path, serde_json::to_string_pretty(state)?)
            .with_context(|| format!("writing {}", path.display()))
    }

    pub fn list(&self, rotate: &[String]) -> Result<Vec<PictureListItem>> {
        let lib = self.load_library()?;
        let rotate_set: std::collections::HashSet<&str> =
            rotate.iter().map(|s| s.as_str()).collect();
        Ok(lib
            .pictures
            .into_iter()
            .map(|p| PictureListItem {
                in_rotate: rotate_set.contains(p.id.as_str()),
                thumb_url: format!("/api/pictures/{}/thumb.jpg", p.id),
                dither_url: format!("/api/pictures/{}/dither.png", p.id),
                original_url: format!("/api/pictures/{}/original", p.id),
                id: p.id,
                filename: p.filename,
                uploaded_at: p.uploaded_at,
            })
            .collect())
    }

    pub async fn add(&self, filename: &str, bytes: Vec<u8>) -> Result<PictureMeta> {
        let _guard = self.lock.lock().await;
        let id = Uuid::new_v4().to_string();
        let ext = extension_for(filename, &bytes);
        let original_path = self.root.join(format!("{id}.{ext}"));
        std::fs::write(&original_path, &bytes)
            .with_context(|| format!("writing {}", original_path.display()))?;

        let (bin, preview, panel) = tokio::task::spawn_blocking(move || {
            photo::process_photo_bytes(&bytes)
        })
        .await
        .context("photo worker panicked")??;

        let cache = self.cache_dir();
        std::fs::write(cache.join(format!("{id}.bin")), &bin)?;
        std::fs::write(cache.join(format!("{id}.png")), &preview)?;
        let thumb = photo::make_thumb_jpeg(&panel, 480)?;
        std::fs::write(cache.join(format!("{id}-thumb.jpg")), &thumb)?;

        let meta = PictureMeta {
            id: id.clone(),
            filename: filename.to_string(),
            uploaded_at: chrono::Utc::now().to_rfc3339(),
            dither_version: DITHER_VERSION,
        };
        let mut lib = self.load_library()?;
        lib.pictures.push(meta.clone());
        self.save_library(&lib)?;
        info!(%id, filename, bytes = bin.len(), "added picture");
        Ok(meta)
    }

    pub async fn delete(&self, id: &str) -> Result<()> {
        let _guard = self.lock.lock().await;
        let mut lib = self.load_library()?;
        let before = lib.pictures.len();
        lib.pictures.retain(|p| p.id != id);
        if lib.pictures.len() == before {
            bail!("picture `{id}` not found");
        }
        self.save_library(&lib)?;
        self.remove_files(id);
        Ok(())
    }

    fn remove_files(&self, id: &str) {
        let cache = self.cache_dir();
        for path in [
            cache.join(format!("{id}.bin")),
            cache.join(format!("{id}.png")),
            cache.join(format!("{id}-thumb.jpg")),
        ] {
            let _ = std::fs::remove_file(path);
        }
        if let Ok(entries) = std::fs::read_dir(&self.root) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with(id) && name.contains('.') && !name.starts_with('.') {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }

    pub fn original_path(&self, id: &str) -> Result<PathBuf> {
        let lib = self.load_library()?;
        let meta = lib
            .pictures
            .iter()
            .find(|p| p.id == id)
            .with_context(|| format!("picture `{id}` not found"))?;
        let ext = extension_for(&meta.filename, &[]);
        // Prefer stored extension from actual file.
        if let Ok(entries) = std::fs::read_dir(&self.root) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if let Some(rest) = name.strip_prefix(&format!("{id}.")) {
                    if !rest.is_empty() && !name.contains('-') {
                        return Ok(entry.path());
                    }
                }
            }
        }
        Ok(self.root.join(format!("{id}.{ext}")))
    }

    pub fn thumb_path(&self, id: &str) -> PathBuf {
        self.cache_dir().join(format!("{id}-thumb.jpg"))
    }

    pub fn dither_png_path(&self, id: &str) -> PathBuf {
        self.cache_dir().join(format!("{id}.png"))
    }

    pub fn bin_path(&self, id: &str) -> PathBuf {
        self.cache_dir().join(format!("{id}.bin"))
    }

    /// Ensure cached dither artifacts exist (regenerate if version drifted).
    pub async fn ensure_cache(&self, id: &str) -> Result<()> {
        let lib = self.load_library()?;
        let meta = lib
            .pictures
            .iter()
            .find(|p| p.id == id)
            .with_context(|| format!("picture `{id}` not found"))?
            .clone();
        let bin_path = self.bin_path(id);
        let png_path = self.dither_png_path(id);
        if bin_path.exists()
            && png_path.exists()
            && meta.dither_version == DITHER_VERSION
        {
            return Ok(());
        }
        let original = self.original_path(id)?;
        let bytes = std::fs::read(&original)
            .with_context(|| format!("reading {}", original.display()))?;
        let (bin, preview, panel) = tokio::task::spawn_blocking(move || {
            photo::process_photo_bytes(&bytes)
        })
        .await
        .context("photo worker panicked")??;
        let cache = self.cache_dir();
        std::fs::create_dir_all(&cache)?;
        std::fs::write(bin_path, &bin)?;
        std::fs::write(png_path, &preview)?;
        let thumb = photo::make_thumb_jpeg(&panel, 480)?;
        std::fs::write(self.thumb_path(id), &thumb)?;

        let _guard = self.lock.lock().await;
        let mut lib = self.load_library()?;
        if let Some(p) = lib.pictures.iter_mut().find(|p| p.id == id) {
            p.dither_version = DITHER_VERSION;
        }
        self.save_library(&lib)?;
        Ok(())
    }

    pub fn current_index(&self) -> Result<usize> {
        Ok(self.load_state()?.index)
    }

    pub fn reset_index(&self) -> Result<()> {
        self.save_state(&RotateState { index: 0 })
    }

    /// Current photo id from playlist + index (no advance).
    pub fn current_id(&self, rotate: &[String]) -> Option<String> {
        if rotate.is_empty() {
            return None;
        }
        let idx = self.load_state().ok()?.index % rotate.len();
        Some(rotate[idx].clone())
    }

    /// After a Pico POST, advance to the next playlist entry.
    pub fn advance(&self, rotate: &[String]) -> Result<()> {
        if rotate.is_empty() {
            return Ok(());
        }
        let mut state = self.load_state()?;
        state.index = (state.index + 1) % rotate.len();
        self.save_state(&state)
    }

    pub fn exists(&self, id: &str) -> bool {
        self.load_library()
            .map(|lib| lib.pictures.iter().any(|p| p.id == id))
            .unwrap_or(false)
    }

    pub fn validate_rotate_ids(&self, ids: &[String]) -> Result<()> {
        let lib = self.load_library()?;
        let known: HashMap<&str, ()> = lib.pictures.iter().map(|p| (p.id.as_str(), ())).collect();
        for id in ids {
            if !known.contains_key(id.as_str()) {
                bail!("unknown picture id `{id}`");
            }
        }
        Ok(())
    }
}

fn extension_for(filename: &str, bytes: &[u8]) -> &'static str {
    let lower = filename.to_ascii_lowercase();
    if lower.ends_with(".png") {
        return "png";
    }
    if lower.ends_with(".webp") {
        return "webp";
    }
    if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        return "jpg";
    }
    if bytes.len() >= 8 && &bytes[..8] == b"\x89PNG\r\n\x1a\n" {
        return "png";
    }
    if bytes.len() >= 3 && bytes[0] == 0xff && bytes[1] == 0xd8 && bytes[2] == 0xff {
        return "jpg";
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return "webp";
    }
    "jpg"
}
