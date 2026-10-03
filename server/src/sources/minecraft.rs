//! Minecraft allowance: minutes earned during the week, played at the weekend.
//!
//! Household timezone. **Week** is Monday 00:00 to Friday 12:00; changes
//! land on the weekend that opens that Friday. **Weekend** is Friday 12:00
//! to Monday 00:00; that weekend's total is locked and changes land on the
//! following Friday. Each entry stores the Friday it belongs to, so the
//! Monday rollover needs no reset.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, Timelike, Utc, Weekday};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use super::context::SourceContext;
use super::contribute::{Contribution, SourceOutcome};
use super::DataSource;
use crate::config::Config;
use crate::model::MinecraftView;

pub const LEDGER_FILE: &str = "minecraft-allowance.json";
/// Guards against a typo (`150` for `15`) from a Shortcut number prompt.
pub const MAX_STEP_MINUTES: i32 = 600;

static WRITE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Week,
    Weekend,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clock {
    pub phase: Phase,
    /// Friday shown large: the coming weekend (week) or the current one (weekend).
    pub headline: NaiveDate,
}

impl Clock {
    pub fn at_local(local: NaiveDateTime) -> Self {
        let date = local.date();
        let monday = date - Duration::days(i64::from(date.weekday().num_days_from_monday()));
        let friday = monday + Duration::days(4);
        let phase = match date.weekday() {
            Weekday::Sat | Weekday::Sun => Phase::Weekend,
            Weekday::Fri if local.hour() >= 12 => Phase::Weekend,
            _ => Phase::Week,
        };
        Self {
            phase,
            headline: friday,
        }
    }

    pub fn at(now: DateTime<Utc>, tz: Tz) -> Self {
        Self::at_local(now.with_timezone(&tz).naive_local())
    }

    /// Weekend that new changes count toward.
    pub fn target(&self) -> NaiveDate {
        match self.phase {
            Phase::Week => self.headline,
            Phase::Weekend => self.headline + Duration::days(7),
        }
    }

    pub fn target_label(&self) -> &'static str {
        match self.phase {
            Phase::Week => "Coming weekend",
            Phase::Weekend => "Next weekend",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Entry {
    pub at: DateTime<Utc>,
    pub minutes: i32,
    /// Friday that opens the weekend these minutes belong to.
    pub weekend: NaiveDate,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Ledger {
    #[serde(default)]
    pub entries: Vec<Entry>,
}

impl Ledger {
    pub fn balance(&self, weekend: NaiveDate) -> i32 {
        self.entries
            .iter()
            .filter(|e| e.weekend == weekend)
            .map(|e| e.minutes)
            .sum::<i32>()
            .max(0)
    }

    /// Record `minutes` against the clock's target weekend. A removal never
    /// takes the balance below zero; returns the delta actually stored.
    pub fn add(&mut self, at: DateTime<Utc>, clock: Clock, minutes: i32) -> i32 {
        let weekend = clock.target();
        let applied = minutes.max(-self.balance(weekend));
        if applied != 0 {
            self.entries.push(Entry {
                at,
                minutes: applied,
                weekend,
            });
        }
        applied
    }

    pub fn view(&self, clock: Clock) -> MinecraftView {
        let minutes = self.balance(clock.headline);
        let (phase, next_minutes) = match clock.phase {
            Phase::Week => ("week", 0),
            Phase::Weekend => ("weekend", self.balance(clock.target())),
        };
        MinecraftView {
            phase: phase.into(),
            minutes,
            time: format_minutes(minutes),
            next_minutes,
            next_time: if clock.phase == Phase::Weekend {
                format_minutes(next_minutes)
            } else {
                String::new()
            },
        }
    }
}

/// `0m`, `45m`, `2h`, `2h 15`.
pub fn format_minutes(minutes: i32) -> String {
    let m = minutes.max(0);
    match (m / 60, m % 60) {
        (0, mm) => format!("{mm}m"),
        (h, 0) => format!("{h}h"),
        (h, mm) => format!("{h}h {mm:02}"),
    }
}

pub fn ledger_path(config_dir: &Path) -> PathBuf {
    config_dir.join(LEDGER_FILE)
}

pub fn load(config_dir: &Path) -> Result<Ledger> {
    let path = ledger_path(config_dir);
    if !path.exists() {
        return Ok(Ledger::default());
    }
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

fn save(config_dir: &Path, ledger: &Ledger) -> Result<()> {
    let path = ledger_path(config_dir);
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(dir)
        .with_context(|| format!("creating temp file in {}", dir.display()))?;
    tmp.write_all(serde_json::to_string_pretty(ledger)?.as_bytes())?;
    tmp.persist(&path)
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct Adjusted {
    pub requested: i32,
    pub applied: i32,
    pub weekend: NaiveDate,
    pub summary: String,
    pub report: Report,
}

pub fn validate_step(minutes: i32) -> Result<()> {
    if minutes == 0 {
        bail!("minutes must be non-zero");
    }
    if minutes.abs() > MAX_STEP_MINUTES {
        bail!("minutes must be between -{MAX_STEP_MINUTES} and {MAX_STEP_MINUTES}");
    }
    Ok(())
}

/// Whole minutes from a query value or body: `15`, `-10`, ` 15` (a `+`
/// decoded as a space), `15.0` (Shortcuts numbers).
pub fn parse_minutes(raw: &str) -> Result<i32> {
    let s = raw.trim().trim_start_matches('+');
    if let Ok(n) = s.parse::<i32>() {
        return Ok(n);
    }
    match s.parse::<f64>() {
        Ok(f) if f.fract() == 0.0 && f.abs() <= f64::from(i32::MAX) => Ok(f as i32),
        _ => bail!("minutes must be a whole number, got `{}`", raw.trim()),
    }
}

/// `{"minutes": 15}`, `{"minutes": "15"}`, `15`, or `minutes=15`.
pub fn minutes_from_body(body: &[u8]) -> Option<Result<i32>> {
    let text = std::str::from_utf8(body).ok()?.trim();
    if text.is_empty() {
        return None;
    }
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(text) {
        let field = match &value {
            serde_json::Value::Object(map) => map.get("minutes")?.clone(),
            other => other.clone(),
        };
        return match field {
            serde_json::Value::Number(n) => Some(parse_minutes(&n.to_string())),
            serde_json::Value::String(s) => Some(parse_minutes(&s)),
            _ => Some(Err(anyhow::anyhow!("minutes must be a number"))),
        };
    }
    if let Some(raw) = text
        .split('&')
        .find_map(|pair| pair.strip_prefix("minutes="))
    {
        return Some(parse_minutes(&raw.replace('+', " ")));
    }
    Some(parse_minutes(text))
}

pub fn adjust(config_dir: &Path, tz: Tz, now: DateTime<Utc>, minutes: i32) -> Result<Adjusted> {
    validate_step(minutes)?;
    let _guard = WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut ledger = load(config_dir)?;
    let clock = Clock::at(now, tz);
    let applied = ledger.add(now, clock, minutes);
    if applied != 0 {
        save(config_dir, &ledger)?;
    }
    let target_time = format_minutes(ledger.balance(clock.target()));
    let lead = if applied == 0 {
        "Nothing left to remove.".to_string()
    } else if applied != minutes {
        format!("{} min (that was all that was left).", signed(applied))
    } else {
        format!("{} min.", signed(applied))
    };
    Ok(Adjusted {
        requested: minutes,
        applied,
        weekend: clock.target(),
        summary: format!("{lead} {}: {target_time}", clock.target_label()),
        report: report(&ledger, clock, tz),
    })
}

fn signed(minutes: i32) -> String {
    if minutes > 0 {
        format!("+{minutes}")
    } else {
        minutes.to_string()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct WeekendTotal {
    pub weekend: NaiveDate,
    pub label: String,
    pub minutes: i32,
    pub time: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WeekendStats {
    pub weekend: NaiveDate,
    pub label: String,
    pub minutes: i32,
    pub time: String,
    pub added: i32,
    pub removed: i32,
    pub changes: usize,
    /// `past`, `now` (locked, being played), `coming`, or `next`.
    pub status: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct Totals {
    pub added: i32,
    pub removed: i32,
    pub changes: usize,
    /// Weekends already locked (played or being played) that had any change.
    pub locked_weekends: usize,
    pub average_minutes: i32,
    pub average_time: String,
    pub best: Option<WeekendTotal>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WeekdayStats {
    pub day: &'static str,
    pub added: i32,
    pub removed: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct EntryView {
    pub at: DateTime<Utc>,
    pub local: String,
    /// Household wall-clock time, `YYYY-MM-DDTHH:MM`, for charting.
    pub local_iso: String,
    pub minutes: i32,
    pub weekend: NaiveDate,
    pub weekend_label: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub phase: Phase,
    pub headline: WeekendTotal,
    /// Weekend phase only: minutes already banked for the following weekend.
    pub next: Option<WeekendTotal>,
    pub target: NaiveDate,
    pub target_label: &'static str,
    pub target_minutes: i32,
    pub summary: String,
    pub weekends: Vec<WeekendStats>,
    pub totals: Totals,
    pub by_weekday: Vec<WeekdayStats>,
    /// Newest first.
    pub entries: Vec<EntryView>,
    pub max_step_minutes: i32,
}

fn weekend_label(friday: NaiveDate) -> String {
    friday.format("%a %-d %b").to_string()
}

fn total(ledger: &Ledger, weekend: NaiveDate) -> WeekendTotal {
    let minutes = ledger.balance(weekend);
    WeekendTotal {
        weekend,
        label: weekend_label(weekend),
        minutes,
        time: format_minutes(minutes),
    }
}

pub fn report(ledger: &Ledger, clock: Clock, tz: Tz) -> Report {
    let headline = total(ledger, clock.headline);
    let next = (clock.phase == Phase::Weekend).then(|| total(ledger, clock.target()));
    let summary = match &next {
        None => format!("Coming weekend: {}", headline.time),
        Some(n) => format!("This weekend: {}. Next weekend: {}", headline.time, n.time),
    };

    let mut fridays: Vec<NaiveDate> = ledger.entries.iter().map(|e| e.weekend).collect();
    fridays.push(clock.headline);
    fridays.push(clock.target());
    fridays.sort_unstable();
    fridays.dedup();
    let weekends: Vec<WeekendStats> = fridays
        .iter()
        .rev()
        .map(|&friday| {
            let mine = ledger.entries.iter().filter(|e| e.weekend == friday);
            let (added, removed, changes) = mine.fold((0, 0, 0), |(a, r, n), e| {
                if e.minutes > 0 {
                    (a + e.minutes, r, n + 1)
                } else {
                    (a, r - e.minutes, n + 1)
                }
            });
            let status = if friday < clock.headline {
                "past"
            } else if friday == clock.headline {
                match clock.phase {
                    Phase::Week => "coming",
                    Phase::Weekend => "now",
                }
            } else {
                "next"
            };
            let minutes = ledger.balance(friday);
            WeekendStats {
                weekend: friday,
                label: weekend_label(friday),
                minutes,
                time: format_minutes(minutes),
                added,
                removed,
                changes,
                status,
            }
        })
        .collect();

    let locked: Vec<&WeekendStats> = weekends
        .iter()
        .filter(|w| (w.status == "past" || w.status == "now") && w.changes > 0)
        .collect();
    let average_minutes = if locked.is_empty() {
        0
    } else {
        let sum: i32 = locked.iter().map(|w| w.minutes).sum();
        (f64::from(sum) / locked.len() as f64).round() as i32
    };
    let best = locked
        .iter()
        .max_by_key(|w| (w.minutes, w.weekend))
        .filter(|w| w.minutes > 0)
        .map(|w| total(ledger, w.weekend));
    let totals = Totals {
        added: ledger.entries.iter().map(|e| e.minutes.max(0)).sum(),
        removed: ledger.entries.iter().map(|e| (-e.minutes).max(0)).sum(),
        changes: ledger.entries.len(),
        locked_weekends: locked.len(),
        average_minutes,
        average_time: format_minutes(average_minutes),
        best,
    };

    const DAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    let mut by_weekday: Vec<WeekdayStats> = DAYS
        .iter()
        .map(|&day| WeekdayStats {
            day,
            added: 0,
            removed: 0,
        })
        .collect();
    for e in &ledger.entries {
        let idx = e.at.with_timezone(&tz).weekday().num_days_from_monday() as usize;
        if e.minutes > 0 {
            by_weekday[idx].added += e.minutes;
        } else {
            by_weekday[idx].removed -= e.minutes;
        }
    }

    let entries = ledger
        .entries
        .iter()
        .rev()
        .map(|e| EntryView {
            at: e.at,
            local: e
                .at
                .with_timezone(&tz)
                .format("%a %-d %b %H:%M")
                .to_string(),
            local_iso: e.at.with_timezone(&tz).format("%Y-%m-%dT%H:%M").to_string(),
            minutes: e.minutes,
            weekend: e.weekend,
            weekend_label: weekend_label(e.weekend),
        })
        .collect();

    Report {
        phase: clock.phase,
        headline,
        next,
        target: clock.target(),
        target_label: clock.target_label(),
        target_minutes: ledger.balance(clock.target()),
        summary,
        weekends,
        totals,
        by_weekday,
        entries,
        max_step_minutes: MAX_STEP_MINUTES,
    }
}

pub fn current_report(config_dir: &Path, tz: Tz, now: DateTime<Utc>) -> Result<Report> {
    let ledger = load(config_dir)?;
    Ok(report(&ledger, Clock::at(now, tz), tz))
}

pub struct MinecraftSource;

#[async_trait::async_trait]
impl DataSource for MinecraftSource {
    fn id(&self) -> &'static str {
        "minecraft"
    }

    fn enabled(&self, cfg: &Config) -> bool {
        cfg.sources.minecraft.enabled
    }

    async fn load(&self, ctx: &SourceContext<'_>) -> Result<SourceOutcome> {
        let ledger = load(ctx.config_dir)?;
        let view = ledger.view(Clock::at(ctx.now, ctx.tz));
        Ok(SourceOutcome::live(
            String::new(),
            Contribution::Minecraft(view),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn local(y: i32, m: u32, d: u32, h: u32, min: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(h, min, 0)
            .unwrap()
    }

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    // 2026-10-09 is a Friday.
    const FRI: (i32, u32, u32) = (2026, 10, 9);

    #[test]
    fn friday_before_noon_is_week() {
        let c = Clock::at_local(local(2026, 10, 9, 11, 59));
        assert_eq!(c.phase, Phase::Week);
        assert_eq!(c.headline, date(FRI.0, FRI.1, FRI.2));
        assert_eq!(c.target(), date(2026, 10, 9));
    }

    #[test]
    fn friday_noon_starts_weekend() {
        let c = Clock::at_local(local(2026, 10, 9, 12, 0));
        assert_eq!(c.phase, Phase::Weekend);
        assert_eq!(c.headline, date(2026, 10, 9));
        assert_eq!(c.target(), date(2026, 10, 16));
    }

    #[test]
    fn saturday_and_late_sunday_are_weekend() {
        for at in [local(2026, 10, 10, 9, 0), local(2026, 10, 11, 23, 59)] {
            let c = Clock::at_local(at);
            assert_eq!(c.phase, Phase::Weekend, "{at}");
            assert_eq!(c.headline, date(2026, 10, 9));
            assert_eq!(c.target(), date(2026, 10, 16));
        }
    }

    #[test]
    fn monday_midnight_returns_to_week() {
        let c = Clock::at_local(local(2026, 10, 12, 0, 0));
        assert_eq!(c.phase, Phase::Week);
        assert_eq!(c.headline, date(2026, 10, 16));
        assert_eq!(c.target(), date(2026, 10, 16));
    }

    #[test]
    fn clock_uses_household_timezone() {
        // 11:30 UTC on Friday in BST is 12:30 local: already the weekend.
        let now = Utc.with_ymd_and_hms(2026, 10, 9, 11, 30, 0).unwrap();
        assert_eq!(
            Clock::at(now, chrono_tz::Europe::London).phase,
            Phase::Weekend
        );
        assert_eq!(Clock::at(now, chrono_tz::UTC).phase, Phase::Week);
    }

    #[test]
    fn weekend_minutes_bank_for_next_weekend_and_roll_over() {
        let mut ledger = Ledger::default();
        let at = Utc::now();
        let wed = Clock::at_local(local(2026, 10, 7, 18, 0));
        assert_eq!(ledger.add(at, wed, 30), 30);
        let sat = Clock::at_local(local(2026, 10, 10, 10, 0));
        assert_eq!(ledger.add(at, sat, 15), 15);

        let view = ledger.view(sat);
        assert_eq!(view.phase, "weekend");
        assert_eq!(view.minutes, 30);
        assert_eq!(view.next_minutes, 15);
        assert_eq!(view.next_time, "15m");

        let mon = Clock::at_local(local(2026, 10, 12, 8, 0));
        let view = ledger.view(mon);
        assert_eq!(view.phase, "week");
        assert_eq!(view.minutes, 15);
        assert!(view.next_time.is_empty());
    }

    #[test]
    fn removal_clamps_at_zero() {
        let mut ledger = Ledger::default();
        let at = Utc::now();
        let tue = Clock::at_local(local(2026, 10, 6, 18, 0));
        ledger.add(at, tue, 10);
        assert_eq!(ledger.add(at, tue, -25), -10);
        assert_eq!(ledger.balance(tue.target()), 0);
        assert_eq!(ledger.add(at, tue, -5), 0);
        assert_eq!(ledger.entries.len(), 2);
    }

    #[test]
    fn locked_weekend_is_untouched_by_weekend_removals() {
        let mut ledger = Ledger::default();
        let at = Utc::now();
        ledger.add(at, Clock::at_local(local(2026, 10, 8, 18, 0)), 60);
        let sun = Clock::at_local(local(2026, 10, 11, 15, 0));
        assert_eq!(ledger.add(at, sun, -20), 0);
        assert_eq!(ledger.view(sun).minutes, 60);
    }

    #[test]
    fn formats_minutes() {
        assert_eq!(format_minutes(0), "0m");
        assert_eq!(format_minutes(45), "45m");
        assert_eq!(format_minutes(120), "2h");
        assert_eq!(format_minutes(135), "2h 15");
        assert_eq!(format_minutes(65), "1h 05");
    }

    #[test]
    fn adjust_persists_and_summarises() {
        let dir = tempfile::tempdir().unwrap();
        let tz = chrono_tz::Europe::London;
        // Wednesday evening.
        let now = Utc.with_ymd_and_hms(2026, 10, 7, 17, 0, 0).unwrap();
        let out = adjust(dir.path(), tz, now, 15).unwrap();
        assert_eq!(out.applied, 15);
        assert_eq!(out.summary, "+15 min. Coming weekend: 15m");
        let out = adjust(dir.path(), tz, now, -30).unwrap();
        assert_eq!(out.applied, -15);
        assert_eq!(
            out.summary,
            "-15 min (that was all that was left). Coming weekend: 0m"
        );
        assert_eq!(load(dir.path()).unwrap().entries.len(), 2);
        assert!(adjust(dir.path(), tz, now, 0).is_err());
        assert!(adjust(dir.path(), tz, now, MAX_STEP_MINUTES + 1).is_err());
    }

    #[test]
    fn parses_minutes_from_query_and_body() {
        assert_eq!(parse_minutes("15").unwrap(), 15);
        assert_eq!(parse_minutes(" 15").unwrap(), 15);
        assert_eq!(parse_minutes("+15").unwrap(), 15);
        assert_eq!(parse_minutes("-10").unwrap(), -10);
        assert_eq!(parse_minutes("15.0").unwrap(), 15);
        assert!(parse_minutes("7.5").is_err());
        assert!(parse_minutes("abc").is_err());

        assert_eq!(
            minutes_from_body(br#"{"minutes": 20}"#).unwrap().unwrap(),
            20
        );
        assert_eq!(
            minutes_from_body(br#"{"minutes": "-5"}"#).unwrap().unwrap(),
            -5
        );
        assert_eq!(minutes_from_body(b"25").unwrap().unwrap(), 25);
        assert_eq!(minutes_from_body(b"minutes=-15").unwrap().unwrap(), -15);
        assert!(minutes_from_body(b"").is_none());
        assert!(minutes_from_body(br#"{"other": 1}"#).is_none());
    }

    #[test]
    fn report_groups_weekends_and_averages_locked_ones() {
        let tz = chrono_tz::UTC;
        let mut ledger = Ledger::default();
        let at = |d: u32| Utc.with_ymd_and_hms(2026, 10, d, 18, 0, 0).unwrap();
        ledger.add(at(1), Clock::at(at(1), tz), 40); // Thu → Fri 2
        ledger.add(at(1), Clock::at(at(1), tz), -10);
        ledger.add(at(7), Clock::at(at(7), tz), 20); // Wed → Fri 9
        ledger.add(at(10), Clock::at(at(10), tz), 5); // Sat → Fri 16

        let sat = Clock::at(at(10), tz);
        let r = report(&ledger, sat, tz);
        assert_eq!(r.phase, Phase::Weekend);
        assert_eq!(r.headline.minutes, 20);
        assert_eq!(r.next.as_ref().unwrap().minutes, 5);
        assert_eq!(r.summary, "This weekend: 20m. Next weekend: 5m");
        let statuses: Vec<_> = r
            .weekends
            .iter()
            .map(|w| (w.label.as_str(), w.status))
            .collect();
        assert_eq!(
            statuses,
            [
                ("Fri 16 Oct", "next"),
                ("Fri 9 Oct", "now"),
                ("Fri 2 Oct", "past")
            ]
        );
        assert_eq!(r.weekends[2].added, 40);
        assert_eq!(r.weekends[2].removed, 10);
        assert_eq!(r.totals.locked_weekends, 2);
        assert_eq!(r.totals.average_minutes, 25);
        assert_eq!(r.totals.best.as_ref().unwrap().minutes, 30);
        assert_eq!(r.by_weekday[3].added, 40);
        assert_eq!(r.by_weekday[3].removed, 10);
        assert_eq!(r.entries[0].minutes, 5);
    }
}
