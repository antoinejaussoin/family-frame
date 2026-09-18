use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
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
    /// Config birthdays, merged into Today / Coming next.
    #[serde(default)]
    pub birthday: bool,
}

/// Today/week title column is ~830px (1600 panel − padding − 460px sidebar −
/// 150px time − gaps) at 30px Noto Sans, ~15.5px per character → ~53 glyphs.
/// 48 leaves room for wide letters and the ellipsis.
pub const EVENT_TITLE_MAX_CHARS: usize = 48;

/// How far ahead to pull events for Coming next. Today stays in Today.
pub const EVENT_HORIZON_DAYS: i64 = 180;

/// Config birthdays are merged into the calendar this far ahead (inclusive).
pub const BIRTHDAY_HORIZON_DAYS: i64 = 14;

/// Pixel budget for the stacked Today + Coming next column on the 1600×1200
/// panel. Keep in sync with `dashboard.css` (`.panel` padding/gaps, `.mast`,
/// `h2`, `li`, `.events { gap }`). Weather sits in the section headers.
pub const EVENTS_COLUMN_PX: i32 = 990;
pub const SECTION_HEAD_PX: i32 = 52;
pub const EVENT_ROW_PX: i32 = 60;
/// Minimum gap between Today and Coming next. Extra leftover space is
/// absorbed above Coming next so that section sits on the column bottom.
pub const SECTION_GAP_PX: i32 = EVENT_ROW_PX;
pub const EMPTY_SECTION_BODY_PX: i32 = 54;

/// Sidebar column (same grid row as events). Keep in sync with
/// `dashboard.css` (`.panel` padding/gaps, `.mast`, `.sidebar { gap }`,
/// `h2`, `.todos li`, `.school-item`, `.tube-line`, `.rooms li`, `.todos-more`).
pub const SIDEBAR_PX: i32 = 1008;
pub const SIDEBAR_GAP_PX: i32 = 28;
pub const TUBE_ROW_PX: i32 = 44;
pub const SCHOOL_ROW_PX: i32 = 48;
pub const ROOM_ROW_PX: i32 = EVENT_ROW_PX;
/// Compact “+ N other todos” line under the pills (margin + height).
pub const TODOS_MORE_PX: i32 = 36;
/// Sidebar inner width (`.panel` `460px` column).
pub const TODO_PILL_MAX_PX: i32 = 460;
pub const TODO_PILL_PAD_X: i32 = 24;
pub const TODO_PILL_BORDER_X: i32 = 4;
/// Conservative Noto Sans width at 22px (same ~0.55em as event titles).
pub const TODO_PILL_CHAR_PX: i32 = 12;
pub const TODO_PILL_ROW_PX: i32 = 40;
pub const TODO_PILL_GAP_PX: i32 = 8;
pub const TODO_PILL_TOP_PX: i32 = 10;

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
    pub sunrise: String,
    pub sunset: String,
    pub pollen: String,
    /// CSS class: `low`, `moderate`, `high`, or empty when unknown.
    pub pollen_level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Weather {
    pub location: String,
    pub days: Vec<WeatherDay>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TubeLine {
    pub id: String,
    pub name: String,
    pub status: String,
    /// CSS class: `good`, `delay`, or `severe`.
    pub severity: String,
    /// Spectra 6 colour class: `black`, `yellow`, `green`, or `blue`.
    pub colour: String,
}

/// Pronote homework and recent grades for the School sidebar section.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct School {
    /// Child or student first name as Pronote shows it.
    #[serde(default)]
    pub student: String,
    /// Period average, for example `14.2`. Empty when Pronote has none.
    #[serde(default)]
    pub average: String,
    #[serde(default)]
    pub items: Vec<SchoolItem>,
}

impl School {
    pub fn is_visible(&self) -> bool {
        !self.items.is_empty() || !self.student.is_empty() || !self.average.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SchoolItem {
    /// `homework` or `grade`.
    pub kind: String,
    pub when: String,
    pub subject: String,
    pub detail: String,
    #[serde(default)]
    pub done: bool,
    /// CSS class for a grade: `high`, `mid`, `low`, or empty.
    #[serde(default)]
    pub level: String,
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
    /// Open tasks that did not fit under Tube + House. Shown as “+ N other todos”.
    #[serde(default)]
    pub todos_more: usize,
    pub rooms: Vec<RoomClimate>,
    pub weather: Weather,
    pub tube: Vec<TubeLine>,
    #[serde(default)]
    pub school: School,
    pub source_note: String,
    /// Pico has reported a battery reading. Hidden on the panel until then.
    #[serde(default)]
    pub has_battery: bool,
    #[serde(default)]
    pub battery_pct: u16,
    /// Spectra class: `ok` (green, ≥25%), `low` (red, <25%).
    #[serde(default)]
    pub battery_level: String,
    /// Local `HH:MM` when this bitmap was painted. Omitted from the layout hash.
    #[serde(default)]
    pub last_refresh: String,
    /// Local `HH:MM`, or `Day HH:MM` when the next poll is tomorrow.
    #[serde(default)]
    pub next_refresh: String,
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
            todos_more: 0,
            rooms: Vec::new(),
            weather: Weather::default(),
            tube: Vec::new(),
            school: School::default(),
            source_note: String::new(),
            has_battery: false,
            battery_pct: 0,
            battery_level: String::new(),
            last_refresh: String::new(),
            next_refresh: String::new(),
        }
    }

    pub fn set_battery(&mut self, pct: u16) {
        let pct = pct.min(100);
        self.has_battery = true;
        self.battery_pct = pct;
        self.battery_level = battery_level(pct).into();
    }

    pub fn set_refresh_window(&mut self, now: DateTime<Utc>, next_secs: u64, tz: Tz) {
        let local = now.with_timezone(&tz);
        self.last_refresh = local.format("%H:%M").to_string();
        let next_local = refresh_until_at(now, next_secs).with_timezone(&tz);
        self.next_refresh = if next_local.date_naive() == local.date_naive() {
            next_local.format("%H:%M").to_string()
        } else {
            next_local.format("%a %H:%M").to_string()
        };
    }

    /// Drop last/next times so a painted clock does not force Chromium
    /// *inside* the current poll window. Battery stays — a drop should
    /// be allowed to wake the panel. The bitmap is still discarded once
    /// [`refresh_until_at`] elapses so the header can advance.
    pub fn for_layout_hash(&self) -> Self {
        let mut hashed = self.clone();
        hashed.last_refresh.clear();
        hashed.next_refresh.clear();
        hashed
    }

    /// Canonical payload hashed so an unchanged family day skips Chromium
    /// and the Pico can skip the panel refresh.
    pub fn content_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(&self.for_layout_hash()).expect("dashboard json")
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

    /// Keep School, Tube, and House in full; fill leftover height with to-do pills.
    pub fn fit_sidebar_to_panel(&mut self) {
        let school_px = if self.school.is_visible() {
            sidebar_block_px(self.school.items.len(), SCHOOL_ROW_PX)
        } else {
            0
        };
        let gaps = if self.school.is_visible() { 3 } else { 2 };
        let remaining = SIDEBAR_PX
            - school_px
            - sidebar_block_px(self.tube.len(), TUBE_ROW_PX)
            - sidebar_block_px(self.rooms.len(), ROOM_ROW_PX)
            - SIDEBAR_GAP_PX * gaps;
        let total = self.todos.len();
        if total == 0 {
            self.todos_more = 0;
            return;
        }
        let body = remaining - SECTION_HEAD_PX;
        if todos_fitting_in(&self.todos, body) == total {
            self.todos_more = 0;
            return;
        }
        let shown =
            todos_fitting_in(&self.todos, body - TODOS_MORE_PX).min(total.saturating_sub(1));
        self.todos_more = total - shown;
        self.todos.truncate(shown);
    }

    pub fn fit_to_panel(&mut self) {
        self.fit_calendar_to_panel();
        self.fit_sidebar_to_panel();
    }
}

/// Instant the painted `next_refresh` refers to (`now + next_secs`).
pub fn refresh_until_at(now: DateTime<Utc>, next_secs: u64) -> DateTime<Utc> {
    let secs = i64::try_from(next_secs).unwrap_or(i64::MAX);
    now + chrono::Duration::seconds(secs)
}

fn sidebar_block_px(rows: usize, row_px: i32) -> i32 {
    SECTION_HEAD_PX
        + if rows == 0 {
            EMPTY_SECTION_BODY_PX
        } else {
            (rows as i32).saturating_mul(row_px)
        }
}

pub fn battery_level(pct: u16) -> &'static str {
    if pct < 25 {
        "low"
    } else {
        "ok"
    }
}

fn todo_pill_width(title: &str) -> i32 {
    let text = (title.chars().count() as i32).saturating_mul(TODO_PILL_CHAR_PX);
    (TODO_PILL_PAD_X + TODO_PILL_BORDER_X + text).clamp(1, TODO_PILL_MAX_PX)
}

/// How many leading to-dos wrap into `body_px` below the section heading.
fn todos_fitting_in(todos: &[TodoItem], body_px: i32) -> usize {
    if body_px < TODO_PILL_TOP_PX + TODO_PILL_ROW_PX {
        return 0;
    }
    let mut rows = 0i32;
    let mut x = 0i32;
    let mut shown = 0usize;
    for todo in todos {
        let w = todo_pill_width(&todo.title);
        let new_row = rows == 0 || x + TODO_PILL_GAP_PX + w > TODO_PILL_MAX_PX;
        if new_row {
            let next_rows = rows + 1;
            let height = TODO_PILL_TOP_PX + next_rows * TODO_PILL_ROW_PX + rows * TODO_PILL_GAP_PX;
            if height > body_px {
                break;
            }
            rows = next_rows;
            x = w;
        } else {
            x += TODO_PILL_GAP_PX + w;
        }
        shown += 1;
    }
    shown
}

#[derive(Debug, Clone, Serialize)]
pub struct FrameInfo {
    pub checksum: String,
    pub bytes: usize,
    pub generated_at: DateTime<chrono::Utc>,
    pub content_hash: String,
    pub source_note: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn short_title_is_unchanged() {
        assert_eq!(
            truncate_event_title("Household Waste and Recycling Centre"),
            "Household Waste and Recycling Centre"
        );
    }

    #[test]
    fn coming_next_fills_space_left_after_today() {
        assert_eq!(coming_event_capacity(0), 12);
        assert_eq!(coming_event_capacity(1), 12);
        assert_eq!(coming_event_capacity(2), 11);
        assert_eq!(coming_event_capacity(8), 5);
        assert_eq!(max_today_events(), 12);
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
        assert_eq!(dash.events_coming.len(), 12);
        assert_eq!(dash.events_coming[0].date, "2026-09-14");
        assert_eq!(dash.events_coming[11].date, "2026-09-25");
    }

    fn todo(title: &str) -> TodoItem {
        TodoItem {
            title: title.into(),
            done: false,
        }
    }

    fn room(name: &str) -> RoomClimate {
        RoomClimate {
            name: name.into(),
            temperature: "20°".into(),
            humidity: "50%".into(),
            online: true,
        }
    }

    fn tube_lines(n: usize) -> Vec<TubeLine> {
        (0..n)
            .map(|i| TubeLine {
                id: format!("l{i}"),
                name: format!("Line {i}"),
                status: "Good service".into(),
                severity: "good".into(),
                colour: "black".into(),
            })
            .collect()
    }

    #[test]
    fn sidebar_keeps_tube_and_house_and_trims_todo_pills() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 13).unwrap();
        let mut dash = Dashboard::empty("Family", today);
        dash.tube = tube_lines(4);
        dash.rooms = ["A", "B", "C", "D", "E"].into_iter().map(room).collect();
        dash.todos = (0..50).map(|_| todo("Milk")).collect();
        dash.fit_sidebar_to_panel();
        assert_eq!(dash.tube.len(), 4);
        assert_eq!(dash.rooms.len(), 5);
        assert!(
            dash.todos.len() > 4,
            "pills should beat one-per-line packing"
        );
        assert!(dash.todos_more > 0);
        assert_eq!(dash.todos.len() + dash.todos_more, 50);
    }

    #[test]
    fn sidebar_short_todos_share_a_row() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 13).unwrap();
        let mut dash = Dashboard::empty("Family", today);
        dash.tube = tube_lines(4);
        dash.rooms = ["Kitchen"].into_iter().map(room).collect();
        dash.todos = (0..8).map(|_| todo("Ok")).collect();
        dash.fit_sidebar_to_panel();
        assert_eq!(dash.todos.len(), 8);
        assert_eq!(dash.todos_more, 0);
    }

    #[test]
    fn sidebar_shows_only_more_line_when_no_todo_row_fits() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 13).unwrap();
        let mut dash = Dashboard::empty("Family", today);
        dash.tube = tube_lines(4);
        dash.rooms = (0..12).map(|i| room(&format!("R{i}"))).collect();
        dash.todos = (0..5).map(|i| todo(&format!("Task {i}"))).collect();
        dash.fit_sidebar_to_panel();
        assert_eq!(dash.rooms.len(), 12);
        assert_eq!(dash.tube.len(), 4);
        assert!(dash.todos.is_empty());
        assert_eq!(dash.todos_more, 5);
    }

    #[test]
    fn sidebar_keeps_school_tube_and_house() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 13).unwrap();
        let mut dash = Dashboard::empty("Family", today);
        dash.school = School {
            student: "Léa".into(),
            average: "14.2".into(),
            items: vec![
                SchoolItem {
                    kind: "homework".into(),
                    when: "Today".into(),
                    subject: "Maths".into(),
                    detail: "p.24".into(),
                    done: false,
                    level: String::new(),
                },
                SchoolItem {
                    kind: "grade".into(),
                    when: "Fri".into(),
                    subject: "French".into(),
                    detail: "15/20".into(),
                    done: false,
                    level: "high".into(),
                },
            ],
        };
        dash.tube = tube_lines(4);
        dash.rooms = ["Kitchen"].into_iter().map(room).collect();
        dash.todos = (0..40).map(|_| todo("Milk")).collect();
        dash.fit_sidebar_to_panel();
        assert_eq!(dash.school.items.len(), 2);
        assert_eq!(dash.tube.len(), 4);
        assert_eq!(dash.rooms.len(), 1);
        assert!(dash.todos_more > 0);
    }

    #[test]
    fn sidebar_keeps_all_todos_when_they_fit() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 13).unwrap();
        let mut dash = Dashboard::empty("Family", today);
        dash.tube = tube_lines(4);
        dash.rooms = ["Kitchen"].into_iter().map(room).collect();
        dash.todos = vec![todo("One"), todo("Two")];
        dash.fit_sidebar_to_panel();
        assert_eq!(dash.todos.len(), 2);
        assert_eq!(dash.todos_more, 0);
    }

    #[test]
    fn battery_level_thresholds() {
        assert_eq!(battery_level(100), "ok");
        assert_eq!(battery_level(25), "ok");
        assert_eq!(battery_level(24), "low");
        assert_eq!(battery_level(0), "low");
    }

    #[test]
    fn refresh_times_are_omitted_from_the_layout_hash() {
        let mut dash = Dashboard::empty("Family", NaiveDate::from_ymd_opt(2026, 9, 17).unwrap());
        dash.set_battery(62);
        dash.last_refresh = "17:53".into();
        dash.next_refresh = "18:53".into();
        let hashed = dash.for_layout_hash();
        assert!(hashed.has_battery);
        assert_eq!(hashed.battery_pct, 62);
        assert!(hashed.last_refresh.is_empty());
        assert!(hashed.next_refresh.is_empty());
        let mut later = dash.clone();
        later.last_refresh = "18:00".into();
        later.next_refresh = "19:00".into();
        assert_eq!(dash.content_bytes(), later.content_bytes());
        later.set_battery(61);
        assert_ne!(dash.content_bytes(), later.content_bytes());
    }

    #[test]
    fn refresh_until_matches_next_refresh() {
        let tz = chrono_tz::Europe::London;
        let now = tz
            .with_ymd_and_hms(2026, 9, 17, 20, 54, 0)
            .unwrap()
            .with_timezone(&Utc);
        let until = refresh_until_at(now, 300);
        assert_eq!(
            until.with_timezone(&tz).format("%H:%M").to_string(),
            "20:59"
        );
        assert!(now < until);
    }

    #[test]
    fn refresh_window_same_day_is_hhmm() {
        let tz = chrono_tz::Europe::London;
        let now = tz
            .with_ymd_and_hms(2026, 9, 17, 17, 53, 0)
            .unwrap()
            .with_timezone(&Utc);
        let mut dash = Dashboard::empty("Family", now.date_naive());
        dash.set_refresh_window(now, 300, tz);
        assert_eq!(dash.last_refresh, "17:53");
        assert_eq!(dash.next_refresh, "17:58");
    }

    #[test]
    fn refresh_window_next_day_includes_weekday() {
        let tz = chrono_tz::Europe::London;
        let now = tz
            .with_ymd_and_hms(2026, 9, 17, 23, 50, 0)
            .unwrap()
            .with_timezone(&Utc);
        let mut dash = Dashboard::empty("Family", now.date_naive());
        dash.set_refresh_window(now, 20 * 60, tz);
        assert_eq!(dash.last_refresh, "23:50");
        assert_eq!(dash.next_refresh, "Fri 00:10");
    }

    fn event(date: NaiveDate, start: &str, title: &str) -> CalendarEvent {
        CalendarEvent {
            start: start.into(),
            title: title.into(),
            who: String::new(),
            all_day: false,
            day_label: date.format("%a %-d").to_string(),
            date: date.format("%Y-%m-%d").to_string(),
            birthday: false,
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
