use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Deserializer, Serialize};
use toml_edit::{Array, DocumentMut, Item, Value};

/// Seconds the Pico sleeps between polls when the active schedule is the interval.
pub const DEFAULT_POLL_INTERVAL_SECS: u64 = 3600;

/// Which stored schedule a display mode uses. The other value is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ScheduleKind {
    #[default]
    Interval,
    Times,
}

impl ScheduleKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Interval => "interval",
            Self::Times => "times",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "interval" | "every" | "poll" => Ok(Self::Interval),
            "times" | "time" | "wake" | "wake-up" | "wakeup" | "wake_up" => Ok(Self::Times),
            other => bail!("unknown schedule_kind `{other}` (use interval or times)"),
        }
    }
}

impl<'de> Deserialize<'de> for ScheduleKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        ScheduleKind::parse(&s).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum FrameMode {
    #[default]
    Dashboard,
    Picture,
}

impl FrameMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dashboard => "dashboard",
            Self::Picture => "picture",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "dashboard" => Ok(Self::Dashboard),
            "picture" | "pictures" | "photo" | "photos" => Ok(Self::Picture),
            other => bail!("unknown mode `{other}` (use dashboard or picture)"),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct PicturesConfig {
    /// Ordered picture ids to rotate through in picture mode.
    pub rotate: Vec<String>,
    /// Picture-mode poll interval. When omitted, inherits the dashboard value.
    pub poll_interval_secs: Option<u64>,
    /// Picture-mode wake times. `None` inherits dashboard; `Some` (including empty) is stored.
    #[serde(
        default,
        rename = "wake-up",
        alias = "wake_up",
        deserialize_with = "deserialize_optional_wake_times"
    )]
    pub wake_up: Option<Vec<NaiveTime>>,
    /// Picture-mode schedule kind. `None` is inferred from that mode’s wake list.
    #[serde(default, alias = "schedule-kind")]
    pub schedule_kind: Option<ScheduleKind>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub bind: String,
    pub timezone: String,
    pub family_name: String,
    /// Display mode: family dashboard or rotating photos.
    #[serde(default)]
    pub mode: FrameMode,
    /// Seconds between Pico polls when [`Self::schedule_kind`] is `interval`.
    #[serde(default = "default_poll_interval_secs", alias = "interval")]
    pub poll_interval_secs: u64,
    /// Local `HH:MM` times (in [`Config::timezone`]). Used when the kind is `times`.
    #[serde(
        default,
        rename = "wake-up",
        alias = "wake_up",
        deserialize_with = "deserialize_wake_times"
    )]
    pub wake_up: Vec<NaiveTime>,
    /// Which stored schedule is active. Omitted: `times` if `wake-up` is non-empty.
    #[serde(default, alias = "schedule-kind")]
    pub schedule_kind: Option<ScheduleKind>,
    /// Fractional Pico timer error vs wall clock (`(elapsed - asked) / asked`).
    /// Positive = woke late. Written automatically from timer polls; capped at ±5%.
    #[serde(default)]
    pub pico_drift: f64,
    pub chrome_path: String,
    pub icloud: IcloudConfig,
    pub todoist: TodoistConfig,
    pub meross: MerossConfig,
    pub weather: WeatherConfig,
    pub sources: SourcesConfig,
    #[serde(default)]
    pub pictures: PicturesConfig,
    /// `"Name,YYYY-MM-DD"` entries, merged into the calendar up to two weeks ahead.
    pub birthdays: Vec<Birthday>,
    #[serde(skip)]
    pub config_dir: PathBuf,
    /// Absolute path to the loaded config.toml (when one was loaded from disk).
    #[serde(skip)]
    pub config_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Birthday {
    pub name: String,
    pub dob: NaiveDate,
}

impl Birthday {
    /// Parse `Name,YYYY-MM-DD`. The date is the last comma-separated field
    /// so names may contain commas.
    pub fn parse(entry: &str) -> Result<Self, String> {
        let entry = entry.trim();
        let Some((name, dob)) = entry.rsplit_once(',') else {
            return Err(format!("birthday `{entry}` should be `Name,YYYY-MM-DD`"));
        };
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err(format!("birthday `{entry}` is missing a name"));
        }
        let dob = NaiveDate::parse_from_str(dob.trim(), "%Y-%m-%d")
            .map_err(|_| format!("birthday `{entry}` has an invalid date (use YYYY-MM-DD)"))?;
        Ok(Self { name, dob })
    }
}

impl<'de> Deserialize<'de> for Birthday {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Birthday::parse(&s).map_err(serde::de::Error::custom)
    }
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
            mode: FrameMode::Dashboard,
            poll_interval_secs: DEFAULT_POLL_INTERVAL_SECS,
            wake_up: Vec::new(),
            schedule_kind: None,
            pico_drift: 0.0,
            chrome_path: String::new(),
            icloud: IcloudConfig::default(),
            todoist: TodoistConfig::default(),
            meross: MerossConfig::default(),
            weather: WeatherConfig::default(),
            sources: SourcesConfig::default(),
            pictures: PicturesConfig::default(),
            birthdays: Vec::new(),
            config_dir: PathBuf::from("."),
            config_path: None,
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

/// Public schedule for one display mode. Both values are always present;
/// [`Self::schedule_kind`] says which one the Pico currently follows.
#[derive(Debug, Clone, Serialize)]
pub struct PublicSchedule {
    pub poll_interval_secs: u64,
    pub wake_up: Vec<String>,
    pub schedule_kind: String,
}

/// Public settings exposed to the family UI (no secrets).
#[derive(Debug, Clone, Serialize)]
pub struct PublicSettings {
    pub mode: String,
    /// Schedule for the current [`Self::mode`] (family UI editor).
    pub poll_interval_secs: u64,
    pub wake_up: Vec<String>,
    pub schedule_kind: String,
    pub dashboard_schedule: PublicSchedule,
    pub pictures_schedule: PublicSchedule,
    pub timezone: String,
    pub family_name: String,
    pub next_sleep_secs: u64,
    /// Auto-measured Pico timer error (fraction). See [`Config::pico_drift`].
    pub pico_drift: f64,
    pub rotate: Vec<String>,
}

/// Patchable fields from the family UI.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SettingsPatch {
    pub mode: Option<String>,
    pub poll_interval_secs: Option<u64>,
    /// When present (including empty), replaces the stored wake-up list.
    /// Does not by itself choose the interval; send [`Self::schedule_kind`].
    pub wake_up: Option<Vec<String>>,
    /// `"interval"` or `"times"`. When omitted, inferred from a wake-up patch
    /// (empty → interval, non-empty → times) so older clients keep working.
    pub schedule_kind: Option<String>,
    /// Which mode's schedule to patch. Defaults to the (possibly newly set) mode.
    pub schedule_for: Option<String>,
    pub rotate: Option<Vec<String>>,
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
        cfg.config_path = Some(path.to_path_buf());
        cfg.pico_drift = crate::schedule::clamp_pico_drift(cfg.pico_drift);
        cfg.materialize_pictures_schedule();
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

    pub fn effective_mode(&self) -> FrameMode {
        match self.mode {
            FrameMode::Picture if self.pictures.rotate.is_empty() => FrameMode::Dashboard,
            other => other,
        }
    }

    fn materialize_pictures_schedule(&mut self) {
        if self.pictures.poll_interval_secs.is_none() {
            self.pictures.poll_interval_secs = Some(self.poll_interval_secs);
        }
        let inheriting_wakes = self.pictures.wake_up.is_none();
        if inheriting_wakes {
            self.pictures.wake_up = Some(self.wake_up.clone());
        }
        if self.pictures.schedule_kind.is_none() && inheriting_wakes {
            let kind = self.mode_schedule(FrameMode::Dashboard).kind;
            self.pictures.schedule_kind = Some(kind);
        }
    }

    fn mode_schedule(&self, mode: FrameMode) -> ModeSchedule<'_> {
        match mode {
            FrameMode::Dashboard => ModeSchedule {
                interval_secs: self.poll_interval_secs,
                wake_up: self.wake_up.as_slice(),
                kind: infer_schedule_kind(self.schedule_kind, &self.wake_up),
            },
            FrameMode::Picture => {
                let interval = self
                    .pictures
                    .poll_interval_secs
                    .unwrap_or(self.poll_interval_secs);
                let wakes = self
                    .pictures
                    .wake_up
                    .as_deref()
                    .unwrap_or(self.wake_up.as_slice());
                ModeSchedule {
                    interval_secs: interval,
                    wake_up: wakes,
                    kind: infer_schedule_kind(self.pictures.schedule_kind, wakes),
                }
            }
        }
    }

    /// Effective poll for a display mode: interval always, wake times only when selected.
    /// Picture mode inherits the dashboard schedule until it is saved separately.
    pub fn schedule(&self, mode: FrameMode) -> (u64, &[NaiveTime]) {
        let stored = self.mode_schedule(mode);
        match stored.kind {
            ScheduleKind::Interval => (stored.interval_secs, &[]),
            ScheduleKind::Times => (stored.interval_secs, stored.wake_up),
        }
    }

    fn public_schedule(stored: ModeSchedule<'_>) -> PublicSchedule {
        PublicSchedule {
            poll_interval_secs: stored.interval_secs,
            wake_up: format_wake_times(stored.wake_up),
            schedule_kind: stored.kind.as_str().to_string(),
        }
    }

    pub fn public_settings(&self, now: DateTime<Utc>) -> PublicSettings {
        let current = self.mode_schedule(self.mode);
        PublicSettings {
            mode: self.mode.as_str().to_string(),
            poll_interval_secs: current.interval_secs,
            wake_up: format_wake_times(current.wake_up),
            schedule_kind: current.kind.as_str().to_string(),
            dashboard_schedule: Self::public_schedule(self.mode_schedule(FrameMode::Dashboard)),
            pictures_schedule: Self::public_schedule(self.mode_schedule(FrameMode::Picture)),
            timezone: self.timezone.clone(),
            family_name: self.family_name.clone(),
            next_sleep_secs: self.next_poll_secs(now),
            pico_drift: self.pico_drift,
            rotate: self.pictures.rotate.clone(),
        }
    }

    /// Apply a settings patch in memory and persist the editable keys with `toml_edit`.
    pub fn apply_patch(&mut self, patch: SettingsPatch) -> Result<()> {
        if let Some(rotate) = &patch.rotate {
            self.pictures.rotate = rotate.clone();
        }
        if let Some(mode_s) = patch.mode.as_deref() {
            self.mode = FrameMode::parse(mode_s)?;
        }
        let schedule_mode = match patch.schedule_for.as_deref() {
            Some(s) => FrameMode::parse(s)?,
            None => self.mode,
        };
        if patch.poll_interval_secs.is_some()
            || patch.wake_up.is_some()
            || patch.schedule_kind.is_some()
        {
            self.set_schedule(
                schedule_mode,
                patch.poll_interval_secs,
                match &patch.wake_up {
                    Some(times) => Some(parse_wake_list(times)?),
                    None => None,
                },
                match patch.schedule_kind.as_deref() {
                    Some(s) => Some(ScheduleKind::parse(s)?),
                    None => None,
                },
            )?;
        }
        if self.mode == FrameMode::Picture && self.pictures.rotate.is_empty() {
            bail!("picture mode needs at least one photo in the rotation");
        }
        self.persist_editable()?;
        Ok(())
    }

    fn set_schedule(
        &mut self,
        mode: FrameMode,
        interval: Option<u64>,
        wakes: Option<Vec<NaiveTime>>,
        kind: Option<ScheduleKind>,
    ) -> Result<()> {
        if let Some(secs) = interval {
            if secs == 0 {
                bail!("poll_interval_secs must be at least 1");
            }
        }
        let kind = kind.or_else(|| {
            wakes.as_ref().map(|times| {
                if times.is_empty() {
                    ScheduleKind::Interval
                } else {
                    ScheduleKind::Times
                }
            })
        });
        match mode {
            FrameMode::Dashboard => {
                if let Some(secs) = interval {
                    self.poll_interval_secs = secs;
                }
                if let Some(times) = wakes {
                    self.wake_up = times;
                }
                if let Some(k) = kind {
                    self.schedule_kind = Some(k);
                }
            }
            FrameMode::Picture => {
                if let Some(secs) = interval {
                    self.pictures.poll_interval_secs = Some(secs);
                }
                if let Some(times) = wakes {
                    self.pictures.wake_up = Some(times);
                }
                if let Some(k) = kind {
                    self.pictures.schedule_kind = Some(k);
                }
            }
        }
        Ok(())
    }

    /// Write `mode`, dashboard schedule, and `[pictures]` rotate + schedule.
    pub fn persist_editable(&self) -> Result<()> {
        let Some(path) = self.config_path.as_ref() else {
            tracing::warn!("no config path — settings kept in memory only");
            return Ok(());
        };
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let mut doc: DocumentMut = text
            .parse()
            .with_context(|| format!("parsing {} for edit", path.display()))?;

        let dash = self.mode_schedule(FrameMode::Dashboard);
        doc["mode"] = Item::Value(Value::from(self.mode.as_str()));
        doc["poll_interval_secs"] = Item::Value(Value::from(dash.interval_secs as i64));
        doc["wake-up"] = Item::Value(Value::Array(wake_toml_array(dash.wake_up)));
        doc["schedule_kind"] = Item::Value(Value::from(dash.kind.as_str()));
        write_pico_drift(&mut doc, self.pico_drift);

        if !doc.as_table().contains_key("pictures") {
            doc["pictures"] = Item::Table(toml_edit::Table::new());
        }
        let mut rotate = Array::new();
        for id in &self.pictures.rotate {
            rotate.push(id.as_str());
        }
        doc["pictures"]["rotate"] = Item::Value(Value::Array(rotate));

        let pic = self.mode_schedule(FrameMode::Picture);
        doc["pictures"]["poll_interval_secs"] = Item::Value(Value::from(pic.interval_secs as i64));
        doc["pictures"]["wake-up"] = Item::Value(Value::Array(wake_toml_array(pic.wake_up)));
        doc["pictures"]["schedule_kind"] = Item::Value(Value::from(pic.kind.as_str()));

        std::fs::write(path, doc.to_string())
            .with_context(|| format!("writing {}", path.display()))?;
        Ok(())
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

    pub fn tz(&self) -> Tz {
        self.timezone.parse().unwrap_or(chrono_tz::Europe::London)
    }

    /// Wall-clock seconds until the next intended poll (no Pico timer compensation).
    pub fn next_poll_secs(&self, now: DateTime<Utc>) -> u64 {
        let (interval, wakes) = self.schedule(self.effective_mode());
        crate::schedule::seconds_until_next_poll(now, self.tz(), interval, wakes)
    }

    /// Seconds the Pico should POWMAN-sleep after this poll, shortened if its
    /// low-power oscillator runs slow.
    pub fn pico_sleep_secs(&self, now: DateTime<Utc>) -> u64 {
        crate::schedule::compensate_sleep_secs(self.next_poll_secs(now), self.pico_drift)
    }

    /// Blend a timer-poll measurement into [`Self::pico_drift`] and persist it.
    /// Returns whether the stored value changed.
    pub fn record_pico_drift(&mut self, measured: f64) -> Result<bool> {
        let next = crate::schedule::blend_pico_drift(self.pico_drift, measured);
        if (self.pico_drift - next).abs() < 5e-5 {
            return Ok(false);
        }
        self.pico_drift = next;
        self.persist_pico_drift()?;
        Ok(true)
    }

    fn persist_pico_drift(&self) -> Result<()> {
        let Some(path) = self.config_path.as_ref() else {
            return Ok(());
        };
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let mut doc: DocumentMut = text
            .parse()
            .with_context(|| format!("parsing {} for edit", path.display()))?;
        write_pico_drift(&mut doc, self.pico_drift);
        std::fs::write(path, doc.to_string())
            .with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }
}

struct ModeSchedule<'a> {
    interval_secs: u64,
    wake_up: &'a [NaiveTime],
    kind: ScheduleKind,
}

fn infer_schedule_kind(kind: Option<ScheduleKind>, wakes: &[NaiveTime]) -> ScheduleKind {
    kind.unwrap_or(if wakes.is_empty() {
        ScheduleKind::Interval
    } else {
        ScheduleKind::Times
    })
}

fn format_wake_times(times: &[NaiveTime]) -> Vec<String> {
    times
        .iter()
        .map(|t| t.format("%H:%M").to_string())
        .collect()
}

fn write_pico_drift(doc: &mut DocumentMut, drift: f64) {
    doc["pico_drift"] = Item::Value(Value::from(crate::schedule::round_pico_drift(drift)));
}

fn wake_toml_array(times: &[NaiveTime]) -> Array {
    let mut wake = Array::new();
    for t in times {
        wake.push(t.format("%H:%M").to_string());
    }
    wake
}

fn parse_wake_list(times: &[String]) -> Result<Vec<NaiveTime>> {
    times
        .iter()
        .map(|s| parse_wake_time(s).map_err(|e| anyhow::anyhow!(e)))
        .collect()
}

fn default_poll_interval_secs() -> u64 {
    DEFAULT_POLL_INTERVAL_SECS
}

pub fn parse_wake_time(entry: &str) -> Result<NaiveTime, String> {
    let entry = entry.trim();
    let Some((hour_s, minute_s)) = entry.split_once(':') else {
        return Err(format!("wake-up `{entry}` should be HH:MM"));
    };
    if minute_s.contains(':') {
        return Err(format!("wake-up `{entry}` should be HH:MM"));
    }
    let hour: u32 = hour_s
        .trim()
        .parse()
        .map_err(|_| format!("wake-up `{entry}` has an invalid hour"))?;
    let minute: u32 = minute_s
        .trim()
        .parse()
        .map_err(|_| format!("wake-up `{entry}` has an invalid minute"))?;
    NaiveTime::from_hms_opt(hour, minute, 0)
        .ok_or_else(|| format!("wake-up `{entry}` is not a valid time of day"))
}

fn deserialize_wake_times<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<NaiveTime>, D::Error> {
    let raw = Option::<Vec<String>>::deserialize(deserializer)?.unwrap_or_default();
    raw.iter()
        .map(|s| parse_wake_time(s).map_err(serde::de::Error::custom))
        .collect()
}

fn deserialize_optional_wake_times<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Vec<NaiveTime>>, D::Error> {
    let raw = Vec::<String>::deserialize(deserializer)?;
    let times = raw
        .iter()
        .map(|s| parse_wake_time(s).map_err(serde::de::Error::custom))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Some(times))
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
        assert_eq!(cfg.poll_interval_secs, DEFAULT_POLL_INTERVAL_SECS);
        assert!(cfg.wake_up.is_empty());
        assert_eq!(cfg.todoist.project, "Family");
        assert!(!cfg.todoist_enabled());
        assert_eq!(cfg.meross.api_base_url, "https://iotx-eu.meross.com");
        assert!(!cfg.meross_enabled());
        assert_eq!(cfg.weather.location_id, "2643743");
        assert!(cfg.weather_enabled());
        assert_eq!(cfg.birthdays.len(), 2);
        assert_eq!(cfg.birthdays[0].name, "Maya");
        assert_eq!(
            cfg.birthdays[0].dob,
            NaiveDate::from_ymd_opt(2018, 3, 15).unwrap()
        );
        assert_eq!(cfg.birthdays[1].name, "Sam");
    }

    #[test]
    fn birthdays_parse_from_name_date_strings() {
        let cfg: Config = toml::from_str(
            r#"
            birthdays = ["Maya,2018-03-15", "Bob, 2020-01-02"]
            "#,
        )
        .unwrap();
        assert_eq!(cfg.birthdays[0].name, "Maya");
        assert_eq!(
            cfg.birthdays[0].dob,
            NaiveDate::from_ymd_opt(2018, 3, 15).unwrap()
        );
        assert_eq!(cfg.birthdays[1].name, "Bob");
        assert_eq!(
            cfg.birthdays[1].dob,
            NaiveDate::from_ymd_opt(2020, 1, 2).unwrap()
        );
    }

    #[test]
    fn birthday_string_allows_comma_in_name() {
        let b = Birthday::parse("Maya Jane, Jr.,2018-03-15").unwrap();
        assert_eq!(b.name, "Maya Jane, Jr.");
        assert_eq!(b.dob, NaiveDate::from_ymd_opt(2018, 3, 15).unwrap());
    }

    #[test]
    fn birthday_string_rejects_bad_entries() {
        assert!(Birthday::parse("Maya").is_err());
        assert!(Birthday::parse(",2018-03-15").is_err());
        assert!(Birthday::parse("Maya,14-09-2018").is_err());
    }

    #[test]
    fn poll_interval_defaults_to_one_hour() {
        let cfg: Config = toml::from_str("family_name = \"X\"").unwrap();
        assert_eq!(cfg.poll_interval_secs, 3600);
        assert!(cfg.wake_up.is_empty());
    }

    #[test]
    fn interval_alias_and_wake_up_times_parse() {
        let cfg: Config = toml::from_str(
            r#"
            interval = 120
            wake-up = ["06:00", "7:30", "23:15"]
            "#,
        )
        .unwrap();
        assert_eq!(cfg.poll_interval_secs, 120);
        assert_eq!(
            cfg.wake_up,
            vec![
                NaiveTime::from_hms_opt(6, 0, 0).unwrap(),
                NaiveTime::from_hms_opt(7, 30, 0).unwrap(),
                NaiveTime::from_hms_opt(23, 15, 0).unwrap(),
            ]
        );
    }

    #[test]
    fn wake_up_snake_case_alias() {
        let cfg: Config = toml::from_str(r#"wake_up = ["08:30"]"#).unwrap();
        assert_eq!(
            cfg.wake_up,
            vec![NaiveTime::from_hms_opt(8, 30, 0).unwrap()]
        );
    }

    #[test]
    fn wake_up_rejects_bad_entries() {
        assert!(parse_wake_time("6").is_err());
        assert!(parse_wake_time("24:00").is_err());
        assert!(parse_wake_time("08:60").is_err());
        assert!(parse_wake_time("08:00:00").is_err());
        assert!(toml::from_str::<Config>(r#"wake-up = ["nope"]"#).is_err());
    }

    #[test]
    fn mode_defaults_to_dashboard() {
        let cfg: Config = toml::from_str("family_name = \"X\"").unwrap();
        assert_eq!(cfg.mode, FrameMode::Dashboard);
        assert!(cfg.pictures.rotate.is_empty());
    }

    #[test]
    fn picture_mode_and_rotate_parse() {
        let cfg: Config = toml::from_str(
            r#"
            mode = "picture"
            [pictures]
            rotate = ["aaa", "bbb"]
            "#,
        )
        .unwrap();
        assert_eq!(cfg.mode, FrameMode::Picture);
        assert_eq!(cfg.pictures.rotate, vec!["aaa", "bbb"]);
    }

    #[test]
    fn toml_edit_preserves_secrets_and_comments() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"# keep me
family_name = "Family"
mode = "dashboard"
poll_interval_secs = 3600
# wake times
wake-up = []

[meross]
email = "secret@example.com"
password = "hunter2"

[pictures]
rotate = []
"#,
        )
        .unwrap();
        let mut cfg = Config::load(&path).unwrap();
        cfg.apply_patch(SettingsPatch {
            mode: Some("dashboard".into()),
            poll_interval_secs: Some(1800),
            wake_up: Some(vec!["07:00".into(), "18:30".into()]),
            rotate: Some(vec!["photo-1".into()]),
            ..Default::default()
        })
        .unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# keep me"));
        assert!(text.contains("secret@example.com"));
        assert!(text.contains("hunter2"));
        assert!(text.contains("1800"));
        assert!(text.contains("07:00"));
        assert!(text.contains("photo-1"));
        let reloaded = Config::load(&path).unwrap();
        assert_eq!(reloaded.poll_interval_secs, 1800);
        assert_eq!(reloaded.pictures.poll_interval_secs, Some(3600));
        assert_eq!(reloaded.pictures.rotate, vec!["photo-1"]);
        assert_eq!(reloaded.meross.password, "hunter2");
    }

    #[test]
    fn picture_mode_rejects_empty_rotate() {
        let mut cfg = Config::default();
        cfg.config_path = None;
        let err = cfg
            .apply_patch(SettingsPatch {
                mode: Some("picture".into()),
                rotate: Some(vec![]),
                ..Default::default()
            })
            .unwrap_err();
        assert!(err.to_string().contains("at least one"));
    }

    #[test]
    fn pico_sleep_secs_prefers_next_wake_up() {
        use chrono::TimeZone;
        let cfg: Config = toml::from_str(
            r#"
            timezone = "Europe/London"
            poll_interval_secs = 3600
            wake-up = ["06:00", "15:00"]
            "#,
        )
        .unwrap();
        let now = chrono_tz::Europe::London
            .with_ymd_and_hms(2026, 9, 16, 12, 0, 0)
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(cfg.pico_sleep_secs(now), 3 * 3600);
    }

    #[test]
    fn pictures_schedule_inherits_dashboard_until_set() {
        let cfg: Config = toml::from_str(
            r#"
            poll_interval_secs = 180
            wake-up = ["07:00"]
            [pictures]
            rotate = ["aaa"]
            "#,
        )
        .unwrap();
        assert_eq!(cfg.pictures.poll_interval_secs, None);
        assert_eq!(cfg.pictures.wake_up, None);
        let (interval, wakes) = cfg.schedule(FrameMode::Picture);
        assert_eq!(interval, 180);
        assert_eq!(wakes, cfg.wake_up.as_slice());
    }

    #[test]
    fn pictures_schedule_can_diverge() {
        let cfg: Config = toml::from_str(
            r#"
            mode = "picture"
            poll_interval_secs = 3600
            wake-up = ["07:00"]
            [pictures]
            rotate = ["aaa"]
            poll_interval_secs = 120
            wake-up = []
            "#,
        )
        .unwrap();
        let (dash_i, dash_w) = cfg.schedule(FrameMode::Dashboard);
        let (pic_i, pic_w) = cfg.schedule(FrameMode::Picture);
        assert_eq!(dash_i, 3600);
        assert_eq!(dash_w.len(), 1);
        assert_eq!(pic_i, 120);
        assert!(pic_w.is_empty());
        let now = Utc::now();
        assert_eq!(cfg.pico_sleep_secs(now), 120);
    }

    #[test]
    fn patch_schedule_for_picture_leaves_dashboard() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
family_name = "Family"
mode = "dashboard"
poll_interval_secs = 3600
wake-up = ["07:00"]

[pictures]
rotate = ["photo-1"]
"#,
        )
        .unwrap();
        let mut cfg = Config::load(&path).unwrap();
        cfg.apply_patch(SettingsPatch {
            mode: Some("picture".into()),
            poll_interval_secs: Some(90),
            wake_up: Some(vec![]),
            schedule_for: Some("picture".into()),
            rotate: None,
            schedule_kind: None,
        })
        .unwrap();
        assert_eq!(cfg.pictures.schedule_kind, Some(ScheduleKind::Interval));
        assert_eq!(cfg.mode, FrameMode::Picture);
        assert_eq!(cfg.poll_interval_secs, 3600);
        assert_eq!(cfg.wake_up.len(), 1);
        assert_eq!(cfg.pictures.poll_interval_secs, Some(90));
        assert_eq!(cfg.pictures.wake_up.as_deref(), Some(&[][..]));
        let reloaded = Config::load(&path).unwrap();
        assert_eq!(reloaded.poll_interval_secs, 3600);
        assert_eq!(reloaded.pictures.poll_interval_secs, Some(90));
        assert_eq!(reloaded.pico_sleep_secs(Utc::now()), 90);
    }

    #[test]
    fn schedule_kind_parse_aliases() {
        assert_eq!(
            ScheduleKind::parse("interval").unwrap(),
            ScheduleKind::Interval
        );
        assert_eq!(ScheduleKind::parse("times").unwrap(), ScheduleKind::Times);
        assert_eq!(ScheduleKind::parse("wake-up").unwrap(), ScheduleKind::Times);
        assert!(ScheduleKind::parse("nope").is_err());
    }

    #[test]
    fn interval_kind_keeps_wake_times_out_of_sleep() {
        use chrono::TimeZone;
        let cfg: Config = toml::from_str(
            r#"
            timezone = "Europe/London"
            poll_interval_secs = 1800
            schedule_kind = "interval"
            wake-up = ["06:00", "15:00"]
            "#,
        )
        .unwrap();
        let now = chrono_tz::Europe::London
            .with_ymd_and_hms(2026, 9, 16, 12, 0, 0)
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(cfg.pico_sleep_secs(now), 1800);
        let public = cfg.public_settings(now);
        assert_eq!(public.schedule_kind, "interval");
        assert_eq!(public.wake_up, vec!["06:00", "15:00"]);
        assert_eq!(public.dashboard_schedule.poll_interval_secs, 1800);
        assert_eq!(public.dashboard_schedule.schedule_kind, "interval");
        assert_eq!(public.pico_drift, 0.0);
    }

    #[test]
    fn patch_interval_kind_preserves_wake_times() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
family_name = "Family"
mode = "dashboard"
poll_interval_secs = 3600
wake-up = ["07:00", "18:30"]
"#,
        )
        .unwrap();
        let mut cfg = Config::load(&path).unwrap();
        cfg.apply_patch(SettingsPatch {
            poll_interval_secs: Some(120),
            schedule_kind: Some("interval".into()),
            schedule_for: Some("dashboard".into()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(cfg.poll_interval_secs, 120);
        assert_eq!(cfg.wake_up.len(), 2);
        assert_eq!(cfg.schedule_kind, Some(ScheduleKind::Interval));
        assert_eq!(cfg.pico_sleep_secs(Utc::now()), 120);
        let reloaded = Config::load(&path).unwrap();
        assert_eq!(reloaded.poll_interval_secs, 120);
        assert_eq!(reloaded.wake_up.len(), 2);
        assert_eq!(reloaded.schedule_kind, Some(ScheduleKind::Interval));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("07:00"));
        assert!(text.contains("18:30"));
        assert!(text.contains("interval"));
    }

    #[test]
    fn patch_times_kind_preserves_interval() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
family_name = "Family"
poll_interval_secs = 900
schedule_kind = "interval"
wake-up = ["08:00"]
"#,
        )
        .unwrap();
        let mut cfg = Config::load(&path).unwrap();
        cfg.apply_patch(SettingsPatch {
            schedule_kind: Some("times".into()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(cfg.poll_interval_secs, 900);
        assert_eq!(cfg.schedule_kind, Some(ScheduleKind::Times));
        let reloaded = Config::load(&path).unwrap();
        assert_eq!(reloaded.poll_interval_secs, 900);
        assert_eq!(reloaded.wake_up.len(), 1);
        assert_eq!(reloaded.schedule_kind, Some(ScheduleKind::Times));
    }

    #[test]
    fn pico_drift_shortens_sleep_but_public_next_is_wall_clock() {
        use chrono::TimeZone;
        let mut cfg: Config = toml::from_str(
            r#"
            timezone = "Europe/London"
            poll_interval_secs = 3600
            schedule_kind = "interval"
            pico_drift = 0.03
            "#,
        )
        .unwrap();
        cfg.pico_drift = crate::schedule::clamp_pico_drift(cfg.pico_drift);
        let now = chrono_tz::Europe::London
            .with_ymd_and_hms(2026, 9, 16, 12, 0, 0)
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(cfg.next_poll_secs(now), 3600);
        assert_eq!(cfg.pico_sleep_secs(now), 3495);
        let public = cfg.public_settings(now);
        assert_eq!(public.next_sleep_secs, 3600);
        assert_eq!(public.pico_drift, 0.03);
    }

    #[test]
    fn pico_drift_over_five_percent_is_clamped_on_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "pico_drift = 0.2\n").unwrap();
        let cfg = Config::load(&path).unwrap();
        assert_eq!(cfg.pico_drift, 0.05);
    }

    #[test]
    fn record_pico_drift_preserves_comments() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"# keep me
family_name = "Family"
poll_interval_secs = 3600
"#,
        )
        .unwrap();
        let mut cfg = Config::load(&path).unwrap();
        assert!(cfg.record_pico_drift(0.03).unwrap());
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# keep me"));
        assert!(text.contains("pico_drift"));
        let reloaded = Config::load(&path).unwrap();
        assert_eq!(reloaded.pico_drift, 0.03);
        assert!(!cfg.record_pico_drift(0.03).unwrap());
    }
}
