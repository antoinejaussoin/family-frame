//! LiPo rest-voltage curve, remaining mAh, and a running idle / per-wake fit.
//!
//! The Pico still POSTs a linear 3.3–4.2 V percent. Product percentage and
//! autonomy use millivolts through this module.

use chrono::Duration;
use serde::Serialize;

use crate::debug::Poll;

/// Nameplate used when `battery_mah` is omitted from config.
pub const DEFAULT_CAPACITY_MAH: u32 = 10_000;
/// VSYS treated as 0% usable (same cutoff as the Pico’s linear map).
pub const DEFAULT_EMPTY_MV: u32 = 3300;
/// Fully charged 1S LiPo.
pub const FULL_MV: u32 = 4200;

const PRIOR_IDLE_MA: f64 = 2.0;
const PRIOR_WAKE_MAH: f64 = 1.0;
const PRIOR_REFRESH_MAH: f64 = 3.0;
const PRIOR_CYCLE_MAH: f64 = PRIOR_WAKE_MAH + PRIOR_REFRESH_MAH;

const LAMBDA_IDLE: f64 = 0.15;
const LAMBDA_WAKE: f64 = 0.12;
const LAMBDA_REFRESH: f64 = 0.08;
const LAMBDA_CYCLE: f64 = 0.08;

const MIN_INTERVAL_SECS: i64 = 5 * 60;
const MAX_INTERVAL_SECS: i64 = 3 * 24 * 3600;
const ADC_STEP_MV: f64 = 2.4;
const MIN_SOAK_HOURS: f64 = 0.5;
const SPLIT_SOAKS: usize = 3;
const SPLIT_SOAK_HOURS: f64 = 6.0;

/// Chemical SoC (%) at rest, high voltage first. 4.00 V is ~90%, not the
/// linear map’s 78%.
const LUT: [(u32, f64); 11] = [
    (4200, 100.0),
    (4000, 90.0),
    (3900, 80.0),
    (3800, 70.0),
    (3700, 60.0),
    (3600, 50.0),
    (3500, 40.0),
    (3400, 30.0),
    (3300, 20.0),
    (3200, 10.0),
    (3000, 0.0),
];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cell {
    pub capacity_mah: u32,
    pub empty_mv: u32,
}

impl Cell {
    pub const DEFAULT: Self = Self {
        capacity_mah: DEFAULT_CAPACITY_MAH,
        empty_mv: DEFAULT_EMPTY_MV,
    };

    pub fn clamp(self) -> Self {
        Self {
            capacity_mah: self.capacity_mah.max(1),
            empty_mv: self.empty_mv.clamp(2500, 4000),
        }
    }
}

impl Default for Cell {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Numbers the debug page needs to simulate wake-ups without another fetch.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct BatteryReport {
    pub soc_pct: u16,
    pub linear_pct: u16,
    pub mv: u32,
    pub remaining_mah: f64,
    pub capacity_mah: u32,
    pub idle_ma: f64,
    pub wake_mah: f64,
    pub refresh_mah: f64,
    pub cycle_mah: f64,
    pub refresh_rate: f64,
    pub split_refresh: bool,
    pub confidence: String,
    pub model_note: String,
    pub interval_count: usize,
    pub wakes_by_day: [f64; 7],
    pub wakes_per_week: f64,
    pub wakes_per_day_avg: f64,
    pub idle_mah_per_day: f64,
    pub schedule_mah_per_day: f64,
    pub eta_seconds: i64,
    pub eta_text: String,
    pub eta_kind: String,
    pub on_usb: bool,
}

impl BatteryReport {
    pub fn empty(wakes_per_day: [f64; 7]) -> Self {
        let wakes_per_week: f64 = wakes_per_day.iter().sum();
        let wakes_per_day_avg = wakes_per_week / 7.0;
        let idle_mah_per_day = PRIOR_IDLE_MA * 24.0;
        let schedule_mah_per_day = idle_mah_per_day + wakes_per_day_avg * PRIOR_CYCLE_MAH;
        Self {
            soc_pct: 0,
            linear_pct: 0,
            mv: 0,
            remaining_mah: 0.0,
            capacity_mah: DEFAULT_CAPACITY_MAH,
            idle_ma: PRIOR_IDLE_MA,
            wake_mah: PRIOR_WAKE_MAH,
            refresh_mah: PRIOR_REFRESH_MAH,
            cycle_mah: PRIOR_CYCLE_MAH,
            refresh_rate: 1.0,
            split_refresh: false,
            confidence: "low".into(),
            model_note: "Starting from typical idle and per-wake priors.".into(),
            interval_count: 0,
            wakes_by_day: wakes_per_day,
            wakes_per_week,
            wakes_per_day_avg,
            idle_mah_per_day,
            schedule_mah_per_day,
            eta_seconds: 0,
            eta_text: "No Pico polls yet.".into(),
            eta_kind: "empty".into(),
            on_usb: false,
        }
    }
}

/// Pico firmware linear map (3.3–4.2 V). Integer divide, same as the device.
pub fn linear_pct(mv: u32) -> u16 {
    if mv >= FULL_MV {
        100
    } else if mv <= 3300 {
        0
    } else {
        ((mv - 3300) * 100 / (FULL_MV - 3300)) as u16
    }
}

/// Chemical rest SoC, 0–100. `mv ≥ 4200` is full.
pub fn chemical_soc(mv: u32) -> f64 {
    if mv >= LUT[0].0 {
        return LUT[0].1;
    }
    if mv <= LUT[LUT.len() - 1].0 {
        return LUT[LUT.len() - 1].1;
    }
    for w in LUT.windows(2) {
        let (v0, s0) = w[0];
        let (v1, s1) = w[1];
        if mv <= v0 && mv >= v1 {
            let t = (v0 - mv) as f64 / (v0 - v1) as f64;
            return s0 + t * (s1 - s0);
        }
    }
    0.0
}

/// Usable percent: `empty_mv` → 0, 4.20 V → 100.
pub fn usable_soc(mv: u32, empty_mv: u32) -> f64 {
    let chem = chemical_soc(mv);
    let empty = chemical_soc(empty_mv);
    let span = (100.0 - empty).max(1.0);
    ((chem - empty) / span * 100.0).clamp(0.0, 100.0)
}

pub fn soc_pct(mv: u32, empty_mv: u32) -> u16 {
    usable_soc(mv, empty_mv).round().clamp(0.0, 100.0) as u16
}

pub fn remaining_mah(mv: u32, cell: Cell) -> f64 {
    let cell = cell.clamp();
    let chem = chemical_soc(mv);
    let empty = chemical_soc(cell.empty_mv);
    (f64::from(cell.capacity_mah) * (chem - empty) / 100.0).max(0.0)
}

/// |d(chemical SoC fraction)/d(mV)| at `mv`, for ADC noise → mAh.
pub fn d_chem_frac_dmv(mv: u32) -> f64 {
    let mv = mv.clamp(LUT[LUT.len() - 1].0, LUT[0].0);
    for w in LUT.windows(2) {
        let (v0, s0) = w[0];
        let (v1, s1) = w[1];
        if mv <= v0 && mv >= v1 {
            return ((s0 - s1) / 100.0) / f64::from(v0 - v1);
        }
    }
    0.0004
}

pub fn format_duration(d: Duration) -> String {
    let secs = d.num_seconds().max(0);
    let days = secs / 86_400;
    let hours = (secs % 86_400) / 3_600;
    let mins = (secs % 3_600) / 60;
    if days > 0 {
        format!(
            "{days} day{} {hours} hour{}",
            if days == 1 { "" } else { "s" },
            if hours == 1 { "" } else { "s" }
        )
    } else if hours > 0 {
        format!(
            "{hours} hour{} {mins} min",
            if hours == 1 { "" } else { "s" }
        )
    } else {
        format!("{mins} min")
    }
}

pub fn daily_mah(idle_ma: f64, cycle_mah: f64, wakes_per_day: [f64; 7]) -> f64 {
    let wakes: f64 = wakes_per_day.iter().sum::<f64>() / 7.0;
    idle_ma * 24.0 + wakes * cycle_mah
}

#[derive(Debug, Clone)]
struct Interval {
    hours: f64,
    dm_ah: f64,
    refresh: bool,
    weight: f64,
}

#[derive(Debug, Clone)]
struct Fit {
    idle_ma: f64,
    wake_mah: f64,
    refresh_mah: f64,
    cycle_mah: f64,
    split_refresh: bool,
}

pub fn report(polls: &[Poll], cell: Cell, wakes_per_day: [f64; 7]) -> BatteryReport {
    let cell = cell.clamp();
    let wakes_per_week: f64 = wakes_per_day.iter().sum();
    let wakes_per_day_avg = wakes_per_week / 7.0;
    if polls.is_empty() {
        let mut empty = BatteryReport::empty(wakes_per_day);
        empty.capacity_mah = cell.capacity_mah;
        return empty;
    }

    let last = polls.last().unwrap();
    let remaining = remaining_mah(last.mv, cell);
    let intervals = collect_intervals(polls, cell);
    let refresh_rate = observed_refresh_rate(polls);
    let fit = fit_from_intervals(&intervals);
    let cycle_mah = if fit.split_refresh {
        fit.wake_mah + refresh_rate * fit.refresh_mah
    } else {
        fit.cycle_mah
    };
    let idle_mah_per_day = fit.idle_ma * 24.0;
    let schedule_mah_per_day = idle_mah_per_day + wakes_per_day_avg * cycle_mah.max(0.0);
    let (eta_kind, eta_text, eta_seconds) =
        eta_from_model(last.usb, remaining, schedule_mah_per_day);

    let span_h = interval_span_hours(&intervals);
    let mv_range = battery_mv_range(polls);
    let confidence = confidence_label(
        intervals.len(),
        span_h,
        mv_range,
        fit.split_refresh,
        last.usb,
    )
    .to_string();

    BatteryReport {
        soc_pct: soc_pct(last.mv, cell.empty_mv),
        linear_pct: linear_pct(last.mv),
        mv: last.mv,
        remaining_mah: remaining,
        capacity_mah: cell.capacity_mah,
        idle_ma: fit.idle_ma,
        wake_mah: fit.wake_mah,
        refresh_mah: fit.refresh_mah,
        cycle_mah,
        refresh_rate,
        split_refresh: fit.split_refresh,
        confidence,
        model_note: model_note(&intervals, &fit, fit.idle_ma < PRIOR_IDLE_MA * 0.92),
        interval_count: intervals.len(),
        wakes_by_day: wakes_per_day,
        wakes_per_week,
        wakes_per_day_avg,
        idle_mah_per_day,
        schedule_mah_per_day,
        eta_seconds,
        eta_text,
        eta_kind,
        on_usb: last.usb,
    }
}

fn eta_from_model(usb: bool, remaining: f64, daily: f64) -> (String, String, i64) {
    if usb {
        return (
            "usb".into(),
            "On USB — discharge estimate paused.".into(),
            0,
        );
    }
    if remaining <= 1.0 {
        return ("dead".into(), "Battery looks empty.".into(), 0);
    }
    if daily <= 0.05 {
        return (
            "stable".into(),
            "Battery is not draining in recent samples.".into(),
            0,
        );
    }
    let hours_left = remaining / (daily / 24.0);
    let remaining_secs = (hours_left * 3600.0) as i64;
    if remaining_secs <= 0 {
        return ("dead".into(), "Battery looks empty.".into(), 0);
    }
    (
        "ok".into(),
        format!(
            "About {} at the current schedule.",
            format_duration(Duration::seconds(remaining_secs))
        ),
        remaining_secs,
    )
}

fn observed_refresh_rate(polls: &[Poll]) -> f64 {
    let battery: Vec<&Poll> = polls.iter().filter(|p| !p.usb).collect();
    if battery.is_empty() {
        return 1.0;
    }
    let painted = battery.iter().filter(|p| p.status == 200).count().max(1);
    (painted as f64 / battery.len() as f64).clamp(0.05, 1.0)
}

fn collect_intervals(polls: &[Poll], cell: Cell) -> Vec<Interval> {
    let mut out = Vec::new();
    for w in polls.windows(2) {
        let a = &w[0];
        let b = &w[1];
        if a.usb || b.usb {
            continue;
        }
        let secs = b.t.signed_duration_since(a.t).num_seconds();
        if secs < MIN_INTERVAL_SECS || secs > MAX_INTERVAL_SECS {
            continue;
        }
        let dm = remaining_mah(a.mv, cell) - remaining_mah(b.mv, cell);
        if dm < 0.0 {
            continue;
        }
        let mid = a.mv.saturating_add(b.mv) / 2;
        let sigma_mv = ADC_STEP_MV * std::f64::consts::SQRT_2;
        let sigma_mah = (f64::from(cell.capacity_mah) * d_chem_frac_dmv(mid) * sigma_mv).max(4.0);
        let weight = 1.0 / (sigma_mah * sigma_mah);
        out.push(Interval {
            hours: secs as f64 / 3600.0,
            dm_ah: dm,
            refresh: a.status == 200,
            weight,
        });
    }
    out
}

fn interval_span_hours(intervals: &[Interval]) -> f64 {
    intervals.iter().map(|i| i.hours).sum()
}

fn battery_mv_range(polls: &[Poll]) -> u32 {
    let mut min_v = u32::MAX;
    let mut max_v = 0u32;
    for p in polls.iter().filter(|p| !p.usb) {
        min_v = min_v.min(p.mv);
        max_v = max_v.max(p.mv);
    }
    if min_v == u32::MAX {
        0
    } else {
        max_v.saturating_sub(min_v)
    }
}

fn can_split(intervals: &[Interval]) -> bool {
    let soaks: Vec<&Interval> = intervals
        .iter()
        .filter(|i| !i.refresh && i.hours >= MIN_SOAK_HOURS)
        .collect();
    let hours: f64 = soaks.iter().map(|i| i.hours).sum();
    soaks.len() >= SPLIT_SOAKS && hours >= SPLIT_SOAK_HOURS
}

fn fit_from_intervals(intervals: &[Interval]) -> Fit {
    if intervals.is_empty() {
        return Fit {
            idle_ma: PRIOR_IDLE_MA,
            wake_mah: PRIOR_WAKE_MAH,
            refresh_mah: PRIOR_REFRESH_MAH,
            cycle_mah: PRIOR_CYCLE_MAH,
            split_refresh: false,
        };
    }
    if can_split(intervals) {
        if let Some(beta) = fit_three(intervals) {
            let idle = beta[0].clamp(0.05, 20.0);
            let wake = beta[1].clamp(0.05, 30.0);
            let refresh = beta[2].clamp(0.0, 40.0);
            return Fit {
                idle_ma: idle,
                wake_mah: wake,
                refresh_mah: refresh,
                cycle_mah: wake + refresh,
                split_refresh: true,
            };
        }
    }
    let beta = fit_two(intervals).unwrap_or([PRIOR_IDLE_MA, PRIOR_CYCLE_MAH]);
    let idle = beta[0].clamp(0.05, 20.0);
    let cycle = beta[1].clamp(0.2, 40.0);
    Fit {
        idle_ma: idle,
        wake_mah: cycle,
        refresh_mah: 0.0,
        cycle_mah: cycle,
        split_refresh: false,
    }
}

fn fit_two(intervals: &[Interval]) -> Option<[f64; 2]> {
    let prior = [PRIOR_IDLE_MA, PRIOR_CYCLE_MAH];
    let lambda = [LAMBDA_IDLE, LAMBDA_CYCLE];
    let xs: Vec<[f64; 2]> = intervals.iter().map(|i| [i.hours, 1.0]).collect();
    let y: Vec<f64> = intervals.iter().map(|i| i.dm_ah).collect();
    let w: Vec<f64> = intervals.iter().map(|i| i.weight).collect();
    solve_ridge(&xs, &y, &w, &prior, &lambda)
}

fn fit_three(intervals: &[Interval]) -> Option<[f64; 3]> {
    let prior = [PRIOR_IDLE_MA, PRIOR_WAKE_MAH, PRIOR_REFRESH_MAH];
    let lambda = [LAMBDA_IDLE, LAMBDA_WAKE, LAMBDA_REFRESH];
    let xs: Vec<[f64; 3]> = intervals
        .iter()
        .map(|i| [i.hours, 1.0, if i.refresh { 1.0 } else { 0.0 }])
        .collect();
    let y: Vec<f64> = intervals.iter().map(|i| i.dm_ah).collect();
    let w: Vec<f64> = intervals.iter().map(|i| i.weight).collect();
    solve_ridge(&xs, &y, &w, &prior, &lambda)
}

fn solve_ridge<const N: usize>(
    xs: &[[f64; N]],
    y: &[f64],
    w: &[f64],
    prior: &[f64; N],
    lambda: &[f64; N],
) -> Option<[f64; N]> {
    let mut a = [[0.0; N]; N];
    let mut b = [0.0; N];
    for ((row, yi), wi) in xs.iter().zip(y.iter()).zip(w.iter()) {
        for i in 0..N {
            b[i] += *wi * row[i] * *yi;
            for j in 0..N {
                a[i][j] += *wi * row[i] * row[j];
            }
        }
    }
    for i in 0..N {
        a[i][i] += lambda[i];
        b[i] += lambda[i] * prior[i];
    }
    gauss(a, b)
}

fn gauss<const N: usize>(mut a: [[f64; N]; N], mut b: [f64; N]) -> Option<[f64; N]> {
    for k in 0..N {
        let mut piv = k;
        let mut best = a[k][k].abs();
        for i in (k + 1)..N {
            let v = a[i][k].abs();
            if v > best {
                best = v;
                piv = i;
            }
        }
        if best < 1e-12 {
            return None;
        }
        if piv != k {
            a.swap(k, piv);
            b.swap(k, piv);
        }
        let diag = a[k][k];
        for j in k..N {
            a[k][j] /= diag;
        }
        b[k] /= diag;
        for i in 0..N {
            if i == k {
                continue;
            }
            let f = a[i][k];
            for j in k..N {
                a[i][j] -= f * a[k][j];
            }
            b[i] -= f * b[k];
        }
    }
    Some(b)
}

fn confidence_label(n: usize, span_h: f64, mv_range: u32, split: bool, usb: bool) -> &'static str {
    if usb {
        return "low";
    }
    if n < 4 || span_h < 8.0 {
        return "low";
    }
    if mv_range < 60 && span_h < 80.0 {
        return "low";
    }
    if (n >= 24 && span_h >= 72.0 && mv_range >= 100) || (split && span_h >= 48.0) {
        return "high";
    }
    "ok"
}

fn model_note(intervals: &[Interval], fit: &Fit, idle_pulled_down: bool) -> String {
    if intervals.is_empty() {
        return "Starting from typical idle and per-wake priors.".into();
    }
    let n = intervals.len();
    let soaks = intervals.iter().filter(|i| !i.refresh).count();
    let mut note = if fit.split_refresh {
        format!("Calibrated from {n} intervals ({soaks} unchanged soaks).")
    } else {
        format!(
            "Calibrated from {n} intervals. Panel refresh not split yet (almost every wake paints)."
        )
    };
    if idle_pulled_down {
        note.push_str(" Idle pulled down by long quiet soaks.");
    }
    note
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn poll(mins: i64, mv: u32, usb: bool, status: u16) -> Poll {
        Poll {
            t: Utc.with_ymd_and_hms(2026, 9, 17, 7, 0, 0).unwrap() + Duration::minutes(mins),
            status,
            offered: String::new(),
            checksum: "aa".into(),
            mv,
            pct: linear_pct(mv),
            usb,
            wake: "timer".into(),
            sleep_s: 3600,
            wake_at: None,
        }
    }

    fn mv_from_chemical(soc: f64) -> u32 {
        let soc = soc.clamp(0.0, 100.0);
        if soc >= 100.0 {
            return FULL_MV;
        }
        if soc <= 0.0 {
            return 3000;
        }
        for w in LUT.windows(2) {
            let (v0, s0) = w[0];
            let (v1, s1) = w[1];
            if soc <= s0 && soc >= s1 {
                let t = (s0 - soc) / (s0 - s1);
                return (f64::from(v0) + t * (f64::from(v1) - f64::from(v0))).round() as u32;
            }
        }
        3000
    }

    #[test]
    fn lut_anchors() {
        assert_eq!(soc_pct(4222, 3300), 100);
        assert_eq!(soc_pct(4200, 3300), 100);
        assert_eq!(linear_pct(4178), 97);
        assert!(soc_pct(4178, 3300) >= 99, "got {}", soc_pct(4178, 3300));
        let at_4v = soc_pct(4000, 3300);
        assert!(at_4v > 78, "curve {at_4v} should beat linear 78");
        assert!(at_4v >= 85 && at_4v <= 92, "got {at_4v}");
        assert_eq!(soc_pct(3300, 3300), 0);
        assert_eq!(linear_pct(4000), 77); // 700*100/900
    }

    #[test]
    fn lut_is_monotonic() {
        let mut prev = 0u16;
        for mv in (3300..=4220).step_by(10) {
            let s = soc_pct(mv, 3300);
            assert!(s >= prev, "{mv} mV → {s} after {prev}");
            prev = s;
        }
    }

    #[test]
    fn remaining_full_is_usable_window() {
        let mah = remaining_mah(4200, Cell::DEFAULT);
        assert!((mah - 8000.0).abs() < 1.0, "got {mah}");
        assert!(remaining_mah(3300, Cell::DEFAULT) < 1.0);
    }

    #[test]
    fn empty_report_uses_priors() {
        let r = report(&[], Cell::DEFAULT, [12.0; 7]);
        assert_eq!(r.eta_kind, "empty");
        assert!((r.idle_ma - PRIOR_IDLE_MA).abs() < 1e-9);
        assert!((r.cycle_mah - PRIOR_CYCLE_MAH).abs() < 1e-9);
        assert_eq!(r.wakes_per_week, 84.0);
    }

    #[test]
    fn fit_204_only_recovers_idle() {
        let mut intervals = Vec::new();
        for _ in 0..12 {
            intervals.push(Interval {
                hours: 1.0,
                dm_ah: 2.5,
                refresh: false,
                weight: 1.0,
            });
            intervals.push(Interval {
                hours: 3.0,
                dm_ah: 7.5,
                refresh: false,
                weight: 1.0,
            });
        }
        let fit = fit_from_intervals(&intervals);
        assert!((fit.idle_ma - 2.5).abs() < 0.4, "idle {}", fit.idle_ma);
        assert!(fit.wake_mah < 0.8, "wake {}", fit.wake_mah);
        assert!(fit.split_refresh);
    }

    #[test]
    fn fit_mix_recovers_refresh_extra() {
        let mut intervals = Vec::new();
        for _ in 0..12 {
            intervals.push(Interval {
                hours: 1.0,
                dm_ah: 2.0,
                refresh: false,
                weight: 1.0,
            });
            intervals.push(Interval {
                hours: 1.0,
                dm_ah: 7.0,
                refresh: true,
                weight: 1.0,
            });
        }
        let fit = fit_from_intervals(&intervals);
        assert!((fit.idle_ma - 2.0).abs() < 0.6, "idle {}", fit.idle_ma);
        assert!(
            (fit.refresh_mah - 5.0).abs() < 1.2,
            "refresh {}",
            fit.refresh_mah
        );
        assert!(fit.split_refresh);
    }

    #[test]
    fn weekend_sparse_uses_less_per_day() {
        let weekday = [7.0, 7.0, 7.0, 7.0, 7.0, 1.0, 1.0];
        let every = [7.0; 7];
        let sparse = daily_mah(2.0, 4.0, weekday);
        let full = daily_mah(2.0, 4.0, every);
        assert!(sparse < full, "{sparse} vs {full}");
    }

    #[test]
    fn one_poll_already_has_an_eta() {
        let polls = [poll(0, 4178, false, 200)];
        let r = report(&polls, Cell::DEFAULT, [12.0; 7]);
        assert_eq!(r.soc_pct, soc_pct(4178, 3300));
        assert_eq!(r.linear_pct, 97);
        assert!(r.remaining_mah > 7000.0);
        assert_eq!(r.eta_kind, "ok");
        assert!(r.eta_seconds > 3600);
        assert_eq!(r.interval_count, 0);
        assert_eq!(r.confidence, "low");
    }

    #[test]
    fn usb_pauses_eta() {
        let polls = [poll(0, 4000, true, 204)];
        let r = report(&polls, Cell::DEFAULT, [8.0; 7]);
        assert_eq!(r.eta_kind, "usb");
        assert!(r.on_usb);
    }

    #[test]
    fn production_log_stays_high_and_calibrates() {
        let raw = include_str!("testdata/pico_polls_production.jsonl");
        let polls: Vec<Poll> = raw
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l).expect(l))
            .collect();
        assert_eq!(polls.len(), 34);
        let last = polls.last().unwrap();
        assert_eq!(last.mv, 4178);
        assert!(soc_pct(last.mv, 3300) >= 99);
        assert_eq!(linear_pct(last.mv), 97);

        let r = report(&polls, Cell::DEFAULT, [12.0; 7]);
        assert_eq!(r.soc_pct, soc_pct(4178, 3300));
        assert_eq!(r.linear_pct, 97);
        assert!(r.interval_count > 10, "intervals {}", r.interval_count);
        assert!(
            r.idle_ma < PRIOR_IDLE_MA,
            "overnight should pull idle below prior, got {}",
            r.idle_ma
        );
        assert!(r.cycle_mah > 0.2);
        assert_eq!(r.eta_kind, "ok");
        assert!(r.eta_seconds > 86_400);
        assert_eq!(r.confidence, "low");
        assert!(!r.split_refresh);
    }

    #[test]
    fn voltage_roundtrip_helper() {
        let mv = mv_from_chemical(90.0);
        assert!(
            (chemical_soc(mv) - 90.0).abs() < 0.6,
            "{mv} → {}",
            chemical_soc(mv)
        );
    }
}
