//! Kid-friendly dad jokes for the family frame.
//!
//! [icanhazdadjoke](https://icanhazdadjoke.com/api) is a large, curated dad-joke
//! feed (no API key). A random search page is fetched on every dashboard
//! rebuild so the panel can change the joke at each refresh. Grim, adult, and
//! bar-room items are dropped before anything hits the kitchen board.

use std::sync::Mutex;

use anyhow::{Context, Result};
use rand::seq::SliceRandom;
use rand::Rng;
use serde::Deserialize;
use tracing::{info, warn};

use crate::model::{joke_fits_panel, Joke};
use crate::sources::filter::is_family_friendly;

use super::contribute::{Contribution, SourceOutcome};
use super::context::SourceContext;
use super::DataSource;

pub struct JokesSource;

#[async_trait::async_trait]
impl DataSource for JokesSource {
    fn id(&self) -> &'static str {
        "jokes"
    }

    fn enabled(&self, cfg: &crate::config::Config) -> bool {
        cfg.sources.jokes.enabled
    }

    async fn load(&self, _ctx: &SourceContext<'_>) -> Result<SourceOutcome> {
        match load_joke().await {
            Ok(joke) => Ok(SourceOutcome::live(
                "icanhazdadjoke",
                Contribution::Joke(joke),
            )),
            Err(err) => {
                warn!(%err, "Joke of the day failed; using a classic");
                Ok(SourceOutcome::unavailable(
                    "demo joke",
                    Contribution::Joke(fallback_joke()),
                ))
            }
        }
    }

    fn demo(&self, _ctx: &SourceContext<'_>) -> Option<Contribution> {
        Some(Contribution::Joke(fallback_joke()))
    }
}

const SEARCH_URL: &str = "https://icanhazdadjoke.com/search";
const RANDOM_URL: &str = "https://icanhazdadjoke.com/";
const SEARCH_PAGES: u32 = 24;
const SEARCH_LIMIT: u32 = 30;

static LAST_ID: Mutex<Option<String>> = Mutex::new(None);

const SKIP_TERMS: &[&str] = &[
    "bartender",
    "boyfriend",
    "cocaine",
    "condom",
    "girlfriend",
    "hangover",
    "marijuana",
    "murder",
    "naked",
    "nsfw",
    "pregnant",
    "suicide",
    "viagra",
    "whiskey",
    "i'm changing",
    "into a bar",
];
const SKIP_WORDS: &[&str] = &[
    "adult", "ass", "beer", "bloody", "boob", "crap", "damn", "dead", "die", "died", "drug",
    "drunk", "fart", "fuck", "hell", "horny", "kill", "nude", "pee", "penis", "piss", "poop",
    "porn", "rape", "sex", "sexy", "shit", "shot", "slut", "stoned", "toilet", "vodka", "weed",
    "wine",
];

const FALLBACK: &[(&str, &str)] = &[
    (
        "Why don't scientists trust atoms?",
        "Because they make up everything.",
    ),
    (
        "Why did the scarecrow win an award?",
        "Because he was outstanding in his field.",
    ),
    ("What do you call a bear with no teeth?", "A gummy bear."),
    (
        "Why did the bicycle fall over?",
        "Because it was two-tired.",
    ),
    ("What do you call cheese that isn't yours?", "Nacho cheese."),
    ("What do you call a fake noodle?", "An impasta."),
    ("What do you call a sleeping bull?", "A bulldozer."),
    (
        "Why did the math book look sad?",
        "Because it had too many problems.",
    ),
    ("What's orange and sounds like a parrot?", "A carrot."),
    ("Why don't eggs tell jokes?", "They'd crack each other up."),
    (
        "What did the ocean say to the beach?",
        "Nothing, it just waved.",
    ),
    (
        "What do you call an alligator in a vest?",
        "An investigator.",
    ),
];

#[derive(Debug, Deserialize, Default)]
struct Feed {
    #[serde(default)]
    results: Vec<RawJoke>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    joke: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawJoke {
    #[serde(default)]
    id: String,
    #[serde(default)]
    joke: String,
}

#[derive(Debug, Clone)]
struct Candidate {
    id: String,
    joke: Joke,
}

pub async fn load_joke() -> Result<Joke> {
    let avoid = LAST_ID.lock().ok().and_then(|g| g.clone());
    match fetch_search_joke(avoid.as_deref()).await {
        Ok(Some(picked)) => {
            remember(&picked.id);
            info!(id = %picked.id, "loaded icanhazdadjoke");
            return Ok(picked.joke);
        }
        Ok(None) => {}
        Err(err) => warn!(%err, "icanhazdadjoke search failed"),
    }
    match fetch_random_joke(avoid.as_deref()).await {
        Ok(Some(picked)) => {
            remember(&picked.id);
            info!(id = %picked.id, "loaded icanhazdadjoke random");
            return Ok(picked.joke);
        }
        Ok(None) => {}
        Err(err) => warn!(%err, "icanhazdadjoke random failed"),
    }
    anyhow::bail!("no family-friendly dad joke")
}

async fn fetch_search_joke(avoid: Option<&str>) -> Result<Option<Candidate>> {
    for _ in 0..2 {
        let page = rand::thread_rng().gen_range(1..=SEARCH_PAGES);
        let raw = fetch_json(&search_url(page)).await?;
        if let Some(picked) = pick_candidate(&candidates_from_raw(&raw), avoid) {
            return Ok(Some(picked));
        }
    }
    Ok(None)
}

async fn fetch_random_joke(avoid: Option<&str>) -> Result<Option<Candidate>> {
    for _ in 0..3 {
        let raw = fetch_json(RANDOM_URL).await?;
        if let Some(picked) = pick_candidate(&candidates_from_raw(&raw), avoid) {
            return Ok(Some(picked));
        }
    }
    Ok(None)
}

async fn fetch_json(url: &str) -> Result<Vec<RawJoke>> {
    let ua = format!(
        "family-frame/{} (https://github.com/antoinejaussoin/family-frame)",
        crate::VERSION
    );
    let body = reqwest::Client::new()
        .get(url)
        .header("accept", "application/json")
        .header("user-agent", &ua)
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .with_context(|| format!("icanhazdadjoke GET {url}"))?
        .error_for_status()
        .with_context(|| format!("icanhazdadjoke status {url}"))?
        .text()
        .await?;
    Ok(raw_from_json(&body))
}

fn search_url(page: u32) -> String {
    format!("{SEARCH_URL}?limit={SEARCH_LIMIT}&page={page}")
}

fn raw_from_json(json: &str) -> Vec<RawJoke> {
    let feed: Feed = serde_json::from_str(json).unwrap_or_default();
    if !feed.results.is_empty() {
        return feed.results;
    }
    match (feed.id, feed.joke) {
        (Some(id), Some(joke)) if !joke.trim().is_empty() => vec![RawJoke { id, joke }],
        _ => Vec::new(),
    }
}

fn candidates_from_raw(raw: &[RawJoke]) -> Vec<Candidate> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for item in raw {
        if item.joke.contains('\n') || item.joke.contains('\r') {
            continue;
        }
        let text = tidy_text(&item.joke);
        if text.is_empty() || !family_friendly(&text) {
            continue;
        }
        let joke = split_joke(&text);
        if !joke_fits_panel(&joke) {
            continue;
        }
        let id = if item.id.is_empty() {
            text.clone()
        } else {
            item.id.clone()
        };
        if !seen.insert(id.clone()) {
            continue;
        }
        out.push(Candidate { id, joke });
    }
    out
}

fn pick_candidate(candidates: &[Candidate], avoid: Option<&str>) -> Option<Candidate> {
    let mut pool: Vec<&Candidate> = candidates
        .iter()
        .filter(|c| Some(c.id.as_str()) != avoid)
        .collect();
    if pool.is_empty() {
        pool = candidates.iter().collect();
    }
    let qa: Vec<&Candidate> = pool
        .iter()
        .copied()
        .filter(|c| !c.joke.punchline.is_empty())
        .collect();
    let use_pool = if qa.is_empty() { pool } else { qa };
    use_pool.choose(&mut rand::thread_rng()).copied().cloned()
}

fn remember(id: &str) {
    if let Ok(mut guard) = LAST_ID.lock() {
        *guard = Some(id.to_string());
    }
}

fn family_friendly(text: &str) -> bool {
    is_family_friendly(text, SKIP_TERMS, SKIP_WORDS)
}

fn split_joke(text: &str) -> Joke {
    if let Some(at) = text.find('?') {
        let setup = text[..=at].trim();
        let punchline = text[at + 1..].trim();
        if setup.chars().count() >= 12 && !punchline.is_empty() {
            return Joke {
                setup: setup.to_string(),
                punchline: punchline.to_string(),
            };
        }
    }
    Joke {
        setup: text.to_string(),
        punchline: String::new(),
    }
}

fn tidy_text(text: &str) -> String {
    let t = text
        .replace('\u{00a0}', " ")
        .replace(['\u{2018}', '\u{2019}'], "'")
        .replace('\r', "\n");
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn demo_joke() -> Joke {
    Joke {
        setup: FALLBACK[0].0.into(),
        punchline: FALLBACK[0].1.into(),
    }
}

pub fn fallback_joke() -> Joke {
    let mut rng = rand::thread_rng();
    let &(setup, punchline) = FALLBACK.choose(&mut rng).unwrap_or(&FALLBACK[0]);
    Joke {
        setup: setup.into(),
        punchline: punchline.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> String {
        std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/dadjokes.json"),
        )
        .unwrap()
    }

    #[test]
    fn skips_adult_jokes_and_splits_setup() {
        let candidates = candidates_from_raw(&raw_from_json(&fixture()));
        let joined = candidates
            .iter()
            .map(|c| format!("{} {}", c.joke.setup, c.joke.punchline))
            .collect::<Vec<_>>()
            .join(" ")
            .to_ascii_lowercase();
        assert!(!joined.contains("pregnant"));
        assert!(!joined.contains("drug dealer"));
        assert!(!joined.contains("walks into a bar"));
        assert!(candidates
            .iter()
            .any(|c| c.joke.setup.contains("Peter Pan")));
        let pan = candidates
            .iter()
            .find(|c| c.joke.setup.contains("Peter Pan"))
            .unwrap();
        assert_eq!(pan.joke.punchline, "Because he Neverlands.");
        assert!(candidates
            .iter()
            .any(|c| c.joke.setup.contains("pirate movie")));
    }

    #[test]
    fn random_endpoint_json_is_accepted() {
        let json = r#"{"id":"abc","joke":"Why did the bicycle fall over? Because it was two-tired.","status":200}"#;
        let candidates = candidates_from_raw(&raw_from_json(json));
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].joke.setup, "Why did the bicycle fall over?");
        assert_eq!(candidates[0].joke.punchline, "Because it was two-tired.");
    }

    #[test]
    fn skips_one_liners_that_are_not_kid_safe() {
        assert!(!family_friendly(
            "I bought some shoes from a drug dealer. I was tripping all day!"
        ));
        assert!(family_friendly(
            "I don't trust stairs. They're always up to something."
        ));
        assert!(!family_friendly(
            "A ghost walks into a bar and asks for a glass of vodka."
        ));
    }

    #[test]
    fn demo_joke_fits_the_panel() {
        assert!(joke_fits_panel(&demo_joke()));
        for (setup, punchline) in FALLBACK {
            let joke = Joke {
                setup: (*setup).into(),
                punchline: (*punchline).into(),
            };
            assert!(joke_fits_panel(&joke), "{setup}");
        }
    }
}
