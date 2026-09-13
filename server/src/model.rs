use chrono::{DateTime, NaiveDate};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CalendarEvent {
    pub start: String,
    pub title: String,
    pub who: String,
    pub all_day: bool,
    pub day_label: String,
    /// Local `YYYY-MM-DD` so Today / Coming next stay in chronological order.
    pub date: String,
}

/// Today/week title column is ~830px (1600 panel − padding − 460px sidebar −
/// 150px time − gaps) at 30px Noto Sans, ~15.5px per character → ~53 glyphs.
/// 48 leaves room for wide letters and the ellipsis.
pub const EVENT_TITLE_MAX_CHARS: usize = 48;

/// How far ahead to pull events for Coming next. Today stays in Today.
pub const EVENT_HORIZON_DAYS: i64 = 180;

/// Pixel budget for the stacked Today + Coming next column on the 1600×1200
/// panel. Keep in sync with `dashboard.css` (`.panel` padding/gaps, `.mast`,
/// `.weather`, `h2`, `li`, `.events { gap }`).
pub const EVENTS_COLUMN_PX: i32 = 780;
pub const SECTION_HEAD_PX: i32 = 52;
pub const EVENT_ROW_PX: i32 = 60;
/// Minimum gap between Today and Coming next. Extra leftover space is
/// absorbed above Coming next so that section sits on the column bottom.
pub const SECTION_GAP_PX: i32 = EVENT_ROW_PX;
pub const EMPTY_SECTION_BODY_PX: i32 = 54;

/// How many Coming next rows fit under Today on the 13.3″ panel.
pub fn coming_event_capacity(today_count: usize) -> usize {
    let today_body = if today_count == 0 {
        EMPTY_SECTION_BODY_PX
    } else {
        (today_count as i32).saturating_mul(EVENT_ROW_PX)
    };
    let leftover =
        EVENTS_COLUMN_PX - SECTION_HEAD_PX - today_body - SECTION_GAP_PX - SECTION_HEAD_PX;
    if leftover < EVENT_ROW_PX {
        0
    } else {
        (leftover / EVENT_ROW_PX) as usize
    }
}

/// Today is first, but always leave Coming next a heading plus one row.
pub fn max_today_events() -> usize {
    let reserved = SECTION_GAP_PX + SECTION_HEAD_PX + EVENT_ROW_PX;
    let body = EVENTS_COLUMN_PX - SECTION_HEAD_PX - reserved;
    (body / EVENT_ROW_PX) as usize
}

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
    pub events_coming: Vec<CalendarEvent>,
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
            events_coming: Vec::new(),
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

    /// Sort the calendar and keep only the rows that fit the panel.
    pub fn fit_calendar_to_panel(&mut self) {
        self.events_today
            .sort_by(|a, b| a.start.cmp(&b.start).then(a.title.cmp(&b.title)));
        self.events_coming.sort_by(|a, b| {
            a.date
                .cmp(&b.date)
                .then(a.start.cmp(&b.start))
                .then(a.title.cmp(&b.title))
        });
        self.events_today.truncate(max_today_events());
        let cap = coming_event_capacity(self.events_today.len());
        self.events_coming.truncate(cap);
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
        assert_eq!(
            truncate_event_title("Household Waste and Recycling Centre"),
            "Household Waste and Recycling Centre"
        );
    }

    #[test]
    fn coming_next_fills_space_left_after_today() {
        assert_eq!(coming_event_capacity(0), 9);
        assert_eq!(coming_event_capacity(1), 9);
        assert_eq!(coming_event_capacity(2), 8);
        assert_eq!(coming_event_capacity(8), 2);
        assert_eq!(max_today_events(), 9);
    }

    #[test]
    fn fit_calendar_keeps_today_and_truncates_coming_next() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 13).unwrap();
        let mut dash = Dashboard::empty("Family", today);
        dash.events_today = vec![event(today, "10:00", "Recycling")];
        dash.events_coming = (1..20)
            .map(|i| event(today + chrono::Duration::days(i), "16:00", "Later"))
            .collect();
        dash.fit_calendar_to_panel();
        assert_eq!(dash.events_today.len(), 1);
        assert_eq!(dash.events_coming.len(), 9);
        assert_eq!(dash.events_coming[0].date, "2026-09-14");
        assert_eq!(dash.events_coming[8].date, "2026-09-22");
    }

    fn event(date: NaiveDate, start: &str, title: &str) -> CalendarEvent {
        CalendarEvent {
            start: start.into(),
            title: title.into(),
            who: String::new(),
            all_day: false,
            day_label: date.format("%a %-d").to_string(),
            date: date.format("%Y-%m-%d").to_string(),
        }
    }

    #[test]
    fn long_title_is_one_line_with_ellipsis() {
        let title = "Your event was created from an email that you received in Gmail. https://mail.google.com/mail?extsrc=cal&plid=ACUX6DC00";
        let out = truncate_event_title(title);
        assert_eq!(out.chars().count(), EVENT_TITLE_MAX_CHARS);
        assert!(out.ends_with('…'));
        assert!(!out.contains("https://"));
        assert_eq!(
            truncate_event_title(
                "Line one is already far too long for the today column on this panel\nLine two"
            ),
            truncate_event_title(
                "Line one is already far too long for the today column on this panel"
            )
        );
    }
}
