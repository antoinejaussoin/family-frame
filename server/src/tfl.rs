//! TfL Underground line status for the family frame.
//!
//! Polls the public Unified API for a fixed set of lines. No API key is
//! required at household poll rates.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::Deserialize;
use tracing::{info, warn};

use crate::model::TubeLine;

const STATUS_URL: &str =
    "https://api.tfl.gov.uk/Line/northern,circle,district,victoria/Status";
const FETCH_TTL: Duration = Duration::from_secs(15 * 60);

/// Display order on the panel.
const LINES: [LineSpec; 4] = [
    LineSpec {
        id: "northern",
        name: "Northern",
        colour: "black",
    },
    LineSpec {
        id: "circle",
        name: "Circle",
        colour: "yellow",
    },
    LineSpec {
        id: "district",
        name: "District",
        colour: "green",
    },
    LineSpec {
        id: "victoria",
        name: "Victoria",
        colour: "blue",
    },
];

struct LineSpec {
    id: &'static str,
    name: &'static str,
    colour: &'static str,
}

static LAST: Mutex<Option<(Instant, Vec<TubeLine>)>> = Mutex::new(None);

#[derive(Debug, Deserialize)]
struct ApiLine {
    id: String,
    #[serde(default)]
    name: String,
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

pub async fn load_tube() -> Result<Vec<TubeLine>> {
    if let Some((at, lines)) = LAST.lock().ok().and_then(|g| g.clone()) {
        if at.elapsed() < FETCH_TTL && !lines.is_empty() {
            return Ok(lines);
        }
    }

    let body = reqwest::Client::new()
        .get(STATUS_URL)
        .header("accept", "application/json")
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .with_context(|| format!("TfL GET {STATUS_URL}"))?
        .error_for_status()
        .with_context(|| format!("TfL status {STATUS_URL}"))?
        .text()
        .await?;

    let lines = tube_from_json(&body)?;
    if let Ok(mut guard) = LAST.lock() {
        *guard = Some((Instant::now(), lines.clone()));
    }
    info!(n = lines.len(), "loaded TfL tube status");
    Ok(lines)
}

pub fn tube_from_json(json: &str) -> Result<Vec<TubeLine>> {
    let parsed: Vec<ApiLine> = serde_json::from_str(json).context("parsing TfL JSON")?;
    let mut by_id = std::collections::HashMap::new();
    for line in parsed {
        by_id.insert(line.id.to_ascii_lowercase(), line);
    }

    let mut out = Vec::with_capacity(LINES.len());
    for spec in &LINES {
        let line = match by_id.get(spec.id) {
            Some(line) => line_from_api(spec, line),
            None => {
                warn!(id = spec.id, "TfL response missing line");
                TubeLine {
                    id: spec.id.into(),
                    name: spec.name.into(),
                    status: "Unknown".into(),
                    severity: "delay".into(),
                    colour: spec.colour.into(),
                }
            }
        };
        out.push(line);
    }
    Ok(out)
}

fn line_from_api(spec: &LineSpec, line: &ApiLine) -> TubeLine {
    let (severity_code, description) = line
        .line_statuses
        .iter()
        .min_by_key(|s| s.status_severity)
        .map(|s| (s.status_severity, s.status_severity_description.as_str()))
        .unwrap_or((-1, "Unknown"));
    let (status, severity) = shorten_status(severity_code, description);
    TubeLine {
        id: spec.id.into(),
        name: if line.name.is_empty() {
            spec.name.into()
        } else {
            // TfL returns "Northern" etc.; keep our short names.
            spec.name.into()
        },
        status,
        severity: severity.into(),
        colour: spec.colour.into(),
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

pub fn demo_tube() -> Vec<TubeLine> {
    vec![
        TubeLine {
            id: "northern".into(),
            name: "Northern".into(),
            status: "Good service".into(),
            severity: "good".into(),
            colour: "black".into(),
        },
        TubeLine {
            id: "circle".into(),
            name: "Circle".into(),
            status: "Good service".into(),
            severity: "good".into(),
            colour: "yellow".into(),
        },
        TubeLine {
            id: "district".into(),
            name: "District".into(),
            status: "Severe delays".into(),
            severity: "severe".into(),
            colour: "green".into(),
        },
        TubeLine {
            id: "victoria".into(),
            name: "Victoria".into(),
            status: "Minor delays".into(),
            severity: "delay".into(),
            colour: "blue".into(),
        },
    ]
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
        let lines = tube_from_json(&fixture()).unwrap();
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
