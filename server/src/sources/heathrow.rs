//! Heathrow landing alternation as a seven-day grid, starting today.
//!
//! Westerly arrivals line up with a runway centreline for the last 10 nm.
//! Heathrow lands on one runway until 15:00 and the other afterwards, then
//! swaps the halves each Monday. The 2026 programme starts Monday 5 January
//! with 27L in the morning. SW11 5QA is about 400 m south of that centreline
//! and about 1.8 km south of 27R, so only 27L half-days pass overhead.
//!
//! Daytime westerly preference holds until the tailwind on runway 27 exceeds
//! 5 knots. The BBC forecast (same location as the weather row) supplies the
//! wind for each half-day. A fresh EGLL observation overrides the half that
//! is happening now. `East` means arrivals are from the west, so the house
//! is clear of both approaches.

use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{Datelike, Duration as ChronoDuration, NaiveDate, Timelike};
use serde::Deserialize;
use tracing::{info, warn};

use crate::model::{PlanesDay, PlanesHalf, PlanesWeek};
use crate::sources::cache::TtlCache;

use super::context::SourceContext;
use super::contribute::{Contribution, SourceOutcome};
use super::{DataSource, DisabledBehaviour};

pub struct HeathrowSource;

const METAR_URL: &str = "https://aviationweather.gov/api/data/metar?ids=EGLL&format=json";
const BBC_URL: &str = "https://weather-broker-cdn.api.bbci.co.uk/en/forecast/aggregated";
const FETCH_TTL: Duration = Duration::from_secs(30 * 60);
/// Monday 5 Jan 2026: morning landings on 27L, afternoon on 27R.
const ANCHOR: NaiveDate = match NaiveDate::from_ymd_opt(2026, 1, 5) {
    Some(date) => date,
    None => panic!("invalid Heathrow anchor"),
};
const DAY_START: u32 = 6;
const SWITCH_HOUR: u32 = 15;
const PREFERENCE_UNTIL: u32 = 23;
const WESTERLY_HEADING: f64 = 270.0;
const MAX_WESTERLY_TAILWIND_KT: f64 = 5.0;
const MPH_TO_KT: f64 = 0.868_976;

static LAST_WIND: TtlCache<Wind> = TtlCache::new();
static LAST_FORECAST: TtlCache<(String, Vec<WindSample>)> = TtlCache::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runway {
    /// Southern runway. Approach from the east is 27L.
    South,
    /// Northern runway. Approach from the east is 27R.
    North,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Wind {
    /// Degrees true, `None` when calm or variable.
    dir: Option<f64>,
    speed_kt: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flow {
    /// Arrivals from the east, over London.
    Westerly,
    /// Arrivals from the west. London is not under the landing path.
    Easterly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Half {
    Morning,
    Afternoon,
}

#[derive(Debug, Clone, Copy)]
struct WindSample {
    date: NaiveDate,
    hour: u32,
    wind: Wind,
}

#[async_trait::async_trait]
impl DataSource for HeathrowSource {
    fn id(&self) -> &'static str {
        "heathrow"
    }

    fn enabled(&self, cfg: &crate::config::Config) -> bool {
        cfg.heathrow_enabled()
    }

    fn when_disabled(&self, _cfg: &crate::config::Config) -> DisabledBehaviour {
        DisabledBehaviour::Skip
    }

    async fn load(&self, ctx: &SourceContext<'_>) -> Result<SourceOutcome> {
        let Some(runway) = parse_runway(&ctx.cfg.sources.heathrow.runway) else {
            return Ok(SourceOutcome::live(String::new(), Contribution::None));
        };
        let forecast = match load_forecast(&ctx.http, &ctx.cfg.weather.location_id).await {
            Ok(samples) => samples,
            Err(err) => {
                warn!(%err, "BBC wind unavailable; using the westerly alternation");
                Vec::new()
            }
        };
        let metar = match load_wind(&ctx.http).await {
            Ok(wind) => Some(wind),
            Err(err) => {
                warn!(%err, "EGLL wind unavailable");
                None
            }
        };
        let local = ctx.now.with_timezone(&ctx.tz);
        let week = planes_week(ctx.today, local.hour(), runway, &forecast, metar);
        info!(
            runway = runway_code(runway),
            days = week.days.len(),
            "loaded Heathrow alternation"
        );
        Ok(SourceOutcome::live(
            "Heathrow arrivals",
            Contribution::Planes(week),
        ))
    }
}

pub fn parse_runway(raw: &str) -> Option<Runway> {
    match raw.trim().to_ascii_uppercase().as_str() {
        "27L" => Some(Runway::South),
        "27R" => Some(Runway::North),
        _ => None,
    }
}

fn runway_code(runway: Runway) -> &'static str {
    match runway {
        Runway::South => "27L",
        Runway::North => "27R",
    }
}

/// Morning landing runway for the week that contains `date`.
pub fn morning_runway(date: NaiveDate) -> Runway {
    let monday = date - ChronoDuration::days(date.weekday().num_days_from_monday().into());
    let weeks = (monday - ANCHOR).num_days().div_euclid(7);
    if weeks.rem_euclid(2) == 0 {
        Runway::South
    } else {
        Runway::North
    }
}

/// Daytime westerly preference holds until the tailwind on runway 27 exceeds 5 kt.
/// At night the preference is lifted and aircraft land into the wind.
fn flow_from_wind(wind: Wind, hour: u32) -> Flow {
    let Some(dir) = wind.dir else {
        return Flow::Westerly;
    };
    let headwind = wind.speed_kt * (dir - WESTERLY_HEADING).to_radians().cos();
    let daytime = (DAY_START..PREFERENCE_UNTIL).contains(&hour);
    if daytime {
        if headwind < -MAX_WESTERLY_TAILWIND_KT {
            Flow::Easterly
        } else {
            Flow::Westerly
        }
    } else if headwind < 0.0 {
        Flow::Easterly
    } else {
        Flow::Westerly
    }
}

fn planes_week(
    today: NaiveDate,
    hour: u32,
    runway: Runway,
    forecast: &[WindSample],
    metar: Option<Wind>,
) -> PlanesWeek {
    let days = (0..7)
        .map(|offset| {
            let date = today + ChronoDuration::days(offset);
            let morning = half_state(date, Half::Morning, today, hour, runway, forecast, metar);
            let afternoon = half_state(date, Half::Afternoon, today, hour, runway, forecast, metar);
            PlanesDay {
                label: date.format("%a %-d").to_string(),
                today: date == today,
                morning,
                afternoon,
            }
        })
        .collect();
    PlanesWeek {
        title: "Planes".into(),
        days,
    }
}

fn half_state(
    date: NaiveDate,
    half: Half,
    today: NaiveDate,
    hour: u32,
    runway: Runway,
    forecast: &[WindSample],
    metar: Option<Wind>,
) -> PlanesHalf {
    let flow = if date == today && half_of(hour) == Some(half) {
        if let Some(wind) = metar {
            flow_from_wind(wind, hour.clamp(DAY_START, PREFERENCE_UNTIL - 1))
        } else {
            forecast_flow(date, half, forecast)
        }
    } else {
        forecast_flow(date, half, forecast)
    };
    let ours = match half {
        Half::Morning => morning_runway(date) == runway,
        Half::Afternoon => morning_runway(date) != runway,
    };
    match (flow, ours) {
        (Flow::Easterly, _) => half_cell("East", "east"),
        (Flow::Westerly, true) => half_cell("On", "on"),
        (Flow::Westerly, false) => half_cell("Off", "off"),
    }
}

fn half_cell(label: &str, kind: &str) -> PlanesHalf {
    PlanesHalf {
        label: label.into(),
        kind: kind.into(),
    }
}

fn forecast_flow(date: NaiveDate, half: Half, forecast: &[WindSample]) -> Flow {
    let mut east = 0usize;
    let mut total = 0usize;
    for sample in forecast {
        if sample.date == date && half_of(sample.hour) == Some(half) {
            total += 1;
            if flow_from_wind(sample.wind, sample.hour) == Flow::Easterly {
                east += 1;
            }
        }
    }
    if total > 0 && east * 2 > total {
        Flow::Easterly
    } else {
        Flow::Westerly
    }
}

fn half_of(hour: u32) -> Option<Half> {
    if (DAY_START..SWITCH_HOUR).contains(&hour) {
        Some(Half::Morning)
    } else if (SWITCH_HOUR..PREFERENCE_UNTIL).contains(&hour) {
        Some(Half::Afternoon)
    } else {
        None
    }
}

#[derive(Debug, Deserialize)]
struct BbcForecast {
    #[serde(default)]
    forecasts: Vec<BbcDay>,
}

#[derive(Debug, Deserialize)]
struct BbcDay {
    #[serde(default)]
    detailed: BbcDetailed,
}

#[derive(Debug, Deserialize, Default)]
struct BbcDetailed {
    #[serde(default)]
    reports: Vec<BbcReport>,
}

#[derive(Debug, Deserialize)]
struct BbcReport {
    #[serde(rename = "localDate")]
    local_date: String,
    timeslot: String,
    #[serde(rename = "windDirection", default)]
    wind_direction: Option<String>,
    #[serde(rename = "windSpeedMph", default)]
    wind_speed_mph: Option<f64>,
}

async fn load_forecast(http: &reqwest::Client, location_id: &str) -> Result<Vec<WindSample>> {
    let location_id = location_id.trim();
    if location_id.is_empty() {
        anyhow::bail!("BBC weather location_id is empty");
    }
    if let Some((_, samples)) = LAST_FORECAST.get(FETCH_TTL, |(id, _)| id == location_id) {
        return Ok(samples);
    }
    let url = format!("{BBC_URL}/{location_id}");
    let body = http
        .get(&url)
        .header("accept", "application/json")
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .with_context(|| format!("BBC wind GET {url}"))?
        .error_for_status()
        .context("BBC wind status")?
        .text()
        .await
        .context("BBC wind body")?;
    let samples = samples_from_bbc(&body).context("parsing BBC wind")?;
    LAST_FORECAST.set((location_id.to_string(), samples.clone()));
    Ok(samples)
}

fn samples_from_bbc(body: &str) -> Result<Vec<WindSample>> {
    let parsed: BbcForecast = serde_json::from_str(body).context("BBC wind JSON")?;
    let mut out = Vec::new();
    for day in parsed.forecasts {
        for report in day.detailed.reports {
            let Some(sample) = sample_from_report(&report) else {
                continue;
            };
            out.push(sample);
        }
    }
    Ok(out)
}

fn sample_from_report(report: &BbcReport) -> Option<WindSample> {
    let date = NaiveDate::parse_from_str(&report.local_date, "%Y-%m-%d").ok()?;
    let hour = hour_of(&report.timeslot)?;
    let wind = wind_from_bbc(report.wind_direction.as_deref(), report.wind_speed_mph)?;
    Some(WindSample { date, hour, wind })
}

fn hour_of(timeslot: &str) -> Option<u32> {
    let hour: u32 = timeslot.split(':').next()?.parse().ok()?;
    (hour <= 23).then_some(hour)
}

fn wind_from_bbc(direction: Option<&str>, mph: Option<f64>) -> Option<Wind> {
    let speed_kt = mph? * MPH_TO_KT;
    let dir = match direction.unwrap_or("").trim().to_ascii_uppercase().as_str() {
        "" | "CALM" | "VRB" => None,
        text => Some(compass_deg(text)?),
    };
    Some(Wind { dir, speed_kt })
}

fn compass_deg(text: &str) -> Option<f64> {
    Some(match text {
        "N" => 0.0,
        "NNE" => 22.5,
        "NE" => 45.0,
        "ENE" => 67.5,
        "E" => 90.0,
        "ESE" => 112.5,
        "SE" => 135.0,
        "SSE" => 157.5,
        "S" => 180.0,
        "SSW" => 202.5,
        "SW" => 225.0,
        "WSW" => 247.5,
        "W" => 270.0,
        "WNW" => 292.5,
        "NW" => 315.0,
        "NNW" => 337.5,
        _ => return None,
    })
}

async fn load_wind(http: &reqwest::Client) -> Result<Wind> {
    if let Some(cached) = LAST_WIND.get(FETCH_TTL, |_| true) {
        return Ok(cached);
    }
    let body = http
        .get(METAR_URL)
        .header("accept", "application/json")
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .context("EGLL METAR")?
        .error_for_status()
        .context("EGLL METAR status")?
        .text()
        .await
        .context("EGLL METAR body")?;
    let wind = wind_from_body(&body).context("parsing EGLL wind")?;
    LAST_WIND.set(wind);
    Ok(wind)
}

fn wind_from_body(body: &str) -> Option<Wind> {
    wind_from_json(body).or_else(|| wind_from_metar_text(body))
}

fn wind_from_json(body: &str) -> Option<Wind> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    let item = value.as_array()?.first()?;
    if let Some(raw) = item.get("rawOb").and_then(|v| v.as_str()) {
        if let Some(wind) = wind_from_metar_text(raw) {
            return Some(wind);
        }
    }
    let speed_kt = json_f64(item.get("wspd"))?;
    let dir = match item.get("wdir") {
        Some(serde_json::Value::Number(n)) => n.as_f64(),
        Some(serde_json::Value::String(text)) if !text.eq_ignore_ascii_case("VRB") => {
            text.parse().ok()
        }
        _ => None,
    };
    Some(Wind { dir, speed_kt })
}

fn json_f64(value: Option<&serde_json::Value>) -> Option<f64> {
    match value? {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(text) => text.parse().ok(),
        _ => None,
    }
}

fn wind_from_metar_text(text: &str) -> Option<Wind> {
    for token in text.split_whitespace() {
        let Some(without_unit) = token.strip_suffix("KT") else {
            continue;
        };
        let core = without_unit.split('G').next().unwrap_or(without_unit);
        if let Some(speed) = core.strip_prefix("VRB") {
            return Some(Wind {
                dir: None,
                speed_kt: speed.parse().ok()?,
            });
        }
        if core.len() >= 5 && core.as_bytes()[..3].iter().all(u8::is_ascii_digit) {
            return Some(Wind {
                dir: core[..3].parse().ok(),
                speed_kt: core[3..].parse().ok()?,
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ymd(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).unwrap()
    }

    fn wind(dir: impl Into<Option<f64>>, speed_kt: f64) -> Wind {
        Wind {
            dir: dir.into(),
            speed_kt,
        }
    }

    const WEEK_JSON: &str = r#"{
      "forecasts": [{
        "detailed": { "reports": [
          {"localDate":"2026-11-09","timeslot":"09:00","windDirection":"ENE","windSpeedMph":4},
          {"localDate":"2026-11-09","timeslot":"12:00","windDirection":"ENE","windSpeedMph":6},
          {"localDate":"2026-11-09","timeslot":"16:00","windDirection":"ENE","windSpeedMph":5},
          {"localDate":"2026-11-10","timeslot":"09:00","windDirection":"E","windSpeedMph":9},
          {"localDate":"2026-11-10","timeslot":"11:00","windDirection":"E","windSpeedMph":9},
          {"localDate":"2026-11-10","timeslot":"16:00","windDirection":"E","windSpeedMph":8},
          {"localDate":"2026-11-10","timeslot":"18:00","windDirection":"E","windSpeedMph":7}
        ]}
      }]
    }"#;

    #[test]
    fn alternation_matches_the_2026_programme() {
        assert_eq!(morning_runway(ymd(2026, 1, 5)), Runway::South);
        assert_eq!(morning_runway(ymd(2026, 1, 11)), Runway::South);
        assert_eq!(morning_runway(ymd(2026, 1, 12)), Runway::North);
        assert_eq!(morning_runway(ymd(2026, 11, 2)), Runway::North);
        assert_eq!(morning_runway(ymd(2026, 11, 8)), Runway::North);
        assert_eq!(morning_runway(ymd(2026, 11, 9)), Runway::South);
        assert_eq!(morning_runway(ymd(2026, 11, 16)), Runway::North);
    }

    #[test]
    fn westerly_preference_holds_until_five_knots_of_tailwind() {
        assert_eq!(flow_from_wind(wind(Some(90.0), 5.0), 10), Flow::Westerly);
        assert_eq!(flow_from_wind(wind(Some(90.0), 8.0), 10), Flow::Easterly);
        assert_eq!(flow_from_wind(wind(Some(180.0), 15.0), 10), Flow::Westerly);
        assert_eq!(flow_from_wind(wind(Some(250.0), 12.0), 10), Flow::Westerly);
        assert_eq!(flow_from_wind(wind(None::<f64>, 0.0), 10), Flow::Westerly);
        assert_eq!(flow_from_wind(wind(Some(90.0), 3.0), 1), Flow::Easterly);
        assert_eq!(flow_from_wind(wind(Some(270.0), 3.0), 1), Flow::Westerly);
    }

    #[test]
    fn week_grid_uses_forecast_wind_for_each_half() {
        let today = ymd(2026, 11, 8);
        let forecast = samples_from_bbc(WEEK_JSON).unwrap();
        let week = planes_week(today, 16, Runway::South, &forecast, None);
        assert_eq!(week.days.len(), 7);
        assert_eq!(week.days[0].label, "Sun 8");
        assert!(week.days[0].today);
        assert_eq!(week.days[0].morning.kind, "off");
        assert_eq!(week.days[0].afternoon.kind, "on");
        assert_eq!(week.days[1].label, "Mon 9");
        assert_eq!(week.days[1].morning.kind, "on");
        assert_eq!(week.days[1].afternoon.kind, "off");
        assert_eq!(week.days[2].morning.kind, "east");
        assert_eq!(week.days[2].afternoon.kind, "east");
        assert_eq!(week.days[6].label, "Sat 14");
    }

    #[test]
    fn metar_overrides_only_the_current_half() {
        let today = ymd(2026, 11, 9);
        let forecast = samples_from_bbc(WEEK_JSON).unwrap();
        let week = planes_week(
            today,
            10,
            Runway::South,
            &forecast,
            Some(wind(Some(90.0), 12.0)),
        );
        assert_eq!(week.days[0].morning.kind, "east");
        assert_eq!(week.days[0].afternoon.kind, "off");
        assert_eq!(week.days[1].morning.kind, "east");
    }

    #[test]
    fn missing_wind_keeps_the_published_halves() {
        let today = ymd(2026, 11, 9);
        let week = planes_week(today, 8, Runway::South, &[], None);
        assert!(week.days.iter().all(|day| day.morning.kind == "on"));
        assert!(week.days.iter().all(|day| day.afternoon.kind == "off"));
    }

    #[test]
    fn parses_metar_json_and_raw_wind() {
        let json =
            r#"[{"icaoId":"EGLL","wdir":240,"wspd":11,"rawOb":"EGLL 081620Z 24011KT 9999"}]"#;
        let wind = wind_from_body(json).unwrap();
        assert_eq!(wind.dir, Some(240.0));
        assert_eq!(wind.speed_kt, 11.0);
        let text = "2026/11/08 16:20\nEGLL 081620Z VRB03KT 9999";
        let wind = wind_from_body(text).unwrap();
        assert_eq!(wind.dir, None);
        assert_eq!(wind.speed_kt, 3.0);
    }

    #[test]
    fn runway_config_accepts_only_the_two_westerly_codes() {
        assert_eq!(parse_runway(" 27l "), Some(Runway::South));
        assert_eq!(parse_runway("27R"), Some(Runway::North));
        assert_eq!(parse_runway(""), None);
        assert_eq!(parse_runway("09R"), None);
    }
}
