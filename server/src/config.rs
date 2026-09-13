use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub bind: String,
    pub timezone: String,
    pub family_name: String,
    pub refresh_minutes: u64,
    pub chrome_path: String,
    pub icloud: IcloudConfig,
    pub todoist: TodoistConfig,
    pub meross: MerossConfig,
    pub weather: WeatherConfig,
    pub sources: SourcesConfig,
    #[serde(skip)]
    pub config_dir: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct IcloudConfig {
    pub apple_id: String,
    pub app_password: String,
    pub calendars: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct TodoistConfig {
    /// Personal API token from Todoist → Settings → Integrations → Developer.
    pub token: String,
    /// Shared project name or id. Invite the family to this project.
    pub project: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct MerossConfig {
    pub email: String,
    pub password: String,
    pub mfa_code: String,
    pub api_base_url: String,
    pub country_code: String,
    pub hub_hosts: Vec<String>,
    pub rooms: Vec<String>,
    pub labels: HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct WeatherConfig {
    /// GeoNames id from `https://www.bbc.co.uk/weather/<id>`.
    /// Empty disables the live BBC fetch and shows demo weather.
    pub location_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SourcesConfig {
    pub ics_urls: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bind: "0.0.0.0:8765".into(),
            timezone: "Europe/London".into(),
            family_name: "Family".into(),
            refresh_minutes: 60,
            chrome_path: String::new(),
            icloud: IcloudConfig::default(),
            todoist: TodoistConfig::default(),
            meross: MerossConfig::default(),
            weather: WeatherConfig::default(),
            sources: SourcesConfig::default(),
            config_dir: PathBuf::from("."),
        }
    }
}

impl Default for IcloudConfig {
    fn default() -> Self {
        Self {
            apple_id: String::new(),
            app_password: String::new(),
            calendars: vec!["Family".into()],
        }
    }
}

impl Default for TodoistConfig {
    fn default() -> Self {
        Self {
            token: String::new(),
            project: "Family".into(),
        }
    }
}

impl Default for MerossConfig {
    fn default() -> Self {
        Self {
            email: String::new(),
            password: String::new(),
            mfa_code: String::new(),
            api_base_url: "https://iotx-eu.meross.com".into(),
            country_code: "GB".into(),
            hub_hosts: Vec::new(),
            rooms: Vec::new(),
            labels: HashMap::new(),
        }
    }
}

impl Default for WeatherConfig {
    fn default() -> Self {
        Self {
            location_id: "2643743".into(),
        }
    }
}

impl Default for SourcesConfig {
    fn default() -> Self {
        Self {
            ics_urls: Vec::new(),
        }
    }
}

/// Directory that holds `templates/`, `static/`, and `fixtures/`.
/// Set `EINK_HOME` in Docker so the binary does not depend on the compile-time crate path.
pub fn asset_root() -> PathBuf {
    std::env::var("EINK_HOME")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")))
}

impl Config {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading config {}", path.display()))?;
        let mut cfg: Config = toml::from_str(&text).context("parsing config.toml")?;
        cfg.config_dir = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        Ok(cfg)
    }

    pub fn load_or_default(path: Option<&Path>) -> Result<Self> {
        if let Some(p) = path {
            return Self::load(p);
        }
        if let Ok(from_env) = std::env::var("EINK_CONFIG") {
            let trimmed = from_env.trim();
            if !trimmed.is_empty() {
                return Self::load(Path::new(trimmed));
            }
        }
        for candidate in [Path::new("config.toml"), Path::new("server/config.toml")] {
            if candidate.exists() {
                return Self::load(candidate);
            }
        }
        let mut cfg = Config::default();
        cfg.config_dir = asset_root();
        Ok(cfg)
    }

    pub fn icloud_enabled(&self) -> bool {
        !self.icloud.apple_id.trim().is_empty() && !self.icloud.app_password.trim().is_empty()
    }

    pub fn todoist_enabled(&self) -> bool {
        !self.todoist.token.trim().is_empty()
    }

    pub fn meross_enabled(&self) -> bool {
        !self.meross.email.trim().is_empty() && !self.meross.password.trim().is_empty()
    }

    pub fn meross_creds_path(&self) -> PathBuf {
        self.config_dir.join("meross-creds.json")
    }

    pub fn weather_enabled(&self) -> bool {
        !self.weather.location_id.trim().is_empty()
    }

    pub fn weather_cache_path(&self) -> PathBuf {
        self.config_dir.join("weather-cache.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_root_defaults_to_crate_dir() {
        assert_eq!(asset_root(), PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    }

    #[test]
    fn example_config_parses() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("config.example.toml");
        let cfg = Config::load(path).unwrap();
        assert_eq!(cfg.family_name, "Family");
        assert_eq!(cfg.refresh_minutes, 60);
        assert_eq!(cfg.todoist.project, "Family");
        assert!(!cfg.todoist_enabled());
        assert_eq!(cfg.meross.api_base_url, "https://iotx-eu.meross.com");
        assert!(!cfg.meross_enabled());
        assert_eq!(cfg.weather.location_id, "2643743");
        assert!(cfg.weather_enabled());
    }
}
