//! Dashboard datasources: one [`DataSource`] impl each, registered in
//! [`all_sources`].

use std::collections::HashMap;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use futures::future::join_all;
use tracing::warn;

use crate::config::Config;
use crate::model::Dashboard;

pub mod bins;
pub mod birthdays;
pub mod cache;
pub mod calendar;
pub mod context;
pub mod contribute;
pub mod filter;
pub mod history;
pub mod ics;
pub mod jokes;
#[cfg(feature = "meross")]
pub mod meross;
#[cfg(not(feature = "meross"))]
pub mod meross {
    use super::*;

    pub struct MerossSource;

    #[async_trait]
    impl DataSource for MerossSource {
        fn id(&self) -> &'static str {
            "meross"
        }
        fn enabled(&self, _cfg: &Config) -> bool {
            false
        }
        fn private(&self) -> bool {
            true
        }
        async fn load(&self, _ctx: &SourceContext<'_>) -> Result<SourceOutcome> {
            Ok(SourceOutcome::live(String::new(), Contribution::None))
        }
    }

    pub fn invalidate_rooms() {}

    pub fn demo_rooms() -> Vec<crate::model::RoomClimate> {
        Vec::new()
    }
}
#[cfg(feature = "pronote")]
pub mod pronote;
#[cfg(not(feature = "pronote"))]
pub mod pronote {
    use super::*;
    use crate::model::{CalendarEvent, School, SchoolDay};
    use chrono::NaiveDate;

    pub struct PronoteSource;

    #[async_trait]
    impl DataSource for PronoteSource {
        fn id(&self) -> &'static str {
            "pronote"
        }
        fn enabled(&self, _cfg: &Config) -> bool {
            false
        }
        fn private(&self) -> bool {
            true
        }
        async fn load(&self, _ctx: &SourceContext<'_>) -> Result<SourceOutcome> {
            Ok(SourceOutcome::live(String::new(), Contribution::None))
        }
    }

    pub fn demo_school(_today: NaiveDate) -> School {
        School::default()
    }

    pub fn display_student(_cfg: &crate::config::PronoteConfig, fetched: &str) -> String {
        fetched.trim().to_string()
    }

    pub fn school_day_events(
        _student: &str,
        _days: &[SchoolDay],
        _today: NaiveDate,
    ) -> Vec<CalendarEvent> {
        Vec::new()
    }
}
pub mod saints;
pub mod tfl;
pub mod todoist;
pub mod weather;

pub use calendar::{demo_events, merge_events};
pub use context::SourceContext;
pub use contribute::{apply, Contribution, SourceOutcome, SourceStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisabledBehaviour {
    Skip,
    Demo,
}

/// One dashboard input. Implement this, register it, ship it.
#[async_trait]
pub trait DataSource: Send + Sync {
    /// Stable id: "tfl", "weather", "pronote", … Matches `[sources.<id>]`.
    fn id(&self) -> &'static str;

    fn enabled(&self, cfg: &Config) -> bool;

    /// Household data that `--fake` must not fetch (ICS, todos, school, house, birthdays).
    fn private(&self) -> bool {
        false
    }

    fn when_disabled(&self, _cfg: &Config) -> DisabledBehaviour {
        DisabledBehaviour::Skip
    }

    /// Live HTTP/MQTT only when enabled and not a private source in screenshot mode.
    fn uses_live_fetch(&self, cfg: &Config) -> bool {
        self.enabled(cfg) && !(cfg.fake_private && self.private())
    }

    fn disabled_note(&self) -> String {
        format!("{} demo", self.id())
    }

    /// Called only when enabled. Never mutate the dashboard directly.
    async fn load(&self, ctx: &SourceContext<'_>) -> Result<SourceOutcome>;

    fn demo(&self, ctx: &SourceContext<'_>) -> Option<Contribution> {
        let _ = ctx;
        None
    }
}

/// Load order matches the pre-registry orchestrator (Phase 1 parity).
pub fn all_sources() -> Vec<Box<dyn DataSource>> {
    vec![
        Box::new(todoist::TodoistSource),
        Box::new(meross::MerossSource),
        Box::new(calendar::IcsSource),
        Box::new(bins::BinsSource),
        Box::new(weather::WeatherSource),
        Box::new(tfl::TflSource),
        Box::new(jokes::JokesSource),
        Box::new(history::HistorySource),
        Box::new(pronote::PronoteSource),
        Box::new(birthdays::BirthdaysSource),
        Box::new(saints::SaintsSource),
    ]
}

pub async fn load_dashboard(cfg: &Config) -> Result<Dashboard> {
    let ctx = SourceContext::from_config(cfg);
    let mut dash = Dashboard::empty(&cfg.family_name, ctx.today);
    let mut notes: Vec<String> = Vec::new();
    let mut calendar = Vec::new();
    let sources = all_sources();

    // Independent HTTPS/MQTT loads in parallel; apply stays in registry order.
    let fetched = join_all(sources.iter().map(|src| {
        let live = src.uses_live_fetch(cfg);
        let ctx = ctx.clone();
        async move {
            if !live {
                return (src.id(), None);
            }
            let timed = tokio::time::timeout(Duration::from_secs(35), src.load(&ctx)).await;
            let result = match timed {
                Ok(inner) => inner,
                Err(_) => Err(anyhow::anyhow!("source timed out")),
            };
            (src.id(), Some(result))
        }
    }))
    .await;
    let mut by_id: HashMap<&'static str, Result<SourceOutcome>> = fetched
        .into_iter()
        .filter_map(|(id, maybe)| maybe.map(|result| (id, result)))
        .collect();

    for src in &sources {
        if !src.uses_live_fetch(cfg) {
            let screenshot_demo = cfg.fake_private && src.private() && src.enabled(cfg);
            if screenshot_demo || src.when_disabled(cfg) == DisabledBehaviour::Demo {
                if let Some(demo) = src.demo(&ctx) {
                    let note = src.disabled_note();
                    if !note.is_empty() {
                        notes.push(note);
                    }
                    apply(&mut dash, &mut calendar, demo, ctx.today);
                }
            }
            continue;
        }
        match by_id
            .remove(&src.id())
            .unwrap_or_else(|| Err(anyhow::anyhow!("missing load result for {}", src.id())))
        {
            Ok(out) => {
                if !out.note.is_empty() {
                    notes.push(out.note);
                }
                apply(&mut dash, &mut calendar, out.contribution, ctx.today);
            }
            Err(err) => {
                warn!(source = src.id(), %err, "source failed");
                if let Some(demo) = src.demo(&ctx) {
                    notes.push(format!("{} unavailable", src.id()));
                    apply(&mut dash, &mut calendar, demo, ctx.today);
                }
            }
        }
    }

    if calendar.is_empty() {
        calendar.extend(demo_events(ctx.today));
        notes.push("demo calendar (no ICS events)".into());
    }

    let now_local = ctx.now.with_timezone(&ctx.tz).naive_local();
    merge_events(&mut dash, calendar, ctx.today, now_local);
    dash.show_school_sections = cfg.pronote.show_sections;
    dash.fit_to_panel();
    dash.source_note = notes.join(" · ");
    Ok(dash)
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;
    use serde_json::Value;

    use super::*;

    #[test]
    fn registry_ids_match_legacy_load_order() {
        let ids: Vec<_> = all_sources().iter().map(|s| s.id()).collect();
        assert_eq!(
            ids,
            [
                "todoist",
                "meross",
                "calendar",
                "bins",
                "weather",
                "tfl",
                "jokes",
                "history",
                "pronote",
                "birthdays",
                "saints",
            ]
        );
    }

    #[test]
    fn private_sources_are_household_only() {
        let mut ids: Vec<_> = all_sources()
            .iter()
            .filter(|s| s.private())
            .map(|s| s.id())
            .collect();
        ids.sort();
        assert_eq!(
            ids,
            [
                "bins",
                "birthdays",
                "calendar",
                "meross",
                "pronote",
                "todoist"
            ]
        );
    }

    #[test]
    fn fake_mode_blocks_live_fetch_for_private_sources() {
        let mut cfg = Config::default();
        cfg.fake_private = true;
        cfg.todoist.token = "secret".into();
        cfg.sources.ics_urls = vec!["https://example.invalid/family.ics".into()];
        cfg.sources.bins.uprn = "100022658374".into();
        cfg.meross.email = "a@b.c".into();
        cfg.meross.password = "pw".into();
        cfg.pronote.url = "https://example.invalid/pronote".into();
        cfg.pronote.username = "kid".into();
        cfg.pronote.password = "pw".into();
        cfg.weather.location_id = "2643743".into();
        let live: Vec<_> = all_sources()
            .iter()
            .filter(|s| s.uses_live_fetch(&cfg))
            .map(|s| s.id())
            .collect();
        assert_eq!(live, ["weather", "tfl", "jokes", "history", "saints"]);
        assert!(all_sources()
            .iter()
            .all(|s| !s.private() || !s.uses_live_fetch(&cfg)));
    }

    #[test]
    fn credentials_enable_live_fetch_unless_fake() {
        let mut cfg = Config::default();
        cfg.todoist.token = "secret".into();
        let todoist = all_sources()
            .into_iter()
            .find(|s| s.id() == "todoist")
            .unwrap();
        assert!(todoist.uses_live_fetch(&cfg));
        cfg.fake_private = true;
        assert!(!todoist.uses_live_fetch(&cfg));
    }

    #[test]
    fn default_config_enablement_skips_credential_sources() {
        let cfg = Config::default();
        assert!(!cfg.todoist_enabled());
        assert!(!cfg.meross_enabled());
        assert!(!cfg.weather_enabled());
        assert!(!cfg.pronote_enabled());
        let enabled: Vec<_> = all_sources()
            .iter()
            .filter(|s| s.enabled(&cfg))
            .map(|s| s.id())
            .collect();
        assert_eq!(enabled, ["tfl", "jokes", "history", "birthdays", "saints"]);
    }

    fn offline_demo_dashboard(today: NaiveDate) -> Dashboard {
        let cfg = Config::default();
        let mut dash = Dashboard::empty(&cfg.family_name, today);
        let mut calendar = Vec::new();
        apply(
            &mut dash,
            &mut calendar,
            Contribution::Todos(todoist::demo_todos()),
            today,
        );
        apply(
            &mut dash,
            &mut calendar,
            Contribution::Rooms(meross::demo_rooms()),
            today,
        );
        apply(
            &mut dash,
            &mut calendar,
            Contribution::Weather(weather::demo_weather()),
            today,
        );
        apply(
            &mut dash,
            &mut calendar,
            Contribution::Transit(tfl::demo_tube()),
            today,
        );
        apply(
            &mut dash,
            &mut calendar,
            Contribution::Joke(jokes::demo_joke()),
            today,
        );
        apply(
            &mut dash,
            &mut calendar,
            Contribution::History(history::demo_history()),
            today,
        );
        apply(
            &mut dash,
            &mut calendar,
            Contribution::School(pronote::demo_school(today)),
            today,
        );
        apply(
            &mut dash,
            &mut calendar,
            Contribution::Calendar(birthdays::upcoming_events(&cfg.birthdays, today)),
            today,
        );
        if calendar.is_empty() {
            calendar.extend(demo_events(today));
        }
        merge_events(
            &mut dash,
            calendar,
            today,
            today.and_hms_opt(12, 0, 0).unwrap(),
        );
        dash.fit_to_panel();
        dash.source_note = [
            "demo to-dos (no Todoist token)",
            "demo rooms (no Meross credentials)",
            "demo weather (no BBC location)",
            "TfL tube",
            "icanhazdadjoke",
            "Wikipedia on this day",
            "demo school (no Pronote credentials)",
        ]
        .join(" · ");
        dash
    }

    #[test]
    fn birthdays_suppress_demo_calendar() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 18).unwrap();
        let people = [crate::config::Birthday {
            name: "Maya".into(),
            dob: NaiveDate::from_ymd_opt(2018, 9, 20).unwrap(),
        }];
        let mut calendar = birthdays::upcoming_events(&people, today);
        assert!(!calendar.is_empty());
        if calendar.is_empty() {
            calendar.extend(demo_events(today));
        }
        assert!(calendar.iter().all(|e| e.birthday));
        assert!(!calendar.iter().any(|e| e.title == "School run"));
    }

    #[test]
    fn demo_dashboard_golden_is_stable() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 18).unwrap();
        let dash = offline_demo_dashboard(today);
        let json = serde_json::to_value(dash.for_layout_hash()).unwrap();
        let expected: Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/dashboard_demo.json"
        )))
        .unwrap();
        assert_eq!(json, expected);
    }
}
