use chrono::{Datelike, Duration, NaiveDate};

use crate::config::Birthday;
use crate::model::{CalendarEvent, BIRTHDAY_HORIZON_DAYS};

use super::contribute::{Contribution, SourceOutcome};
use super::context::SourceContext;
use super::ics;
use super::DataSource;

pub struct BirthdaysSource;

#[async_trait::async_trait]
impl DataSource for BirthdaysSource {
    fn id(&self) -> &'static str {
        "birthdays"
    }

    fn enabled(&self, _cfg: &crate::config::Config) -> bool {
        true
    }

    fn private(&self) -> bool {
        true
    }

    fn disabled_note(&self) -> String {
        String::new()
    }

    fn demo(&self, ctx: &SourceContext<'_>) -> Option<Contribution> {
        Some(Contribution::Calendar(demo_birthday_events(ctx.today)))
    }

    async fn load(&self, ctx: &SourceContext<'_>) -> anyhow::Result<SourceOutcome> {
        Ok(SourceOutcome::live(
            String::new(),
            Contribution::Calendar(upcoming_events(&ctx.cfg.birthdays, ctx.today)),
        ))
    }
}

/// Canned people for `--fake` screenshots. Dates sit inside the two-week window.
pub fn demo_birthday_events(today: NaiveDate) -> Vec<CalendarEvent> {
    upcoming_events(&demo_people(today), today)
}

fn demo_people(today: NaiveDate) -> Vec<Birthday> {
    let person = |name: &str, offset_days: i64, age: i32| {
        let next = today + Duration::days(offset_days);
        let dob = NaiveDate::from_ymd_opt(next.year() - age, next.month(), next.day())
            .unwrap_or(next);
        Birthday {
            name: name.into(),
            dob,
        }
    };
    vec![person("Maya", 0, 8), person("Sam", 6, 11)]
}

/// Birthdays whose next occurrence is today or within two weeks.
pub fn upcoming_events(birthdays: &[Birthday], today: NaiveDate) -> Vec<CalendarEvent> {
    let last = today + Duration::days(BIRTHDAY_HORIZON_DAYS);
    let mut out = Vec::new();
    for person in birthdays {
        let name = person.name.trim();
        if name.is_empty() {
            continue;
        }
        let Some(next) = next_occurrence(person.dob, today) else {
            continue;
        };
        if next > last {
            continue;
        }
        let age = next.year() - person.dob.year();
        out.push(CalendarEvent {
            start: String::new(),
            title: crate::model::truncate_event_title(&format!("{name} turns {age}")),
            all_day: true,
            day_label: ics::day_label(next, today),
            date: next.format("%Y-%m-%d").to_string(),
            birthday: true,
            school: false,
            recurring: false,
        });
    }
    out
}

fn next_occurrence(dob: NaiveDate, today: NaiveDate) -> Option<NaiveDate> {
    let this_year = birthday_in_year(dob, today.year())?;
    if this_year >= today {
        Some(this_year)
    } else {
        birthday_in_year(dob, today.year() + 1)
    }
}

fn birthday_in_year(dob: NaiveDate, year: i32) -> Option<NaiveDate> {
    dob.with_year(year).or_else(|| {
        if dob.month() == 2 && dob.day() == 29 {
            NaiveDate::from_ymd_opt(year, 2, 28)
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(name: &str, y: i32, m: u32, d: u32) -> Birthday {
        Birthday {
            name: name.into(),
            dob: NaiveDate::from_ymd_opt(y, m, d).unwrap(),
        }
    }

    #[test]
    fn today_and_two_week_horizon() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 14).unwrap();
        let events = upcoming_events(
            &[
                person("Maya", 2018, 9, 14),
                person("Sam", 2015, 9, 28),
                person("Later", 2010, 9, 29),
            ],
            today,
        );
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].title, "Maya turns 8");
        assert_eq!(events[0].day_label, "Today");
        assert!(events[0].birthday);
        assert!(events[0].all_day);
        assert_eq!(events[1].title, "Sam turns 11");
        assert_eq!(events[1].date, "2026-09-28");
    }

    #[test]
    fn wraps_into_next_year_near_new_year() {
        let today = NaiveDate::from_ymd_opt(2026, 12, 25).unwrap();
        let events = upcoming_events(&[person("New year baby", 2020, 1, 3)], today);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].date, "2027-01-03");
        assert_eq!(events[0].title, "New year baby turns 7");
        assert_eq!(events[0].day_label, "3 Jan 2027");
    }

    #[test]
    fn skips_birthdays_already_past_this_year() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 14).unwrap();
        let events = upcoming_events(&[person("June", 2012, 6, 1)], today);
        assert!(events.is_empty());
    }

    #[test]
    fn leap_day_falls_back_to_28_feb() {
        let today = NaiveDate::from_ymd_opt(2027, 2, 20).unwrap();
        let events = upcoming_events(&[person("Leap", 2016, 2, 29)], today);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].date, "2027-02-28");
        assert_eq!(events[0].title, "Leap turns 11");
    }

    #[test]
    fn demo_birthdays_sit_inside_the_horizon() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 18).unwrap();
        let events = demo_birthday_events(today);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].title, "Maya turns 8");
        assert_eq!(events[0].day_label, "Today");
        assert!(events[0].birthday);
        assert_eq!(events[1].title, "Sam turns 11");
        assert_eq!(events[1].date, "2026-09-24");
        assert!(!events.iter().any(|e| e.title.contains("REAL")));
    }

    #[test]
    fn blank_names_are_ignored() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 14).unwrap();
        let events = upcoming_events(&[person("  ", 2018, 9, 14)], today);
        assert!(events.is_empty());
    }
}
