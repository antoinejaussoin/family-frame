use chrono::{DateTime, NaiveDate};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CalendarEvent {
    pub start: String,
    pub title: String,
    pub who: String,
    pub all_day: bool,
    pub day_label: String,
}

/// Today/week title column is ~830px (1600 panel − padding − 460px sidebar −
/// 150px time − gaps) at 30px Noto Sans, ~15.5px per character → ~53 glyphs.
/// 48 leaves room for wide letters and the ellipsis.
pub const EVENT_TITLE_MAX_CHARS: usize = 48;

pub fn truncate_event_title(title: &str) -> String {
    let title = title.lines().next().unwrap_or("").trim();
    if title.chars().count() <= EVENT_TITLE_MAX_CHARS {
        return title.to_string();
    }
    let take = EVENT_TITLE_MAX_CHARS.saturating_sub(1);
    let mut out: String = title.chars().take(take).collect();
    while out.ends_with(' ') {
        out.pop();
    }
    out.push('…');
    out
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TodoItem {
    pub title: String,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RoomClimate {
    pub name: String,
    pub temperature: String,
    pub humidity: String,
    pub online: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WeatherSlot {
    pub period: String,
    pub icon: String,
    pub temperature: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WeatherDay {
    pub label: String,
    pub slots: Vec<WeatherSlot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Weather {
    pub location: String,
    pub days: Vec<WeatherDay>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Dashboard {
    pub family_name: String,
    pub weekday: String,
    pub date_long: String,
    pub date_iso: String,
    pub events_today: Vec<CalendarEvent>,
    pub events_week: Vec<CalendarEvent>,
    pub todos: Vec<TodoItem>,
    pub rooms: Vec<RoomClimate>,
    pub weather: Weather,
    pub source_note: String,
}

impl Dashboard {
    pub fn empty(family_name: &str, date: NaiveDate) -> Self {
        Self {
            family_name: family_name.to_string(),
            weekday: date.format("%A").to_string(),
            date_long: date.format("%-d %B %Y").to_string(),
            date_iso: date.format("%Y-%m-%d").to_string(),
            events_today: Vec::new(),
            events_week: Vec::new(),
            todos: Vec::new(),
            rooms: Vec::new(),
            weather: Weather::default(),
            source_note: String::new(),
        }
    }

    /// Canonical payload hashed so an unchanged family day skips Chromium
    /// and the Pico can skip the panel refresh.
    pub fn content_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("dashboard json")
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FrameInfo {
    pub checksum: String,
    pub bytes: usize,
    pub generated_at: DateTime<chrono::Utc>,
    pub content_hash: String,
    pub source_note: String,
}

#[derive(Debug, Deserialize)]
pub struct FileTodo {
    pub title: String,
    #[serde(default)]
    pub done: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_title_is_unchanged() {
        assert_eq!(truncate_event_title("Household Waste and Recycling Centre"), "Household Waste and Recycling Centre");
    }

    #[test]
    fn long_title_is_one_line_with_ellipsis() {
        let title = "Your event was created from an email that you received in Gmail. https://mail.google.com/mail?extsrc=cal&plid=ACUX6DC00";
        let out = truncate_event_title(title);
        assert_eq!(out.chars().count(), EVENT_TITLE_MAX_CHARS);
        assert!(out.ends_with('…'));
        assert!(!out.contains("https://"));
        assert_eq!(
            truncate_event_title("Line one is already far too long for the today column on this panel\nLine two"),
            truncate_event_title("Line one is already far too long for the today column on this panel")
        );
    }
}

