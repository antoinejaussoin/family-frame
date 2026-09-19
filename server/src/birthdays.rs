use chrono::{Datelike, Duration, NaiveDate};

use crate::config::Birthday;
use crate::ics;
use crate::model::{CalendarEvent, BIRTHDAY_HORIZON_DAYS};

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
            who: String::new(),
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
    fn blank_names_are_ignored() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 14).unwrap();
        let events = upcoming_events(&[person("  ", 2018, 9, 14)], today);
        assert!(events.is_empty());
    }
}
