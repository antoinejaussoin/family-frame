use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Deserializer, Serialize};
use toml_edit::{Array, DocumentMut, Item, Value};

use crate::battery::{Cell, DEFAULT_CAPACITY_MAH, DEFAULT_EMPTY_MV};
use crate::schedule::{WeeklyWakes, WEEKDAY_KEYS};

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
        deserialize_with = "deserialize_optional_weekly_wakes",
        skip_serializing
    )]
    pub wake_up: Option<WeeklyWakes>,
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
    /// Local `HH:MM` times per weekday (in [`Config::timezone`]). Used when the kind is `times`.
    /// A TOML array applies to every day; a table keys `mon`…`sun`.
    #[serde(
        default,
        rename = "wake-up",
        alias = "wake_up",
        deserialize_with = "deserialize_weekly_wakes"
    )]
    pub wake_up: WeeklyWakes,
    /// Which stored schedule is active. Omitted: `times` if `wake-up` is non-empty.
    #[serde(default, alias = "schedule-kind")]
    pub schedule_kind: Option<ScheduleKind>,
    /// Fractional Pico timer error vs wall clock (`(elapsed - asked) / asked`).
    /// Positive = woke late. Written automatically from timer polls; capped at ±5%.
    #[serde(default)]
    pub pico_drift: f64,
    /// Nameplate of the 1S LiPo pouch (mAh). Used for remaining-energy math.
    #[serde(default = "default_battery_mah")]
    pub battery_mah: u32,
    /// VSYS millivolts treated as 0% usable. Default matches the Pico cutoff.
    #[serde(default = "default_battery_empty_mv")]
    pub battery_empty_mv: u32,
    pub chrome_path: String,
    pub icloud: IcloudConfig,
    pub todoist: TodoistConfig,
    pub meross: MerossConfig,
    pub weather: WeatherConfig,
    pub pronote: PronoteConfig,
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
pub struct PronoteConfig {
    /// Direct PRONOTE space URL (`eleve.html` or `parent.html`), not an ENT portal.
    pub url: String,
    pub username: String,
    pub password: String,
    /// `"eleve"` or `"parent"`. Empty = infer from the URL.
    pub account: String,
    /// Parent accounts: child's name as Pronote shows it. Empty = first child.
    pub child: String,
    /// First name on Today / next school-day hours. Empty = Pronote / child name.
    pub student: String,
    /// Optional 2FA PIN if Pronote asks for one.
    pub pin: String,
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
            wake_up: WeeklyWakes::EMPTY,
            schedule_kind: None,
            pico_drift: 0.0,
            battery_mah: DEFAULT_CAPACITY_MAH,
            battery_empty_mv: DEFAULT_EMPTY_MV,
            chrome_path: String::new(),
            icloud: IcloudConfig::default(),
            todoist: TodoistConfig::default(),
            meross: MerossConfig::default(),
            weather: WeatherConfig::default(),
            pronote: PronoteConfig::default(),
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

impl Default for PronoteConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            username: String::new(),
            password: String::new(),
            account: String::new(),
            child: String::new(),
            student: String::new(),
            pin: String::new(),
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

/// Wake times for each weekday, Monday first.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PublicWakeDays {
    #[serde(default)]
    pub mon: Vec<String>,
    #[serde(default)]
    pub tue: Vec<String>,
    #[serde(default)]
    pub wed: Vec<String>,
    #[serde(default)]
    pub thu: Vec<String>,
    #[serde(default)]
    pub fri: Vec<String>,
    #[serde(default)]
    pub sat: Vec<String>,
    #[serde(default)]
    pub sun: Vec<String>,
}

/// Public schedule for one display mode. Both values are always present;
/// [`Self::schedule_kind`] says which one the Pico currently follows.
#[derive(Debug, Clone, Serialize)]
pub struct PublicSchedule {
    pub poll_interval_secs: u64,
    /// Same list every day, or the unique union when days differ (older clients).
    pub wake_up: Vec<String>,
    pub wake_up_by_day: PublicWakeDays,
    pub schedule_kind: String,
}

/// Public settings exposed to the family UI (no secrets).
#[derive(Debug, Clone, Serialize)]
pub struct PublicSettings {
    pub mode: String,
    /// Schedule for the current [`Self::mode`] (family UI editor).
    pub poll_interval_secs: u64,
    pub wake_up: Vec<String>,
    pub wake_up_by_day: PublicWakeDays,
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
    /// When present (including empty), replaces every weekday with this list.
    /// Does not by itself choose the interval; send [`Self::schedule_kind`].
    /// Ignored when [`Self::wake_up_by_day`] is set.
    pub wake_up: Option<Vec<String>>,
    /// When present, replaces the whole week. Missing days are empty.
    pub wake_up_by_day: Option<PublicWakeDays>,
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
        cfg.battery_mah = cfg.battery_mah.max(1);
        cfg.battery_empty_mv = cfg.battery_empty_mv.clamp(2500, 4000);
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
                wake_up: &self.wake_up,
                kind: infer_schedule_kind(self.schedule_kind, &self.wake_up),
            },
            FrameMode::Picture => {
                let interval = self
                    .pictures
                    .poll_interval_secs
                    .unwrap_or(self.poll_interval_secs);
                let wakes = self.pictures.wake_up.as_ref().unwrap_or(&self.wake_up);
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
    pub fn battery_cell(&self) -> Cell {
        Cell {
            capacity_mah: self.battery_mah,
            empty_mv: self.battery_empty_mv,
        }
        .clamp()
    }

    /// Wakes each weekday for the mode the Pico is actually following.
    pub fn wakes_per_weekday(&self) -> [f64; 7] {
        let (interval, wakes) = self.schedule(self.effective_mode());
        wakes.wakes_per_weekday(interval)
    }

    pub fn schedule(&self, mode: FrameMode) -> (u64, &WeeklyWakes) {
        static EMPTY: WeeklyWakes = WeeklyWakes::EMPTY;
        let stored = self.mode_schedule(mode);
        match stored.kind {
            ScheduleKind::Interval => (stored.interval_secs, &EMPTY),
            ScheduleKind::Times => (stored.interval_secs, stored.wake_up),
        }
    }

    fn public_schedule(stored: ModeSchedule<'_>) -> PublicSchedule {
        PublicSchedule {
            poll_interval_secs: stored.interval_secs,
            wake_up: format_public_wake_list(stored.wake_up),
            wake_up_by_day: format_wake_days(stored.wake_up),
            schedule_kind: stored.kind.as_str().to_string(),
        }
    }

    pub fn public_settings(&self, now: DateTime<Utc>) -> PublicSettings {
        let current = self.mode_schedule(self.mode);
        PublicSettings {
            mode: self.mode.as_str().to_string(),
            poll_interval_secs: current.interval_secs,
            wake_up: format_public_wake_list(current.wake_up),
            wake_up_by_day: format_wake_days(current.wake_up),
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
            || patch.wake_up_by_day.is_some()
            || patch.schedule_kind.is_some()
        {
            let wakes = if let Some(by_day) = &patch.wake_up_by_day {
                Some(weekly_from_public_days(by_day)?)
            } else if let Some(times) = &patch.wake_up {
                Some(WeeklyWakes::every_day(parse_wake_list(times)?))
            } else {
                None
            };
            self.set_schedule(
                schedule_mode,
                patch.poll_interval_secs,
                wakes,
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
        wakes: Option<WeeklyWakes>,
        kind: Option<ScheduleKind>,
    ) -> Result<()> {
        if let Some(secs) = interval {
            if secs == 0 {
                bail!("poll_interval_secs must be at least 1");
            }
        }
        let kind = kind.or_else(|| {
            wakes.as_ref().map(|week| {
                if week.is_empty() {
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
        write_wake_key(doc.as_table_mut(), "wake-up", dash.wake_up);
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
        if let Some(Item::Table(pictures)) = doc.get_mut("pictures") {
            write_wake_key(pictures, "wake-up", pic.wake_up);
        }
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

    pub fn pronote_enabled(&self) -> bool {
        !self.pronote.url.trim().is_empty()
            && !self.pronote.username.trim().is_empty()
            && !self.pronote.password.trim().is_empty()
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

    /// POWMAN sleep and wall-clock slot for a Pico POST.
    ///
    /// `assigned_wake` is the slot the previous response told a timer poll to
    /// hit. Button and cold boots pass `None`.
    pub fn pico_sleep_plan(
        &self,
        now: DateTime<Utc>,
        assigned_wake: Option<DateTime<Utc>>,
    ) -> (u64, DateTime<Utc>) {
        let (interval, wakes) = self.schedule(self.effective_mode());
        let wall = crate::schedule::seconds_until_next_poll_for_timer(
            now,
            self.tz(),
            interval,
            wakes,
            assigned_wake,
        );
        let sleep_s = crate::schedule::compensate_sleep_secs(wall, self.pico_drift);
        (sleep_s, crate::schedule::instant_after(now, wall))
    }

    /// Like [`Self::pico_sleep_secs`], but a timer poll uses the stored slot.
    pub fn pico_sleep_secs_for_timer(
        &self,
        now: DateTime<Utc>,
        assigned_wake: Option<DateTime<Utc>>,
    ) -> u64 {
        self.pico_sleep_plan(now, assigned_wake).0
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
    wake_up: &'a WeeklyWakes,
    kind: ScheduleKind,
}

fn infer_schedule_kind(kind: Option<ScheduleKind>, wakes: &WeeklyWakes) -> ScheduleKind {
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

fn format_public_wake_list(wakes: &WeeklyWakes) -> Vec<String> {
    if let Some(shared) = wakes.shared_times() {
        return format_wake_times(shared);
    }
    let mut all: Vec<NaiveTime> = (0..7)
        .flat_map(|i| wakes.get_index(i).iter().copied())
        .collect();
    all.sort();
    all.dedup();
    format_wake_times(&all)
}

fn format_wake_days(wakes: &WeeklyWakes) -> PublicWakeDays {
    PublicWakeDays {
        mon: format_wake_times(wakes.get_index(0)),
        tue: format_wake_times(wakes.get_index(1)),
        wed: format_wake_times(wakes.get_index(2)),
        thu: format_wake_times(wakes.get_index(3)),
        fri: format_wake_times(wakes.get_index(4)),
        sat: format_wake_times(wakes.get_index(5)),
        sun: format_wake_times(wakes.get_index(6)),
    }
}

fn weekly_from_public_days(days: &PublicWakeDays) -> Result<WeeklyWakes> {
    Ok(WeeklyWakes::from_days([
        parse_wake_list(&days.mon)?,
        parse_wake_list(&days.tue)?,
        parse_wake_list(&days.wed)?,
        parse_wake_list(&days.thu)?,
        parse_wake_list(&days.fri)?,
        parse_wake_list(&days.sat)?,
        parse_wake_list(&days.sun)?,
    ]))
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

fn write_wake_key(parent: &mut toml_edit::Table, key: &str, wakes: &WeeklyWakes) {
    // Drop any previous array/table so a week table is a real `[wake-up]`
    // section, not `wake-up = { mon = ... }` inline.
    parent.remove(key);
    parent[key] = wake_toml_item(wakes);
}

fn wake_toml_item(wakes: &WeeklyWakes) -> Item {
    if wakes.is_uniform() {
        return Item::Value(Value::Array(wake_toml_array(wakes.get_index(0))));
    }
    let mut table = toml_edit::Table::new();
    table.set_implicit(false);
    table.set_dotted(false);
    for (i, key) in WEEKDAY_KEYS.iter().enumerate() {
        table[*key] = Item::Value(Value::Array(wake_toml_array(wakes.get_index(i))));
    }
    Item::Table(table)
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

fn default_battery_mah() -> u32 {
    DEFAULT_CAPACITY_MAH
}

fn default_battery_empty_mv() -> u32 {
    DEFAULT_EMPTY_MV
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

#[derive(Deserialize)]
#[serde(untagged)]
enum RawWeeklyWakes {
    Daily(Vec<String>),
    Weekly(BTreeMap<String, RawDayTimes>),
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawDayTimes {
    One(String),
    Many(Vec<String>),
}

impl RawDayTimes {
    fn into_vec(self) -> Vec<String> {
        match self {
            Self::One(s) => vec![s],
            Self::Many(v) => v,
        }
    }
}

fn weekly_from_raw(raw: RawWeeklyWakes) -> Result<WeeklyWakes, String> {
    match raw {
        RawWeeklyWakes::Daily(times) => {
            let parsed = times
                .iter()
                .map(|s| parse_wake_time(s))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(WeeklyWakes::every_day(parsed))
        }
        RawWeeklyWakes::Weekly(map) => {
            let mut days: [Vec<NaiveTime>; 7] = Default::default();
            let mut seen = [false; 7];
            for (key, times) in map {
                let idx = WeeklyWakes::parse_day_key(&key)?;
                if seen[idx] {
                    return Err(format!("wake-up day `{key}` specified more than once"));
                }
                seen[idx] = true;
                days[idx] = times
                    .into_vec()
                    .iter()
                    .map(|s| parse_wake_time(s))
                    .collect::<Result<Vec<_>, _>>()?;
            }
            Ok(WeeklyWakes::from_days(days))
        }
    }
}

fn deserialize_weekly_wakes<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<WeeklyWakes, D::Error> {
    let raw = Option::<RawWeeklyWakes>::deserialize(deserializer)?;
    match raw {
        None => Ok(WeeklyWakes::EMPTY),
        Some(raw) => weekly_from_raw(raw).map_err(serde::de::Error::custom),
    }
}

fn deserialize_optional_weekly_wakes<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<WeeklyWakes>, D::Error> {
    let raw = RawWeeklyWakes::deserialize(deserializer)?;
    weekly_from_raw(raw)
        .map(Some)
        .map_err(serde::de::Error::custom)
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
        assert!(!cfg.pronote_enabled());
        assert_eq!(cfg.birthdays.len(), 2);
        assert_eq!(cfg.birthdays[0].name, "Maya");
        assert_eq!(
            cfg.birthdays[0].dob,
            NaiveDate::from_ymd_opt(2018, 3, 15).unwrap()
        );
        assert_eq!(cfg.birthdays[1].name, "Sam");
        assert_eq!(cfg.battery_mah, DEFAULT_CAPACITY_MAH);
        assert_eq!(cfg.battery_empty_mv, DEFAULT_EMPTY_MV);
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
            WeeklyWakes::every_day(vec![
                NaiveTime::from_hms_opt(6, 0, 0).unwrap(),
                NaiveTime::from_hms_opt(7, 30, 0).unwrap(),
                NaiveTime::from_hms_opt(23, 15, 0).unwrap(),
            ])
        );
    }

    #[test]
    fn wake_up_snake_case_alias() {
        let cfg: Config = toml::from_str(r#"wake_up = ["08:30"]"#).unwrap();
        assert_eq!(
            cfg.wake_up,
            WeeklyWakes::every_day(vec![NaiveTime::from_hms_opt(8, 30, 0).unwrap()])
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
    fn pico_timer_sleep_skips_a_slot_the_frame_already_hit_early() {
        use chrono::TimeZone;
        let cfg: Config = toml::from_str(
            r#"
            timezone = "Europe/London"
            poll_interval_secs = 3600
            wake-up = ["06:00", "07:00"]
            "#,
        )
        .unwrap();
        let now = chrono_tz::Europe::London
            .with_ymd_and_hms(2026, 9, 16, 5, 55, 0)
            .unwrap()
            .with_timezone(&Utc);
        let intended = chrono_tz::Europe::London
            .with_ymd_and_hms(2026, 9, 16, 6, 0, 0)
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(cfg.pico_sleep_secs(now), 5 * 60);
        assert_eq!(cfg.pico_sleep_secs_for_timer(now, Some(intended)), 65 * 60);
        assert_eq!(cfg.pico_sleep_secs_for_timer(now, None), 5 * 60);
        let (sleep_s, wake_at) = cfg.pico_sleep_plan(now, Some(intended));
        assert_eq!(sleep_s, 65 * 60);
        assert_eq!(
            wake_at,
            chrono_tz::Europe::London
                .with_ymd_and_hms(2026, 9, 16, 7, 0, 0)
                .unwrap()
                .with_timezone(&Utc)
        );
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
        assert_eq!(wakes, &cfg.wake_up);
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
        assert_eq!(dash_w.shared_times().map(|t| t.len()), Some(1));
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
            wake_up_by_day: None,
            schedule_for: Some("picture".into()),
            rotate: None,
            schedule_kind: None,
        })
        .unwrap();
        assert_eq!(cfg.pictures.schedule_kind, Some(ScheduleKind::Interval));
        assert_eq!(cfg.mode, FrameMode::Picture);
        assert_eq!(cfg.poll_interval_secs, 3600);
        assert_eq!(cfg.wake_up.shared_times().map(|t| t.len()), Some(1));
        assert_eq!(cfg.pictures.poll_interval_secs, Some(90));
        assert_eq!(cfg.pictures.wake_up.as_ref(), Some(&WeeklyWakes::EMPTY));
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
        assert_eq!(cfg.wake_up.shared_times().map(|t| t.len()), Some(2));
        assert_eq!(cfg.schedule_kind, Some(ScheduleKind::Interval));
        assert_eq!(cfg.pico_sleep_secs(Utc::now()), 120);
        let reloaded = Config::load(&path).unwrap();
        assert_eq!(reloaded.poll_interval_secs, 120);
        assert_eq!(reloaded.wake_up.shared_times().map(|t| t.len()), Some(2));
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
        assert_eq!(reloaded.wake_up.shared_times().map(|t| t.len()), Some(1));
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

    fn hhmm(s: &str) -> NaiveTime {
        let (h, m) = s.split_once(':').unwrap();
        NaiveTime::from_hms_opt(h.parse().unwrap(), m.parse().unwrap(), 0).unwrap()
    }

    #[test]
    fn wake_up_table_parses_per_day() {
        let cfg: Config = toml::from_str(
            r#"
            [wake-up]
            monday = ["06:30", "15:30"]
            tue = ["06:30"]
            sat = "08:00"
            sunday = ["09:00"]
            "#,
        )
        .unwrap();
        assert!(!cfg.wake_up.is_uniform());
        assert_eq!(
            cfg.wake_up.get_index(0),
            &[hhmm("06:30"), hhmm("15:30")][..]
        );
        assert_eq!(cfg.wake_up.get_index(1), &[hhmm("06:30")][..]);
        assert!(cfg.wake_up.get_index(2).is_empty());
        assert_eq!(cfg.wake_up.get_index(5), &[hhmm("08:00")][..]);
        assert_eq!(cfg.wake_up.get_index(6), &[hhmm("09:00")][..]);
        let public = cfg.public_settings(Utc::now());
        assert_eq!(public.wake_up_by_day.sat, vec!["08:00"]);
        assert_eq!(public.wake_up, vec!["06:30", "08:00", "09:00", "15:30"]);
    }

    #[test]
    fn wake_up_table_rejects_unknown_day() {
        assert!(toml::from_str::<Config>(
            r#"
            [wake-up]
            fun = ["08:00"]
            "#
        )
        .is_err());
    }

    #[test]
    fn patch_wake_up_by_day_persists_table() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
family_name = "Family"
poll_interval_secs = 3600
wake-up = ["07:00"]
"#,
        )
        .unwrap();
        let mut cfg = Config::load(&path).unwrap();
        cfg.apply_patch(SettingsPatch {
            wake_up_by_day: Some(PublicWakeDays {
                mon: vec!["06:30".into()],
                tue: vec!["06:30".into()],
                wed: vec!["06:30".into()],
                thu: vec!["06:30".into()],
                fri: vec!["06:30".into()],
                sat: vec!["08:00".into()],
                sun: vec!["08:30".into()],
            }),
            schedule_kind: Some("times".into()),
            schedule_for: Some("dashboard".into()),
            ..Default::default()
        })
        .unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("[wake-up]"));
        assert!(text.contains("08:00"));
        assert!(text.contains("08:30"));
        let reloaded = Config::load(&path).unwrap();
        assert_eq!(reloaded.wake_up.get_index(5), &[hhmm("08:00")][..]);
        assert_eq!(reloaded.wake_up.get_index(6), &[hhmm("08:30")][..]);
        assert_eq!(
            reloaded
                .public_settings(Utc::now())
                .dashboard_schedule
                .wake_up_by_day
                .mon,
            vec!["06:30"]
        );
    }

    #[test]
    fn mixed_then_uniform_writes_array_again() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "wake-up = [\"07:00\"]\n").unwrap();
        let mut cfg = Config::load(&path).unwrap();
        cfg.apply_patch(SettingsPatch {
            wake_up_by_day: Some(PublicWakeDays {
                mon: vec!["06:30".into()],
                sat: vec!["08:00".into()],
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("[wake-up]"));
        cfg.apply_patch(SettingsPatch {
            wake_up: Some(vec!["07:15".into()]),
            ..Default::default()
        })
        .unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("wake-up = ["));
        assert!(text.contains("07:15"));
        assert!(!text.contains("[wake-up]"));
        let reloaded = Config::load(&path).unwrap();
        assert!(reloaded.wake_up.is_uniform());
        assert_eq!(reloaded.wake_up.get_index(0), &[hhmm("07:15")][..]);
    }

    #[test]
    fn per_day_wakes_change_sleep() {
        use chrono::TimeZone;
        let cfg: Config = toml::from_str(
            r#"
            timezone = "Europe/London"
            schedule_kind = "times"
            [wake-up]
            mon = ["06:30"]
            tue = ["06:30"]
            wed = ["06:30"]
            thu = ["06:30"]
            fri = ["06:30", "15:30"]
            sat = ["08:00"]
            sun = ["08:30"]
            "#,
        )
        .unwrap();
        let friday_evening = chrono_tz::Europe::London
            .with_ymd_and_hms(2026, 9, 18, 22, 0, 0)
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(cfg.pico_sleep_secs(friday_evening), 10 * 3600);
    }
}
