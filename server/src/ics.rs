use chrono::{DateTime, Duration, Local, NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use icalendar::{Calendar, CalendarComponent, CalendarDateTime, Component, DatePerhapsTime};

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
    let mut out = Vec::new();

    for component in calendar.iter() {
        let CalendarComponent::Event(event) = component else {
            continue;
        };
        let Some(title) = event.get_summary().map(str::to_string) else {
            continue;
        };
        let Some(when) = event.get_start() else {
            continue;
        };
        let (local, all_day) = match date_to_local(when, tz) {
            Some(v) => v,
            None => continue,
        };
        if local < start || local >= end {
            continue;
        }
        out.push(CalendarEvent {
            start: if all_day {
                String::new()
            } else {
                local.format("%H:%M").to_string()
            },
            title: crate::model::truncate_event_title(&title),
            who: String::new(),
            all_day,
            day_label: day_label(local.date_naive(), from),
            date: local.date_naive().format("%Y-%m-%d").to_string(),
            birthday: false,
        });
    }
    out.sort_by(|a, b| {
        a.date
            .cmp(&b.date)
            .then(a.start.cmp(&b.start))
            .then(a.title.cmp(&b.title))
    });
    Ok(out)
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

pub fn _now_local() -> DateTime<Local> {
    Local::now()
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

    #[test]
    fn parses_timed_and_all_day_events() {
        let tz: Tz = "UTC".parse().unwrap();
        let day = NaiveDate::from_ymd_opt(2026, 9, 12).unwrap();
        let events = parse_events(SAMPLE, tz, day, 7).unwrap();
        assert_eq!(events[0].title, "School run");
        assert_eq!(events[0].start, "09:00");
        assert_eq!(events[1].title, "Swim");
        assert!(events[1].all_day);
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
}
