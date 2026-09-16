//! When the Pico should next wake: a fixed interval, or the next `wake-up` clock time.

use chrono::{DateTime, Duration, NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;

/// Seconds until the Pico should poll again.
///
/// An empty `wake_ups` list uses `interval_secs`. Otherwise the next clock
/// time strictly after `now` in `tz` wins, wrapping to tomorrow if needed.
pub fn seconds_until_next_poll(
    now: DateTime<Utc>,
    tz: Tz,
    interval_secs: u64,
    wake_ups: &[NaiveTime],
) -> u64 {
    let interval = interval_secs.max(1);
    if wake_ups.is_empty() {
        return interval;
    }

    let mut times: Vec<NaiveTime> = wake_ups.to_vec();
    times.sort();
    times.dedup();

    let now_local = now.with_timezone(&tz);
    let today = now_local.date_naive();
    // Today, tomorrow, and the day after cover a spring-forward skip.
    for day_offset in 0..3 {
        let date = today + Duration::days(day_offset);
        for &time in &times {
            let Some(dt) = resolve_local(tz, date, time) else {
                continue;
            };
            if dt > now_local {
                let secs = (dt - now_local).num_seconds();
                return (secs.max(1)) as u64;
            }
        }
    }
    interval
}

fn resolve_local(tz: Tz, date: NaiveDate, time: NaiveTime) -> Option<DateTime<Tz>> {
    match tz.from_local_datetime(&date.and_time(time)) {
        chrono::LocalResult::Single(dt) => Some(dt),
        chrono::LocalResult::Ambiguous(earliest, _) => Some(earliest),
        chrono::LocalResult::None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn london() -> Tz {
        chrono_tz::Europe::London
    }

    fn t(hhmm: &str) -> NaiveTime {
        let (h, m) = hhmm.split_once(':').unwrap();
        NaiveTime::from_hms_opt(h.parse().unwrap(), m.parse().unwrap(), 0).unwrap()
    }

    fn at_london(y: i32, month: u32, d: u32, h: u32, min: u32, s: u32) -> DateTime<Utc> {
        london()
            .with_ymd_and_hms(y, month, d, h, min, s)
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn empty_wake_ups_use_interval() {
        let now = at_london(2026, 9, 16, 12, 0, 0);
        assert_eq!(seconds_until_next_poll(now, london(), 3600, &[]), 3600);
        assert_eq!(seconds_until_next_poll(now, london(), 0, &[]), 1);
    }

    #[test]
    fn next_slot_later_today() {
        let now = at_london(2026, 9, 16, 12, 0, 0);
        let wakes = [t("06:00"), t("15:00"), t("23:15")];
        assert_eq!(
            seconds_until_next_poll(now, london(), 3600, &wakes),
            3 * 3600
        );
    }

    #[test]
    fn unsorted_times_still_pick_next() {
        let now = at_london(2026, 9, 16, 12, 0, 0);
        let wakes = [t("23:15"), t("06:00"), t("15:00")];
        assert_eq!(
            seconds_until_next_poll(now, london(), 3600, &wakes),
            3 * 3600
        );
    }

    #[test]
    fn wraps_past_last_slot_to_tomorrow() {
        let now = at_london(2026, 9, 16, 23, 30, 0);
        let wakes = [t("06:00"), t("23:15")];
        assert_eq!(
            seconds_until_next_poll(now, london(), 3600, &wakes),
            6 * 3600 + 30 * 60
        );
    }

    #[test]
    fn skips_the_slot_already_in_progress() {
        let now = at_london(2026, 9, 16, 6, 0, 0);
        let wakes = [t("06:00"), t("07:00")];
        assert_eq!(seconds_until_next_poll(now, london(), 3600, &wakes), 3600);
    }

    #[test]
    fn single_daily_time_sleeps_until_tomorrow() {
        let now = at_london(2026, 9, 16, 6, 0, 5);
        let wakes = [t("06:00")];
        assert_eq!(
            seconds_until_next_poll(now, london(), 3600, &wakes),
            24 * 3600 - 5
        );
    }

    #[test]
    fn before_first_slot_same_day() {
        let now = at_london(2026, 9, 16, 5, 0, 0);
        let wakes = [t("06:00"), t("07:00")];
        assert_eq!(seconds_until_next_poll(now, london(), 3600, &wakes), 3600);
    }

    #[test]
    fn spring_forward_skips_missing_local_time() {
        // 29 Mar 2026: 01:00 GMT → 02:00 BST, so 01:30 does not exist.
        let now = at_london(2026, 3, 29, 0, 30, 0);
        let wakes = [t("01:30"), t("03:00")];
        assert_eq!(
            seconds_until_next_poll(now, london(), 3600, &wakes),
            90 * 60
        );
    }

    #[test]
    fn autumn_ambiguity_picks_the_earlier_offset() {
        // 25 Oct 2026: 02:00 BST → 01:00 GMT. 01:30 occurs twice; use the first.
        let now = london()
            .from_local_datetime(
                &NaiveDate::from_ymd_opt(2026, 10, 25)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap(),
            )
            .earliest()
            .unwrap()
            .with_timezone(&Utc);
        let wakes = [t("01:30")];
        let secs = seconds_until_next_poll(now, london(), 3600, &wakes);
        assert_eq!(secs, 90 * 60);
    }
}
