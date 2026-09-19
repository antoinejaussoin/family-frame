use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Duration, NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use icalendar::{
    Calendar, CalendarComponent, CalendarDateTime, Component, DatePerhapsTime, Event, EventStatus,
    Property,
};
use rrule::RRuleSet;
use tracing::warn;

use crate::model::CalendarEvent;

pub fn parse_events(
    ics: &str,
    tz: Tz,
    from: NaiveDate,
    days: i64,
) -> anyhow::Result<Vec<CalendarEvent>> {
    let calendar: Calendar = ics
        .parse()
        .map_err(|e| anyhow::anyhow!("parsing ICS: {e}"))?;
    let start = tz
        .with_ymd_and_hms(from.year(), from.month(), from.day(), 0, 0, 0)
        .single()
        .ok_or_else(|| anyhow::anyhow!("invalid start date"))?;
    let end = start + Duration::days(days);

    let mut masters = Vec::new();
    let mut overrides: HashMap<String, Vec<Event>> = HashMap::new();
    for component in calendar.iter() {
        let CalendarComponent::Event(event) = component else {
            continue;
        };
        if event.get_summary().is_none() || event.get_start().is_none() {
            continue;
        }
        if event.get_recurrence_id().is_some() {
            let uid = event.get_uid().unwrap_or("").to_string();
            overrides.entry(uid).or_default().push(event.clone());
        } else {
            masters.push(event.clone());
        }
    }

    let mut out = Vec::new();
    for event in &masters {
        if event.get_status() == Some(EventStatus::Cancelled) {
            continue;
        }
        let Some(title) = event.get_summary() else {
            continue;
        };
        let Some(when) = event.get_start() else {
            continue;
        };
        let Some((_, all_day)) = date_to_local(when, tz) else {
            continue;
        };
        let uid = event.get_uid().unwrap_or("");
        let ovs = overrides.get(uid).cloned().unwrap_or_default();
        let overridden = override_keys(&ovs, tz);

        for local in expand_occurrences(event, tz, start, end) {
            if local < start || local >= end {
                continue;
            }
            if overridden.contains(&occurrence_key(local, all_day)) {
                continue;
            }
            out.push(calendar_event(
                title,
                local,
                all_day,
                from,
                is_recurring(event),
            ));
        }
    }

    for ovs in overrides.values() {
        for event in ovs {
            if event.get_status() == Some(EventStatus::Cancelled) {
                continue;
            }
            let Some(title) = event.get_summary() else {
                continue;
            };
            let Some(when) = event.get_start() else {
                continue;
            };
            let Some((local, all_day)) = date_to_local(when, tz) else {
                continue;
            };
            if local < start || local >= end {
                continue;
            }
            out.push(calendar_event(
                title,
                local,
                all_day,
                from,
                is_recurring(event),
            ));
        }
    }

    out.sort_by(|a, b| {
        a.date
            .cmp(&b.date)
            .then(a.start.cmp(&b.start))
            .then(a.title.cmp(&b.title))
    });
    Ok(out)
}

fn calendar_event(
    title: &str,
    local: DateTime<Tz>,
    all_day: bool,
    from: NaiveDate,
    recurring: bool,
) -> CalendarEvent {
    CalendarEvent {
        start: if all_day {
            String::new()
        } else {
            local.format("%H:%M").to_string()
        },
        title: crate::model::truncate_event_title(title),
        who: String::new(),
        all_day,
        day_label: day_label(local.date_naive(), from),
        date: local.date_naive().format("%Y-%m-%d").to_string(),
        birthday: false,
        school: false,
        recurring,
    }
}

fn is_recurring(event: &Event) -> bool {
    event.property_value("RRULE").is_some()
        || !properties_named(event, "RDATE").is_empty()
        || event.get_recurrence_id().is_some()
}

fn occurrence_key(local: DateTime<Tz>, all_day: bool) -> i64 {
    if all_day {
        local
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp()
    } else {
        local.timestamp()
    }
}

fn override_keys(ovs: &[Event], tz: Tz) -> HashSet<i64> {
    ovs.iter()
        .filter_map(|event| {
            let rid = event.get_recurrence_id()?;
            let (local, all_day) = date_to_local(rid, tz)?;
            Some(occurrence_key(local, all_day))
        })
        .collect()
}

fn expand_occurrences(
    event: &Event,
    tz: Tz,
    window_start: DateTime<Tz>,
    window_end: DateTime<Tz>,
) -> Vec<DateTime<Tz>> {
    let Some(when) = event.get_start() else {
        return Vec::new();
    };
    let Some((dt_start, _)) = date_to_local(when, tz) else {
        return Vec::new();
    };

    if event.property_value("RRULE").is_none() {
        let mut dates = Vec::new();
        if dt_start >= window_start && dt_start < window_end {
            dates.push(dt_start);
        }
        dates.extend(
            extra_dates(event, "RDATE", tz)
                .into_iter()
                .filter(|d| *d >= window_start && *d < window_end),
        );
        return dates;
    }

    match expand_rrule(event, tz, window_start, window_end) {
        Some(dates) => dates,
        None => {
            if dt_start >= window_start && dt_start < window_end {
                vec![dt_start]
            } else {
                Vec::new()
            }
        }
    }
}

fn expand_rrule(
    event: &Event,
    tz: Tz,
    window_start: DateTime<Tz>,
    window_end: DateTime<Tz>,
) -> Option<Vec<DateTime<Tz>>> {
    let rrule = event.property_value("RRULE")?;
    let dtstart = event.properties().get("DTSTART")?;
    let blob = format!("{}\nRRULE:{rrule}", format_property(dtstart));
    let set: RRuleSet = match blob.parse() {
        Ok(set) => set,
        Err(err) => {
            warn!(rrule, %err, "invalid RRULE; using DTSTART only");
            return None;
        }
    };

    let rrule_tz = set.get_dt_start().timezone();
    let mut set = set;
    for local in extra_dates(event, "EXDATE", tz) {
        set = set.exdate(local.with_timezone(&rrule_tz));
    }
    for local in extra_dates(event, "RDATE", tz) {
        set = set.rdate(local.with_timezone(&rrule_tz));
    }

    // `after` is exclusive, so nudge back to keep occurrences at window start.
    let after = window_start.with_timezone(&rrule_tz) - Duration::nanoseconds(1);
    let before = window_end.with_timezone(&rrule_tz);
    let result = set.after(after).before(before).all(u16::MAX);
    Some(
        result
            .dates
            .into_iter()
            .map(|dt| dt.with_timezone(&tz))
            .collect(),
    )
}

fn extra_dates(event: &Event, key: &str, tz: Tz) -> Vec<DateTime<Tz>> {
    properties_named(event, key)
        .into_iter()
        .flat_map(|prop| datetimes_from_property(prop, tz))
        .map(|(local, _)| local)
        .collect()
}

fn properties_named<'a>(event: &'a Event, key: &str) -> Vec<&'a Property> {
    if let Some(values) = event.multi_properties().get(key) {
        return values.iter().collect();
    }
    event.properties().get(key).into_iter().collect()
}

fn datetimes_from_property(prop: &Property, tz: Tz) -> Vec<(DateTime<Tz>, bool)> {
    prop.value()
        .split(',')
        .filter_map(|part| {
            let part = part.trim();
            if part.is_empty() {
                return None;
            }
            let mut parsed = Property::new(prop.key(), part);
            for param in prop.params().values() {
                parsed.add_parameter(param.key(), param.value());
            }
            let when = DatePerhapsTime::from_property(&parsed)?;
            date_to_local(when, tz)
        })
        .collect()
}

fn format_property(prop: &Property) -> String {
    let mut line = prop.key().to_string();
    for param in prop.params().values() {
        line.push(';');
        line.push_str(param.key());
        line.push('=');
        line.push_str(param.value());
    }
    line.push(':');
    line.push_str(prop.value());
    line
}

fn date_to_local(when: DatePerhapsTime, tz: Tz) -> Option<(DateTime<Tz>, bool)> {
    match when {
        DatePerhapsTime::DateTime(dt) => {
            let local = match dt {
                CalendarDateTime::Utc(utc) => utc.with_timezone(&tz),
                CalendarDateTime::Floating(naive) => tz.from_local_datetime(&naive).single()?,
                CalendarDateTime::WithTimezone { date_time, tzid } => {
                    let event_tz: Tz = tzid.parse().unwrap_or(tz);
                    event_tz
                        .from_local_datetime(&date_time)
                        .single()?
                        .with_timezone(&tz)
                }
            };
            Some((local, false))
        }
        DatePerhapsTime::Date(date) => {
            let naive = date.and_time(NaiveTime::from_hms_opt(0, 0, 0)?);
            let local = tz.from_local_datetime(&naive).single()?;
            Some((local, true))
        }
    }
}

pub(crate) fn day_label(date: NaiveDate, today: NaiveDate) -> String {
    if date == today {
        "Today".into()
    } else if date == today + Duration::days(1) {
        "Tomorrow".into()
    } else if date.year() == today.year() && date.month() == today.month() {
        date.format("%a %-d").to_string()
    } else if date.year() == today.year() {
        date.format("%-d %b").to_string()
    } else {
        date.format("%-d %b %Y").to_string()
    }
}

/// Used by fixture tests; keeps the compiler aware of Local if callers need it.
pub fn today_local(tz: Tz) -> NaiveDate {
    Utc::now().with_timezone(&tz).date_naive()
}

trait YearMonthDay {
    fn year(&self) -> i32;
    fn month(&self) -> u32;
    fn day(&self) -> u32;
}

impl YearMonthDay for NaiveDate {
    fn year(&self) -> i32 {
        chrono::Datelike::year(self)
    }
    fn month(&self) -> u32 {
        chrono::Datelike::month(self)
    }
    fn day(&self) -> u32 {
        chrono::Datelike::day(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"BEGIN:VCALENDAR
VERSION:2.0
BEGIN:VEVENT
DTSTART:20260912T090000Z
DTEND:20260912T100000Z
SUMMARY:School run
DESCRIPTION:Alex
END:VEVENT
BEGIN:VEVENT
DTSTART;VALUE=DATE:20260913
SUMMARY:Swim
END:VEVENT
END:VCALENDAR
"#;

    const PICKUP: &str = r#"BEGIN:VCALENDAR
VERSION:2.0
BEGIN:VEVENT
SUMMARY:Antoine - Pick-up Armand
DTSTART;TZID=Europe/London:20260910T151500
DTEND;TZID=Europe/London:20260910T154500
UID:0951A873-1F9C-40DB-817F-E554BB287C49
RRULE:FREQ=WEEKLY;BYDAY=TH;WKST=SU
STATUS:CONFIRMED
END:VEVENT
END:VCALENDAR
"#;

    #[test]
    fn parses_timed_and_all_day_events() {
        let tz: Tz = "UTC".parse().unwrap();
        let day = NaiveDate::from_ymd_opt(2026, 9, 12).unwrap();
        let events = parse_events(SAMPLE, tz, day, 7).unwrap();
        assert_eq!(events[0].title, "School run");
        assert_eq!(events[0].start, "09:00");
        assert!(!events[0].recurring);
        assert_eq!(events[1].title, "Swim");
        assert!(events[1].all_day);
        assert!(!events[1].recurring);
    }

    #[test]
    fn long_gmail_invite_keeps_truncated_title_only() {
        let ics = r#"BEGIN:VCALENDAR
VERSION:2.0
BEGIN:VEVENT
DTSTART:20260913T090000Z
SUMMARY:Your event was created from an email that you received in Gmail. https://mail.google.com/mail?extsrc=cal&plid=ACUX6DC00
DESCRIPTION:Book MOT\nPlease confirm the garage slot.\n\nhttps://mail.google.com/mail?extsrc=cal&plid=ACUX6DC00
END:VEVENT
END:VCALENDAR
"#;
        let tz: Tz = "UTC".parse().unwrap();
        let day = NaiveDate::from_ymd_opt(2026, 9, 13).unwrap();
        let events = parse_events(ics, tz, day, 1).unwrap();
        assert_eq!(events.len(), 1);
        assert!(events[0].who.is_empty());
        assert!(!events[0].title.contains("https://"));
        assert!(!events[0].title.contains("Book MOT"));
        assert!(events[0].title.ends_with('…'));
        assert!(events[0].title.chars().count() <= crate::model::EVENT_TITLE_MAX_CHARS);
        assert!(events[0].title.starts_with("Your event was created"));
    }

    #[test]
    fn coming_next_labels_use_month_when_needed() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 13).unwrap();
        assert_eq!(day_label(today, today), "Today");
        assert_eq!(day_label(today + Duration::days(1), today), "Tomorrow");
        assert_eq!(
            day_label(NaiveDate::from_ymd_opt(2026, 9, 17).unwrap(), today),
            "Thu 17"
        );
        assert_eq!(
            day_label(NaiveDate::from_ymd_opt(2026, 10, 3).unwrap(), today),
            "3 Oct"
        );
        assert_eq!(
            day_label(NaiveDate::from_ymd_opt(2027, 1, 15).unwrap(), today),
            "15 Jan 2027"
        );
    }

    #[test]
    fn weekly_rrule_shows_later_thursdays() {
        let tz: Tz = "Europe/London".parse().unwrap();
        let day = NaiveDate::from_ymd_opt(2026, 9, 17).unwrap();
        let events = parse_events(PICKUP, tz, day, 1).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].title, "Antoine - Pick-up Armand");
        assert_eq!(events[0].start, "15:15");
        assert_eq!(events[0].date, "2026-09-17");
        assert_eq!(events[0].day_label, "Today");
        assert!(events[0].recurring);
    }

    #[test]
    fn weekly_rrule_expands_across_horizon() {
        let tz: Tz = "Europe/London".parse().unwrap();
        let day = NaiveDate::from_ymd_opt(2026, 9, 17).unwrap();
        let events = parse_events(PICKUP, tz, day, 15).unwrap();
        let dates: Vec<_> = events.iter().map(|e| e.date.as_str()).collect();
        assert_eq!(dates, ["2026-09-17", "2026-09-24", "2026-10-01"]);
    }

    #[test]
    fn weekly_rrule_respects_exdate() {
        let ics = r#"BEGIN:VCALENDAR
VERSION:2.0
BEGIN:VEVENT
SUMMARY:Antoine - Pick-up Armand
DTSTART;TZID=Europe/London:20260910T151500
UID:0951A873-1F9C-40DB-817F-E554BB287C49
RRULE:FREQ=WEEKLY;BYDAY=TH;WKST=SU
EXDATE;TZID=Europe/London:20260917T151500
END:VEVENT
END:VCALENDAR
"#;
        let tz: Tz = "Europe/London".parse().unwrap();
        let day = NaiveDate::from_ymd_opt(2026, 9, 17).unwrap();
        let events = parse_events(ics, tz, day, 1).unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn recurrence_id_override_replaces_instance() {
        let ics = r#"BEGIN:VCALENDAR
VERSION:2.0
BEGIN:VEVENT
SUMMARY:Antoine - Pick-up Armand
DTSTART;TZID=Europe/London:20260910T151500
UID:0951A873-1F9C-40DB-817F-E554BB287C49
RRULE:FREQ=WEEKLY;BYDAY=TH;WKST=SU
END:VEVENT
BEGIN:VEVENT
SUMMARY:Pick-up moved
DTSTART;TZID=Europe/London:20260917T160000
UID:0951A873-1F9C-40DB-817F-E554BB287C49
RECURRENCE-ID;TZID=Europe/London:20260917T151500
END:VEVENT
END:VCALENDAR
"#;
        let tz: Tz = "Europe/London".parse().unwrap();
        let day = NaiveDate::from_ymd_opt(2026, 9, 17).unwrap();
        let events = parse_events(ics, tz, day, 1).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].title, "Pick-up moved");
        assert_eq!(events[0].start, "16:00");
        assert!(events[0].recurring);
    }

    #[test]
    fn cancelled_recurrence_instance_is_omitted() {
        let ics = r#"BEGIN:VCALENDAR
VERSION:2.0
BEGIN:VEVENT
SUMMARY:Antoine - Pick-up Armand
DTSTART;TZID=Europe/London:20260910T151500
UID:0951A873-1F9C-40DB-817F-E554BB287C49
RRULE:FREQ=WEEKLY;BYDAY=TH;WKST=SU
END:VEVENT
BEGIN:VEVENT
SUMMARY:Antoine - Pick-up Armand
DTSTART;TZID=Europe/London:20260917T151500
UID:0951A873-1F9C-40DB-817F-E554BB287C49
RECURRENCE-ID;TZID=Europe/London:20260917T151500
STATUS:CANCELLED
END:VEVENT
END:VCALENDAR
"#;
        let tz: Tz = "Europe/London".parse().unwrap();
        let day = NaiveDate::from_ymd_opt(2026, 9, 17).unwrap();
        let events = parse_events(ics, tz, day, 1).unwrap();
        assert!(events.is_empty());
    }
}
