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
    /// Fractional Pico timer error vs wall clock (`elapsed ≈ (1+drift)*asked + overhead`).
    /// Positive = woke late. Written automatically from timer polls; capped at ±5%.
    #[serde(default)]
    pub pico_drift: f64,
    /// Fixed seconds added to every wake (boot, Wi-Fi, fetch, panel write).
    /// Independent of sleep length. Written with [`Self::pico_drift`].
    #[serde(default)]
    pub pico_overhead_secs: f64,
    /// Nameplate of the 1S LiPo pouch (mAh). Used for remaining-energy math.
    #[serde(default = "default_battery_mah")]
    pub battery_mah: u32,
    /// VSYS millivolts treated as 0% usable. Default matches the Pico cutoff.
    #[serde(default = "default_battery_empty_mv")]
    pub battery_empty_mv: u32,
    pub chrome_path: String,
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
    /// Screenshot mode (`--fake` / `FAMILY_FRAME_FAKE`): demo household sources,
    /// live public ones. Never written to config.toml.
    #[serde(skip)]
    pub fake_private: bool,
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

    pub fn to_entry(&self) -> String {
        format!("{},{}", self.name, self.dob.format("%Y-%m-%d"))
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
    /// Homework / grades quarter-columns. Hours still merge when the source runs.
    #[serde(default)]
    pub show_sections: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct CalendarSourceConfig {
    pub ics_urls: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct BinsSourceConfig {
    /// Wandsworth Unique Property Reference Number, or a My Property URL.
    pub uprn: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct BirthdaysSourceConfig {
    pub people: Vec<Birthday>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct TflLineConfig {
    pub id: String,
    pub name: String,
    pub colour: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct TflConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_tfl_lines")]
    pub lines: Vec<TflLineConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ToggleConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SourcesConfig {
    /// Legacy `[sources].ics_urls`. Prefer `[sources.calendar].ics_urls`.
    pub ics_urls: Vec<String>,
    pub calendar: Option<CalendarSourceConfig>,
    pub bins: BinsSourceConfig,
    pub birthdays: Option<BirthdaysSourceConfig>,
    pub todoist: Option<TodoistConfig>,
    pub meross: Option<MerossConfig>,
    pub weather: Option<WeatherConfig>,
    pub tfl: TflConfig,
    pub jokes: ToggleConfig,
    pub history: ToggleConfig,
    pub saints: ToggleConfig,
    pub pronote: Option<PronoteConfig>,
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
            pico_overhead_secs: 0.0,
            battery_mah: DEFAULT_CAPACITY_MAH,
            battery_empty_mv: DEFAULT_EMPTY_MV,
            chrome_path: String::new(),
            todoist: TodoistConfig::default(),
            meross: MerossConfig::default(),
            weather: WeatherConfig::default(),
            pronote: PronoteConfig::default(),
            sources: SourcesConfig::default(),
            pictures: PicturesConfig::default(),
            birthdays: Vec::new(),
            config_dir: PathBuf::from("."),
            config_path: None,
            fake_private: false,
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
            location_id: String::new(),
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
            show_sections: false,
        }
    }
}

impl Default for CalendarSourceConfig {
    fn default() -> Self {
        Self {
            ics_urls: Vec::new(),
        }
    }
}

impl Default for BinsSourceConfig {
    fn default() -> Self {
        Self {
            uprn: String::new(),
        }
    }
}

impl Default for BirthdaysSourceConfig {
    fn default() -> Self {
        Self { people: Vec::new() }
    }
}

impl Default for TflConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            lines: default_tfl_lines(),
        }
    }
}

impl Default for ToggleConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

impl Default for SourcesConfig {
    fn default() -> Self {
        Self {
            ics_urls: Vec::new(),
            calendar: None,
            bins: BinsSourceConfig::default(),
            birthdays: None,
            todoist: None,
            meross: None,
            weather: None,
            tfl: TflConfig::default(),
            jokes: ToggleConfig::default(),
            history: ToggleConfig::default(),
            saints: ToggleConfig::default(),
            pronote: None,
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

/// Birthday as shown and edited in the family UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicBirthday {
    pub name: String,
    pub dob: String,
}

impl PublicBirthday {
    pub fn from_birthday(person: &Birthday) -> Self {
        Self {
            name: person.name.clone(),
            dob: person.dob.format("%Y-%m-%d").to_string(),
        }
    }

    pub fn into_birthday(&self) -> Result<Birthday, String> {
        Birthday::parse(&format!("{},{}", self.name.trim(), self.dob.trim()))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PublicCalendar {
    pub ics_urls: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PublicBins {
    pub uprn: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PublicTodoist {
    pub token: String,
    pub project: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PublicWeather {
    pub location_id: String,
}

/// Public settings exposed to the family UI.
///
/// Trusted LAN only — the Setup page edits household secrets (Todoist token)
/// so they are included here. There is no auth.
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
    pub battery_mah: u32,
    pub calendar: PublicCalendar,
    pub bins: PublicBins,
    pub birthdays: Vec<PublicBirthday>,
    pub todoist: PublicTodoist,
    pub weather: PublicWeather,
    /// Wall-clock seconds until the Pico's last commanded wake. `None` if it
    /// has never been given a sleep — the current editor schedule is not used,
    /// because the frame only learns that on its next poll.
    pub next_sleep_secs: Option<u64>,
    /// Auto-measured Pico timer error (fraction). See [`Config::pico_drift`].
    pub pico_drift: f64,
    /// Auto-measured fixed wake overhead in seconds. See [`Config::pico_overhead_secs`].
    pub pico_overhead_secs: f64,
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
    pub family_name: Option<String>,
    pub timezone: Option<String>,
    pub battery_mah: Option<u32>,
    pub calendar: Option<CalendarPatch>,
    pub bins: Option<BinsPatch>,
    pub birthdays: Option<Vec<PublicBirthday>>,
    pub todoist: Option<TodoistPatch>,
    pub weather: Option<WeatherPatch>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CalendarPatch {
    pub ics_urls: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct BinsPatch {
    pub uprn: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TodoistPatch {
    pub token: Option<String>,
    pub project: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WeatherPatch {
    pub location_id: Option<String>,
}

impl SettingsPatch {
    /// Household fields that change what the dashboard paints.
    pub fn touches_household(&self) -> bool {
        self.family_name.is_some()
            || self.timezone.is_some()
            || self.calendar.is_some()
            || self.bins.is_some()
            || self.birthdays.is_some()
            || self.todoist.is_some()
            || self.weather.is_some()
    }
}

impl Config {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading config {}", path.display()))?;
        let mut cfg: Config = toml::from_str(&text).context("parsing config.toml")?;
        cfg.resolve_source_aliases();
        cfg.config_dir = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        cfg.config_path = Some(path.to_path_buf());
        cfg.pico_drift = crate::schedule::clamp_pico_drift(cfg.pico_drift);
        cfg.pico_overhead_secs = crate::schedule::clamp_pico_overhead(cfg.pico_overhead_secs);
        cfg.battery_mah = cfg.battery_mah.max(1);
        cfg.battery_empty_mv = cfg.battery_empty_mv.clamp(2500, 4000);
        cfg.materialize_pictures_schedule();
        Ok(cfg)
    }

    /// Copy `[sources.<id>]` over legacy top-level keys when the new table is set.
    /// An explicit table wins even when it is empty, so the family UI can clear a source.
    fn resolve_source_aliases(&mut self) {
        if let Some(cal) = &self.sources.calendar {
            self.sources.ics_urls = cal.ics_urls.clone();
        }
        if let Some(b) = &self.sources.birthdays {
            self.birthdays = b.people.clone();
        }
        if let Some(todoist) = self.sources.todoist.clone() {
            self.todoist = todoist;
        }
        if let Some(meross) = self.sources.meross.clone() {
            self.meross = meross;
        }
        if let Some(weather) = self.sources.weather.clone() {
            self.weather = weather;
        }
        if let Some(pronote) = self.sources.pronote.clone() {
            self.pronote = pronote;
        }
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
        self.public_settings_for_assigned_wake(now, None)
    }

    /// Like [`Self::public_settings`], with the wake last told to the Pico.
    pub fn public_settings_for_assigned_wake(
        &self,
        now: DateTime<Utc>,
        assigned_wake: Option<DateTime<Utc>>,
    ) -> PublicSettings {
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
            battery_mah: self.battery_mah,
            calendar: PublicCalendar {
                ics_urls: self.sources.ics_urls.clone(),
            },
            bins: PublicBins {
                uprn: self.sources.bins.uprn.clone(),
            },
            birthdays: self
                .birthdays
                .iter()
                .map(PublicBirthday::from_birthday)
                .collect(),
            todoist: PublicTodoist {
                token: self.todoist.token.clone(),
                project: self.todoist.project.clone(),
            },
            weather: PublicWeather {
                location_id: self.weather.location_id.clone(),
            },
            next_sleep_secs: assigned_wake.map(|at| {
                u64::try_from(at.signed_duration_since(now).num_seconds().max(0)).unwrap_or(0)
            }),
            pico_drift: self.pico_drift,
            pico_overhead_secs: self.pico_overhead_secs,
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
        self.apply_household_patch(&patch)?;
        self.persist_editable()?;
        Ok(())
    }

    fn apply_household_patch(&mut self, patch: &SettingsPatch) -> Result<()> {
        if let Some(name) = patch.family_name.as_deref() {
            let name = name.trim();
            if name.is_empty() {
                bail!("family_name must not be empty");
            }
            self.family_name = name.to_string();
        }
        if let Some(tz) = patch.timezone.as_deref() {
            let tz = tz.trim();
            if tz.parse::<Tz>().is_err() {
                bail!("unknown timezone `{tz}` (use an IANA name like Europe/London)");
            }
            self.timezone = tz.to_string();
        }
        if let Some(mah) = patch.battery_mah {
            self.battery_mah = mah.max(1);
        }
        if let Some(calendar) = &patch.calendar {
            if let Some(urls) = &calendar.ics_urls {
                let urls = normalize_ics_urls(urls)?;
                self.sources.ics_urls = urls.clone();
                self.sources.calendar = Some(CalendarSourceConfig { ics_urls: urls });
            }
        }
        if let Some(bins) = &patch.bins {
            if let Some(uprn) = &bins.uprn {
                self.sources.bins.uprn = normalize_uprn(uprn)?;
            }
        }
        if let Some(people) = &patch.birthdays {
            let people = people
                .iter()
                .map(PublicBirthday::into_birthday)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|err| anyhow::anyhow!(err))?;
            self.birthdays = people.clone();
            self.sources.birthdays = Some(BirthdaysSourceConfig { people });
        }
        if let Some(todoist) = &patch.todoist {
            if let Some(token) = &todoist.token {
                self.todoist.token = token.trim().to_string();
            }
            if let Some(project) = &todoist.project {
                let project = project.trim();
                self.todoist.project = if project.is_empty() {
                    "Family".into()
                } else {
                    project.to_string()
                };
            }
            self.sources.todoist = Some(self.todoist.clone());
        }
        if let Some(weather) = &patch.weather {
            if let Some(id) = &weather.location_id {
                self.weather.location_id = normalize_weather_location_id(id);
            }
            self.sources.weather = Some(self.weather.clone());
        }
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

    /// Write family-UI keys (mode, schedule, pictures, household sources).
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

        doc["family_name"] = Item::Value(Value::from(self.family_name.as_str()));
        doc["timezone"] = Item::Value(Value::from(self.timezone.as_str()));
        doc["battery_mah"] = Item::Value(Value::from(i64::from(self.battery_mah)));
        let dash = self.mode_schedule(FrameMode::Dashboard);
        doc["mode"] = Item::Value(Value::from(self.mode.as_str()));
        doc["poll_interval_secs"] = Item::Value(Value::from(dash.interval_secs as i64));
        write_wake_key(doc.as_table_mut(), "wake-up", dash.wake_up);
        doc["schedule_kind"] = Item::Value(Value::from(dash.kind.as_str()));
        write_pico_timing(&mut doc, self.pico_drift, self.pico_overhead_secs);
        persist_household_sources(&mut doc, self);

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

    pub fn bins_enabled(&self) -> bool {
        !self.sources.bins.uprn.trim().is_empty()
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
        crate::schedule::secs_until(now, self.next_poll_at(now))
    }

    /// Next schedule instant (clock `HH:MM` in `timezone`, or `now + interval`).
    pub fn next_poll_at(&self, now: DateTime<Utc>) -> DateTime<Utc> {
        self.refresh_window(now, None).1
    }

    /// Last/next instants painted on the dashboard (slot-linked for a timer poll).
    pub fn refresh_window(
        &self,
        now: DateTime<Utc>,
        assigned_wake: Option<DateTime<Utc>>,
    ) -> (DateTime<Utc>, DateTime<Utc>) {
        let (interval, wakes) = self.schedule(self.effective_mode());
        crate::schedule::refresh_window(now, self.tz(), interval, wakes, assigned_wake)
    }

    /// Seconds the Pico should POWMAN-sleep after this poll, shortened if its
    /// low-power oscillator runs slow.
    pub fn pico_sleep_secs(&self, now: DateTime<Utc>) -> u64 {
        crate::schedule::compensate_sleep_secs(
            self.next_poll_secs(now),
            self.pico_drift,
            self.pico_overhead_secs,
        )
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
        let wake_at =
            crate::schedule::next_poll_at_for_timer(now, self.tz(), interval, wakes, assigned_wake);
        let wall = crate::schedule::secs_until(now, wake_at);
        let sleep_s =
            crate::schedule::compensate_sleep_secs(wall, self.pico_drift, self.pico_overhead_secs);
        (sleep_s, wake_at)
    }

    /// Like [`Self::pico_sleep_secs`], but a timer poll uses the stored slot.
    pub fn pico_sleep_secs_for_timer(
        &self,
        now: DateTime<Utc>,
        assigned_wake: Option<DateTime<Utc>>,
    ) -> u64 {
        self.pico_sleep_plan(now, assigned_wake).0
    }

    /// Blend a timer-poll fit into stored drift and overhead and persist them.
    /// Returns whether the stored values changed.
    pub fn record_pico_timing(&mut self, measured: crate::schedule::PicoTiming) -> Result<bool> {
        let next = crate::schedule::blend_pico_timing(self.pico_timing(), measured);
        let drift_same = (self.pico_drift - next.drift).abs() < 5e-5;
        let overhead_same = (self.pico_overhead_secs - next.overhead_secs).abs() < 0.5;
        if drift_same && overhead_same {
            return Ok(false);
        }
        self.pico_drift = next.drift;
        self.pico_overhead_secs = next.overhead_secs;
        self.persist_pico_timing()?;
        Ok(true)
    }

    pub fn pico_timing(&self) -> crate::schedule::PicoTiming {
        crate::schedule::PicoTiming {
            drift: self.pico_drift,
            overhead_secs: self.pico_overhead_secs,
        }
    }

    /// Blend a timer-poll drift sample into [`Self::pico_drift`] and persist it.
    /// Returns whether the stored value changed.
    pub fn record_pico_drift(&mut self, measured: f64) -> Result<bool> {
        self.record_pico_timing(crate::schedule::PicoTiming {
            drift: measured,
            overhead_secs: self.pico_overhead_secs,
        })
    }

    fn persist_pico_timing(&self) -> Result<()> {
        let Some(path) = self.config_path.as_ref() else {
            return Ok(());
        };
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let mut doc: DocumentMut = text
            .parse()
            .with_context(|| format!("parsing {} for edit", path.display()))?;
        write_pico_timing(&mut doc, self.pico_drift, self.pico_overhead_secs);
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

fn write_pico_timing(doc: &mut DocumentMut, drift: f64, overhead_secs: f64) {
    doc["pico_drift"] = Item::Value(Value::from(crate::schedule::round_pico_drift(drift)));
    doc["pico_overhead_secs"] = Item::Value(Value::from(crate::schedule::round_pico_overhead(
        overhead_secs,
    )));
}

fn persist_household_sources(doc: &mut DocumentMut, cfg: &Config) {
    let root = doc.as_table_mut();
    root.remove("birthdays");
    root.remove("todoist");
    root.remove("weather");

    if !root.contains_key("sources") {
        root["sources"] = Item::Table(toml_edit::Table::new());
    }
    let Some(sources) = root.get_mut("sources").and_then(Item::as_table_mut) else {
        return;
    };
    sources.remove("ics_urls");

    let people: Vec<String> = cfg.birthdays.iter().map(Birthday::to_entry).collect();
    write_source_value(
        sources,
        "calendar",
        "ics_urls",
        Value::Array(toml_string_array(&cfg.sources.ics_urls)),
    );
    write_source_value(
        sources,
        "birthdays",
        "people",
        Value::Array(toml_string_array(&people)),
    );
    write_source_value(
        sources,
        "todoist",
        "token",
        Value::from(cfg.todoist.token.as_str()),
    );
    write_source_value(
        sources,
        "todoist",
        "project",
        Value::from(cfg.todoist.project.as_str()),
    );
    write_source_value(
        sources,
        "weather",
        "location_id",
        Value::from(cfg.weather.location_id.as_str()),
    );
    write_source_value(
        sources,
        "bins",
        "uprn",
        Value::from(cfg.sources.bins.uprn.as_str()),
    );
}

fn write_source_value(sources: &mut toml_edit::Table, table: &str, key: &str, value: Value) {
    if sources.get(table).and_then(Item::as_table).is_none() {
        sources[table] = Item::Table(toml_edit::Table::new());
    }
    sources[table][key] = Item::Value(value);
}

fn toml_string_array(items: &[String]) -> Array {
    let mut arr = Array::new();
    if items.is_empty() {
        return arr;
    }
    if items.len() == 1 && items[0].len() < 72 {
        arr.push(items[0].as_str());
        return arr;
    }
    arr.set_trailing_comma(true);
    arr.set_trailing("\n");
    for item in items {
        let mut value = Value::from(item.as_str());
        value.decor_mut().set_prefix("\n    ");
        arr.push_formatted(value);
    }
    arr
}

fn normalize_ics_urls(urls: &[String]) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for url in urls {
        let url = url.trim();
        if url.is_empty() {
            continue;
        }
        let Some((scheme, _)) = url.split_once("://") else {
            bail!("calendar URL should start with webcal:// or https://");
        };
        match scheme.to_ascii_lowercase().as_str() {
            "http" | "https" | "webcal" | "webcals" => {}
            _ => bail!("calendar URL should start with webcal:// or https://"),
        }
        if !out.iter().any(|existing| existing == url) {
            out.push(url.to_string());
        }
    }
    Ok(out)
}

/// Digits, or a Wandsworth My Property URL containing `UPRN=`.
pub fn normalize_uprn(raw: &str) -> Result<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(String::new());
    }
    if let Some(idx) = raw.to_ascii_uppercase().find("UPRN=") {
        let digits: String = raw[idx + 5..]
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if !digits.is_empty() {
            return Ok(digits);
        }
    }
    let digits: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
    if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
        return Ok(digits);
    }
    bail!("UPRN should be digits, or a Wandsworth My Property URL");
}

fn normalize_weather_location_id(raw: &str) -> String {
    let raw = raw.trim();
    let lowered = raw.to_ascii_lowercase();
    const PREFIXES: &[&str] = &[
        "https://www.bbc.co.uk/weather/",
        "http://www.bbc.co.uk/weather/",
        "https://bbc.co.uk/weather/",
        "http://bbc.co.uk/weather/",
        "www.bbc.co.uk/weather/",
        "bbc.co.uk/weather/",
    ];
    for prefix in PREFIXES {
        if lowered.starts_with(prefix) {
            let rest = &raw[prefix.len()..];
            return rest
                .split(['/', '?', '#'])
                .next()
                .unwrap_or(rest)
                .trim()
                .to_string();
        }
    }
    raw.to_string()
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

fn default_true() -> bool {
    true
}

pub fn default_tfl_lines() -> Vec<TflLineConfig> {
    vec![
        TflLineConfig {
            id: "northern".into(),
            name: "Northern".into(),
            colour: "black".into(),
        },
        TflLineConfig {
            id: "circle".into(),
            name: "Circle".into(),
            colour: "yellow".into(),
        },
        TflLineConfig {
            id: "district".into(),
            name: "District".into(),
            colour: "green".into(),
        },
        TflLineConfig {
            id: "victoria".into(),
            name: "Victoria".into(),
            colour: "blue".into(),
        },
    ]
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
        assert!(!cfg.pronote.show_sections);
        assert!(cfg.sources.tfl.enabled);
        assert_eq!(cfg.sources.tfl.lines.len(), 4);
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
    fn weather_default_location_is_empty() {
        let cfg = Config::default();
        assert!(cfg.weather.location_id.is_empty());
        assert!(!cfg.weather_enabled());
    }

    #[test]
    fn leftover_icloud_table_is_ignored() {
        let cfg: Config = toml::from_str(
            r#"
            [icloud]
            apple_id = "x@icloud.com"
            app_password = "secret"
            "#,
        )
        .unwrap();
        assert!(!cfg.weather_enabled());
    }

    #[test]
    fn sources_tables_alias_legacy_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
[sources.calendar]
ics_urls = ["https://example.com/family.ics"]

[sources.birthdays]
people = ["Maya,2018-03-15"]

[sources.todoist]
token = "tok"
project = "Chores"

[sources.weather]
location_id = "2643743"

[sources.pronote]
url = "https://example.com/pronote/eleve.html"
username = "a"
password = "b"
show_sections = true
"#,
        )
        .unwrap();
        let cfg = Config::load(&path).unwrap();
        assert_eq!(cfg.sources.ics_urls, ["https://example.com/family.ics"]);
        assert_eq!(cfg.birthdays.len(), 1);
        assert_eq!(cfg.birthdays[0].name, "Maya");
        assert_eq!(cfg.todoist.token, "tok");
        assert_eq!(cfg.todoist.project, "Chores");
        assert!(cfg.todoist_enabled());
        assert_eq!(cfg.weather.location_id, "2643743");
        assert!(cfg.pronote.show_sections);
        assert!(cfg.pronote_enabled());
    }

    #[test]
    fn legacy_todoist_and_birthdays_still_parse() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
birthdays = ["Sam,2015-11-02"]
[todoist]
token = "legacy"
project = "Family"
[sources]
ics_urls = ["https://example.com/old.ics"]
"#,
        )
        .unwrap();
        let cfg = Config::load(&path).unwrap();
        assert_eq!(cfg.todoist.token, "legacy");
        assert_eq!(cfg.birthdays[0].name, "Sam");
        assert_eq!(cfg.sources.ics_urls, ["https://example.com/old.ics"]);
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
        assert_eq!(cfg.refresh_window(now, Some(intended)), (now, wake_at));
        assert_eq!(cfg.refresh_window(now, None), (now, intended));
    }

    #[test]
    fn pico_sleep_plan_stores_the_clock_slot_not_now_plus_secs() {
        use chrono::{Duration, TimeZone};
        let cfg: Config = toml::from_str(
            r#"
            timezone = "Europe/London"
            wake-up = ["18:00", "21:00"]
            "#,
        )
        .unwrap();
        let now = chrono_tz::Europe::London
            .with_ymd_and_hms(2026, 9, 16, 16, 0, 3)
            .unwrap()
            .with_timezone(&Utc)
            + Duration::milliseconds(475);
        let (_, wake_at) = cfg.pico_sleep_plan(now, None);
        assert_eq!(
            wake_at,
            chrono_tz::Europe::London
                .with_ymd_and_hms(2026, 9, 16, 18, 0, 0)
                .unwrap()
                .with_timezone(&Utc)
        );
        let early = chrono_tz::Europe::London
            .with_ymd_and_hms(2026, 9, 16, 17, 59, 40)
            .unwrap()
            .with_timezone(&Utc);
        let slot = chrono_tz::Europe::London
            .with_ymd_and_hms(2026, 9, 16, 18, 0, 0)
            .unwrap()
            .with_timezone(&Utc);
        let (sleep_s, next) = cfg.pico_sleep_plan(early, Some(slot));
        assert_eq!(
            next,
            chrono_tz::Europe::London
                .with_ymd_and_hms(2026, 9, 16, 21, 0, 0)
                .unwrap()
                .with_timezone(&Utc)
        );
        assert_eq!(sleep_s, 3 * 3600 + 20);
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
            schedule_for: Some("picture".into()),
            ..Default::default()
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
        assert_eq!(public.pico_overhead_secs, 0.0);
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
        assert_eq!(public.next_sleep_secs, None);
        assert_eq!(public.pico_drift, 0.03);
        assert_eq!(public.pico_overhead_secs, 0.0);
        let assigned = now + chrono::Duration::seconds(3600);
        let public = cfg.public_settings_for_assigned_wake(now, Some(assigned));
        assert_eq!(public.next_sleep_secs, Some(3600));
    }

    #[test]
    fn public_next_sleep_ignores_the_editor_schedule() {
        use chrono::TimeZone;
        let cfg: Config = toml::from_str(
            r#"
            timezone = "Europe/London"
            poll_interval_secs = 300
            schedule_kind = "interval"
            "#,
        )
        .unwrap();
        let now = chrono_tz::Europe::London
            .with_ymd_and_hms(2026, 9, 16, 12, 0, 0)
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(cfg.next_poll_secs(now), 300);
        let assigned = now + chrono::Duration::seconds(7200);
        let public = cfg.public_settings_for_assigned_wake(now, Some(assigned));
        assert_eq!(public.next_sleep_secs, Some(7200));
        let overdue =
            cfg.public_settings_for_assigned_wake(now, Some(now - chrono::Duration::seconds(90)));
        assert_eq!(overdue.next_sleep_secs, Some(0));
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
    fn pico_overhead_over_max_is_clamped_on_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "pico_overhead_secs = 500\n").unwrap();
        let cfg = Config::load(&path).unwrap();
        assert_eq!(cfg.pico_overhead_secs, 180.0);
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

    #[test]
    fn sources_birthdays_table_wins_even_when_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
birthdays = ["Sam,2015-11-02"]
[sources.birthdays]
people = []
"#,
        )
        .unwrap();
        let cfg = Config::load(&path).unwrap();
        assert!(cfg.birthdays.is_empty());
    }

    #[test]
    fn sources_calendar_table_wins_even_when_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
[sources]
ics_urls = ["https://example.com/old.ics"]
[sources.calendar]
ics_urls = []
"#,
        )
        .unwrap();
        let cfg = Config::load(&path).unwrap();
        assert!(cfg.sources.ics_urls.is_empty());
    }

    #[test]
    fn patch_household_settings_persists_and_reloads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"# keep me
family_name = "Family"
timezone = "Europe/London"

[meross]
email = "secret@example.com"
password = "hunter2"
"#,
        )
        .unwrap();
        let mut cfg = Config::load(&path).unwrap();
        cfg.apply_patch(SettingsPatch {
            family_name: Some("Famille Test".into()),
            timezone: Some("Europe/Paris".into()),
            battery_mah: Some(5000),
            calendar: Some(CalendarPatch {
                ics_urls: Some(vec![
                    "webcal://calendar.example.com/family.ics".into(),
                    "https://example.com/school.ics".into(),
                    "".into(),
                ]),
            }),
            birthdays: Some(vec![
                PublicBirthday {
                    name: "Maya".into(),
                    dob: "2018-03-15".into(),
                },
                PublicBirthday {
                    name: "Sam".into(),
                    dob: "2015-11-02".into(),
                },
            ]),
            todoist: Some(TodoistPatch {
                token: Some("tok_123".into()),
                project: Some("Chores".into()),
            }),
            weather: Some(WeatherPatch {
                location_id: Some("https://www.bbc.co.uk/weather/2643743?day=1".into()),
            }),
            bins: Some(BinsPatch {
                uprn: Some(
                    "https://www.wandsworth.gov.uk/my-property/?UPRN=100022658374&propertyidentified=Select".into(),
                ),
            }),
            ..Default::default()
        })
        .unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# keep me"));
        assert!(text.contains("secret@example.com"));
        assert!(text.contains("hunter2"));
        assert!(text.contains("Famille Test"));
        assert!(text.contains("Europe/Paris"));
        assert!(text.contains("5000"));
        assert!(text.contains("[sources.calendar]"));
        assert!(text.contains("[sources.birthdays]"));
        assert!(text.contains("[sources.todoist]"));
        assert!(text.contains("[sources.weather]"));
        assert!(text.contains("[sources.bins]"));
        assert!(text.contains("tok_123"));
        assert!(text.contains("2643743"));
        assert!(text.contains("100022658374"));
        assert!(!text.contains("propertyidentified"));
        assert!(!text.contains("?day=1"));
        assert!(!text.contains("birthdays = ["));

        let reloaded = Config::load(&path).unwrap();
        assert_eq!(reloaded.family_name, "Famille Test");
        assert_eq!(reloaded.timezone, "Europe/Paris");
        assert_eq!(reloaded.battery_mah, 5000);
        assert_eq!(
            reloaded.sources.ics_urls,
            [
                "webcal://calendar.example.com/family.ics",
                "https://example.com/school.ics"
            ]
        );
        assert_eq!(reloaded.birthdays.len(), 2);
        assert_eq!(reloaded.birthdays[0].name, "Maya");
        assert_eq!(reloaded.todoist.token, "tok_123");
        assert_eq!(reloaded.todoist.project, "Chores");
        assert_eq!(reloaded.weather.location_id, "2643743");
        assert_eq!(reloaded.sources.bins.uprn, "100022658374");
        assert_eq!(reloaded.meross.password, "hunter2");

        let public = reloaded.public_settings(Utc::now());
        assert_eq!(public.family_name, "Famille Test");
        assert_eq!(public.battery_mah, 5000);
        assert_eq!(public.calendar.ics_urls.len(), 2);
        assert_eq!(public.birthdays[0].dob, "2018-03-15");
        assert_eq!(public.todoist.token, "tok_123");
        assert_eq!(public.weather.location_id, "2643743");
        assert_eq!(public.bins.uprn, "100022658374");
    }

    #[test]
    fn patch_rejects_unknown_timezone_and_bad_calendar_url() {
        let mut cfg = Config::default();
        cfg.config_path = None;
        let tz_err = cfg
            .apply_patch(SettingsPatch {
                timezone: Some("London".into()),
                ..Default::default()
            })
            .unwrap_err();
        assert!(tz_err.to_string().contains("timezone"));

        let url_err = cfg
            .apply_patch(SettingsPatch {
                calendar: Some(CalendarPatch {
                    ics_urls: Some(vec!["ftp://example.com/cal.ics".into()]),
                }),
                ..Default::default()
            })
            .unwrap_err();
        assert!(url_err.to_string().contains("webcal"));
    }

    #[test]
    fn normalize_uprn_from_digits_or_url() {
        assert_eq!(normalize_uprn("100022658374").unwrap(), "100022658374");
        assert_eq!(normalize_uprn(" 100022658374 ").unwrap(), "100022658374");
        assert_eq!(
            normalize_uprn(
                "https://www.wandsworth.gov.uk/my-property/?UPRN=100022658374&propertyidentified=Select"
            )
            .unwrap(),
            "100022658374"
        );
        assert!(normalize_uprn("").unwrap().is_empty());
        assert!(normalize_uprn("SW11 5QA").is_err());
    }

    #[test]
    fn normalize_weather_id_from_bbc_url() {
        assert_eq!(
            normalize_weather_location_id("https://www.bbc.co.uk/weather/2643743"),
            "2643743"
        );
        assert_eq!(
            normalize_weather_location_id("www.bbc.co.uk/weather/10102218#day2"),
            "10102218"
        );
        assert_eq!(normalize_weather_location_id(" 2652951 "), "2652951");
    }
}
