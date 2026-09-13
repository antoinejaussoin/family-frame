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

