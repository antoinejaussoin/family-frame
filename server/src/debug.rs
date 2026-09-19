//! Pico poll history for the family UI stats page (`GET /stats`, `GET /api/debug`).
//!
//! POST `/api/frame.bin` appends one JSONL row and, on 200, a dithered PNG keyed
//! by checksum. GET `/api/frame.bin` from a browser is not recorded.
//! `DELETE /api/debug` wipes `polls.jsonl` and `frames/` so history starts empty.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::warn;

use crate::battery::{self, BatteryReport, Cell};

pub const MAX_POLLS: usize = 500;
pub const POLLS_PER_PAGE: usize = 20;

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
    /// Wall-clock instant that sleep was aiming for (not the POWMAN seconds).
    #[serde(default)]
    pub wake_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct DebugExtras {
    pub cell: Cell,
    pub wakes_per_day: [f64; 7],
}

impl Default for DebugExtras {
    fn default() -> Self {
        Self {
            cell: Cell::DEFAULT,
            wakes_per_day: [24.0; 7],
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DebugPage {
    pub version: &'static str,
    pub has_polls: bool,
    pub battery: BatteryReport,
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
    pub pico_drift: f64,
    pub pico_drift_label: String,
    pub graph_svg: String,
    pub debug_dir_bytes: u64,
    pub debug_dir_label: String,
    pub poll_count: usize,
    pub page: usize,
    pub page_size: usize,
    pub page_count: usize,
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
        let mut guard = self.inner.lock().await;
        if let Some(png) = png {
            if is_hex_checksum(&poll.checksum) {
                let path = self.frame_path(&poll.checksum);
                if !path.exists() {
                    fs::write(&path, png).with_context(|| format!("writing {}", path.display()))?;
                }
            }
        }
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

    pub fn dir_bytes(&self) -> u64 {
        dir_size(&self.dir)
    }

    pub async fn clear(&self) -> Result<()> {
        let mut guard = self.inner.lock().await;
        guard.clear();
        if self.dir.exists() {
            fs::remove_dir_all(&self.dir)
                .with_context(|| format!("removing {}", self.dir.display()))?;
        }
        fs::create_dir_all(self.dir.join("frames"))
            .with_context(|| format!("creating {}", self.dir.display()))?;
        Ok(())
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
    page_from_polls_with_drift(polls, tz, 0.0, 1, 0, has_frame)
}

pub fn page_from_polls_with_drift(
    polls: &[Poll],
    tz: Tz,
    pico_drift: f64,
    page: usize,
    dir_bytes: u64,
    has_frame: impl Fn(&str) -> bool,
) -> DebugPage {
    page_from_polls_full(
        polls,
        tz,
        pico_drift,
        page,
        dir_bytes,
        &DebugExtras::default(),
        has_frame,
    )
}

pub fn page_from_polls_full(
    polls: &[Poll],
    tz: Tz,
    pico_drift: f64,
    page: usize,
    dir_bytes: u64,
    extras: &DebugExtras,
    has_frame: impl Fn(&str) -> bool,
) -> DebugPage {
    let now = Utc::now();
    let (poll_count, page, page_count) = page_bounds(polls.len(), page);
    let debug_dir_label = format_bytes(dir_bytes);
    let battery = battery::report(polls, extras.cell, extras.wakes_per_day);
    if polls.is_empty() {
        return DebugPage {
            version: crate::VERSION,
            has_polls: false,
            battery,
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
            pico_drift: 0.0,
            pico_drift_label: String::new(),
            graph_svg: String::new(),
            debug_dir_bytes: dir_bytes,
            debug_dir_label,
            poll_count,
            page,
            page_size: POLLS_PER_PAGE,
            page_count,
            polls: Vec::new(),
        };
    }

    let last = polls.last().unwrap();
    let (next_refresh, next_refresh_rel) = next_refresh_copy(last, now, tz, pico_drift);
    let skip = (page - 1) * POLLS_PER_PAGE;
    let empty_mv = extras.cell.empty_mv;

    DebugPage {
        version: crate::VERSION,
        has_polls: true,
        last_pct: battery.soc_pct,
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
        eta_text: battery.eta_text.clone(),
        eta_kind: battery.eta_kind.clone(),
        pico_drift,
        pico_drift_label: pico_drift_label(pico_drift),
        graph_svg: graph_svg(polls, extras.cell, battery.eta_seconds),
        debug_dir_bytes: dir_bytes,
        debug_dir_label,
        poll_count,
        page,
        page_size: POLLS_PER_PAGE,
        page_count,
        battery,
        polls: polls
            .iter()
            .rev()
            .skip(skip)
            .take(POLLS_PER_PAGE)
            .map(|p| DebugPollView {
                when: format_when(p.t, tz),
                status: p.status,
                status_label: status_label(p.status).into(),
                pct: battery::soc_pct(p.mv, empty_mv),
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

fn page_bounds(poll_count: usize, page: usize) -> (usize, usize, usize) {
    let page_count = poll_count.div_ceil(POLLS_PER_PAGE);
    let page = if page_count == 0 {
        1
    } else {
        page.clamp(1, page_count)
    };
    (poll_count, page, page_count)
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
    let at = if let Some(wake_at) = last.wake_at {
        wake_at
    } else {
        if last.sleep_s == 0 {
            return (String::new(), String::new());
        }
        let wall = crate::schedule::wall_secs_from_commanded(last.sleep_s, pico_drift);
        last.t + Duration::seconds(i64::try_from(wall).unwrap_or(i64::MAX))
    };
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
        format_human_span(secs, HumanTone::Until)
    } else {
        format_human_span(-secs, HumanTone::Overdue)
    }
}

fn format_rel(then: DateTime<Utc>, now: DateTime<Utc>) -> String {
    format_human_span(
        now.signed_duration_since(then).num_seconds().max(0),
        HumanTone::Ago,
    )
}

#[derive(Clone, Copy)]
enum HumanTone {
    Until,
    Overdue,
    Ago,
}

struct HumanUnit {
    n: i64,
    word: &'static str,
    about: bool,
}

/// Nearest minute / hour / day so 1 h 59 m reads as “about 2 hours”, not “1 hour”.
fn rounded_human_unit(secs: i64) -> HumanUnit {
    if secs < 50 * 60 {
        let n = ((secs as f64) / 60.0).round().max(1.0) as i64;
        return HumanUnit {
            n,
            word: "min",
            about: false,
        };
    }
    if secs < 36 * 3_600 {
        let raw = secs as f64 / 3_600.0;
        let n = raw.round().max(1.0) as i64;
        let floored = secs / 3_600;
        return HumanUnit {
            n,
            word: if n == 1 { "hour" } else { "hours" },
            about: n != floored,
        };
    }
    let raw = secs as f64 / 86_400.0;
    let n = raw.round().max(1.0) as i64;
    let floored = secs / 86_400;
    HumanUnit {
        n,
        word: if n == 1 { "day" } else { "days" },
        about: n != floored,
    }
}

fn format_human_span(secs: i64, tone: HumanTone) -> String {
    let secs = secs.max(0);
    if secs < 60 {
        return match tone {
            HumanTone::Until => "any moment".into(),
            HumanTone::Overdue => "overdue".into(),
            HumanTone::Ago => "just now".into(),
        };
    }
    let unit = rounded_human_unit(secs);
    let n = unit.n;
    let word = unit.word;
    match tone {
        HumanTone::Until if unit.about => format!("in about {n} {word}"),
        HumanTone::Until => format!("in {n} {word}"),
        HumanTone::Overdue if unit.about => format!("about {n} {word} overdue"),
        HumanTone::Overdue => format!("{n} {word} overdue"),
        HumanTone::Ago if unit.about => format!("about {n} {word} ago"),
        HumanTone::Ago => format!("{n} {word} ago"),
    }
}

fn graph_svg(polls: &[Poll], cell: Cell, eta_seconds: i64) -> String {
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
    let pct_of = |p: &Poll| f64::from(battery::soc_pct(p.mv, cell.empty_mv));

    let mut points = String::new();
    for p in polls {
        let _ = write!(
            points,
            "{:.1},{:.1} ",
            x_of(p.t.timestamp() as f64),
            y_of(pct_of(p))
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
        let cy = y_of(pct_of(p));
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
    if eta_seconds > 0 {
        let last = polls.last().unwrap();
        let x1 = x_of(last.t.timestamp() as f64);
        let y1 = y_of(pct_of(last));
        let x2 = W - PAD_R;
        let y2 = y_of(0.0);
        if x2 > x1 + 4.0 {
            let _ = write!(
                proj,
                r##"<line x1="{x1:.1}" y1="{y1:.1}" x2="{x2:.1}" y2="{y2:.1}" stroke="{dash}" stroke-dasharray="4 3" stroke-width="1.5"/>"##
            );
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
        // One value per line is the usual case; also recover glued records
        // (`}{`) left by a crash between writing JSON and the trailing newline.
        for item in serde_json::Deserializer::from_str(line).into_iter::<Poll>() {
            match item {
                Ok(p) => polls.push(p),
                Err(err) => {
                    warn!(%err, "skipping bad debug poll line");
                    break;
                }
            }
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
        .read(true)
        .append(true)
        .open(path)
        .with_context(|| format!("opening {}", path.display()))?;
    ensure_trailing_newline(&mut file)?;
    serde_json::to_writer(&mut file, poll)?;
    file.write_all(b"\n")?;
    Ok(())
}

fn ensure_trailing_newline(file: &mut File) -> Result<()> {
    let len = file.metadata().context("stat poll log")?.len();
    if len == 0 {
        return Ok(());
    }
    file.seek(SeekFrom::Start(len - 1))
        .context("seek poll log")?;
    let mut last = [0u8; 1];
    file.read_exact(&mut last).context("read poll log tail")?;
    if last[0] != b'\n' {
        file.write_all(b"\n").context("repair poll log newline")?;
    }
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

fn dir_size(path: &Path) -> u64 {
    let Ok(rd) = fs::read_dir(path) else {
        return 0;
    };
    let mut total = 0u64;
    for entry in rd.flatten() {
        let child = entry.path();
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_dir() {
            total += dir_size(&child);
        } else if meta.is_file() {
            total += meta.len();
        }
    }
    total
}

fn format_bytes(n: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} B")
    } else if value >= 10.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
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
            wake_at: None,
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
    async fn loads_concatenated_json_objects_on_one_line() {
        let dir = tempfile::tempdir().unwrap();
        let debug_dir = dir.path().join("debug");
        fs::create_dir_all(debug_dir.join("frames")).unwrap();
        let a = poll_at(0, 80, false, 200, "aa");
        let b = poll_at(60, 78, false, 204, "bb");
        fs::write(
            debug_dir.join("polls.jsonl"),
            format!(
                "{}{}\n",
                serde_json::to_string(&a).unwrap(),
                serde_json::to_string(&b).unwrap()
            ),
        )
        .unwrap();
        let log = DebugLog::open(dir.path()).unwrap();
        let snap = log.snapshot().await;
        assert_eq!(snap.len(), 2);
        assert_eq!(snap[0].checksum, "aa");
        assert_eq!(snap[1].checksum, "bb");
    }

    #[tokio::test]
    async fn append_starts_new_line_if_file_missing_newline() {
        let dir = tempfile::tempdir().unwrap();
        let debug_dir = dir.path().join("debug");
        fs::create_dir_all(&debug_dir).unwrap();
        let first = poll_at(0, 80, false, 200, "aa");
        fs::write(
            debug_dir.join("polls.jsonl"),
            serde_json::to_string(&first).unwrap(),
        )
        .unwrap();
        let log = DebugLog::open(dir.path()).unwrap();
        log.record(poll_at(60, 78, false, 204, "bb"), None)
            .await
            .unwrap();
        let text = fs::read_to_string(debug_dir.join("polls.jsonl")).unwrap();
        assert_eq!(text.matches('\n').count(), 2, "{text}");
        assert!(!text.contains("}{"), "{text}");
        let snap = log.snapshot().await;
        assert_eq!(snap.len(), 2);
        assert_eq!(snap[1].checksum, "bb");
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
    fn page_lists_newest_first_with_image() {
        let polls = vec![
            poll_at(0, 80, false, 200, "deadbeef"),
            poll_at(60, 78, false, 204, "deadbeef"),
        ];
        let page = page_from_polls(&polls, chrono_tz::Europe::London, |c| c == "deadbeef");
        assert!(page.has_polls);
        assert_eq!(page.last_pct, battery::soc_pct(polls[1].mv, 3300));
        assert_eq!(page.battery.linear_pct, 78);
        assert!(page.battery.soc_pct > page.battery.linear_pct);
        assert_eq!(page.poll_count, 2);
        assert_eq!(page.page, 1);
        assert_eq!(page.page_size, POLLS_PER_PAGE);
        assert_eq!(page.page_count, 1);
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
        assert_eq!(page.version, crate::VERSION);
        assert!(!page.has_polls);
        assert!(page.graph_svg.is_empty());
        assert!(page.eta_text.contains("No Pico"));
        assert_eq!(page.poll_count, 0);
        assert_eq!(page.page, 1);
        assert_eq!(page.page_count, 0);
        assert_eq!(page.debug_dir_label, "0 B");
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
    fn page_next_refresh_uses_stored_wake_at() {
        let mut polls = vec![poll_at(60, 78, false, 204, "deadbeef")];
        polls[0].sleep_s = 1;
        polls[0].wake_at = Some(Utc.with_ymd_and_hms(2026, 9, 15, 14, 0, 0).unwrap());
        let page = page_from_polls(&polls, chrono_tz::Europe::London, |_| true);
        assert!(
            page.next_refresh.contains("15:00"),
            "got {}",
            page.next_refresh
        );
    }

    #[test]
    fn page_next_refresh_undoes_pico_drift() {
        let mut polls = vec![poll_at(60, 78, false, 204, "deadbeef")];
        polls[0].sleep_s = 3495; // 3600 wall-clock seconds at 3% slow
        let page =
            page_from_polls_with_drift(&polls, chrono_tz::Europe::London, 0.03, 1, 0, |_| true);
        assert!(
            page.next_refresh.contains("15:00"),
            "got {}",
            page.next_refresh
        );
        assert_eq!(page.pico_drift, 0.03);
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
            format_until(now + Duration::hours(1) + Duration::minutes(59), now).as_str(),
            "in about 2 hours"
        );
        assert_eq!(
            format_rel(now - Duration::hours(1) - Duration::minutes(59), now).as_str(),
            "about 2 hours ago"
        );
        assert_eq!(
            format_until(now + Duration::hours(25) + Duration::minutes(50), now).as_str(),
            "in about 26 hours"
        );
        assert_eq!(
            format_until(now + Duration::hours(44), now).as_str(),
            "in about 2 days"
        );
        assert_eq!(
            format_until(now - Duration::minutes(5), now).as_str(),
            "5 min overdue"
        );
    }

    #[test]
    fn page_paginates_newest_first() {
        let polls: Vec<_> = (0..21)
            .map(|i| poll_at(i as i64, i as u16, false, 200, "aa"))
            .collect();
        let page =
            page_from_polls_with_drift(&polls, chrono_tz::Europe::London, 0.0, 1, 0, |_| false);
        assert_eq!(page.poll_count, 21);
        assert_eq!(page.page, 1);
        assert_eq!(page.page_count, 2);
        assert_eq!(page.polls.len(), POLLS_PER_PAGE);
        assert_eq!(page.polls[0].mv, 3300 + 20 * 9);
        assert_eq!(page.polls[19].mv, 3300 + 1 * 9);

        let page2 =
            page_from_polls_with_drift(&polls, chrono_tz::Europe::London, 0.0, 2, 0, |_| false);
        assert_eq!(page2.page, 2);
        assert_eq!(page2.polls.len(), 1);
        assert_eq!(page2.polls[0].mv, 3300);

        let clamped =
            page_from_polls_with_drift(&polls, chrono_tz::Europe::London, 0.0, 99, 0, |_| false);
        assert_eq!(clamped.page, 2);
        assert_eq!(clamped.polls.len(), 1);
    }

    #[test]
    fn format_bytes_uses_binary_units() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(10 * 1024), "10 KB");
        assert_eq!(format_bytes(1024 * 1024), "1.0 MB");
    }

    #[tokio::test]
    async fn dir_bytes_counts_frames_and_jsonl() {
        let dir = tempfile::tempdir().unwrap();
        let log = DebugLog::open(dir.path()).unwrap();
        let png = b"\x89PNG\r\n\x1a\nfake-image-bytes";
        log.record(poll_at(0, 80, false, 200, "aa11bb22"), Some(png))
            .await
            .unwrap();
        let n = log.dir_bytes();
        assert!(n >= png.len() as u64, "got {n}");
        let page = page_from_polls_with_drift(
            &log.snapshot().await,
            chrono_tz::Europe::London,
            0.0,
            1,
            n,
            |_| true,
        );
        assert_eq!(page.debug_dir_bytes, n);
        assert!(!page.debug_dir_label.is_empty());
        assert_ne!(page.debug_dir_label, "0 B");
    }

    #[tokio::test]
    async fn clear_wipes_polls_and_frames() {
        let dir = tempfile::tempdir().unwrap();
        let log = DebugLog::open(dir.path()).unwrap();
        log.record(poll_at(0, 80, false, 200, "aa11bb22"), Some(b"png"))
            .await
            .unwrap();
        log.record(poll_at(60, 78, false, 204, "aa11bb22"), None)
            .await
            .unwrap();
        assert_eq!(log.snapshot().await.len(), 2);
        assert!(log.has_frame("aa11bb22"));
        log.clear().await.unwrap();
        assert!(log.snapshot().await.is_empty());
        assert!(!log.has_frame("aa11bb22"));
        assert!(!dir.path().join("debug/polls.jsonl").exists());
        assert!(dir.path().join("debug/frames").is_dir());
        assert_eq!(log.dir_bytes(), 0);
    }
}
