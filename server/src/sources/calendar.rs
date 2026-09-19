use anyhow::Result;
use chrono::Duration;
use tracing::{info, warn};

use super::contribute::{Contribution, SourceOutcome};
use super::context::SourceContext;
use super::ics;
use super::{DataSource, DisabledBehaviour};
use crate::model::{CalendarEvent, Dashboard, EVENT_HORIZON_DAYS};

/// Public ICS URLs. Parser lives in [`super::ics`]; merge policy lives here.
pub struct IcsSource;

#[async_trait::async_trait]
impl DataSource for IcsSource {
    fn id(&self) -> &'static str {
        "calendar"
    }

    fn enabled(&self, cfg: &crate::config::Config) -> bool {
        cfg.sources.ics_urls.iter().any(|u| !u.trim().is_empty())
    }

    fn private(&self) -> bool {
        true
    }

    fn when_disabled(&self, _cfg: &crate::config::Config) -> DisabledBehaviour {
        DisabledBehaviour::Skip
    }

    fn disabled_note(&self) -> String {
        "demo calendar".into()
    }

    fn demo(&self, ctx: &SourceContext<'_>) -> Option<Contribution> {
        Some(Contribution::Calendar(demo_events(ctx.today)))
    }

    async fn load(&self, ctx: &SourceContext<'_>) -> Result<SourceOutcome> {
        let mut events = Vec::new();
        let mut notes: Vec<String> = Vec::new();
        for url in &ctx.cfg.sources.ics_urls {
            if url.trim().is_empty() {
                continue;
            }
            match fetch_ics(url).await {
                Ok(body) => {
                    let parsed = ics::parse_events(&body, ctx.tz, ctx.today, EVENT_HORIZON_DAYS)?;
                    info!(url, n = parsed.len(), "loaded public ICS");
                    events.extend(parsed);
                    notes.push("public ICS".into());
                }
                Err(err) => warn!(url, %err, "public ICS failed"),
            }
        }
        Ok(SourceOutcome::live(notes.join(" · "), Contribution::Calendar(events)))
    }
}

/// Calendar.app copies `webcal://…`. That is just HTTPS with a scheme
/// HTTP clients do not speak.
pub fn http_ics_url(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_string();
    };
    match scheme.to_ascii_lowercase().as_str() {
        "webcal" | "webcals" => format!("https://{rest}"),
        _ => url.to_string(),
    }
}

pub async fn fetch_ics(url: &str) -> Result<String> {
    let url = http_ics_url(url);
    let text = reqwest::Client::new()
        .get(&url)
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    Ok(text)
}

pub fn merge_events(dash: &mut Dashboard, events: Vec<CalendarEvent>) {
    for mut ev in events {
        if !ev.school {
            ev.title = crate::model::truncate_event_title(&ev.title);
        }
        if ev.day_label == "Today" {
            dash.events_today.push(ev);
        } else {
            dash.events_coming.push(ev);
        }
    }
}

pub fn demo_events(today: chrono::NaiveDate) -> Vec<CalendarEvent> {
    let ev = |offset: i64, start: &str, title: &str, all_day: bool, recurring: bool| {
        let date = today + Duration::days(offset);
        CalendarEvent {
            start: if all_day { String::new() } else { start.into() },
            title: title.into(),
            all_day,
            day_label: ics::day_label(date, today),
            date: date.format("%Y-%m-%d").to_string(),
            birthday: false,
            school: false,
            recurring,
        }
    };

    vec![
        ev(0, "08:15", "School run", false, true),
        ev(0, "18:30", "Dinner at Sam’s", false, false),
        ev(1, "", "Swim", true, true),
        ev(3, "16:00", "Parents’ evening", false, false),
        ev(5, "09:30", "Dentist", false, false),
        ev(8, "18:00", "Cinema", false, false),
        ev(12, "", "Half term", true, false),
        ev(18, "10:00", "Football club", false, true),
        ev(25, "19:00", "Book club", false, true),
        ev(40, "15:00", "Granny’s birthday", false, false),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webcal_becomes_https() {
        assert_eq!(
            http_ics_url("webcal://calendar.example.com/published/family.ics"),
            "https://calendar.example.com/published/family.ics"
        );
        assert_eq!(
            http_ics_url("WEBCALS://example.com/cal.ics"),
            "https://example.com/cal.ics"
        );
        assert_eq!(
            http_ics_url("https://example.com/cal.ics"),
            "https://example.com/cal.ics"
        );
    }
}
