//! When the Pico should next wake: a fixed interval, or the next `wake-up` clock time.

use chrono::{DateTime, Duration, NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;

/// Largest stored/applied Pico timer error, as a fraction of the asked sleep.
/// Bigger gaps usually mean a button wake, USB wait, or a missed poll.
pub const MAX_PICO_DRIFT: f64 = 0.05;

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

/// Result of comparing two Pico polls for LPOSC drift.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DriftSample {
    /// Button, USB, cold boot, missing sleep, or a non-timer pair.
    Skip,
    /// `(elapsed - asked) / asked`. Positive means the Pico woke late.
    Measured(f64),
    /// Magnitude over [`MAX_PICO_DRIFT`] — leave the stored value alone.
    OutOfRange {
        asked: u64,
        elapsed: i64,
        drift: f64,
    },
}

pub fn is_timer_wake(wake: &str) -> bool {
    wake.eq_ignore_ascii_case("timer")
}

/// Measure drift only between two consecutive automatic timer polls.
///
/// Button wakes (early), USB waits (different clock), and cold boots are skipped.
pub fn drift_between_polls(
    prev_wake: &str,
    prev_usb: bool,
    prev_sleep_s: u64,
    prev_at: DateTime<Utc>,
    wake: &str,
    usb: bool,
    now: DateTime<Utc>,
) -> DriftSample {
    if !is_timer_wake(wake) || !is_timer_wake(prev_wake) || usb || prev_usb || prev_sleep_s == 0 {
        return DriftSample::Skip;
    }
    let elapsed = now.signed_duration_since(prev_at).num_seconds();
    if elapsed <= 0 {
        return DriftSample::Skip;
    }
    measure_pico_drift(prev_sleep_s, elapsed)
}

/// Fractional error of a completed sleep: `(elapsed - asked) / asked`.
pub fn measure_pico_drift(asked_secs: u64, elapsed_secs: i64) -> DriftSample {
    if asked_secs == 0 || elapsed_secs <= 0 {
        return DriftSample::Skip;
    }
    let drift = (elapsed_secs as f64 - asked_secs as f64) / asked_secs as f64;
    if !drift.is_finite() {
        return DriftSample::Skip;
    }
    if drift.abs() > MAX_PICO_DRIFT {
        return DriftSample::OutOfRange {
            asked: asked_secs,
            elapsed: elapsed_secs,
            drift,
        };
    }
    DriftSample::Measured(drift)
}

pub fn clamp_pico_drift(drift: f64) -> f64 {
    if !drift.is_finite() {
        0.0
    } else {
        drift.clamp(-MAX_PICO_DRIFT, MAX_PICO_DRIFT)
    }
}

/// Round to 0.01 percentage points so config.toml stays stable.
pub fn round_pico_drift(drift: f64) -> f64 {
    (clamp_pico_drift(drift) * 10_000.0).round() / 10_000.0
}

/// First sample replaces zero; later samples are averaged so one slow Wi-Fi
/// join does not yank the stored value.
pub fn blend_pico_drift(stored: f64, measured: f64) -> f64 {
    let measured = clamp_pico_drift(measured);
    let stored = clamp_pico_drift(stored);
    if stored.abs() < 1e-12 {
        return round_pico_drift(measured);
    }
    round_pico_drift(0.5 * stored + 0.5 * measured)
}

/// Shorten (or lengthen) the POWMAN sleep so wall-clock arrival matches `target_secs`.
pub fn compensate_sleep_secs(target_secs: u64, drift: f64) -> u64 {
    let target = target_secs.max(1) as f64;
    let factor = 1.0 + clamp_pico_drift(drift);
    if factor <= 0.5 {
        return target_secs.max(1);
    }
    (target / factor).round().max(1.0) as u64
}

/// Expand a commanded POWMAN sleep back to expected wall-clock seconds.
pub fn wall_secs_from_commanded(commanded_secs: u64, drift: f64) -> u64 {
    let commanded = commanded_secs.max(1) as f64;
    let factor = 1.0 + clamp_pico_drift(drift);
    (commanded * factor).round().max(1.0) as u64
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

    #[test]
    fn two_minutes_late_per_hour_is_under_five_percent() {
        // 3720s wall vs 3600s asked ≈ 3.33% slow (LPOSC).
        assert_eq!(
            measure_pico_drift(3600, 3720),
            DriftSample::Measured(120.0 / 3600.0)
        );
        assert_eq!(compensate_sleep_secs(3600, 120.0 / 3600.0), 3484);
        assert_eq!(
            wall_secs_from_commanded(compensate_sleep_secs(3600, 0.03), 0.03),
            3600
        );
    }

    #[test]
    fn drift_over_five_percent_is_rejected() {
        // 6% late, or a button cutting the interval in half.
        assert!(matches!(
            measure_pico_drift(3600, 3816),
            DriftSample::OutOfRange {
                asked: 3600,
                elapsed: 3816,
                ..
            }
        ));
        assert_eq!(
            measure_pico_drift(3600, 1800),
            DriftSample::OutOfRange {
                asked: 3600,
                elapsed: 1800,
                drift: -0.5,
            }
        );
        assert_eq!(measure_pico_drift(0, 10), DriftSample::Skip);
    }

    #[test]
    fn compensate_clamps_and_never_returns_zero() {
        assert_eq!(compensate_sleep_secs(3600, 0.0), 3600);
        assert_eq!(
            compensate_sleep_secs(3600, 0.2),
            compensate_sleep_secs(3600, 0.05)
        );
        assert_eq!(compensate_sleep_secs(3600, f64::NAN), 3600);
        assert_eq!(compensate_sleep_secs(0, 0.03), 1);
        // Clock runs fast: ask for a longer POWMAN nap.
        assert_eq!(compensate_sleep_secs(3600, -0.05), 3789);
    }

    #[test]
    fn blend_uses_first_sample_then_averages() {
        assert_eq!(blend_pico_drift(0.0, 0.0333), 0.0333);
        assert_eq!(blend_pico_drift(0.02, 0.04), 0.03);
    }

    #[test]
    fn only_timer_to_timer_on_battery_counts() {
        let t0 = at_london(2026, 9, 16, 12, 0, 0);
        let t1 = at_london(2026, 9, 16, 13, 2, 0);
        assert!(matches!(
            drift_between_polls("timer", false, 3600, t0, "timer", false, t1),
            DriftSample::Measured(_)
        ));
        assert_eq!(
            drift_between_polls("timer", false, 3600, t0, "button", false, t1),
            DriftSample::Skip
        );
        assert_eq!(
            drift_between_polls("button", false, 3600, t0, "timer", false, t1),
            DriftSample::Skip
        );
        assert_eq!(
            drift_between_polls("timer", false, 3600, t0, "timer", true, t1),
            DriftSample::Skip
        );
        assert_eq!(
            drift_between_polls("cold", false, 3600, t0, "timer", false, t1),
            DriftSample::Skip
        );
    }
}
