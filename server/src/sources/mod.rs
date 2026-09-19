//! Dashboard datasources: one [`DataSource`] impl each, registered in
//! [`all_sources`].

use anyhow::Result;
use async_trait::async_trait;
use tracing::warn;

use crate::config::Config;
use crate::model::Dashboard;

pub mod birthdays;
pub mod cache;
pub mod calendar;
pub mod contribute;
pub mod context;
pub mod filter;
pub mod history;
pub mod ics;
pub mod jokes;
pub mod meross;
pub mod pronote;
pub mod saints;
pub mod tfl;
pub mod todoist;
pub mod weather;

pub use calendar::{demo_events, merge_events};
pub use contribute::{apply, Contribution, SourceOutcome, SourceStatus};
pub use context::SourceContext;

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

    fn when_disabled(&self, _cfg: &Config) -> DisabledBehaviour {
        DisabledBehaviour::Skip
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

    for src in all_sources() {
        if !src.enabled(cfg) {
            if src.when_disabled(cfg) == DisabledBehaviour::Demo {
                if let Some(demo) = src.demo(&ctx) {
                    let note = src.disabled_note();
                    if !note.is_empty() {
                        notes.push(note);
                    }
                    apply(&mut dash, &mut calendar, demo, ctx.today);
                }
            }
        } else {
            match src.load(&ctx).await {
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
    }

    if calendar.is_empty() {
        calendar.extend(demo_events(ctx.today));
        notes.push("demo calendar (no ICS events)".into());
    }

    merge_events(&mut dash, calendar);
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
        assert_eq!(
            enabled,
            ["tfl", "jokes", "history", "birthdays", "saints"]
        );
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
        merge_events(&mut dash, calendar);
        dash.fit_to_panel();
        dash.source_note = [
            "demo to-dos (no Todoist token)",
            "demo rooms (no Meross credentials)",
            "demo weather (no BBC location)",
            "TfL tube",
            "icanhazdadjoke",
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
