//! TfL Underground line status for the family frame.
//!
//! Polls the public Unified API for a fixed set of lines. No API key is
//! required at household poll rates.

use std::time::Duration;

use anyhow::{Context, Result};
use serde::Deserialize;
use tracing::{info, warn};

use crate::config::TflLineConfig;
use crate::model::StatusLine;
use crate::sources::cache::TtlCache;

use super::context::SourceContext;
use super::contribute::{Contribution, SourceOutcome};
use super::DataSource;

pub struct TflSource;

#[async_trait::async_trait]
impl DataSource for TflSource {
    fn id(&self) -> &'static str {
        "tfl"
    }

    fn enabled(&self, cfg: &crate::config::Config) -> bool {
        cfg.sources.tfl.enabled
    }

    async fn load(&self, ctx: &SourceContext<'_>) -> Result<SourceOutcome> {
        let specs = &ctx.cfg.sources.tfl.lines;
        match load_tube(specs).await {
            Ok(lines) if !lines.is_empty() => Ok(SourceOutcome::live(
                "TfL tube",
                Contribution::Transit(lines),
            )),
            Ok(_) => {
                warn!("TfL returned no lines");
                Ok(SourceOutcome::unavailable(
                    "TfL empty — demo tube",
                    Contribution::Transit(demo_tube_for(specs)),
                ))
            }
            Err(err) => {
                warn!(%err, "TfL failed; using demo tube");
                Ok(SourceOutcome::unavailable(
                    "TfL unavailable",
                    Contribution::Transit(demo_tube_for(specs)),
                ))
            }
        }
    }

    fn demo(&self, ctx: &SourceContext<'_>) -> Option<Contribution> {
        Some(Contribution::Transit(demo_tube_for(
            &ctx.cfg.sources.tfl.lines,
        )))
    }
}

const FETCH_TTL: Duration = Duration::from_secs(15 * 60);

static LAST: TtlCache<(String, Vec<StatusLine>)> = TtlCache::new();

#[derive(Debug, Deserialize)]
struct ApiLine {
    id: String,
    #[serde(rename = "lineStatuses", default)]
    line_statuses: Vec<ApiStatus>,
}

#[derive(Debug, Deserialize)]
struct ApiStatus {
    #[serde(rename = "statusSeverity", default)]
    status_severity: i32,
    #[serde(rename = "statusSeverityDescription", default)]
    status_severity_description: String,
}

fn status_url(lines: &[TflLineConfig]) -> String {
    let ids = lines
        .iter()
        .map(|l| l.id.as_str())
        .collect::<Vec<_>>()
        .join(",");
    format!("https://api.tfl.gov.uk/Line/{ids}/Status")
}

pub async fn load_tube(lines: &[TflLineConfig]) -> Result<Vec<StatusLine>> {
    let url = status_url(lines);
    if let Some((_, cached)) = LAST.get(FETCH_TTL, |(cached_url, rows)| {
        cached_url == &url && !rows.is_empty()
    }) {
        return Ok(cached);
    }

    let body = reqwest::Client::new()
        .get(&url)
        .header("accept", "application/json")
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .with_context(|| format!("TfL GET {url}"))?
        .error_for_status()
        .with_context(|| format!("TfL status {url}"))?
        .text()
        .await?;

    let parsed = tube_from_json(&body, lines)?;
    LAST.set((url, parsed.clone()));
    info!(n = parsed.len(), "loaded TfL tube status");
    Ok(parsed)
}

pub fn tube_from_json(json: &str, lines: &[TflLineConfig]) -> Result<Vec<StatusLine>> {
    let parsed: Vec<ApiLine> = serde_json::from_str(json).context("parsing TfL JSON")?;
    let mut by_id = std::collections::HashMap::new();
    for line in parsed {
        by_id.insert(line.id.to_ascii_lowercase(), line);
    }

    let mut out = Vec::with_capacity(lines.len());
    for spec in lines {
        let line = match by_id.get(&spec.id.to_ascii_lowercase()) {
            Some(line) => line_from_api(spec, line),
            None => {
                warn!(id = %spec.id, "TfL response missing line");
                StatusLine {
                    id: spec.id.clone(),
                    name: spec.name.clone(),
                    status: "Unknown".into(),
                    severity: "delay".into(),
                    colour: spec.colour.clone(),
                }
            }
        };
        out.push(line);
    }
    Ok(out)
}

fn line_from_api(spec: &TflLineConfig, line: &ApiLine) -> StatusLine {
    let (severity_code, description) = line
        .line_statuses
        .iter()
        .min_by_key(|s| s.status_severity)
        .map(|s| (s.status_severity, s.status_severity_description.as_str()))
        .unwrap_or((-1, "Unknown"));
    let (status, severity) = shorten_status(severity_code, description);
    StatusLine {
        id: spec.id.clone(),
        name: spec.name.clone(),
        status,
        severity: severity.into(),
        colour: spec.colour.clone(),
    }
}

/// Map TfL severity (10 = good … 0 = special) to a short label + CSS class.
fn shorten_status(severity: i32, description: &str) -> (String, &'static str) {
    let desc = description.trim();
    let lower = desc.to_ascii_lowercase();
    if severity >= 10 || lower == "good service" {
        return ("Good service".into(), "good");
    }
    if lower.contains("part closed") || lower.contains("suspended") || lower.contains("closed") {
        return ("Suspended".into(), "severe");
    }
    if lower.contains("severe") {
        return ("Severe delays".into(), "severe");
    }
    if lower.contains("minor") {
        return ("Minor delays".into(), "delay");
    }
    if lower.contains("reduced") || lower.contains("delay") {
        return ("Delays".into(), "delay");
    }
    if severity <= 4 {
        return ("Disrupted".into(), "severe");
    }
    if severity <= 8 {
        return ("Delays".into(), "delay");
    }
    if desc.is_empty() {
        return ("Check status".into(), "delay");
    }
    if desc.chars().count() <= 22 {
        return (desc.to_string(), "delay");
    }
    ("Check status".into(), "delay")
}

pub fn demo_tube() -> Vec<StatusLine> {
    demo_tube_for(&crate::config::default_tfl_lines())
}

pub fn demo_tube_for(lines: &[TflLineConfig]) -> Vec<StatusLine> {
    let demos = [
        ("Good service", "good"),
        ("Good service", "good"),
        ("Severe delays", "severe"),
        ("Minor delays", "delay"),
    ];
    lines
        .iter()
        .enumerate()
        .map(|(i, spec)| {
            let (status, severity) = demos.get(i).copied().unwrap_or(("Good service", "good"));
            StatusLine {
                id: spec.id.clone(),
                name: spec.name.clone(),
                status: status.into(),
                severity: severity.into(),
                colour: spec.colour.clone(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> String {
        std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/tfl_status.json"),
        )
        .unwrap()
    }

    #[test]
    fn parses_ordered_lines() {
        let lines = tube_from_json(&fixture(), &crate::config::default_tfl_lines()).unwrap();
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0].name, "Northern");
        assert_eq!(lines[0].status, "Good service");
        assert_eq!(lines[0].severity, "good");
        assert_eq!(lines[0].colour, "black");
        assert_eq!(lines[1].name, "Circle");
        assert_eq!(lines[1].severity, "good");
        assert_eq!(lines[1].colour, "yellow");
        assert_eq!(lines[2].name, "District");
        assert_eq!(lines[2].status, "Severe delays");
        assert_eq!(lines[2].severity, "severe");
        assert_eq!(lines[2].colour, "green");
        assert_eq!(lines[3].name, "Victoria");
        assert_eq!(lines[3].status, "Good service");
        assert_eq!(lines[3].colour, "blue");
    }

    #[test]
    fn shortens_minor_delays() {
        let (status, severity) = shorten_status(9, "Minor Delays");
        assert_eq!(status, "Minor delays");
        assert_eq!(severity, "delay");
    }
}
