//! Wikipedia “On this day” facts for the family frame.
//!
//! Uses the public REST feed, keeps a day’s facts in memory, and drops grim
//! items (crashes, murders) so the kitchen board stays family-friendly.

use std::sync::Mutex;

use anyhow::{Context, Result};
use chrono::{Datelike, NaiveDate};
use serde::Deserialize;
use tracing::info;

use crate::model::{HistoryFact, HISTORY_POOL};

use super::contribute::{Contribution, SourceOutcome};
use super::context::SourceContext;
use super::DataSource;

pub struct HistorySource;

#[async_trait::async_trait]
impl DataSource for HistorySource {
    fn id(&self) -> &'static str {
        "history"
    }

    fn enabled(&self, _cfg: &crate::config::Config) -> bool {
        true
    }

    async fn load(&self, ctx: &SourceContext<'_>) -> Result<SourceOutcome> {
        match load_facts(ctx.today).await {
            Ok(facts) if !facts.is_empty() => Ok(SourceOutcome::live(
                "Wikipedia on this day",
                Contribution::History(facts),
            )),
            Ok(_) => Ok(SourceOutcome::live(
                "On this day empty",
                Contribution::History(Vec::new()),
            )),
            Err(err) => {
                tracing::warn!(%err, "On this day failed");
                Ok(SourceOutcome::unavailable(
                    "On this day unavailable",
                    Contribution::None,
                ))
            }
        }
    }
}

const SELECTED_URL: &str = "https://en.wikipedia.org/api/rest_v1/feed/onthisday/selected";
const EVENTS_URL: &str = "https://en.wikipedia.org/api/rest_v1/feed/onthisday/events";

static LAST: Mutex<Option<(NaiveDate, Vec<HistoryFact>)>> = Mutex::new(None);

const SKIP_TERMS: &[&str] = &[
    "assassin",
    "battle",
    "bomb",
    "crash",
    "dead",
    "death",
    "died",
    "executed",
    "fatal",
    "genocide",
    "holocaust",
    "kill",
    "lynch",
    "massacre",
    "murder",
    "rape",
    "slaughter",
    "suicide",
    "terror",
    "torture",
];
/// Short words that would over-match as substrings (`war` in `award`).
const SKIP_WORDS: &[&str] = &["shot", "slave", "slaves", "war", "wars"];

#[derive(Debug, Deserialize, Default)]
struct Feed {
    #[serde(default)]
    selected: Vec<RawEvent>,
    #[serde(default)]
    events: Vec<RawEvent>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawEvent {
    year: Option<i32>,
    text: Option<String>,
}

pub async fn load_facts(today: NaiveDate) -> Result<Vec<HistoryFact>> {
    if let Some((date, facts)) = LAST.lock().ok().and_then(|g| g.clone()) {
        if date == today && !facts.is_empty() {
            return Ok(facts);
        }
    }

    let mut raw = fetch_feed(SELECTED_URL, today).await.unwrap_or_default();
    let selected = facts_from_raw(&raw);
    if selected.len() < HISTORY_POOL {
        if let Ok(extra) = fetch_feed(EVENTS_URL, today).await {
            raw.extend(extra);
        }
    }
    let facts = pick_facts(&raw, HISTORY_POOL);
    if facts.is_empty() {
        anyhow::bail!("no family-friendly on-this-day facts");
    }
    if let Ok(mut guard) = LAST.lock() {
        *guard = Some((today, facts.clone()));
    }
    info!(n = facts.len(), "loaded Wikipedia on this day");
    Ok(facts)
}

async fn fetch_feed(base: &str, today: NaiveDate) -> Result<Vec<RawEvent>> {
    let url = format!("{base}/{:02}/{:02}", today.month(), today.day());
    let ua = format!("family-frame/{} (household e-ink frame)", crate::VERSION);
    let body = reqwest::Client::new()
        .get(&url)
        .header("accept", "application/json")
        .header("user-agent", &ua)
        .header("api-user-agent", &ua)
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .with_context(|| format!("Wikipedia GET {url}"))?
        .error_for_status()
        .with_context(|| format!("Wikipedia status {url}"))?
        .text()
        .await?;
    Ok(raw_from_json(&body))
}

fn raw_from_json(json: &str) -> Vec<RawEvent> {
    let feed: Feed = serde_json::from_str(json).unwrap_or_default();
    let mut raw = feed.selected;
    raw.extend(feed.events);
    raw
}

fn facts_from_raw(raw: &[RawEvent]) -> Vec<HistoryFact> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for event in raw {
        let Some(year) = event.year else { continue };
        let Some(text) = event.text.as_deref() else {
            continue;
        };
        let text = tidy_text(text);
        if text.is_empty() || !family_friendly(&text) {
            continue;
        }
        let key = (year, text.clone());
        if !seen.insert(key) {
            continue;
        }
        out.push(HistoryFact {
            year: format_year(year),
            text,
        });
    }
    out
}

fn pick_facts(raw: &[RawEvent], n: usize) -> Vec<HistoryFact> {
    let mut facts = facts_from_raw(raw);
    facts.truncate(n);
    facts.sort_by_key(|fact| history_year_key(&fact.year));
    facts
}

fn history_year_key(year: &str) -> i32 {
    if let Some(bc) = year.strip_suffix(" BC") {
        return -bc.parse::<i32>().unwrap_or(0);
    }
    year.parse().unwrap_or(0)
}

fn format_year(year: i32) -> String {
    if year < 0 {
        format!("{} BC", -year)
    } else {
        year.to_string()
    }
}

fn family_friendly(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    if SKIP_TERMS.iter().any(|term| lower.contains(term)) {
        return false;
    }
    !SKIP_WORDS.iter().any(|word| contains_word(&lower, word))
}

fn contains_word(hay: &str, word: &str) -> bool {
    let mut from = 0;
    while let Some(rel) = hay[from..].find(word) {
        let at = from + rel;
        let before_ok = at == 0 || !hay.as_bytes()[at - 1].is_ascii_alphabetic();
        let end = at + word.len();
        let after_ok = end >= hay.len() || !hay.as_bytes()[end].is_ascii_alphabetic();
        if before_ok && after_ok {
            return true;
        }
        from = at + 1;
    }
    false
}

fn tidy_text(text: &str) -> String {
    let mut t = text.replace('\u{00a0}', " ");
    for needle in [" (example pictured)", " (pictured)"] {
        if let Some(at) = t.find(needle) {
            t.replace_range(at..at + needle.len(), "");
        }
    }
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn demo_history() -> Vec<HistoryFact> {
    vec![
        HistoryFact {
            year: "1851".into(),
            text: "The New York Times is founded.".into(),
        },
        HistoryFact {
            year: "1879".into(),
            text: "Blackpool Illuminations are switched on for the first time.".into(),
        },
        HistoryFact {
            year: "1964".into(),
            text: "King Constantine II of Greece marries Princess Anne-Marie.".into(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> String {
        std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/onthisday.json"),
        )
        .unwrap()
    }

    #[test]
    fn skips_grim_events_and_orders_oldest_first() {
        let facts = pick_facts(&raw_from_json(&fixture()), 3);
        assert_eq!(facts.len(), 3);
        let joined = facts
            .iter()
            .map(|f| format!("{} {}", f.year, f.text))
            .collect::<Vec<_>>()
            .join(" ");
        assert!(!joined.to_ascii_lowercase().contains("murder"));
        assert!(!joined.to_ascii_lowercase().contains("crash"));
        assert!(!joined.to_ascii_lowercase().contains("battle"));
        assert_eq!(facts[0].year, "1851");
        assert_eq!(facts[1].year, "1879");
        assert_eq!(facts[2].year, "1964");
        assert!(facts.iter().any(|f| f.text.contains("Blackpool")));
        assert!(facts.iter().any(|f| f.text.contains("New York Times")));
    }

    #[test]
    fn formats_bc_years_and_strips_pictured() {
        let json = r#"{"selected":[{"year":-44,"text":"Julius Caesar (pictured) is born."}]}"#;
        let facts = facts_from_raw(&raw_from_json(json));
        assert_eq!(facts[0].year, "44 BC");
        assert_eq!(facts[0].text, "Julius Caesar is born.");
    }

    #[test]
    fn keeps_full_fact_text() {
        let json = r#"{"selected":[{"year":1851,"text":"The New York Times, the largest metropolitan newspaper in the United States, was founded."}]}"#;
        let facts = facts_from_raw(&raw_from_json(json));
        assert!(facts[0].text.contains("largest metropolitan newspaper"));
    }

    #[test]
    fn skips_wars_but_not_awards() {
        assert!(family_friendly(
            "Marie Curie receives an award in Stockholm."
        ));
        assert!(!family_friendly(
            "Korean War: troops retreat from the Pusan Perimeter."
        ));
        assert!(!family_friendly(
            "Constantine defeated Licinius in the Battle of Chrysopolis."
        ));
    }
}
