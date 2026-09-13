//! Minimal CalDAV client aimed at iCloud Calendar / Reminders.
//!
//! iCloud: `https://caldav.icloud.com` + Apple ID + app-specific password.
//! Family Sharing calendars appear as ordinary named calendars after discovery.

use anyhow::{Context, Result};
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use reqwest::{Client, Method, StatusCode, Url};
use tracing::{info, warn};

use crate::config::IcloudConfig;

const NS_DAV: &str = "DAV:";
const NS_CAL: &str = "urn:ietf:params:xml:ns:caldav";

#[derive(Debug, Clone)]
pub struct CalendarRef {
    pub href: String,
    pub name: String,
    pub supports_events: bool,
    pub supports_todos: bool,
}

pub struct CalDav {
    client: Client,
    apple_id: String,
    password: String,
}

impl CalDav {
    pub fn new(cfg: &IcloudConfig) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/xml; charset=utf-8"),
        );
        let client = Client::builder()
            .default_headers(headers)
            .redirect(reqwest::redirect::Policy::limited(8))
            .timeout(std::time::Duration::from_secs(30))
            .build()?;
        Ok(Self {
            client,
            apple_id: cfg.apple_id.clone(),
            password: cfg.app_password.clone(),
        })
    }

    pub async fn list_calendars(&self) -> Result<Vec<CalendarRef>> {
        let principal = self
            .discover_href("https://caldav.icloud.com/.well-known/caldav", "current-user-principal")
            .await
            .context("CalDAV principal discovery")?;
        let home = self
            .discover_href(&principal, "calendar-home-set")
            .await
            .context("CalDAV calendar-home-set")?;
        info!(home, "discovered iCloud calendar home");
        self.propfind_calendars(&home).await
    }

    pub async fn fetch_calendar_data(&self, href: &str, report_body: &str) -> Result<String> {
        let resp = self
            .client
            .request(Method::from_bytes(b"REPORT").unwrap(), href)
            .basic_auth(&self.apple_id, Some(&self.password))
            .header("Depth", "1")
            .body(report_body.to_string())
            .send()
            .await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("CalDAV REPORT {href} failed: {status} {text}");
        }
        Ok(extract_calendar_data(&text))
    }

    async fn discover_href(&self, url: &str, tag: &str) -> Result<String> {
        let body = format!(
            r#"<?xml version="1.0" encoding="utf-8" ?>
<d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:prop><d:{tag}/><c:{tag}/></d:prop>
</d:propfind>"#,
            tag = if tag == "calendar-home-set" {
                // calendar-home-set lives in the CalDAV namespace
                return self.discover_named(url, "calendar-home-set").await;
            } else {
                tag
            }
        );
        let _ = body;
        self.discover_named(url, tag).await
    }

    async fn discover_named(&self, url: &str, tag: &str) -> Result<String> {
        let body = if tag == "calendar-home-set" {
            r#"<?xml version="1.0" encoding="utf-8" ?>
<d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:prop><c:calendar-home-set/></d:prop>
</d:propfind>"#
                .to_string()
        } else {
            format!(
                r#"<?xml version="1.0" encoding="utf-8" ?>
<d:propfind xmlns:d="DAV:">
  <d:prop><d:{tag}/></d:prop>
</d:propfind>"#
            )
        };
        let resp = self
            .client
            .request(Method::from_bytes(b"PROPFIND").unwrap(), url)
            .basic_auth(&self.apple_id, Some(&self.password))
            .header("Depth", "0")
            .body(body)
            .send()
            .await?;
        let status = resp.status();
        let final_url = resp.url().clone();
        let text = resp.text().await?;
        if status != StatusCode::MULTI_STATUS && !status.is_success() {
            anyhow::bail!("PROPFIND {url} failed: {status} {text}");
        }
        href_from_multistatus(&text, tag)
            .map(|href| resolve_href(&final_url, &href))
            .with_context(|| format!("missing {tag} in {text}"))
    }

    async fn propfind_calendars(&self, home: &str) -> Result<Vec<CalendarRef>> {
        let body = r#"<?xml version="1.0" encoding="utf-8" ?>
<d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:prop>
    <d:displayname/>
    <d:resourcetype/>
    <c:supported-calendar-component-set/>
  </d:prop>
</d:propfind>"#;
        let resp = self
            .client
            .request(Method::from_bytes(b"PROPFIND").unwrap(), home)
            .basic_auth(&self.apple_id, Some(&self.password))
            .header("Depth", "1")
            .body(body)
            .send()
            .await?;
        let status = resp.status();
        let base = resp.url().clone();
        let text = resp.text().await?;
        if status != StatusCode::MULTI_STATUS && !status.is_success() {
            anyhow::bail!("PROPFIND calendars failed: {status} {text}");
        }
        Ok(parse_calendar_list(&text, &base))
    }
}

pub fn event_report(start_utc: &str, end_utc: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8" ?>
<c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:prop><c:calendar-data/></d:prop>
  <c:filter>
    <c:comp-filter name="VCALENDAR">
      <c:comp-filter name="VEVENT">
        <c:time-range start="{start_utc}" end="{end_utc}"/>
      </c:comp-filter>
    </c:comp-filter>
  </c:filter>
</c:calendar-query>"#
    )
}

pub fn todo_report() -> String {
    r#"<?xml version="1.0" encoding="utf-8" ?>
<c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:prop><c:calendar-data/></d:prop>
  <c:filter>
    <c:comp-filter name="VCALENDAR">
      <c:comp-filter name="VTODO"/>
    </c:comp-filter>
  </c:filter>
</c:calendar-query>"#
        .into()
}

fn local_name<'a>(e: roxmltree::Node<'a, 'a>) -> &'a str {
    e.tag_name().name()
}

fn href_from_multistatus(xml: &str, tag: &str) -> Option<String> {
    let doc = roxmltree::Document::parse(xml).ok()?;
    let node = doc.descendants().find(|n| local_name(*n) == tag)?;
    node.descendants()
        .find(|n| local_name(*n) == "href")
        .and_then(|n| n.text())
        .map(|s| s.trim().to_string())
}

fn parse_calendar_list(xml: &str, base: &Url) -> Vec<CalendarRef> {
    let Ok(doc) = roxmltree::Document::parse(xml) else {
        warn!("calendar list was not valid XML");
        return Vec::new();
    };
    let mut out = Vec::new();
    for resp in doc.descendants().filter(|n| local_name(*n) == "response") {
        let is_calendar = resp.descendants().any(|n| local_name(n) == "calendar");
        if !is_calendar {
            continue;
        }
        let href = resp
            .descendants()
            .find(|n| local_name(*n) == "href")
            .and_then(|n| n.text())
            .unwrap_or("")
            .trim()
            .to_string();
        if href.is_empty() {
            continue;
        }
        let name = resp
            .descendants()
            .find(|n| local_name(*n) == "displayname")
            .and_then(|n| n.text())
            .unwrap_or(&href)
            .trim()
            .to_string();
        let mut supports_events = false;
        let mut supports_todos = false;
        for comp in resp
            .descendants()
            .filter(|n| local_name(*n) == "comp" || local_name(*n) == "calendar-component")
        {
            let n = comp.attribute("name").unwrap_or("").to_ascii_uppercase();
            if n == "VEVENT" {
                supports_events = true;
            }
            if n == "VTODO" {
                supports_todos = true;
            }
        }
        if !supports_events && !supports_todos {
            supports_events = true;
        }
        out.push(CalendarRef {
            href: resolve_href(base, &href),
            name,
            supports_events,
            supports_todos,
        });
    }
    out
}

fn extract_calendar_data(xml: &str) -> String {
    if !xml.trim_start().starts_with('<') {
        return xml.to_string();
    }
    let Ok(doc) = roxmltree::Document::parse(xml) else {
        return xml.to_string();
    };
    let mut chunks = Vec::new();
    for node in doc
        .descendants()
        .filter(|n| local_name(*n) == "calendar-data")
    {
        if let Some(text) = node.text() {
            if text.contains("BEGIN:VCALENDAR") {
                chunks.push(text.trim().to_string());
            }
        }
    }
    if chunks.is_empty() {
        xml.to_string()
    } else {
        chunks.join("\n")
    }
}

fn resolve_href(base: &Url, href: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") {
        href.to_string()
    } else {
        base.join(href)
            .map(|u| u.to_string())
            .unwrap_or_else(|_| href.to_string())
    }
}

pub fn match_named<'a>(cals: &'a [CalendarRef], names: &[String]) -> Vec<&'a CalendarRef> {
    if names.is_empty() {
        return cals.iter().collect();
    }
    cals.iter()
        .filter(|c| {
            names.iter().any(|want| {
                c.name.eq_ignore_ascii_case(want) || c.href.to_ascii_lowercase().contains(&want.to_ascii_lowercase())
            })
        })
        .collect()
}

#[allow(dead_code)]
fn _ns_unused() -> (&'static str, &'static str) {
    (NS_DAV, NS_CAL)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MULTI: &str = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:response>
    <d:href>/123/calendars/family/</d:href>
    <d:propstat>
      <d:prop>
        <d:displayname>Family</d:displayname>
        <d:resourcetype><d:collection/><c:calendar/></d:resourcetype>
        <c:supported-calendar-component-set>
          <c:comp name="VEVENT"/>
        </c:supported-calendar-component-set>
      </d:prop>
    </d:propstat>
  </d:response>
  <d:response>
    <d:href>/123/calendars/shopping/</d:href>
    <d:propstat>
      <d:prop>
        <d:displayname>Shopping</d:displayname>
        <d:resourcetype><d:collection/><c:calendar/></d:resourcetype>
        <c:supported-calendar-component-set>
          <c:comp name="VTODO"/>
        </c:supported-calendar-component-set>
      </d:prop>
    </d:propstat>
  </d:response>
</d:multistatus>"#;

    #[test]
    fn parses_family_and_shopping_lists() {
        let base = Url::parse("https://p65-caldav.icloud.com/123/calendars/").unwrap();
        let cals = parse_calendar_list(MULTI, &base);
        assert_eq!(cals.len(), 2);
        assert_eq!(cals[0].name, "Family");
        assert!(cals[0].supports_events);
        assert_eq!(cals[1].name, "Shopping");
        assert!(cals[1].supports_todos);
        let matched = match_named(&cals, &["Shopping".into()]);
        assert_eq!(matched.len(), 1);
    }

    #[test]
    fn extracts_embedded_ics() {
        let xml = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:response><d:propstat><d:prop>
    <c:calendar-data>BEGIN:VCALENDAR
BEGIN:VEVENT
SUMMARY:Hi
END:VEVENT
END:VCALENDAR</c:calendar-data>
  </d:prop></d:propstat></d:response>
</d:multistatus>"#;
        let ics = extract_calendar_data(xml);
        assert!(ics.contains("SUMMARY:Hi"));
    }
}
