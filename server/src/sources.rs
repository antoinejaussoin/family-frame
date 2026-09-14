use anyhow::Result;
use chrono::{Duration, TimeZone, Utc};
use chrono_tz::Tz;
use tracing::{info, warn};

use crate::caldav::{self, CalDav};
use crate::config::Config;
use crate::ics;
use crate::meross;
use crate::model::Dashboard;
use crate::tfl;
use crate::todoist;
use crate::weather;

pub async fn load_dashboard(cfg: &Config) -> Result<Dashboard> {
    let tz: Tz = cfg.timezone.parse().unwrap_or(chrono_tz::Europe::London);
    let today = Utc::now().with_timezone(&tz).date_naive();
    let mut dash = Dashboard::empty(&cfg.family_name, today);
    let mut notes: Vec<String> = Vec::new();

    if cfg.todoist_enabled() {
        match todoist::load_todos(&cfg.todoist).await {
            Ok(todos) => {
                dash.todos = todos;
                notes.push(format!("Todoist “{}”", cfg.todoist.project));
            }
            Err(err) => {
                warn!(%err, "Todoist failed; using demo to-dos");
                dash.todos = todoist::demo_todos();
                notes.push("Todoist unavailable".into());
            }
        }
    } else {
        dash.todos = todoist::demo_todos();
        notes.push("demo to-dos (no Todoist token)".into());
    }
    if cfg.meross_enabled() {
        match meross::load_rooms(&cfg.meross, &cfg.meross_creds_path()).await {
            Ok(rooms) if !rooms.is_empty() => {
                dash.rooms = rooms;
                notes.push("Meross sensors".into());
            }
            Ok(_) => {
                warn!("Meross login worked but no thermometer readings came back");
                notes.push("Meross: no sensor readings".into());
            }
            Err(err) => {
                warn!(%err, "Meross failed; keeping empty rooms");
                notes.push("Meross unavailable".into());
            }
        }
    }

    for url in &cfg.sources.ics_urls {
        match fetch_ics(url).await {
            Ok(ics) => {
                let events = ics::parse_events(&ics, tz, today, crate::model::EVENT_HORIZON_DAYS)?;
                info!(url, n = events.len(), "loaded public ICS");
                merge_events(&mut dash, events);
                notes.push("public ICS".into());
            }
            Err(err) => warn!(url, %err, "public ICS failed"),
        }
    }

    if cfg.icloud_enabled() {
        match load_icloud(cfg, tz, today).await {
            Ok((events, icloud_notes)) => {
                if !events.is_empty() {
                    dash.events_today.clear();
                    dash.events_coming.clear();
                    merge_events(&mut dash, events);
                }
                notes.extend(icloud_notes);
            }
            Err(err) => {
                warn!(%err, "iCloud CalDAV failed; keeping ICS/demo calendar");
                notes.push("iCloud unavailable".into());
            }
        }
    } else if dash.events_today.is_empty() && dash.events_coming.is_empty() {
        merge_events(&mut dash, demo_events(today));
        notes.push("demo calendar (no iCloud credentials)".into());
    }

    if dash.rooms.is_empty() && !cfg.meross_enabled() {
        dash.rooms = meross::demo_rooms();
        notes.push("demo rooms (no Meross credentials)".into());
    }

    if cfg.weather_enabled() {
        match weather::load_forecast(&cfg.weather, &cfg.weather_cache_path(), today).await {
            Ok(forecast) if !forecast.days.is_empty() => {
                notes.push(format!("BBC weather “{}”", forecast.location));
                dash.weather = forecast;
            }
            Ok(_) => {
                warn!("BBC weather returned no days");
                dash.weather = weather::demo_weather();
                notes.push("BBC weather empty — demo forecast".into());
            }
            Err(err) => {
                warn!(%err, "BBC weather failed; using demo forecast");
                dash.weather = weather::demo_weather();
                notes.push("BBC weather unavailable".into());
            }
        }
    } else {
        dash.weather = weather::demo_weather();
        notes.push("demo weather (no BBC location)".into());
    }

    match tfl::load_tube().await {
        Ok(lines) if !lines.is_empty() => {
            dash.tube = lines;
            notes.push("TfL tube".into());
        }
        Ok(_) => {
            warn!("TfL returned no lines");
            dash.tube = tfl::demo_tube();
            notes.push("TfL empty — demo tube".into());
        }
        Err(err) => {
            warn!(%err, "TfL failed; using demo tube");
            dash.tube = tfl::demo_tube();
            notes.push("TfL unavailable".into());
        }
    }

    dash.fit_to_panel();
    dash.source_note = notes.join(" · ");
    Ok(dash)
}

async fn load_icloud(
    cfg: &Config,
    tz: Tz,
    today: chrono::NaiveDate,
) -> Result<(Vec<crate::model::CalendarEvent>, Vec<String>)> {
    let client = CalDav::new(&cfg.icloud)?;
    let calendars = client.list_calendars().await?;
    let start = tz
        .with_ymd_and_hms(
            chrono::Datelike::year(&today),
            chrono::Datelike::month(&today),
            chrono::Datelike::day(&today),
            0,
            0,
            0,
        )
        .single()
        .ok_or_else(|| anyhow::anyhow!("invalid timezone date"))?;
    let end = start + Duration::days(crate::model::EVENT_HORIZON_DAYS);
    let start_utc = start
        .with_timezone(&Utc)
        .format("%Y%m%dT%H%M%SZ")
        .to_string();
    let end_utc = end.with_timezone(&Utc).format("%Y%m%dT%H%M%SZ").to_string();

    let mut events = Vec::new();
    let mut notes = Vec::new();
    for cal in caldav::match_named(&calendars, &cfg.icloud.calendars) {
        if !cal.supports_events {
            continue;
        }
        let ics = client
            .fetch_calendar_data(&cal.href, &caldav::event_report(&start_utc, &end_utc))
            .await?;
        let parsed = ics::parse_events(&ics, tz, today, crate::model::EVENT_HORIZON_DAYS)?;
        info!(calendar = %cal.name, n = parsed.len(), "iCloud events");
        events.extend(parsed);
        notes.push(format!("iCloud calendar “{}”", cal.name));
    }

    Ok((events, notes))
}

/// Calendar.app copies `webcal://…`. That is just HTTPS with a scheme
/// HTTP clients do not speak.
fn http_ics_url(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_string();
    };
    match scheme.to_ascii_lowercase().as_str() {
        "webcal" | "webcals" => format!("https://{rest}"),
        _ => url.to_string(),
    }
}

async fn fetch_ics(url: &str) -> Result<String> {
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

fn merge_events(dash: &mut Dashboard, events: Vec<crate::model::CalendarEvent>) {
    for mut ev in events {
        ev.title = crate::model::truncate_event_title(&ev.title);
        ev.who.clear();
        if ev.day_label == "Today" {
            dash.events_today.push(ev);
        } else {
            dash.events_coming.push(ev);
        }
    }
}

fn demo_events(today: chrono::NaiveDate) -> Vec<crate::model::CalendarEvent> {
    use crate::model::CalendarEvent;

    let ev = |offset: i64, start: &str, title: &str, all_day: bool| {
        let date = today + Duration::days(offset);
        CalendarEvent {
            start: if all_day { String::new() } else { start.into() },
            title: title.into(),
            who: String::new(),
            all_day,
            day_label: ics::day_label(date, today),
            date: date.format("%Y-%m-%d").to_string(),
        }
    };

    vec![
        ev(0, "08:15", "School run", false),
        ev(0, "18:30", "Dinner at Sam’s", false),
        ev(1, "", "Swim", true),
        ev(3, "16:00", "Parents’ evening", false),
        ev(5, "09:30", "Dentist", false),
        ev(8, "18:00", "Cinema", false),
        ev(12, "", "Half term", true),
        ev(18, "10:00", "Football club", false),
        ev(25, "19:00", "Book club", false),
        ev(40, "15:00", "Granny’s birthday", false),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webcal_becomes_https() {
        assert_eq!(
            http_ics_url("webcal://p01-caldav.icloud.com/published/2/abc"),
            "https://p01-caldav.icloud.com/published/2/abc"
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
