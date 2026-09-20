//! When the Pico should next wake: a fixed interval, or the next `wake-up` clock time.

use chrono::{
    DateTime, Datelike, Duration, NaiveDate, NaiveTime, SecondsFormat, TimeZone, Utc, Weekday,
};
use chrono_tz::Tz;

/// `mon` … `sun`, Monday first (UK week).
pub const WEEKDAY_KEYS: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

/// Wake times for each weekday. An empty day is skipped when scheduling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeeklyWakes {
    days: [Vec<NaiveTime>; 7],
}

impl Default for WeeklyWakes {
    fn default() -> Self {
        Self::EMPTY
    }
}

impl WeeklyWakes {
    pub const EMPTY: Self = Self {
        days: [
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        ],
    };

    pub fn from_days(mut days: [Vec<NaiveTime>; 7]) -> Self {
        for day in &mut days {
            day.sort();
            day.dedup();
        }
        Self { days }
    }

    pub fn every_day(times: Vec<NaiveTime>) -> Self {
        let mut times = times;
        times.sort();
        times.dedup();
        Self {
            days: std::array::from_fn(|_| times.clone()),
        }
    }

    pub fn parse_day_key(key: &str) -> Result<usize, String> {
        match key.trim().to_ascii_lowercase().trim_end_matches('.') {
            "mon" | "monday" => Ok(0),
            "tue" | "tues" | "tuesday" => Ok(1),
            "wed" | "wednesday" => Ok(2),
            "thu" | "thur" | "thurs" | "thursday" => Ok(3),
            "fri" | "friday" => Ok(4),
            "sat" | "saturday" => Ok(5),
            "sun" | "sunday" => Ok(6),
            other => Err(format!(
                "unknown wake-up day `{other}` (use mon, tue, wed, thu, fri, sat, sun)"
            )),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.days.iter().all(|d| d.is_empty())
    }

    pub fn is_uniform(&self) -> bool {
        self.days.windows(2).all(|w| w[0] == w[1])
    }

    /// Shared list when every day is the same; otherwise `None`.
    pub fn shared_times(&self) -> Option<&[NaiveTime]> {
        self.is_uniform().then_some(self.days[0].as_slice())
    }

    pub fn get(&self, weekday: Weekday) -> &[NaiveTime] {
        self.get_index(weekday.num_days_from_monday() as usize)
    }

    pub fn get_index(&self, idx: usize) -> &[NaiveTime] {
        &self.days[idx]
    }

    /// Scheduled wakes each weekday (Monday first). An empty week uses the
    /// interval: `86400 / interval_secs` every day.
    pub fn wakes_per_weekday(&self, interval_secs: u64) -> [f64; 7] {
        if self.is_empty() {
            let n = 86_400.0 / interval_secs.max(1) as f64;
            [n; 7]
        } else {
            std::array::from_fn(|i| self.get_index(i).len() as f64)
        }
    }
}

/// Largest stored/applied Pico timer error, as a fraction of the asked sleep.
/// Bigger gaps usually mean a button wake, USB wait, or a missed poll.
pub const MAX_PICO_DRIFT: f64 = 0.05;

/// Seconds until the Pico should poll again.
///
/// An empty week uses `interval_secs`. Otherwise the next clock time strictly
/// after `now` in `tz` wins, using that weekday’s list and wrapping to the
/// next day that has a time (including next week).
pub fn seconds_until_next_poll(
    now: DateTime<Utc>,
    tz: Tz,
    interval_secs: u64,
    wake_ups: &WeeklyWakes,
) -> u64 {
    secs_until(now, next_poll_at(now, tz, interval_secs, wake_ups))
}

/// Wall-clock instant of the next intended poll (the schedule slot, not
/// `now` plus a truncated second count).
pub fn next_poll_at(
    now: DateTime<Utc>,
    tz: Tz,
    interval_secs: u64,
    wake_ups: &WeeklyWakes,
) -> DateTime<Utc> {
    match next_wake_after(now, tz, wake_ups) {
        Some(dt) => dt.with_timezone(&Utc),
        None => instant_after(now, interval_secs.max(1)),
    }
}

/// Wall-clock seconds until the next poll after a `wake=timer` request.
///
/// The previous response's stored slot (`assigned_wake`) *is* this poll, as
/// long as the following slot has not started yet. Early or late arrival does
/// not matter; a missed cycle (now at or after the slot after that) schedules
/// from `now` instead.
pub fn seconds_until_next_poll_for_timer(
    now: DateTime<Utc>,
    tz: Tz,
    interval_secs: u64,
    wake_ups: &WeeklyWakes,
    assigned_wake: Option<DateTime<Utc>>,
) -> u64 {
    secs_until(
        now,
        next_poll_at_for_timer(now, tz, interval_secs, wake_ups, assigned_wake),
    )
}

/// Next slot after a timer poll that is serving `assigned_wake`.
///
/// `assigned_wake` is the Pico's echoed `X-Wake-At` — the exact instant from
/// the previous response, or nothing. No snapping or reconstruction.
pub fn next_poll_at_for_timer(
    now: DateTime<Utc>,
    tz: Tz,
    interval_secs: u64,
    wake_ups: &WeeklyWakes,
    assigned_wake: Option<DateTime<Utc>>,
) -> DateTime<Utc> {
    refresh_window(now, tz, interval_secs, wake_ups, assigned_wake).1
}

/// Last/next instants painted on the dashboard for this contact.
///
/// A timer poll serving `assigned_wake` uses that clock slot as last (not the
/// slightly early or late arrival) and the following slot as next — the same
/// rule as [`next_poll_at_for_timer`]. Button, cold, and unlinked contacts
/// paint `now` → the upcoming slot.
pub fn refresh_window(
    now: DateTime<Utc>,
    tz: Tz,
    interval_secs: u64,
    wake_ups: &WeeklyWakes,
    assigned_wake: Option<DateTime<Utc>>,
) -> (DateTime<Utc>, DateTime<Utc>) {
    if let Some(assigned) = assigned_wake {
        let following = next_poll_at(assigned, tz, interval_secs, wake_ups);
        if now < following {
            return (assigned, following);
        }
    }
    (now, next_poll_at(now, tz, interval_secs, wake_ups))
}

/// Seconds from `now` to `at`, at least 1.
pub fn secs_until(now: DateTime<Utc>, at: DateTime<Utc>) -> u64 {
    let secs = at.signed_duration_since(now).num_seconds();
    secs.max(1) as u64
}

/// Wall-clock instant `secs` after `now`.
pub fn instant_after(now: DateTime<Utc>, secs: u64) -> DateTime<Utc> {
    now + secs_as_duration(secs)
}

/// `X-Wake-At` / Pico `wake_at=` token: UTC RFC3339 at second precision.
pub fn format_wake_at_slot(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// Parse the Pico's echoed slot (or an `X-Wake-At` value).
pub fn parse_wake_at_slot(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw.trim())
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

fn secs_as_duration(secs: u64) -> Duration {
    Duration::seconds(i64::try_from(secs).unwrap_or(i64::MAX))
}

/// Slot the previous poll told the Pico to hit: stored `wake_at`, or reconstructed
/// from compensated `X-Sleep-Seconds` for older debug logs.
pub fn assigned_wake_from_poll(
    wake_at: Option<DateTime<Utc>>,
    prev_at: DateTime<Utc>,
    prev_sleep_s: u64,
    drift: f64,
) -> Option<DateTime<Utc>> {
    wake_at.or_else(|| intended_wake_at(prev_at, prev_sleep_s, drift))
}

/// Expected wall-clock wake from a previous `X-Sleep-Seconds` command.
pub fn intended_wake_at(
    prev_at: DateTime<Utc>,
    prev_sleep_s: u64,
    drift: f64,
) -> Option<DateTime<Utc>> {
    if prev_sleep_s == 0 {
        return None;
    }
    let wall = wall_secs_from_commanded(prev_sleep_s, drift);
    Some(instant_after(prev_at, wall))
}

fn next_wake_after(now: DateTime<Utc>, tz: Tz, wake_ups: &WeeklyWakes) -> Option<DateTime<Tz>> {
    if wake_ups.is_empty() {
        return None;
    }

    let now_local = now.with_timezone(&tz);
    let today = now_local.date_naive();
    // A full week plus one day covers “only Mondays” and a spring-forward skip.
    for day_offset in 0..8 {
        let date = today + Duration::days(day_offset);
        for &time in wake_ups.get(date.weekday()) {
            let Some(dt) = resolve_local(tz, date, time) else {
                continue;
            };
            if dt > now_local {
                return Some(dt);
            }
        }
    }
    None
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

    fn daily(times: &[&str]) -> WeeklyWakes {
        WeeklyWakes::every_day(times.iter().copied().map(t).collect())
    }

    fn weekly(pairs: &[(&str, &[&str])]) -> WeeklyWakes {
        let mut days: [Vec<NaiveTime>; 7] = Default::default();
        for (key, times) in pairs {
            let idx = WeeklyWakes::parse_day_key(key).unwrap();
            days[idx] = times.iter().copied().map(t).collect();
        }
        WeeklyWakes::from_days(days)
    }

    #[test]
    fn wakes_per_weekday_interval_and_sparse_weekend() {
        assert_eq!(WeeklyWakes::EMPTY.wakes_per_weekday(3600), [24.0; 7]);
        let week = weekly(&[
            ("mon", &["06:30", "07:45", "15:30"]),
            ("tue", &["06:30", "07:45", "15:30"]),
            ("wed", &["06:30", "07:45", "15:30"]),
            ("thu", &["06:30", "07:45", "15:30"]),
            ("fri", &["06:30", "07:45", "15:30"]),
            ("sat", &["08:00"]),
            ("sun", &["08:00"]),
        ]);
        assert_eq!(
            week.wakes_per_weekday(3600),
            [3.0, 3.0, 3.0, 3.0, 3.0, 1.0, 1.0]
        );
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
        assert_eq!(
            seconds_until_next_poll(now, london(), 3600, &WeeklyWakes::EMPTY),
            3600
        );
        assert_eq!(
            seconds_until_next_poll(now, london(), 0, &WeeklyWakes::EMPTY),
            1
        );
    }

    #[test]
    fn next_slot_later_today() {
        let now = at_london(2026, 9, 16, 12, 0, 0);
        let wakes = daily(&["06:00", "15:00", "23:15"]);
        assert_eq!(
            seconds_until_next_poll(now, london(), 3600, &wakes),
            3 * 3600
        );
    }

    #[test]
    fn unsorted_times_still_pick_next() {
        let now = at_london(2026, 9, 16, 12, 0, 0);
        let wakes = daily(&["23:15", "06:00", "15:00"]);
        assert_eq!(
            seconds_until_next_poll(now, london(), 3600, &wakes),
            3 * 3600
        );
    }

    #[test]
    fn wraps_past_last_slot_to_tomorrow() {
        let now = at_london(2026, 9, 16, 23, 30, 0);
        let wakes = daily(&["06:00", "23:15"]);
        assert_eq!(
            seconds_until_next_poll(now, london(), 3600, &wakes),
            6 * 3600 + 30 * 60
        );
    }

    #[test]
    fn skips_the_slot_already_in_progress() {
        let now = at_london(2026, 9, 16, 6, 0, 0);
        let wakes = daily(&["06:00", "07:00"]);
        assert_eq!(seconds_until_next_poll(now, london(), 3600, &wakes), 3600);
    }

    #[test]
    fn single_daily_time_sleeps_until_tomorrow() {
        let now = at_london(2026, 9, 16, 6, 0, 5);
        let wakes = daily(&["06:00"]);
        assert_eq!(
            seconds_until_next_poll(now, london(), 3600, &wakes),
            24 * 3600 - 5
        );
    }

    #[test]
    fn before_first_slot_same_day() {
        let now = at_london(2026, 9, 16, 5, 0, 0);
        let wakes = daily(&["06:00", "07:00"]);
        assert_eq!(seconds_until_next_poll(now, london(), 3600, &wakes), 3600);
    }

    #[test]
    fn spring_forward_skips_missing_local_time() {
        // 29 Mar 2026: 01:00 GMT → 02:00 BST, so 01:30 does not exist.
        let now = at_london(2026, 3, 29, 0, 30, 0);
        let wakes = daily(&["01:30", "03:00"]);
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
        let wakes = daily(&["01:30"]);
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

    #[test]
    fn timer_early_for_assigned_slot_sleeps_until_the_next() {
        let now = at_london(2026, 9, 16, 5, 55, 0);
        let assigned = at_london(2026, 9, 16, 6, 0, 0);
        let wakes = daily(&["06:00", "07:00"]);
        assert_eq!(
            seconds_until_next_poll_for_timer(now, london(), 3600, &wakes, Some(assigned)),
            65 * 60
        );
        // No stored slot: wait for 06:00 rather than guessing.
        assert_eq!(
            seconds_until_next_poll_for_timer(now, london(), 3600, &wakes, None),
            5 * 60
        );
    }

    #[test]
    fn timer_long_before_assigned_slot_still_counts() {
        // Overnight POWMAN can beat a 10-minute guess; the stored slot is the rule.
        let now = at_london(2026, 9, 16, 5, 20, 0);
        let assigned = at_london(2026, 9, 16, 6, 0, 0);
        let wakes = daily(&["06:00", "07:00"]);
        assert_eq!(
            seconds_until_next_poll_for_timer(now, london(), 3600, &wakes, Some(assigned)),
            100 * 60
        );
    }

    #[test]
    fn timer_late_for_assigned_slot_still_sleeps_until_the_next() {
        let now = at_london(2026, 9, 16, 6, 5, 0);
        let assigned = at_london(2026, 9, 16, 6, 0, 0);
        let wakes = daily(&["06:00", "07:00"]);
        assert_eq!(
            seconds_until_next_poll_for_timer(now, london(), 3600, &wakes, Some(assigned)),
            55 * 60
        );
    }

    #[test]
    fn timer_after_the_following_slot_schedules_from_now() {
        let now = at_london(2026, 9, 16, 7, 5, 0);
        let assigned = at_london(2026, 9, 16, 6, 0, 0);
        let wakes = daily(&["06:00", "07:00", "08:00"]);
        assert_eq!(
            seconds_until_next_poll_for_timer(now, london(), 3600, &wakes, Some(assigned)),
            55 * 60
        );
    }

    #[test]
    fn timer_on_time_does_not_skip_the_following_close_slot() {
        let now = at_london(2026, 9, 16, 8, 50, 0);
        let assigned = now;
        let wakes = daily(&["08:50", "08:51", "08:55"]);
        assert_eq!(
            seconds_until_next_poll_for_timer(now, london(), 3600, &wakes, Some(assigned)),
            60
        );
    }

    #[test]
    fn timer_slightly_late_does_not_skip_the_next_close_slot() {
        let now = at_london(2026, 9, 16, 8, 50, 30);
        let assigned = at_london(2026, 9, 16, 8, 50, 0);
        let wakes = daily(&["08:50", "08:51", "08:55"]);
        assert_eq!(
            seconds_until_next_poll_for_timer(now, london(), 3600, &wakes, Some(assigned)),
            30
        );
    }

    #[test]
    fn timer_early_for_a_close_slot_uses_the_commanded_wake() {
        let now = at_london(2026, 9, 16, 8, 50, 50);
        let assigned = at_london(2026, 9, 16, 8, 51, 0);
        let wakes = daily(&["08:50", "08:51", "08:55"]);
        assert_eq!(
            seconds_until_next_poll_for_timer(now, london(), 3600, &wakes, Some(assigned)),
            4 * 60 + 10
        );
    }

    #[test]
    fn timer_interval_keeps_cadence_when_early_or_late() {
        let assigned = at_london(2026, 9, 16, 13, 0, 0);
        assert_eq!(
            seconds_until_next_poll_for_timer(
                at_london(2026, 9, 16, 12, 58, 0),
                london(),
                3600,
                &WeeklyWakes::EMPTY,
                Some(assigned)
            ),
            3600 + 2 * 60
        );
        assert_eq!(
            seconds_until_next_poll_for_timer(
                at_london(2026, 9, 16, 13, 2, 0),
                london(),
                3600,
                &WeeklyWakes::EMPTY,
                Some(assigned)
            ),
            58 * 60
        );
    }

    #[test]
    fn intended_wake_undoes_compensated_sleep() {
        let prev = at_london(2026, 9, 16, 12, 0, 0);
        let commanded = compensate_sleep_secs(3600, 0.03);
        let intended = intended_wake_at(prev, commanded, 0.03).unwrap();
        assert_eq!(intended, at_london(2026, 9, 16, 13, 0, 0));
        assert_eq!(
            assigned_wake_from_poll(Some(intended), prev, commanded, 0.0),
            Some(intended)
        );
    }

    #[test]
    fn friday_evening_uses_saturday_times() {
        // 18 Sep 2026 is a Friday.
        let now = at_london(2026, 9, 18, 22, 0, 0);
        let wakes = weekly(&[
            ("mon", &["06:30"]),
            ("tue", &["06:30"]),
            ("wed", &["06:30"]),
            ("thu", &["06:30"]),
            ("fri", &["06:30", "15:30"]),
            ("sat", &["08:00"]),
            ("sun", &["08:30"]),
        ]);
        assert_eq!(
            seconds_until_next_poll(now, london(), 3600, &wakes),
            10 * 3600
        );
    }

    #[test]
    fn empty_saturday_wraps_to_sunday() {
        // 19 Sep 2026 is a Saturday.
        let now = at_london(2026, 9, 19, 10, 0, 0);
        let wakes = weekly(&[
            ("mon", &["06:30"]),
            ("fri", &["06:30"]),
            ("sun", &["09:00"]),
        ]);
        assert_eq!(
            seconds_until_next_poll(now, london(), 3600, &wakes),
            23 * 3600
        );
    }

    #[test]
    fn weekday_only_wraps_weekend_to_monday() {
        // Saturday morning, weekdays at 06:30, weekend empty.
        let now = at_london(2026, 9, 19, 8, 0, 0);
        let wakes = weekly(&[
            ("mon", &["06:30"]),
            ("tue", &["06:30"]),
            ("wed", &["06:30"]),
            ("thu", &["06:30"]),
            ("fri", &["06:30"]),
        ]);
        assert_eq!(
            seconds_until_next_poll(now, london(), 3600, &wakes),
            46 * 3600 + 30 * 60
        );
    }

    #[test]
    fn next_poll_at_is_the_clock_slot_not_now_plus_floored_secs() {
        let now = london()
            .with_ymd_and_hms(2026, 9, 16, 16, 0, 3)
            .unwrap()
            .with_timezone(&Utc)
            + Duration::milliseconds(475);
        let wakes = daily(&["18:00", "20:00"]);
        assert_eq!(
            next_poll_at(now, london(), 3600, &wakes),
            at_london(2026, 9, 16, 18, 0, 0)
        );
        // Truncating the duration would land on 17:59:59 and paint "17:59".
        assert_eq!(
            instant_after(now, seconds_until_next_poll(now, london(), 3600, &wakes)),
            at_london(2026, 9, 16, 17, 59, 59) + Duration::milliseconds(475)
        );
    }

    #[test]
    fn timer_early_with_echoed_slot_skips_that_hour() {
        // Pico woke at 17:59:40 and echoed X-Wake-At 18:00 exactly.
        let now = at_london(2026, 9, 16, 17, 59, 40);
        let assigned = at_london(2026, 9, 16, 18, 0, 0);
        let wakes = daily(&["18:00", "21:00"]);
        assert_eq!(
            seconds_until_next_poll_for_timer(now, london(), 3600, &wakes, Some(assigned)),
            3 * 3600 + 20
        );
        assert_eq!(
            next_poll_at_for_timer(now, london(), 3600, &wakes, Some(assigned)),
            at_london(2026, 9, 16, 21, 0, 0)
        );
        // No echo: this contact is not a served slot; wait for 18:00.
        assert_eq!(
            seconds_until_next_poll_for_timer(now, london(), 3600, &wakes, None),
            20
        );
    }

    #[test]
    fn refresh_window_early_timer_paints_the_assigned_slot() {
        // 06:57 serving 07:00 must read "07:00 → 10:00", not "06:57 → 07:00".
        let now = at_london(2026, 9, 20, 6, 57, 0);
        let assigned = at_london(2026, 9, 20, 7, 0, 0);
        let wakes = daily(&["07:00", "10:00"]);
        assert_eq!(
            refresh_window(now, london(), 3600, &wakes, Some(assigned)),
            (assigned, at_london(2026, 9, 20, 10, 0, 0))
        );
        assert_eq!(
            refresh_window(now, london(), 3600, &wakes, None),
            (now, assigned)
        );
    }

    #[test]
    fn refresh_window_late_timer_still_paints_the_assigned_slot() {
        let now = at_london(2026, 9, 20, 7, 5, 0);
        let assigned = at_london(2026, 9, 20, 7, 0, 0);
        let wakes = daily(&["07:00", "10:00"]);
        assert_eq!(
            refresh_window(now, london(), 3600, &wakes, Some(assigned)),
            (assigned, at_london(2026, 9, 20, 10, 0, 0))
        );
    }

    #[test]
    fn refresh_window_missed_cycle_paints_arrival_time() {
        let now = at_london(2026, 9, 20, 10, 5, 0);
        let assigned = at_london(2026, 9, 20, 7, 0, 0);
        let wakes = daily(&["07:00", "10:00", "13:00"]);
        assert_eq!(
            refresh_window(now, london(), 3600, &wakes, Some(assigned)),
            (now, at_london(2026, 9, 20, 13, 0, 0))
        );
    }

    #[test]
    fn wake_at_slot_roundtrips_rfc3339_z() {
        let at = at_london(2026, 9, 19, 18, 0, 0);
        let token = format_wake_at_slot(at);
        assert_eq!(token, "2026-09-19T17:00:00Z");
        assert_eq!(parse_wake_at_slot(&token), Some(at));
        assert_eq!(parse_wake_at_slot(" 2026-09-19T17:00:00+00:00 "), Some(at));
        assert!(parse_wake_at_slot("").is_none());
        assert!(parse_wake_at_slot("nope").is_none());
    }
}
