//! BBC Weather forecast for the family frame.
//!
//! BBC does not publish a documented API. The same CDN JSON the website uses
//! (`weather-broker-cdn`) returns hourly reports; we keep 09:00 / 15:00 / 21:00
//! as morning, afternoon, and evening.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use chrono::{Duration as ChronoDuration, NaiveDate};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::config::WeatherConfig;
use crate::model::{Weather, WeatherDay, WeatherSlot};

const FORECAST_URL: &str = "https://weather-broker-cdn.api.bbci.co.uk/en/forecast/aggregated";
const FETCH_TTL: Duration = Duration::from_secs(15 * 60);

static LAST: Mutex<Option<(Instant, String, Weather)>> = Mutex::new(None);

const PERIODS: [Period; 3] = [
    Period {
        name: "Morning",
        target: 9,
        start: 6,
        end: 11,
    },
    Period {
        name: "Afternoon",
        target: 15,
        start: 12,
        end: 17,
    },
    Period {
        name: "Evening",
        target: 21,
        start: 18,
        end: 23,
    },
];

struct Period {
    name: &'static str,
    target: u32,
    start: u32,
    end: u32,
}

#[derive(Debug, Clone)]
struct Hourly {
    date: NaiveDate,
    hour: u32,
    temperature_c: i32,
    weather_type: i64,
    weather_text: String,
}

#[derive(Debug, Clone)]
struct DaySummary {
    temperature_min: Option<i32>,
    temperature_max: Option<i32>,
    weather_type: i64,
    weather_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct SlotCache {
    location_id: String,
    slots: HashMap<String, CachedSlot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedSlot {
    temperature_c: i32,
    weather_type: i64,
    weather_text: String,
}

#[derive(Debug, Deserialize)]
struct Aggregated {
    #[serde(default)]
    forecasts: Vec<ForecastDay>,
    #[serde(default)]
    location: Location,
}

#[derive(Debug, Deserialize, Default)]
struct Location {
    #[serde(default)]
    name: String,
}

#[derive(Debug, Deserialize)]
struct ForecastDay {
    #[serde(default)]
    detailed: Detailed,
    #[serde(default)]
    summary: Summary,
}

#[derive(Debug, Deserialize, Default)]
struct Summary {
    #[serde(default)]
    report: Option<SummaryReport>,
}

#[derive(Debug, Deserialize)]
struct SummaryReport {
    #[serde(rename = "localDate")]
    local_date: Option<String>,
    #[serde(rename = "weatherType")]
    weather_type: Option<i64>,
    #[serde(rename = "weatherTypeText")]
    weather_type_text: Option<String>,
    #[serde(rename = "minTempC")]
    min_temp_c: Option<i64>,
    #[serde(rename = "maxTempC")]
    max_temp_c: Option<i64>,
}

#[derive(Debug, Deserialize, Default)]
struct Detailed {
    #[serde(default)]
    reports: Vec<HourlyReport>,
}

#[derive(Debug, Deserialize)]
struct HourlyReport {
    #[serde(rename = "localDate")]
    local_date: String,
    timeslot: String,
    #[serde(rename = "temperatureC")]
    temperature_c: Option<i64>,
    #[serde(rename = "weatherType")]
    weather_type: Option<i64>,
    #[serde(rename = "weatherTypeText")]
    weather_type_text: Option<String>,
}

pub async fn load_forecast(
    cfg: &WeatherConfig,
    cache_path: &Path,
    today: NaiveDate,
) -> Result<Weather> {
    let location_id = cfg.location_id.trim();
    if location_id.is_empty() {
        anyhow::bail!("BBC weather location_id is empty");
    }
    if let Some((at, id, weather)) = LAST.lock().ok().and_then(|g| g.clone()) {
        if id == location_id && at.elapsed() < FETCH_TTL && !weather.days.is_empty() {
            return Ok(weather);
        }
    }

    let url = format!("{FORECAST_URL}/{location_id}");
    let body = reqwest::Client::new()
        .get(&url)
        .header("accept", "application/json")
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .with_context(|| format!("BBC weather GET {url}"))?
        .error_for_status()
        .with_context(|| format!("BBC weather status {url}"))?
        .text()
        .await?;

    let mut cache = load_slot_cache(cache_path, location_id);
    let weather = forecast_from_json(&body, today, &mut cache)?;
    save_slot_cache(cache_path, &cache);
    if let Ok(mut guard) = LAST.lock() {
        *guard = Some((Instant::now(), location_id.to_string(), weather.clone()));
    }
    info!(
        location = %weather.location,
        days = weather.days.len(),
        "loaded BBC weather"
    );
    Ok(weather)
}

fn forecast_from_json(json: &str, today: NaiveDate, cache: &mut SlotCache) -> Result<Weather> {
    let parsed: Aggregated = serde_json::from_str(json).context("parsing BBC weather JSON")?;
    let hours = collect_hours(&parsed);
    let summaries = collect_summaries(&parsed);
    let location = if parsed.location.name.is_empty() {
        "BBC Weather".into()
    } else {
        parsed.location.name
    };
    let tomorrow = today + ChronoDuration::days(1);
    let days = [("Today", today), ("Tomorrow", tomorrow)]
        .into_iter()
        .map(|(label, date)| WeatherDay {
            label: label.into(),
            slots: PERIODS
                .iter()
                .map(|period| slot_for(date, period, &hours, &summaries, cache))
                .collect(),
        })
        .collect();

    cache.retain_recent(today);
    Ok(Weather { location, days })
}

pub fn demo_weather() -> Weather {
    Weather {
        location: "London".into(),
        days: vec![
            WeatherDay {
                label: "Today".into(),
                slots: vec![
                    demo_slot("Morning", "sun", "18°", "Sunny"),
                    demo_slot("Afternoon", "partly-cloudy", "21°", "Sunny intervals"),
                    demo_slot("Evening", "rain", "16°", "Light rain"),
                ],
            },
            WeatherDay {
                label: "Tomorrow".into(),
                slots: vec![
                    demo_slot("Morning", "cloud", "15°", "Thick cloud"),
                    demo_slot("Afternoon", "storm", "17°", "Thundery showers"),
                    demo_slot("Evening", "moon", "13°", "Clear sky"),
                ],
            },
        ],
    }
}

pub fn icon_for(code: i64, text: &str) -> &'static str {
    match code {
        0 => "moon",
        1 => "sun",
        2 => "partly-cloudy-night",
        3 => "partly-cloudy",
        5 | 6 => "fog",
        7 | 8 => "cloud",
        9 | 10 | 11 | 12 | 13 | 14 | 15 | 39 => "rain",
        16 | 17 | 18 | 19 | 20 | 21 | 22 | 23 | 24 | 25 | 26 | 27 => "snow",
        28 | 29 | 30 => "storm",
        _ => icon_from_text(text),
    }
}

fn icon_from_text(text: &str) -> &'static str {
    let t = text.to_ascii_lowercase();
    if t.contains("thunder") || t.contains("lightning") {
        "storm"
    } else if t.contains("snow") || t.contains("sleet") || t.contains("hail") {
        "snow"
    } else if t.contains("fog") || t.contains("mist") {
        "fog"
    } else if t.contains("rain") || t.contains("drizzle") || t.contains("shower") {
        "rain"
    } else if t.contains("cloud") || t.contains("overcast") {
        "cloud"
    } else if t.contains("clear") && (t.contains("night") || t.contains("sky")) {
        "moon"
    } else if t.contains("sun") || t.contains("clear") || t.contains("fair") {
        "sun"
    } else {
        "cloud"
    }
}

fn demo_slot(period: &str, icon: &str, temperature: &str, summary: &str) -> WeatherSlot {
    WeatherSlot {
        period: period.into(),
        icon: icon.into(),
        temperature: temperature.into(),
        summary: summary.into(),
    }
}

fn collect_hours(parsed: &Aggregated) -> Vec<Hourly> {
    parsed
        .forecasts
        .iter()
        .flat_map(|day| day.detailed.reports.iter())
        .filter_map(|report| {
            let date = NaiveDate::parse_from_str(&report.local_date, "%Y-%m-%d").ok()?;
            let hour = parse_hour(&report.timeslot)?;
            Some(Hourly {
                date,
                hour,
                temperature_c: report.temperature_c? as i32,
                weather_type: report.weather_type.unwrap_or(-1),
                weather_text: report.weather_type_text.clone().unwrap_or_default(),
            })
        })
        .collect()
}

fn parse_hour(timeslot: &str) -> Option<u32> {
    timeslot.split(':').next()?.parse().ok()
}

fn slot_for(
    date: NaiveDate,
    period: &Period,
    hours: &[Hourly],
    summaries: &HashMap<NaiveDate, DaySummary>,
    cache: &mut SlotCache,
) -> WeatherSlot {
    let picked = hours
        .iter()
        .filter(|h| h.date == date && h.hour >= period.start && h.hour <= period.end)
        .min_by_key(|h| h.hour.abs_diff(period.target));

    if let Some(hour) = picked {
        cache.put(date, period.name, hour);
        return slot_from_parts(
            period.name,
            hour.temperature_c,
            hour.weather_type,
            &hour.weather_text,
        );
    }
    if let Some(cached) = cache.get(date, period.name) {
        return slot_from_parts(
            period.name,
            cached.temperature_c,
            cached.weather_type,
            &cached.weather_text,
        );
    }
    if let Some(summary) = summaries.get(&date) {
        let temperature = match period.name {
            "Morning" => summary.temperature_min.or(summary.temperature_max),
            "Afternoon" => summary.temperature_max.or(summary.temperature_min),
            _ => summary.temperature_max.or(summary.temperature_min),
        };
        if let Some(temperature_c) = temperature {
            return slot_from_parts(
                period.name,
                temperature_c,
                summary.weather_type,
                &summary.weather_text,
            );
        }
    }
    WeatherSlot {
        period: period.name.into(),
        icon: "unknown".into(),
        temperature: "—".into(),
        summary: String::new(),
    }
}

fn collect_summaries(parsed: &Aggregated) -> HashMap<NaiveDate, DaySummary> {
    parsed
        .forecasts
        .iter()
        .filter_map(|day| {
            let report = day.summary.report.as_ref()?;
            let date = NaiveDate::parse_from_str(report.local_date.as_deref()?, "%Y-%m-%d").ok()?;
            Some((
                date,
                DaySummary {
                    temperature_min: report.min_temp_c.map(|t| t as i32),
                    temperature_max: report.max_temp_c.map(|t| t as i32),
                    weather_type: report.weather_type.unwrap_or(-1),
                    weather_text: report.weather_type_text.clone().unwrap_or_default(),
                },
            ))
        })
        .collect()
}

fn slot_from_parts(period: &str, temperature_c: i32, weather_type: i64, text: &str) -> WeatherSlot {
    WeatherSlot {
        period: period.into(),
        icon: icon_for(weather_type, text).into(),
        temperature: format!("{temperature_c}°"),
        summary: text.to_string(),
    }
}

fn cache_key(date: NaiveDate, period: &str) -> String {
    format!("{date}-{period}")
}

impl SlotCache {
    fn get(&self, date: NaiveDate, period: &str) -> Option<CachedSlot> {
        self.slots.get(&cache_key(date, period)).cloned()
    }

    fn put(&mut self, date: NaiveDate, period: &str, hour: &Hourly) {
        self.slots.insert(
            cache_key(date, period),
            CachedSlot {
                temperature_c: hour.temperature_c,
                weather_type: hour.weather_type,
                weather_text: hour.weather_text.clone(),
            },
        );
    }

    fn retain_recent(&mut self, today: NaiveDate) {
        let keep_from = today - ChronoDuration::days(1);
        self.slots.retain(|key, _| {
            key.get(..10)
                .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
                .is_some_and(|d| d >= keep_from)
        });
    }
}

fn load_slot_cache(path: &Path, location_id: &str) -> SlotCache {
    let Ok(text) = std::fs::read_to_string(path) else {
        return SlotCache {
            location_id: location_id.into(),
            slots: HashMap::new(),
        };
    };
    match serde_json::from_str::<SlotCache>(&text) {
        Ok(cache) if cache.location_id == location_id => cache,
        Ok(_) => {
            warn!("BBC weather cache is for another location; starting fresh");
            SlotCache {
                location_id: location_id.into(),
                slots: HashMap::new(),
            }
        }
        Err(err) => {
            warn!(%err, "BBC weather cache unreadable");
            SlotCache {
                location_id: location_id.into(),
                slots: HashMap::new(),
            }
        }
    }
}

fn save_slot_cache(path: &Path, cache: &SlotCache) {
    if let Err(err) = (|| -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_vec_pretty(cache)?)?;
        Ok(())
    })() {
        warn!(%err, "could not write BBC weather cache");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> String {
        std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/bbc_forecast.json"),
        )
        .unwrap()
    }

    #[test]
    fn picks_morning_afternoon_evening() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 12).unwrap();
        let mut cache = SlotCache::default();
        let weather = forecast_from_json(&fixture(), today, &mut cache).unwrap();
        assert_eq!(weather.location, "London");
        assert_eq!(weather.days.len(), 2);

        let today_slots = &weather.days[0].slots;
        assert_eq!(today_slots[0].period, "Morning");
        assert_eq!(today_slots[0].icon, "sun");
        assert_eq!(today_slots[0].temperature, "16°");
        assert_eq!(today_slots[1].icon, "cloud");
        assert_eq!(today_slots[1].temperature, "21°");
        assert_eq!(today_slots[2].icon, "rain");
        assert_eq!(today_slots[2].temperature, "15°");

        let tomorrow = &weather.days[1].slots;
        assert_eq!(tomorrow[0].icon, "cloud");
        assert_eq!(tomorrow[0].temperature, "14°");
        assert_eq!(tomorrow[1].icon, "storm");
        assert_eq!(tomorrow[1].temperature, "17°");
        assert_eq!(tomorrow[2].icon, "moon");
        assert_eq!(tomorrow[2].temperature, "13°");
    }

    #[test]
    fn missing_morning_uses_cache() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 12).unwrap();
        let evening_only = r#"{
            "location": {"name": "London"},
            "forecasts": [{
                "detailed": {"reports": [
                    {"localDate": "2026-09-12", "timeslot": "21:00",
                     "temperatureC": 15, "weatherType": 12, "weatherTypeText": "Light Rain"}
                ]}
            }]
        }"#;
        let mut cache = SlotCache {
            location_id: "2643743".into(),
            slots: HashMap::from([(
                "2026-09-12-Morning".into(),
                CachedSlot {
                    temperature_c: 16,
                    weather_type: 1,
                    weather_text: "Sunny".into(),
                },
            )]),
        };
        let weather = forecast_from_json(evening_only, today, &mut cache).unwrap();
        assert_eq!(weather.days[0].slots[0].temperature, "16°");
        assert_eq!(weather.days[0].slots[0].icon, "sun");
        assert_eq!(weather.days[0].slots[2].temperature, "15°");
        assert_eq!(weather.days[1].slots[0].temperature, "—");
    }

    #[test]
    fn missing_hours_use_daily_high_low() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 12).unwrap();
        let evening_only = r#"{
            "location": {"name": "Clapham"},
            "forecasts": [{
                "summary": {"report": {
                    "localDate": "2026-09-12",
                    "weatherType": 12,
                    "weatherTypeText": "Light Rain",
                    "minTempC": 18,
                    "maxTempC": 23
                }},
                "detailed": {"reports": [
                    {"localDate": "2026-09-12", "timeslot": "21:00",
                     "temperatureC": 19, "weatherType": 7, "weatherTypeText": "Light Cloud"}
                ]}
            }]
        }"#;
        let mut cache = SlotCache::default();
        let weather = forecast_from_json(evening_only, today, &mut cache).unwrap();
        assert_eq!(weather.days[0].slots[0].temperature, "18°");
        assert_eq!(weather.days[0].slots[0].icon, "rain");
        assert_eq!(weather.days[0].slots[1].temperature, "23°");
        assert_eq!(weather.days[0].slots[1].icon, "rain");
        assert_eq!(weather.days[0].slots[2].temperature, "19°");
        assert_eq!(weather.days[0].slots[2].icon, "cloud");
    }

    #[test]
    fn icon_codes() {
        assert_eq!(icon_for(1, ""), "sun");
        assert_eq!(icon_for(0, ""), "moon");
        assert_eq!(icon_for(3, ""), "partly-cloudy");
        assert_eq!(icon_for(12, ""), "rain");
        assert_eq!(icon_for(29, ""), "storm");
        assert_eq!(icon_for(24, ""), "snow");
        assert_eq!(icon_for(6, ""), "fog");
        assert_eq!(icon_for(39, "Light Rain"), "rain");
        assert_eq!(icon_for(99, "Thundery showers"), "storm");
    }

    #[test]
    fn parse_hour_accepts_short_times() {
        assert_eq!(parse_hour("09:00"), Some(9));
        assert_eq!(parse_hour("9:00"), Some(9));
        assert_eq!(parse_hour("21:00"), Some(21));
    }
}
