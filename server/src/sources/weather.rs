//! BBC Weather forecast for the family frame.
//!
//! BBC does not publish a documented API. The same CDN JSON the website uses
//! (`weather-broker-cdn`) returns hourly reports; we keep 09:00 / 15:00 / 21:00
//! as morning, afternoon, and evening for the calendar headers, plus every
//! hour from 08:00–20:00 for the sidebar strip. Three days are built so an
//! evening calendar rollover can show tomorrow and the day after.

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{Duration as ChronoDuration, NaiveDate};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::config::WeatherConfig;
use crate::model::{Weather, WeatherDay, WeatherHour, WeatherSlot};
use crate::sources::cache::TtlCache;

use super::context::SourceContext;
use super::contribute::{Contribution, SourceOutcome};
use super::{DataSource, DisabledBehaviour};

pub struct WeatherSource;

#[async_trait::async_trait]
impl DataSource for WeatherSource {
    fn id(&self) -> &'static str {
        "weather"
    }

    fn enabled(&self, cfg: &crate::config::Config) -> bool {
        cfg.weather_enabled()
    }

    fn when_disabled(&self, _cfg: &crate::config::Config) -> DisabledBehaviour {
        DisabledBehaviour::Demo
    }

    fn disabled_note(&self) -> String {
        "demo weather (no BBC location)".into()
    }

    async fn load(&self, ctx: &SourceContext<'_>) -> Result<SourceOutcome> {
        match load_forecast(&ctx.cfg.weather, &ctx.cfg.weather_cache_path(), ctx.today).await {
            Ok(forecast) if !forecast.days.is_empty() => Ok(SourceOutcome::live(
                format!("BBC weather “{}”", forecast.location),
                Contribution::Weather(forecast),
            )),
            Ok(_) => {
                tracing::warn!("BBC weather returned no days");
                Ok(SourceOutcome::unavailable(
                    "BBC weather empty — demo forecast",
                    Contribution::Weather(demo_weather()),
                ))
            }
            Err(err) => {
                tracing::warn!(%err, "BBC weather failed; using demo forecast");
                Ok(SourceOutcome::unavailable(
                    "BBC weather unavailable",
                    Contribution::Weather(demo_weather()),
                ))
            }
        }
    }

    fn demo(&self, _ctx: &SourceContext<'_>) -> Option<Contribution> {
        Some(Contribution::Weather(demo_weather()))
    }
}

const FORECAST_URL: &str = "https://weather-broker-cdn.api.bbci.co.uk/en/forecast/aggregated";
const FETCH_TTL: Duration = Duration::from_secs(15 * 60);

static LAST: TtlCache<(String, Weather)> = TtlCache::new();

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

/// Inclusive daytime window for the sidebar hourly strip.
const DAYTIME_FIRST_HOUR: u32 = 8;
const DAYTIME_LAST_HOUR: u32 = 20;

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
    rain_percent: Option<i32>,
}

#[derive(Debug, Clone)]
struct DaySummary {
    temperature_min: Option<i32>,
    temperature_max: Option<i32>,
    weather_type: i64,
    weather_text: String,
    sunrise: String,
    sunset: String,
    pollen: String,
    pollen_level: String,
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
    #[serde(default)]
    rain_percent: Option<i32>,
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
    #[serde(default)]
    sunrise: Option<String>,
    #[serde(default)]
    sunset: Option<String>,
    #[serde(rename = "pollenIndexText", default)]
    pollen_index_text: Option<String>,
    #[serde(rename = "pollenIndexBand", default)]
    pollen_index_band: Option<String>,
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
    #[serde(rename = "precipitationProbabilityInPercent", default)]
    precipitation_probability: Option<i64>,
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
    if let Some((_, weather)) = LAST.get(FETCH_TTL, |(id, weather)| {
        id == location_id && !weather.days.is_empty()
    }) {
        return Ok(weather);
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
    LAST.set((location_id.to_string(), weather.clone()));
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
    let days = (0..3)
        .map(|offset| {
            let date = today + ChronoDuration::days(offset);
            let label = match offset {
                0 => "Today".into(),
                1 => "Tomorrow".into(),
                _ => date.format("%a %-d").to_string(),
            };
            let summary = summaries.get(&date);
            WeatherDay {
                label,
                slots: PERIODS
                    .iter()
                    .map(|period| slot_for(date, period, &hours, &summaries, cache))
                    .collect(),
                hours: daytime_hours(date, &hours, cache),
                sunrise: summary.map(|s| s.sunrise.clone()).unwrap_or_default(),
                sunset: summary.map(|s| s.sunset.clone()).unwrap_or_default(),
                pollen: summary.map(|s| s.pollen.clone()).unwrap_or_default(),
                pollen_level: summary.map(|s| s.pollen_level.clone()).unwrap_or_default(),
            }
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
                hours: demo_hours(&[
                    (8, "sun", "15°", "0%"),
                    (9, "sun", "16°", "0%"),
                    (10, "sun", "17°", "0%"),
                    (11, "partly-cloudy", "18°", "10%"),
                    (12, "partly-cloudy", "19°", "10%"),
                    (13, "partly-cloudy", "20°", "20%"),
                    (14, "partly-cloudy", "21°", "20%"),
                    (15, "cloud", "21°", "30%"),
                    (16, "cloud", "20°", "40%"),
                    (17, "showers", "19°", "50%"),
                    (18, "showers", "18°", "60%"),
                    (19, "rain", "17°", "70%"),
                    (20, "rain", "16°", "70%"),
                ]),
                sunrise: "06:33".into(),
                sunset: "19:18".into(),
                pollen: "Low".into(),
                pollen_level: "low".into(),
            },
            WeatherDay {
                label: "Tomorrow".into(),
                slots: vec![
                    demo_slot("Morning", "cloud", "15°", "Thick cloud"),
                    demo_slot("Afternoon", "storm", "17°", "Thundery showers"),
                    demo_slot("Evening", "moon", "13°", "Clear sky"),
                ],
                hours: demo_hours(&[
                    (8, "overcast", "13°", "40%"),
                    (9, "overcast", "14°", "40%"),
                    (10, "cloud", "14°", "50%"),
                    (11, "cloud", "15°", "50%"),
                    (12, "showers", "15°", "60%"),
                    (13, "showers", "16°", "70%"),
                    (14, "storm", "17°", "80%"),
                    (15, "storm", "17°", "80%"),
                    (16, "showers", "16°", "60%"),
                    (17, "cloud", "15°", "40%"),
                    (18, "cloud", "14°", "20%"),
                    (19, "partly-cloudy-night", "13°", "10%"),
                    (20, "moon", "13°", "0%"),
                ]),
                sunrise: "06:35".into(),
                sunset: "19:16".into(),
                pollen: "Moderate".into(),
                pollen_level: "moderate".into(),
            },
            WeatherDay {
                label: "Mon 21".into(),
                slots: vec![
                    demo_slot("Morning", "sun", "19°", "Sunny"),
                    demo_slot("Afternoon", "partly-cloudy", "22°", "Sunny intervals"),
                    demo_slot("Evening", "cloud", "14°", "Light cloud"),
                ],
                hours: demo_hours(&[
                    (8, "sun", "12°", "0%"),
                    (9, "sun", "13°", "0%"),
                    (10, "sun", "15°", "0%"),
                    (11, "sun", "17°", "0%"),
                    (12, "partly-cloudy", "19°", "10%"),
                    (13, "partly-cloudy", "20°", "10%"),
                    (14, "partly-cloudy", "21°", "10%"),
                    (15, "partly-cloudy", "22°", "10%"),
                    (16, "cloud", "20°", "20%"),
                    (17, "cloud", "18°", "20%"),
                    (18, "cloud", "16°", "10%"),
                    (19, "cloud", "15°", "10%"),
                    (20, "cloud", "14°", "10%"),
                ]),
                sunrise: "06:37".into(),
                sunset: "19:14".into(),
                pollen: "High".into(),
                pollen_level: "high".into(),
            },
        ],
    }
}

/// Drawn `wx-*` symbols. Keep in sync with `templates/wx-sprite.html`.
#[derive(Debug, Clone, Copy)]
pub struct IconSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub codes: &'static str,
}

pub const ICONS: &[IconSpec] = &[
    IconSpec {
        id: "sun",
        label: "Sunny",
        codes: "1",
    },
    IconSpec {
        id: "moon",
        label: "Clear night",
        codes: "0",
    },
    IconSpec {
        id: "partly-cloudy",
        label: "Sunny intervals",
        codes: "3",
    },
    IconSpec {
        id: "partly-cloudy-night",
        label: "Clear intervals (night)",
        codes: "2",
    },
    IconSpec {
        id: "cloud",
        label: "Cloudy",
        codes: "7",
    },
    IconSpec {
        id: "overcast",
        label: "Overcast",
        codes: "8",
    },
    IconSpec {
        id: "drizzle",
        label: "Drizzle",
        codes: "11",
    },
    IconSpec {
        id: "rain",
        label: "Rain",
        codes: "12, 15, 39",
    },
    IconSpec {
        id: "showers",
        label: "Showers",
        codes: "9, 10, 13, 14",
    },
    IconSpec {
        id: "storm",
        label: "Thunder",
        codes: "28–30",
    },
    IconSpec {
        id: "snow",
        label: "Snow",
        codes: "22–27",
    },
    IconSpec {
        id: "sleet",
        label: "Sleet",
        codes: "16–18",
    },
    IconSpec {
        id: "hail",
        label: "Hail",
        codes: "19–21",
    },
    IconSpec {
        id: "fog",
        label: "Fog / mist",
        codes: "5, 6",
    },
    IconSpec {
        id: "unknown",
        label: "Missing slot",
        codes: "—",
    },
];

pub fn icon_for(code: i64, text: &str) -> &'static str {
    match code {
        0 => "moon",
        1 => "sun",
        2 => "partly-cloudy-night",
        3 => "partly-cloudy",
        5 | 6 => "fog",
        7 => "cloud",
        8 => "overcast",
        9 | 10 | 13 | 14 => "showers",
        11 => "drizzle",
        12 | 15 | 39 => "rain",
        16 | 17 | 18 => "sleet",
        19 | 20 | 21 => "hail",
        22 | 23 | 24 | 25 | 26 | 27 => "snow",
        28 | 29 | 30 => "storm",
        _ => icon_from_text(text),
    }
}

fn icon_from_text(text: &str) -> &'static str {
    let t = text.to_ascii_lowercase();
    if t.contains("thunder") || t.contains("lightning") {
        "storm"
    } else if t.contains("hail") {
        "hail"
    } else if t.contains("sleet") {
        "sleet"
    } else if t.contains("snow") {
        "snow"
    } else if t.contains("fog") || t.contains("mist") {
        "fog"
    } else if t.contains("drizzle") {
        "drizzle"
    } else if t.contains("shower") {
        "showers"
    } else if t.contains("rain") {
        "rain"
    } else if t.contains("overcast") {
        "overcast"
    } else if t.contains("cloud") {
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

fn demo_hours(rows: &[(u32, &str, &str, &str)]) -> Vec<WeatherHour> {
    rows.iter()
        .map(|(hour, icon, temperature, rain)| WeatherHour {
            label: hour.to_string(),
            icon: (*icon).into(),
            temperature: (*temperature).into(),
            rain: (*rain).into(),
        })
        .collect()
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
                rain_percent: report.precipitation_probability.map(|p| p as i32),
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

fn daytime_hours(date: NaiveDate, hours: &[Hourly], cache: &mut SlotCache) -> Vec<WeatherHour> {
    (DAYTIME_FIRST_HOUR..=DAYTIME_LAST_HOUR)
        .map(|hour| hour_for(date, hour, hours, cache))
        .collect()
}

fn hour_for(date: NaiveDate, hour: u32, hours: &[Hourly], cache: &mut SlotCache) -> WeatherHour {
    let key = hour_cache_key(hour);
    if let Some(picked) = hours.iter().find(|h| h.date == date && h.hour == hour) {
        cache.put(date, &key, picked);
        return weather_hour_from(picked);
    }
    if let Some(cached) = cache.get(date, &key) {
        return WeatherHour {
            label: hour.to_string(),
            icon: icon_for(cached.weather_type, &cached.weather_text).into(),
            temperature: format!("{}°", cached.temperature_c),
            rain: format_rain(cached.rain_percent),
        };
    }
    WeatherHour {
        label: hour.to_string(),
        icon: "unknown".into(),
        temperature: "—".into(),
        rain: "—".into(),
    }
}

fn weather_hour_from(hour: &Hourly) -> WeatherHour {
    WeatherHour {
        label: hour.hour.to_string(),
        icon: icon_for(hour.weather_type, &hour.weather_text).into(),
        temperature: format!("{}°", hour.temperature_c),
        rain: format_rain(hour.rain_percent),
    }
}

fn format_rain(percent: Option<i32>) -> String {
    match percent {
        Some(p) => format!("{p}%"),
        None => "—".into(),
    }
}

fn hour_cache_key(hour: u32) -> String {
    format!("h{hour:02}")
}

fn collect_summaries(parsed: &Aggregated) -> HashMap<NaiveDate, DaySummary> {
    parsed
        .forecasts
        .iter()
        .filter_map(|day| {
            let report = day.summary.report.as_ref()?;
            let date = NaiveDate::parse_from_str(report.local_date.as_deref()?, "%Y-%m-%d").ok()?;
            let (pollen, pollen_level) = pollen_from_report(report);
            Some((
                date,
                DaySummary {
                    temperature_min: report.min_temp_c.map(|t| t as i32),
                    temperature_max: report.max_temp_c.map(|t| t as i32),
                    weather_type: report.weather_type.unwrap_or(-1),
                    weather_text: report.weather_type_text.clone().unwrap_or_default(),
                    sunrise: report.sunrise.clone().unwrap_or_default(),
                    sunset: report.sunset.clone().unwrap_or_default(),
                    pollen,
                    pollen_level,
                },
            ))
        })
        .collect()
}

fn pollen_from_report(report: &SummaryReport) -> (String, String) {
    let text = report
        .pollen_index_text
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_string();
    let band = report
        .pollen_index_band
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let level = match band.as_str() {
        "low" => "low",
        "moderate" | "medium" => "moderate",
        "high" | "very high" | "veryhigh" => "high",
        _ => {
            let t = text.to_ascii_lowercase();
            if t.contains("very high") || t == "high" {
                "high"
            } else if t.contains("moderate") || t.contains("medium") {
                "moderate"
            } else if t.contains("low") {
                "low"
            } else {
                ""
            }
        }
    };
    (text, level.into())
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
                rain_percent: hour.rain_percent,
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
        assert_eq!(weather.days.len(), 3);

        let today_slots = &weather.days[0].slots;
        assert_eq!(today_slots[0].period, "Morning");
        assert_eq!(today_slots[0].icon, "sun");
        assert_eq!(today_slots[0].temperature, "16°");
        assert_eq!(today_slots[1].icon, "cloud");
        assert_eq!(today_slots[1].temperature, "21°");
        assert_eq!(today_slots[2].icon, "rain");
        assert_eq!(today_slots[2].temperature, "15°");

        let tomorrow = &weather.days[1].slots;
        assert_eq!(tomorrow[0].icon, "overcast");
        assert_eq!(tomorrow[0].temperature, "14°");
        assert_eq!(tomorrow[1].icon, "storm");
        assert_eq!(tomorrow[1].temperature, "17°");
        assert_eq!(tomorrow[2].icon, "moon");
        assert_eq!(tomorrow[2].temperature, "13°");

        assert_eq!(weather.days[0].sunrise, "06:33");
        assert_eq!(weather.days[0].sunset, "19:18");
        assert_eq!(weather.days[0].pollen, "Low");
        assert_eq!(weather.days[0].pollen_level, "low");
        assert_eq!(weather.days[1].sunrise, "06:35");
        assert_eq!(weather.days[1].pollen, "Moderate");
        assert_eq!(weather.days[1].pollen_level, "moderate");
        assert_eq!(weather.days[2].label, "Mon 14");
        assert_eq!(weather.days[2].slots[0].icon, "sun");
        assert_eq!(weather.days[2].slots[0].temperature, "12°");
        assert_eq!(weather.days[2].pollen, "High");
        assert_eq!(weather.days[2].pollen_level, "high");
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
                    rain_percent: None,
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
        assert_eq!(icon_for(8, ""), "overcast");
        assert_eq!(icon_for(10, ""), "showers");
        assert_eq!(icon_for(11, ""), "drizzle");
        assert_eq!(icon_for(18, ""), "sleet");
        assert_eq!(icon_for(21, ""), "hail");
        assert_eq!(icon_for(29, ""), "storm");
        assert_eq!(icon_for(24, ""), "snow");
        assert_eq!(icon_for(6, ""), "fog");
        assert_eq!(icon_for(39, "Light Rain"), "rain");
        assert_eq!(icon_for(99, "Thundery showers"), "storm");
        assert_eq!(icon_for(99, "Hail shower"), "hail");
        assert_eq!(icon_for(99, "Light drizzle"), "drizzle");
    }

    #[test]
    fn sprite_defines_every_catalog_icon() {
        let ids: Vec<_> = ICONS.iter().map(|i| i.id).collect();
        let sprite = crate::assets::WX_SPRITE_HTML;
        assert!(sprite.contains("fill=\"#ff8800\""));
        assert!(!sprite.contains("id=\"wx-orange\""));
        assert!(sprite.contains("stroke=\"#000000\""));
        for icon in ICONS {
            assert!(
                sprite.contains(&format!("id=\"wx-{}\"", icon.id)),
                "missing symbol wx-{}",
                icon.id
            );
        }
        for code in 0..40 {
            let id = icon_for(code, "");
            assert!(ids.contains(&id), "BBC {code} maps to {id}, not in ICONS");
        }
        assert!(ids.contains(&"unknown"));
    }

    #[test]
    fn parse_hour_accepts_short_times() {
        assert_eq!(parse_hour("09:00"), Some(9));
        assert_eq!(parse_hour("9:00"), Some(9));
        assert_eq!(parse_hour("21:00"), Some(21));
    }

    #[test]
    fn daytime_hours_include_rain_chance() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 12).unwrap();
        let json = r#"{
            "location": {"name": "London"},
            "forecasts": [{
                "detailed": {"reports": [
                    {"localDate": "2026-09-12", "timeslot": "08:00",
                     "temperatureC": 14, "weatherType": 1, "weatherTypeText": "Sunny",
                     "precipitationProbabilityInPercent": 0},
                    {"localDate": "2026-09-12", "timeslot": "12:00",
                     "temperatureC": 19, "weatherType": 3, "weatherTypeText": "Sunny Intervals",
                     "precipitationProbabilityInPercent": 20},
                    {"localDate": "2026-09-12", "timeslot": "15:00",
                     "temperatureC": 21, "weatherType": 7, "weatherTypeText": "Light Cloud",
                     "precipitationProbabilityInPercent": 30},
                    {"localDate": "2026-09-12", "timeslot": "20:00",
                     "temperatureC": 16, "weatherType": 12, "weatherTypeText": "Light Rain",
                     "precipitationProbabilityInPercent": 70}
                ]}
            }, {
                "detailed": {"reports": [
                    {"localDate": "2026-09-13", "timeslot": "08:00",
                     "temperatureC": 12, "weatherType": 8, "weatherTypeText": "Thick Cloud",
                     "precipitationProbabilityInPercent": 40},
                    {"localDate": "2026-09-13", "timeslot": "14:00",
                     "temperatureC": 17, "weatherType": 28, "weatherTypeText": "Thunder",
                     "precipitationProbabilityInPercent": 80}
                ]}
            }]
        }"#;
        let mut cache = SlotCache::default();
        let weather = forecast_from_json(json, today, &mut cache).unwrap();
        let hours = &weather.days[0].hours;
        assert_eq!(hours.len(), 13);
        assert_eq!(hours[0].label, "8");
        assert_eq!(hours[0].temperature, "14°");
        assert_eq!(hours[0].rain, "0%");
        assert_eq!(hours[0].icon, "sun");
        assert_eq!(hours[4].label, "12");
        assert_eq!(hours[4].temperature, "19°");
        assert_eq!(hours[4].rain, "20%");
        assert_eq!(hours[7].label, "15");
        assert_eq!(hours[7].rain, "30%");
        assert_eq!(hours[12].label, "20");
        assert_eq!(hours[12].temperature, "16°");
        assert_eq!(hours[12].rain, "70%");
        assert_eq!(hours[12].icon, "rain");
        // Gaps between reported hours stay blank until cached.
        assert_eq!(hours[1].temperature, "—");
        assert_eq!(hours[1].rain, "—");

        let tomorrow = &weather.days[1].hours;
        assert_eq!(tomorrow[0].temperature, "12°");
        assert_eq!(tomorrow[0].rain, "40%");
        assert_eq!(tomorrow[6].temperature, "17°");
        assert_eq!(tomorrow[6].rain, "80%");
        assert_eq!(tomorrow[6].icon, "storm");
    }

    #[test]
    fn missing_daytime_hour_uses_cache() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 12).unwrap();
        let evening_only = r#"{
            "location": {"name": "London"},
            "forecasts": [{
                "detailed": {"reports": [
                    {"localDate": "2026-09-12", "timeslot": "20:00",
                     "temperatureC": 16, "weatherType": 12, "weatherTypeText": "Light Rain",
                     "precipitationProbabilityInPercent": 70}
                ]}
            }]
        }"#;
        let mut cache = SlotCache {
            location_id: "2643743".into(),
            slots: HashMap::from([(
                "2026-09-12-h08".into(),
                CachedSlot {
                    temperature_c: 14,
                    weather_type: 1,
                    weather_text: "Sunny".into(),
                    rain_percent: Some(0),
                },
            )]),
        };
        let weather = forecast_from_json(evening_only, today, &mut cache).unwrap();
        let hours = &weather.days[0].hours;
        assert_eq!(hours[0].temperature, "14°");
        assert_eq!(hours[0].icon, "sun");
        assert_eq!(hours[0].rain, "0%");
        assert_eq!(hours[12].temperature, "16°");
        assert_eq!(hours[12].rain, "70%");
        assert_eq!(hours[1].temperature, "—");
    }

    #[test]
    fn demo_weather_has_full_daytime_strip() {
        let weather = demo_weather();
        assert_eq!(weather.days[0].hours.len(), 13);
        assert_eq!(weather.days[0].hours[0].label, "8");
        assert_eq!(weather.days[0].hours[12].label, "20");
        assert!(weather.days[0].hours.iter().all(|h| h.rain.ends_with('%')));
    }
}
