//! Wandsworth food & recycling as one all-day calendar row.
//!
//! Dates come from the [UK Bin Day](https://ukbinday.co.uk/api-docs) JSON
//! lookup (`/api/v1/lookup/{uprn}`). Only food waste and recycling are kept.

use std::time::Duration;

use anyhow::Result;
use chrono::{Datelike, Duration as ChronoDuration, NaiveDate, Weekday};
use serde::Deserialize;
use tracing::{info, warn};

use crate::model::CalendarEvent;
use crate::sources::cache::TtlCache;

use super::context::SourceContext;
use super::contribute::{Contribution, SourceOutcome};
use super::ics;
use super::{DataSource, DisabledBehaviour};

pub struct BinsSource;

const LOOKUP_URL: &str = "https://ukbinday.co.uk/api/v1/lookup";
const COUNCIL: &str = "hacs_wandsworth_gov_uk";
const FETCH_TTL: Duration = Duration::from_secs(6 * 60 * 60);
const KEEP: &[&str] = &["Food waste", "Recycling"];
const MERGED_TITLE: &str = "Bins collection";

static LAST: TtlCache<(String, LookupResponse)> = TtlCache::new();

#[derive(Debug, Clone, Deserialize)]
struct LookupResponse {
    #[serde(default)]
    collections: Vec<CollectionItem>,
}

#[derive(Debug, Clone, Deserialize)]
struct CollectionItem {
    date: NaiveDate,
    #[serde(rename = "type")]
    kind: String,
}

#[async_trait::async_trait]
impl DataSource for BinsSource {
    fn id(&self) -> &'static str {
        "bins"
    }

    fn enabled(&self, cfg: &crate::config::Config) -> bool {
        cfg.bins_enabled()
    }

    fn private(&self) -> bool {
        true
    }

    fn when_disabled(&self, _cfg: &crate::config::Config) -> DisabledBehaviour {
        DisabledBehaviour::Skip
    }

    fn disabled_note(&self) -> String {
        "demo bins".into()
    }

    fn demo(&self, ctx: &SourceContext<'_>) -> Option<Contribution> {
        Some(Contribution::Calendar(demo_events(ctx.today)))
    }

    async fn load(&self, ctx: &SourceContext<'_>) -> Result<SourceOutcome> {
        let uprn = match crate::config::normalize_uprn(&ctx.cfg.sources.bins.uprn) {
            Ok(uprn) if !uprn.is_empty() => uprn,
            Ok(_) => {
                return Ok(SourceOutcome::live(String::new(), Contribution::None));
            }
            Err(err) => {
                warn!(%err, "invalid bin UPRN");
                return Ok(SourceOutcome::unavailable(
                    "bins unavailable",
                    Contribution::Calendar(Vec::new()),
                ));
            }
        };
        match load_lookup(&ctx.http, &uprn).await {
            Ok(lookup) => {
                let events = events_from_lookup(&lookup, ctx.today);
                if events.is_empty() {
                    warn!(uprn, "UK Bin Day returned no food/recycling dates");
                    Ok(SourceOutcome::unavailable(
                        "bins unavailable",
                        Contribution::Calendar(Vec::new()),
                    ))
                } else {
                    info!(uprn, n = events.len(), "loaded UK Bin Day");
                    Ok(SourceOutcome::live(
                        "UK Bin Day",
                        Contribution::Calendar(events),
                    ))
                }
            }
            Err(err) => {
                warn!(uprn, %err, "UK Bin Day failed");
                Ok(SourceOutcome::unavailable(
                    "bins unavailable",
                    Contribution::Calendar(Vec::new()),
                ))
            }
        }
    }
}

pub fn demo_events(today: NaiveDate) -> Vec<CalendarEvent> {
    vec![bin_event(
        next_weekday(today, Weekday::Wed),
        today,
        MERGED_TITLE,
    )]
}

fn next_weekday(today: NaiveDate, weekday: Weekday) -> NaiveDate {
    let ahead = (weekday.num_days_from_monday() + 7 - today.weekday().num_days_from_monday()) % 7;
    today + ChronoDuration::days(i64::from(ahead))
}

async fn load_lookup(http: &reqwest::Client, uprn: &str) -> Result<LookupResponse> {
    if let Some(cached) = LAST.get(FETCH_TTL, |(cached, _)| cached == uprn) {
        return Ok(cached.1);
    }
    let lookup = http
        .get(format!("{LOOKUP_URL}/{uprn}"))
        .query(&[("council", COUNCIL)])
        .send()
        .await?
        .error_for_status()?
        .json::<LookupResponse>()
        .await?;
    LAST.set((uprn.to_string(), lookup.clone()));
    Ok(lookup)
}

fn events_from_lookup(lookup: &LookupResponse, today: NaiveDate) -> Vec<CalendarEvent> {
    merge_collections(&lookup.collections, today)
        .into_iter()
        .collect()
}

fn merge_collections(items: &[CollectionItem], today: NaiveDate) -> Option<CalendarEvent> {
    let kept: Vec<&CollectionItem> = items
        .iter()
        .filter(|item| KEEP.contains(&item.kind.as_str()))
        .collect();
    if kept.is_empty() {
        return None;
    }
    let next = kept.iter().map(|item| item.date).min()?;
    let title = if kept.len() == 1 {
        kept[0].kind.as_str()
    } else {
        MERGED_TITLE
    };
    Some(bin_event(next, today, title))
}

fn bin_event(date: NaiveDate, today: NaiveDate, title: &str) -> CalendarEvent {
    CalendarEvent {
        start: String::new(),
        title: title.into(),
        all_day: true,
        day_label: ics::day_label(date, today),
        date: date.format("%Y-%m-%d").to_string(),
        birthday: false,
        school: false,
        recurring: false,
        bin: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/ukbinday_wandsworth.json"
    ));

    fn lookup(json: &str) -> LookupResponse {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn merges_food_and_recycling_as_all_day() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 19).unwrap();
        let events = events_from_lookup(&lookup(FIXTURE), today);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].title, "Bins collection");
        assert_eq!(events[0].date, "2026-09-23");
        assert_eq!(events[0].start, "");
        assert_eq!(events[0].day_label, "Wed 23");
        assert!(events[0].bin);
        assert!(events[0].all_day);
    }

    #[test]
    fn today_uses_today_label() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 23).unwrap();
        let events = events_from_lookup(&lookup(FIXTURE), today);
        assert_eq!(events[0].day_label, "Today");
        assert!(events[0].all_day);
    }

    #[test]
    fn ignores_rubbish_and_electrical() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 19).unwrap();
        let event = merge_collections(&lookup(FIXTURE).collections, today).unwrap();
        assert_eq!(event.title, "Bins collection");
        assert_eq!(event.date, "2026-09-23");
    }

    #[test]
    fn single_kept_stream_keeps_its_name() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 19).unwrap();
        let items = [CollectionItem {
            date: NaiveDate::from_ymd_opt(2026, 9, 24).unwrap(),
            kind: "Recycling".into(),
        }];
        let event = merge_collections(&items, today).unwrap();
        assert_eq!(event.title, "Recycling");
        assert_eq!(event.date, "2026-09-24");
        assert!(event.all_day);
    }

    #[test]
    fn earlier_next_date_wins_when_streams_split() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 19).unwrap();
        let items = [
            CollectionItem {
                date: NaiveDate::from_ymd_opt(2026, 9, 25).unwrap(),
                kind: "Food waste".into(),
            },
            CollectionItem {
                date: NaiveDate::from_ymd_opt(2026, 9, 23).unwrap(),
                kind: "Recycling".into(),
            },
        ];
        let event = merge_collections(&items, today).unwrap();
        assert_eq!(event.date, "2026-09-23");
        assert_eq!(event.title, "Bins collection");
    }

    #[test]
    fn demo_sits_on_this_or_next_wednesday() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 19).unwrap();
        let events = demo_events(today);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].date, "2026-09-23");
        assert!(events[0].all_day);
        assert!(events[0].bin);
        assert_eq!(
            demo_events(NaiveDate::from_ymd_opt(2026, 9, 23).unwrap())[0].date,
            "2026-09-23"
        );
    }
}
