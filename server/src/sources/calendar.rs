use anyhow::Result;
use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, Timelike};
use tracing::{info, warn};

use super::context::SourceContext;
use super::contribute::{Contribution, SourceOutcome};
use super::ics;
use super::{DataSource, DisabledBehaviour};
use crate::model::{CalendarEvent, Dashboard, Weather, EVENT_HORIZON_DAYS};

/// Local hour after which an empty evening rolls the primary panel to tomorrow.
const EVENING_HOUR: u32 = 18;

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
        Ok(SourceOutcome::live(
            notes.join(" · "),
            Contribution::Calendar(events),
        ))
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

pub fn merge_events(
    dash: &mut Dashboard,
    events: Vec<CalendarEvent>,
    today: NaiveDate,
    now_local: NaiveDateTime,
) {
    let focus = calendar_focus(today, now_local, &events);
    dash.today_title = focus.title;
    dash.today_ordinal = focus.ordinal;
    align_weather(&mut dash.weather, focus.weather_offset);
    let focus_date = focus.date.format("%Y-%m-%d").to_string();
    for mut ev in events {
        if !ev.school {
            ev.title = crate::model::truncate_event_title(&ev.title);
        }
        if ev.date == focus_date {
            dash.events_today.push(ev);
        } else if ev.date > focus_date {
            dash.events_coming.push(ev);
        }
    }
}

struct CalendarFocus {
    date: NaiveDate,
    title: String,
    ordinal: String,
    weather_offset: usize,
}

fn calendar_focus(
    today: NaiveDate,
    now_local: NaiveDateTime,
    events: &[CalendarEvent],
) -> CalendarFocus {
    if shows_tomorrow(today, now_local, events) {
        let date = today + Duration::days(1);
        CalendarFocus {
            title: "Tomorrow".into(),
            ordinal: day_ordinal(date.day()),
            date,
            weather_offset: 1,
        }
    } else {
        CalendarFocus {
            title: "Today".into(),
            ordinal: String::new(),
            date: today,
            weather_offset: 0,
        }
    }
}

/// After 18:00, the primary panel becomes tomorrow unless a later event
/// (start at or after 18:00, and still at or after now) remains today.
fn shows_tomorrow(today: NaiveDate, now_local: NaiveDateTime, events: &[CalendarEvent]) -> bool {
    if now_local.date() != today {
        return false;
    }
    let now_minutes = now_local.hour() * 60 + now_local.minute();
    if now_minutes < EVENING_HOUR * 60 {
        return false;
    }
    let today_s = today.format("%Y-%m-%d").to_string();
    !events
        .iter()
        .any(|ev| remaining_evening_event(ev, &today_s, now_minutes))
}

fn remaining_evening_event(ev: &CalendarEvent, today: &str, now_minutes: u32) -> bool {
    if ev.date != today || ev.all_day {
        return false;
    }
    event_minutes(&ev.start).is_some_and(|start| start >= EVENING_HOUR * 60 && start >= now_minutes)
}

fn event_minutes(start: &str) -> Option<u32> {
    let (hour, minute) = start.split_once(':')?;
    let hour: u32 = hour.parse().ok()?;
    let minute: u32 = minute.parse().ok()?;
    (hour <= 23 && minute <= 59).then_some(hour * 60 + minute)
}

fn day_ordinal(day: u32) -> String {
    let suffix = match day % 100 {
        11 | 12 | 13 => "th",
        _ => match day % 10 {
            1 => "st",
            2 => "nd",
            3 => "rd",
            _ => "th",
        },
    };
    format!("{day}{suffix}")
}

fn align_weather(weather: &mut Weather, offset: usize) {
    if offset > 0 && offset <= weather.days.len() {
        weather.days.drain(..offset);
    }
    weather.days.truncate(2);
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
            bin: false,
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

    fn at(date: NaiveDate, hour: u32, minute: u32) -> NaiveDateTime {
        date.and_hms_opt(hour, minute, 0).unwrap()
    }

    fn ev(today: NaiveDate, offset: i64, start: &str, title: &str, all_day: bool) -> CalendarEvent {
        let date = today + Duration::days(offset);
        CalendarEvent {
            start: if all_day { String::new() } else { start.into() },
            title: title.into(),
            all_day,
            day_label: ics::day_label(date, today),
            date: date.format("%Y-%m-%d").to_string(),
            birthday: false,
            school: false,
            recurring: false,
            bin: false,
        }
    }

    fn merge(today: NaiveDate, now: NaiveDateTime, events: Vec<CalendarEvent>) -> Dashboard {
        let mut dash = Dashboard::empty("Family", today);
        dash.weather = crate::sources::weather::demo_weather();
        merge_events(&mut dash, events, today, now);
        dash
    }

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

    #[test]
    fn afternoon_keeps_today_even_without_evening_events() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 19).unwrap();
        let dash = merge(
            today,
            at(today, 16, 0),
            vec![
                ev(today, 0, "15:00", "Pick-up", false),
                ev(today, 1, "09:00", "Swim", false),
            ],
        );
        assert_eq!(dash.today_title, "Today");
        assert_eq!(
            dash.events_today
                .iter()
                .map(|e| e.title.as_str())
                .collect::<Vec<_>>(),
            ["Pick-up"]
        );
        assert_eq!(
            dash.events_coming
                .iter()
                .map(|e| e.title.as_str())
                .collect::<Vec<_>>(),
            ["Swim"]
        );
        assert_eq!(dash.weather.days[0].label, "Today");
        assert_eq!(dash.weather.days[1].label, "Tomorrow");
    }

    #[test]
    fn evening_rolls_to_tomorrow_when_nothing_is_left() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 19).unwrap();
        let dash = merge(
            today,
            at(today, 18, 5),
            vec![
                ev(today, 0, "15:00", "Pick-up", false),
                ev(today, 1, "09:00", "Swim", false),
                ev(today, 2, "10:00", "Dentist", false),
            ],
        );
        assert_eq!(dash.today_title, "Tomorrow");
        assert_eq!(dash.today_ordinal, "20th");
        assert_eq!(
            dash.events_today
                .iter()
                .map(|e| e.title.as_str())
                .collect::<Vec<_>>(),
            ["Swim"]
        );
        assert_eq!(
            dash.events_coming
                .iter()
                .map(|e| e.title.as_str())
                .collect::<Vec<_>>(),
            ["Dentist"]
        );
        assert_eq!(dash.weather.days.len(), 2);
        assert_eq!(dash.weather.days[0].label, "Tomorrow");
        assert_eq!(dash.weather.days[0].slots[0].temperature, "15°");
        assert_eq!(dash.weather.days[1].label, "Mon 21");
        assert_eq!(dash.weather.days[1].slots[0].temperature, "19°");
    }

    #[test]
    fn remaining_evening_event_keeps_today() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 19).unwrap();
        let dash = merge(
            today,
            at(today, 18, 10),
            vec![
                ev(today, 0, "19:00", "Dinner", false),
                ev(today, 1, "09:00", "Swim", false),
            ],
        );
        assert_eq!(dash.today_title, "Today");
        assert_eq!(dash.events_today[0].title, "Dinner");
        assert_eq!(dash.events_coming[0].title, "Swim");
        assert_eq!(dash.weather.days[0].label, "Today");
    }

    #[test]
    fn past_evening_event_does_not_keep_today() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 19).unwrap();
        let dash = merge(
            today,
            at(today, 19, 5),
            vec![
                ev(today, 0, "18:30", "Dinner", false),
                ev(today, 1, "", "Swim", true),
            ],
        );
        assert_eq!(dash.today_title, "Tomorrow");
        assert_eq!(dash.today_ordinal, "20th");
        assert_eq!(dash.events_today[0].title, "Swim");
        assert!(dash.events_coming.is_empty());
    }

    #[test]
    fn event_at_six_keeps_today() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 19).unwrap();
        let dash = merge(
            today,
            at(today, 18, 0),
            vec![ev(today, 0, "18:00", "Cinema", false)],
        );
        assert_eq!(dash.today_title, "Today");
        assert_eq!(dash.events_today[0].title, "Cinema");
    }

    #[test]
    fn all_day_today_does_not_block_rollover() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 1).unwrap();
        let dash = merge(
            today,
            at(today, 20, 0),
            vec![
                ev(today, 0, "", "Holiday", true),
                ev(today, 1, "08:00", "School run", false),
            ],
        );
        assert_eq!(dash.today_title, "Tomorrow");
        assert_eq!(dash.today_ordinal, "2nd");
        assert_eq!(dash.events_today[0].title, "School run");
        assert!(dash.events_coming.is_empty());
    }

    #[test]
    fn after_midnight_is_today_again() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 20).unwrap();
        let dash = merge(
            today,
            at(today, 0, 20),
            vec![
                ev(today, 0, "09:00", "Swim", false),
                ev(today, 1, "10:00", "Dentist", false),
            ],
        );
        assert_eq!(dash.today_title, "Today");
        assert_eq!(dash.events_today[0].title, "Swim");
        assert_eq!(dash.events_coming[0].title, "Dentist");
        assert_eq!(dash.weather.days[0].label, "Today");
    }

    #[test]
    fn ordinals() {
        assert_eq!(day_ordinal(1), "1st");
        assert_eq!(day_ordinal(2), "2nd");
        assert_eq!(day_ordinal(3), "3rd");
        assert_eq!(day_ordinal(4), "4th");
        assert_eq!(day_ordinal(11), "11th");
        assert_eq!(day_ordinal(12), "12th");
        assert_eq!(day_ordinal(13), "13th");
        assert_eq!(day_ordinal(21), "21st");
        assert_eq!(day_ordinal(22), "22nd");
        assert_eq!(day_ordinal(23), "23rd");
        assert_eq!(day_ordinal(31), "31st");
    }
}
