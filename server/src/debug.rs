//! Pico poll history for the family UI debug page (`GET /api/debug`).
//!
//! POST `/api/frame.bin` appends one JSONL row and, on 200, a dithered PNG keyed
//! by checksum. GET `/api/frame.bin` from a browser is not recorded.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::warn;

pub const MAX_POLLS: usize = 500;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Poll {
    pub t: DateTime<Utc>,
    pub status: u16,
    pub offered: String,
    pub checksum: String,
    pub mv: u32,
    pub pct: u16,
    pub usb: bool,
    pub wake: String,
    /// Seconds the Pico was told to sleep after this poll.
    #[serde(default)]
    pub sleep_s: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatteryEta {
    NoData,
    OnUsb,
    NeedMore,
    Stable,
    Remaining(Duration),
}

#[derive(Debug, Clone, Serialize)]
pub struct DebugPage {
    pub has_polls: bool,
    pub last_pct: u16,
    pub last_mv: u32,
    pub last_usb: bool,
    pub last_wake: String,
    pub last_sleep_s: u64,
    pub has_next_refresh: bool,
    pub next_refresh: String,
    pub next_refresh_rel: String,
    pub last_status: u16,
    pub last_status_label: String,
    pub last_seen: String,
    pub last_seen_rel: String,
    pub power_label: String,
    pub eta_text: String,
    pub eta_kind: String,
    pub pico_drift_label: String,
    pub graph_svg: String,
    pub polls: Vec<DebugPollView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DebugPollView {
    pub when: String,
    pub status: u16,
    pub status_label: String,
    pub pct: u16,
    pub mv: u32,
    pub usb: bool,
    pub wake: String,
    pub sleep_s: u64,
    pub has_image: bool,
    pub image_url: String,
    pub checksum_short: String,
}

pub struct DebugLog {
    dir: PathBuf,
    inner: Mutex<Vec<Poll>>,
}

impl DebugLog {
    pub fn open(config_dir: &Path) -> Result<Arc<Self>> {
        let dir = config_dir.join("debug");
        fs::create_dir_all(dir.join("frames"))
            .with_context(|| format!("creating {}", dir.display()))?;
        let polls = load_jsonl(&dir.join("polls.jsonl"));
        Ok(Arc::new(Self {
            dir,
            inner: Mutex::new(polls),
        }))
    }

    pub async fn record(&self, mut poll: Poll, png: Option<&[u8]>) -> Result<()> {
        poll.pct = poll.pct.min(100);
        if let Some(png) = png {
            if is_hex_checksum(&poll.checksum) {
                let path = self.frame_path(&poll.checksum);
                if !path.exists() {
                    fs::write(&path, png).with_context(|| format!("writing {}", path.display()))?;
                }
            }
        }
        let mut guard = self.inner.lock().await;
        guard.push(poll);
        if guard.len() > MAX_POLLS {
            let drop_n = guard.len() - MAX_POLLS;
            guard.drain(0..drop_n);
            rewrite_jsonl(&self.dir.join("polls.jsonl"), &guard)?;
            prune_frames(&self.dir.join("frames"), &guard);
        } else if let Some(last) = guard.last() {
            append_jsonl(&self.dir.join("polls.jsonl"), last)?;
        }
        Ok(())
    }

    pub async fn snapshot(&self) -> Vec<Poll> {
        self.inner.lock().await.clone()
    }

    pub fn has_frame(&self, checksum: &str) -> bool {
        is_hex_checksum(checksum) && self.frame_path(checksum).is_file()
    }

    pub fn frame_png(&self, checksum: &str) -> Option<Vec<u8>> {
        if !is_hex_checksum(checksum) {
            return None;
        }
        fs::read(self.frame_path(checksum)).ok()
    }

    fn frame_path(&self, checksum: &str) -> PathBuf {
        self.dir.join("frames").join(format!("{checksum}.png"))
    }
}

pub fn is_hex_checksum(s: &str) -> bool {
    let n = s.len();
    (1..=64).contains(&n) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

pub fn page_from_polls(polls: &[Poll], tz: Tz, has_frame: impl Fn(&str) -> bool) -> DebugPage {
    page_from_polls_with_drift(polls, tz, 0.0, has_frame)
}

pub fn page_from_polls_with_drift(
    polls: &[Poll],
    tz: Tz,
    pico_drift: f64,
    has_frame: impl Fn(&str) -> bool,
) -> DebugPage {
    let now = Utc::now();
    if polls.is_empty() {
        return DebugPage {
            has_polls: false,
            last_pct: 0,
            last_mv: 0,
            last_usb: false,
            last_wake: String::new(),
            last_sleep_s: 0,
            has_next_refresh: false,
            next_refresh: String::new(),
            next_refresh_rel: String::new(),
            last_status: 0,
            last_status_label: String::new(),
            last_seen: String::new(),
            last_seen_rel: String::new(),
            power_label: String::new(),
            eta_text: "No Pico polls yet.".into(),
            eta_kind: "empty".into(),
            pico_drift_label: String::new(),
            graph_svg: String::new(),
            polls: Vec::new(),
        };
    }

    let last = polls.last().unwrap();
    let eta = discharge_eta(polls, now);
    let (eta_kind, eta_text) = eta_copy(eta, last.usb);
    let (next_refresh, next_refresh_rel) = next_refresh_copy(last, now, tz, pico_drift);

    DebugPage {
        has_polls: true,
        last_pct: last.pct,
        last_mv: last.mv,
        last_usb: last.usb,
        last_wake: last.wake.clone(),
        last_sleep_s: last.sleep_s,
        has_next_refresh: !next_refresh.is_empty(),
        next_refresh,
        next_refresh_rel,
        last_status: last.status,
        last_status_label: status_label(last.status).into(),
        last_seen: format_when(last.t, tz),
        last_seen_rel: format_rel(last.t, now),
        power_label: if last.usb {
            "USB".into()
        } else {
            "Battery".into()
        },
        eta_text,
        eta_kind,
        pico_drift_label: pico_drift_label(pico_drift),
        graph_svg: graph_svg(polls, eta),
        polls: polls
            .iter()
            .rev()
            .map(|p| DebugPollView {
                when: format_when(p.t, tz),
                status: p.status,
                status_label: status_label(p.status).into(),
                pct: p.pct,
                mv: p.mv,
                usb: p.usb,
                wake: p.wake.clone(),
                sleep_s: p.sleep_s,
                has_image: has_frame(&p.checksum),
                image_url: format!("/api/debug/frames/{}.png", p.checksum),
                checksum_short: checksum_short(&p.checksum),
            })
            .collect(),
    }
}

pub fn discharge_eta(polls: &[Poll], now: DateTime<Utc>) -> BatteryEta {
    if polls.is_empty() {
        return BatteryEta::NoData;
    }
    if polls.last().unwrap().usb {
        return BatteryEta::OnUsb;
    }

    let pts: Vec<(f64, f64)> = polls
        .iter()
        .filter(|p| !p.usb)
        .map(|p| (p.t.timestamp() as f64, f64::from(p.pct)))
        .collect();
    if pts.len() < 3 {
        return BatteryEta::NeedMore;
    }

    let first_pct = pts.first().unwrap().1;
    let last_pct = pts.last().unwrap().1;
    let span = pts.last().unwrap().0 - pts.first().unwrap().0;
    let drop = first_pct - last_pct;
    if span < 30.0 * 60.0 && drop < 5.0 {
        return BatteryEta::NeedMore;
    }

    let n = pts.len() as f64;
    let mut sum_x = 0.0;
    let mut sum_y = 0.0;
    let mut sum_xx = 0.0;
    let mut sum_xy = 0.0;
    for (x, y) in &pts {
        sum_x += x;
        sum_y += y;
        sum_xx += x * x;
        sum_xy += x * y;
    }
    let denom = n * sum_xx - sum_x * sum_x;
    if denom.abs() < 1e-9 {
        return BatteryEta::NeedMore;
    }
    let slope = (n * sum_xy - sum_x * sum_y) / denom;
    if slope >= -1e-12 {
        return BatteryEta::Stable;
    }
    let intercept = (sum_y - slope * sum_x) / n;
    let t_zero = -intercept / slope;
    let remaining = t_zero - now.timestamp() as f64;
    if remaining <= 0.0 {
        return BatteryEta::Remaining(Duration::zero());
    }
    BatteryEta::Remaining(Duration::seconds(remaining as i64))
}

fn eta_copy(eta: BatteryEta, last_usb: bool) -> (String, String) {
    if last_usb {
        return ("usb".into(), "On USB — discharge estimate paused.".into());
    }
    match eta {
        BatteryEta::NoData => ("empty".into(), "No Pico polls yet.".into()),
        BatteryEta::OnUsb => ("usb".into(), "On USB — discharge estimate paused.".into()),
        BatteryEta::NeedMore => (
            "wait".into(),
            "Not enough discharge samples for an estimate.".into(),
        ),
        BatteryEta::Stable => (
            "stable".into(),
            "Battery is not draining in recent samples.".into(),
        ),
        BatteryEta::Remaining(d) if d.num_seconds() <= 0 => {
            ("dead".into(), "Battery looks empty.".into())
        }
        BatteryEta::Remaining(d) => (
            "ok".into(),
            format!("About {} at current drain.", fmt_dur(d)),
        ),
    }
}

fn fmt_dur(d: Duration) -> String {
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

fn status_label(status: u16) -> &'static str {
    match status {
        200 => "200 new frame",
        204 => "204 unchanged",
        304 => "304 unchanged",
        _ => "other",
    }
}

fn checksum_short(checksum: &str) -> String {
    checksum.chars().take(8).collect()
}

fn format_when(t: DateTime<Utc>, tz: Tz) -> String {
    t.with_timezone(&tz).format("%a %-d %b, %H:%M").to_string()
}

fn next_refresh_copy(last: &Poll, now: DateTime<Utc>, tz: Tz, pico_drift: f64) -> (String, String) {
    if last.sleep_s == 0 {
        return (String::new(), String::new());
    }
    let wall = crate::schedule::wall_secs_from_commanded(last.sleep_s, pico_drift);
    let secs = i64::try_from(wall).unwrap_or(i64::MAX);
    let at = last.t + Duration::seconds(secs);
    (format_when(at, tz), format_until(at, now))
}

fn pico_drift_label(drift: f64) -> String {
    if drift.abs() < 0.0005 {
        return String::new();
    }
    let pct = drift * 100.0;
    if pct > 0.0 {
        format!("{pct:.1}% slow")
    } else {
        format!("{:.1}% fast", -pct)
    }
}

fn format_until(then: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let secs = then.signed_duration_since(now).num_seconds();
    if secs >= 0 {
        if secs < 60 {
            "any moment".into()
        } else if secs < 3_600 {
            format!("in {} min", secs / 60)
        } else if secs < 86_400 {
            let h = secs / 3_600;
            format!("in {h} hour{}", if h == 1 { "" } else { "s" })
        } else {
            let d = secs / 86_400;
            format!("in {d} day{}", if d == 1 { "" } else { "s" })
        }
    } else {
        let ago = -secs;
        if ago < 60 {
            "overdue".into()
        } else if ago < 3_600 {
            format!("{} min overdue", ago / 60)
        } else if ago < 86_400 {
            let h = ago / 3_600;
            format!("{h} hour{} overdue", if h == 1 { "" } else { "s" })
        } else {
            let d = ago / 86_400;
            format!("{d} day{} overdue", if d == 1 { "" } else { "s" })
        }
    }
}

fn format_rel(then: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let secs = now.signed_duration_since(then).num_seconds().max(0);
    if secs < 60 {
        "just now".into()
    } else if secs < 3_600 {
        let m = secs / 60;
        format!("{m} min ago")
    } else if secs < 86_400 {
        let h = secs / 3_600;
        format!("{h} hour{} ago", if h == 1 { "" } else { "s" })
    } else {
        let d = secs / 86_400;
        format!("{d} day{} ago", if d == 1 { "" } else { "s" })
    }
}

fn graph_svg(polls: &[Poll], eta: BatteryEta) -> String {
    if polls.is_empty() {
        return String::new();
    }

    const W: f64 = 320.0;
    const H: f64 = 140.0;
    const PAD_L: f64 = 28.0;
    const PAD_R: f64 = 10.0;
    const PAD_T: f64 = 10.0;
    const PAD_B: f64 = 22.0;

    let t0 = polls.first().unwrap().t.timestamp() as f64;
    let t1 = polls.last().unwrap().t.timestamp() as f64;
    let dt = (t1 - t0).max(1.0);
    let inner_w = W - PAD_L - PAD_R;
    let inner_h = H - PAD_T - PAD_B;

    let x_of = |t: f64| PAD_L + (t - t0) / dt * inner_w;
    let y_of = |pct: f64| PAD_T + (1.0 - (pct.clamp(0.0, 100.0) / 100.0)) * inner_h;

    let mut points = String::new();
    for p in polls {
        let _ = write!(
            points,
            "{:.1},{:.1} ",
            x_of(p.t.timestamp() as f64),
            y_of(f64::from(p.pct))
        );
    }

    let mut dots = String::new();
    let paper = "#f3efe6";
    let usb_stroke = "#1d4ed8";
    let ink = "#111827";
    let grid = "#d6d3c9";
    let dash = "#b45309";
    for p in polls {
        let cx = x_of(p.t.timestamp() as f64);
        let cy = y_of(f64::from(p.pct));
        if p.usb {
            let _ = write!(
                dots,
                r##"<circle cx="{cx:.1}" cy="{cy:.1}" r="3.5" fill="{paper}" stroke="{usb_stroke}" stroke-width="1.5"/>"##
            );
        } else {
            let _ = write!(
                dots,
                r##"<circle cx="{cx:.1}" cy="{cy:.1}" r="3" fill="{ink}"/>"##
            );
        }
    }

    let mut proj = String::new();
    if let BatteryEta::Remaining(d) = eta {
        if d.num_seconds() > 0 {
            let last = polls.last().unwrap();
            let x1 = x_of(last.t.timestamp() as f64);
            let y1 = y_of(f64::from(last.pct));
            let x2 = W - PAD_R;
            let y2 = y_of(0.0);
            if x2 > x1 + 4.0 {
                let _ = write!(
                    proj,
                    r##"<line x1="{x1:.1}" y1="{y1:.1}" x2="{x2:.1}" y2="{y2:.1}" stroke="{dash}" stroke-dasharray="4 3" stroke-width="1.5"/>"##
                );
            }
        }
    }

    let y100 = y_of(100.0);
    let y50 = y_of(50.0);
    let y0 = y_of(0.0);
    let y100t = y100 + 4.0;
    let y50t = y50 + 4.0;
    let y0t = y0 + 4.0;
    let right = W - PAD_R;
    format!(
        r##"<svg viewBox="0 0 {W} {H}" role="img" aria-label="Battery percent over time">
  <line x1="{PAD_L}" y1="{y100:.1}" x2="{right:.1}" y2="{y100:.1}" stroke="{grid}" />
  <line x1="{PAD_L}" y1="{y50:.1}" x2="{right:.1}" y2="{y50:.1}" stroke="{grid}" />
  <line x1="{PAD_L}" y1="{y0:.1}" x2="{right:.1}" y2="{y0:.1}" stroke="{grid}" />
  <text x="2" y="{y100t:.1}" class="tick">100</text>
  <text x="2" y="{y50t:.1}" class="tick">50</text>
  <text x="2" y="{y0t:.1}" class="tick">0</text>
  {proj}
  <polyline fill="none" stroke="{ink}" stroke-width="1.75" points="{points}"/>
  {dots}
</svg>"##
    )
}

fn load_jsonl(path: &Path) -> Vec<Poll> {
    let Ok(file) = File::open(path) else {
        return Vec::new();
    };
    let mut polls = Vec::new();
    for line in BufReader::new(file).lines() {
        let Ok(line) = line else {
            continue;
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<Poll>(line) {
            Ok(p) => polls.push(p),
            Err(err) => warn!(%err, "skipping bad debug poll line"),
        }
    }
    if polls.len() > MAX_POLLS {
        let drop_n = polls.len() - MAX_POLLS;
        polls.drain(0..drop_n);
    }
    polls
}

fn append_jsonl(path: &Path, poll: &Poll) -> Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("opening {}", path.display()))?;
    serde_json::to_writer(&mut file, poll)?;
    file.write_all(b"\n")?;
    Ok(())
}

fn rewrite_jsonl(path: &Path, polls: &[Poll]) -> Result<()> {
    let tmp = path.with_extension("jsonl.tmp");
    {
        let mut file = File::create(&tmp).with_context(|| format!("creating {}", tmp.display()))?;
        for p in polls {
            serde_json::to_writer(&mut file, p)?;
            file.write_all(b"\n")?;
        }
        file.sync_all()?;
    }
    fs::rename(&tmp, path).with_context(|| format!("renaming {}", tmp.display()))?;
    Ok(())
}

fn prune_frames(dir: &Path, polls: &[Poll]) {
    let keep: HashSet<&str> = polls.iter().map(|p| p.checksum.as_str()).collect();
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if !keep.contains(stem) {
            let _ = fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn poll_at(mins: i64, pct: u16, usb: bool, status: u16, checksum: &str) -> Poll {
        Poll {
            t: Utc.with_ymd_and_hms(2026, 9, 15, 12, 0, 0).unwrap() + Duration::minutes(mins),
            status,
            offered: checksum.into(),
            checksum: checksum.into(),
            mv: 3300 + u32::from(pct) * 9,
            pct,
            usb,
            wake: "timer".into(),
            sleep_s: 3600,
        }
    }

    #[test]
    fn hex_checksum_rejects_paths() {
        assert!(is_hex_checksum("deadbeef"));
        assert!(is_hex_checksum("abc"));
        assert!(!is_hex_checksum(""));
        assert!(!is_hex_checksum("../x"));
        assert!(!is_hex_checksum("deadbeef.png"));
    }

    #[tokio::test]
    async fn records_png_on_200_not_on_204() {
        let dir = tempfile::tempdir().unwrap();
        let log = DebugLog::open(dir.path()).unwrap();
        let png = b"\x89PNG\r\n\x1a\nfake";
        log.record(poll_at(0, 80, false, 200, "aa11bb22"), Some(png))
            .await
            .unwrap();
        log.record(poll_at(60, 78, false, 204, "aa11bb22"), None)
            .await
            .unwrap();
        assert!(log.has_frame("aa11bb22"));
        assert_eq!(log.frame_png("aa11bb22").unwrap(), png);
        let snap = log.snapshot().await;
        assert_eq!(snap.len(), 2);
        assert_eq!(snap[0].status, 200);
        assert_eq!(snap[1].status, 204);
        let frames: Vec<_> = fs::read_dir(dir.path().join("debug/frames"))
            .unwrap()
            .flatten()
            .collect();
        assert_eq!(frames.len(), 1);
    }

    #[tokio::test]
    async fn compacts_and_prunes_old_frames() {
        let dir = tempfile::tempdir().unwrap();
        let log = DebugLog::open(dir.path()).unwrap();
        for i in 0..(MAX_POLLS + 3) {
            let checksum = format!("aa{i:02x}");
            let mut p = poll_at(i as i64, 50, false, 200, &checksum);
            // unique 2-char hex may collide; use padded hex of i
            p.checksum = format!("{i:064x}");
            log.record(p, Some(b"png")).await.unwrap();
        }
        let snap = log.snapshot().await;
        assert_eq!(snap.len(), MAX_POLLS);
        let n_files = fs::read_dir(dir.path().join("debug/frames"))
            .unwrap()
            .flatten()
            .count();
        assert_eq!(n_files, MAX_POLLS);
    }

    #[test]
    fn eta_from_falling_series() {
        let now = Utc.with_ymd_and_hms(2026, 9, 15, 18, 0, 0).unwrap();
        let polls = vec![
            poll_at(0, 90, false, 200, "aa"),
            poll_at(60, 80, false, 204, "aa"),
            poll_at(120, 70, false, 204, "aa"),
            poll_at(180, 60, false, 204, "aa"),
        ];
        match discharge_eta(&polls, now) {
            BatteryEta::Remaining(d) => {
                assert!(d.num_hours() >= 2, "got {} hours", d.num_hours());
                assert!(d.num_hours() <= 8, "got {} hours", d.num_hours());
            }
            other => panic!("expected remaining, got {other:?}"),
        }
    }

    #[test]
    fn eta_ignores_usb_samples_and_pauses_when_plugged() {
        let now = Utc.with_ymd_and_hms(2026, 9, 15, 18, 0, 0).unwrap();
        let mut polls = vec![
            poll_at(0, 90, false, 200, "aa"),
            poll_at(60, 80, false, 204, "aa"),
            poll_at(120, 70, false, 204, "aa"),
        ];
        polls.push(poll_at(180, 100, true, 204, "aa"));
        assert_eq!(discharge_eta(&polls, now), BatteryEta::OnUsb);
    }

    #[test]
    fn eta_need_more_when_flat_and_short() {
        let now = Utc.with_ymd_and_hms(2026, 9, 15, 12, 10, 0).unwrap();
        let polls = vec![
            poll_at(0, 80, false, 200, "aa"),
            poll_at(5, 80, false, 204, "aa"),
            poll_at(10, 80, false, 204, "aa"),
        ];
        assert_eq!(discharge_eta(&polls, now), BatteryEta::NeedMore);
    }

    #[test]
    fn page_lists_newest_first_with_image() {
        let polls = vec![
            poll_at(0, 80, false, 200, "deadbeef"),
            poll_at(60, 78, false, 204, "deadbeef"),
        ];
        let page = page_from_polls(&polls, chrono_tz::Europe::London, |c| c == "deadbeef");
        assert!(page.has_polls);
        assert_eq!(page.last_pct, 78);
        assert_eq!(page.polls[0].status, 204);
        assert_eq!(page.polls[1].status, 200);
        assert!(page.polls[0].has_image);
        assert_eq!(
            page.polls[0].image_url.as_str(),
            "/api/debug/frames/deadbeef.png"
        );
        assert!(page.graph_svg.contains("<svg"));
    }

    #[test]
    fn empty_page_has_no_graph() {
        let page = page_from_polls(&[], chrono_tz::Europe::London, |_| false);
        assert!(!page.has_polls);
        assert!(page.graph_svg.is_empty());
        assert!(page.eta_text.contains("No Pico"));
    }

    #[test]
    fn page_shows_next_refresh_from_sleep() {
        let polls = vec![poll_at(60, 78, false, 204, "deadbeef")];
        let page = page_from_polls(&polls, chrono_tz::Europe::London, |_| true);
        assert!(page.has_next_refresh);
        // 13:00 UTC + 1h sleep, displayed in BST.
        assert!(
            page.next_refresh.contains("15:00"),
            "got {}",
            page.next_refresh
        );
        assert!(!page.next_refresh_rel.is_empty());
    }

    #[test]
    fn page_next_refresh_undoes_pico_drift() {
        let mut polls = vec![poll_at(60, 78, false, 204, "deadbeef")];
        polls[0].sleep_s = 3495; // 3600 wall-clock seconds at 3% slow
        let page = page_from_polls_with_drift(&polls, chrono_tz::Europe::London, 0.03, |_| true);
        assert!(
            page.next_refresh.contains("15:00"),
            "got {}",
            page.next_refresh
        );
        assert_eq!(page.pico_drift_label, "3.0% slow");
    }

    #[test]
    fn format_until_future_and_overdue() {
        let now = Utc.with_ymd_and_hms(2026, 9, 16, 12, 0, 0).unwrap();
        assert_eq!(
            format_until(now + Duration::seconds(30), now).as_str(),
            "any moment"
        );
        assert_eq!(
            format_until(now + Duration::minutes(12), now).as_str(),
            "in 12 min"
        );
        assert_eq!(
            format_until(now + Duration::hours(2), now).as_str(),
            "in 2 hours"
        );
        assert_eq!(
            format_until(now - Duration::minutes(5), now).as_str(),
            "5 min overdue"
        );
    }
}
