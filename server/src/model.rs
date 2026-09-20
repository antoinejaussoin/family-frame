use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CalendarEvent {
    pub start: String,
    pub title: String,
    pub all_day: bool,
    pub day_label: String,
    /// Local `YYYY-MM-DD` so Today / Coming next stay in chronological order.
    pub date: String,
    /// Config birthdays, merged into Today / Coming next.
    #[serde(default)]
    pub birthday: bool,
    /// Pronote school-day hours, merged into Today and the next school day.
    #[serde(default)]
    pub school: bool,
    /// ICS series (`RRULE` / `RDATE` / `RECURRENCE-ID`). One-off family events
    /// stay `false` so the time column can use a different fill.
    #[serde(default)]
    pub recurring: bool,
    /// Wandsworth food & recycling, merged into one calendar row.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bin: bool,
}

/// Today/week title column is ~570px (half of 1600 − padding − time − gaps)
/// at 33px Atkinson, ~17px per character → ~33 glyphs.
pub const EVENT_TITLE_MAX_CHARS: usize = 33;

/// How far ahead to pull events for Coming next. The focused day stays
/// in the primary list (`Today`, or tomorrow after the evening rollover).
pub const EVENT_HORIZON_DAYS: i64 = 180;

fn default_today_title() -> String {
    "Today".into()
}

/// Config birthdays are merged into the calendar this far ahead (inclusive).
pub const BIRTHDAY_HORIZON_DAYS: i64 = 14;

/// Pixel budget for the stacked Today + Coming next column on the 1600×1200
/// panel. Keep in sync with `dashboard.css` (`.panel` padding/gaps, `.mast`,
/// `h2`, `li`, `.events { gap }`). Weather sits in the section headers.
pub const EVENTS_COLUMN_PX: i32 = 990;
pub const SECTION_HEAD_PX: i32 = 48;
pub const EVENT_ROW_PX: i32 = 60;
/// Minimum gap between Today and Coming next. Extra leftover space is
/// absorbed above Coming next so that section sits on the column bottom.
pub const SECTION_GAP_PX: i32 = EVENT_ROW_PX;
pub const EMPTY_SECTION_BODY_PX: i32 = 54;

/// Right-hand columns (same grid row as events). Keep in sync with
/// `dashboard.css` (`.panel` padding/gaps, `.mast`, homework/grades/todos/joke/week/history/tube/rooms,
/// `h2`, `.todos li`, `.joke`, `.school-item`, `.week-grid`, `.tube-line`, `.rooms li`, `.todos-more`).
pub const SIDEBAR_PX: i32 = 1008;
pub const SIDEBAR_GAP_PX: i32 = 28;
pub const TUBE_ROW_PX: i32 = 44;
pub const COMING_ROW_PX: i32 = TUBE_ROW_PX;
pub const SCHOOL_ROW_PX: i32 = 44;
pub const ROOM_ROW_PX: i32 = TUBE_ROW_PX;
/// Wikipedia pool; the panel then keeps only facts that fit leftover height.
pub const HISTORY_POOL: usize = 12;
pub const MAX_HISTORY_FACTS: usize = 5;
pub const HISTORY_MAX_LINES: usize = 3;
/// 16px TRMNL16 at `line-height: 20px`.
pub const HISTORY_LINE_PX: i32 = 20;
pub const HISTORY_ITEM_PAD_Y: i32 = 12;
pub const HISTORY_ITEM_BORDER_PX: i32 = 2;
pub const HISTORY_YEAR_PX: i32 = 92;
pub const HISTORY_TEXT_GAP_PX: i32 = 12;
/// Conservative 16px TRMNL16 (~0.6em). Prefer skipping a fact to clipping.
pub const HISTORY_CHAR_PX: i32 = 10;
pub const MAX_HOMEWORK_ROWS: usize = 8;
pub const MAX_GRADE_ROWS: usize = 8;
/// Compact “+ N other todos” line under the pills (margin + height).
pub const TODOS_MORE_PX: i32 = 36;
/// One quarter of the 1600px panel (sidebar half of a 2×2).
pub const SIDEBAR_QUARTER_PX: i32 = 362;
/// Full sidebar inner width (two quarters + the 24px pair gap).
pub const SIDEBAR_INNER_PX: i32 = SIDEBAR_QUARTER_PX * 2 + 24;
pub const TODO_PILL_PAD_X: i32 = 24;
pub const TODO_PILL_BORDER_X: i32 = 4;
/// Conservative 21px TRMNL21 bold (~0.65em).
pub const TODO_PILL_CHAR_PX: i32 = 15;
pub const TODO_PILL_ROW_PX: i32 = 37;
pub const TODO_PILL_GAP_PX: i32 = 8;
pub const TODO_PILL_TOP_PX: i32 = 10;
pub const HISTORY_TEXT_MAX_PX: i32 = SIDEBAR_INNER_PX - HISTORY_YEAR_PX - HISTORY_TEXT_GAP_PX;
/// 21px TRMNL21 at `line-height: 26px`. Prefer skipping a joke to clipping.
pub const JOKE_LINE_PX: i32 = 26;
pub const JOKE_PAD_TOP_PX: i32 = 10;
pub const JOKE_PUNCH_GAP_PX: i32 = 4;
pub const JOKE_MAX_LINES: usize = 6;
pub const JOKE_CHAR_PX: i32 = TODO_PILL_CHAR_PX;
pub const JOKE_TEXT_MAX_PX: i32 = SIDEBAR_QUARTER_PX;
/// Pronote week grid (`.week-grid` margin-top, day label, one row per time band).
pub const WEEK_GRID_TOP_PX: i32 = 8;
pub const WEEK_DAY_LABEL_PX: i32 = 22;
pub const WEEK_SLOT_PX: i32 = 18;

/// How many Coming next rows fit under Today on the 13.3″ panel.
pub fn coming_event_capacity(today_count: usize) -> usize {
    let today_body = if today_count == 0 {
        EMPTY_SECTION_BODY_PX
    } else {
        (today_count as i32).saturating_mul(EVENT_ROW_PX)
    };
    let leftover =
        EVENTS_COLUMN_PX - SECTION_HEAD_PX - today_body - SECTION_GAP_PX - SECTION_HEAD_PX;
    if leftover < COMING_ROW_PX {
        0
    } else {
        (leftover / COMING_ROW_PX) as usize
    }
}

/// Today is first, but always leave Coming next a heading plus one row.
pub fn max_today_events() -> usize {
    let reserved = SECTION_GAP_PX + SECTION_HEAD_PX + COMING_ROW_PX;
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
pub struct StatusLine {
    pub id: String,
    pub name: String,
    pub status: String,
    /// CSS class: `good`, `delay`, or `severe`.
    pub severity: String,
    /// Spectra 6 colour class: `black`, `yellow`, `green`, or `blue`.
    pub colour: String,
}

/// Back-compat name; TfL is the only status-line source today.
pub type TubeLine = StatusLine;

/// Pronote homework and recent grades for the two school quarter-sections.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct School {
    /// Child or student first name as Pronote shows it.
    #[serde(default)]
    pub student: String,
    /// Period average, for example `14.2`. Empty when Pronote has none.
    #[serde(default)]
    pub average: String,
    #[serde(default)]
    pub homework: Vec<SchoolItem>,
    #[serde(default)]
    pub grades: Vec<SchoolItem>,
    /// First and last lesson today and on the next school day.
    #[serde(default)]
    pub days: Vec<SchoolDay>,
    /// Monday–Friday timetable for the sidebar week grid.
    #[serde(default)]
    pub week: SchoolWeek,
}

impl School {
    pub fn is_visible(&self) -> bool {
        !self.homework.is_empty()
            || !self.grades.is_empty()
            || !self.student.is_empty()
            || !self.average.is_empty()
            || !self.days.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SchoolDay {
    pub date: String,
    pub start: String,
    pub end: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SchoolWeek {
    /// `School - This week`, or `School - Next week` on Saturday and Sunday.
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub days: Vec<SchoolWeekDay>,
    /// Unique lesson start times for the left-hand axis.
    #[serde(default)]
    pub times: Vec<SchoolWeekTime>,
    /// One row per start time. Keep in sync with CSS
    /// `repeat(var(--week-bands), 18px)`.
    #[serde(default)]
    pub bands: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SchoolWeekTime {
    pub label: String,
    /// CSS `grid-row` (day headers occupy row 1).
    pub row: i32,
    /// Unused: the axis shows start times only.
    #[serde(default)]
    pub end: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SchoolWeekDay {
    pub label: String,
    #[serde(default)]
    pub today: bool,
    /// CSS `grid-column` (time axis is column 1).
    #[serde(default)]
    pub col: i32,
    #[serde(default)]
    pub lessons: Vec<SchoolLesson>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SchoolLesson {
    pub subject: String,
    /// Pronote `CouleurFond` hex, darkened for the chip background.
    #[serde(default)]
    pub colour: String,
    /// Always `#ffffff` so the subject stays readable on the darkened chip.
    #[serde(default)]
    pub ink: String,
    /// Inclusive CSS `grid-row` start (day headers occupy row 1).
    #[serde(default)]
    pub row_start: i32,
    /// Exclusive CSS `grid-row` end.
    #[serde(default)]
    pub row_end: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SchoolItem {
    pub when: String,
    pub subject: String,
    /// Grade mark, for example `15.5/20`. Empty on homework rows.
    #[serde(default)]
    pub detail: String,
    /// CSS class for a grade: `high`, `mid`, `low`, or empty.
    #[serde(default)]
    pub level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoryFact {
    pub year: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Joke {
    pub setup: String,
    #[serde(default)]
    pub punchline: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Dashboard {
    pub family_name: String,
    pub weekday: String,
    pub day: String,
    pub month: String,
    pub date_long: String,
    pub date_iso: String,
    /// `Saint` / `Sainte` / `Saints`, empty for named feasts (Noël, Toussaint).
    #[serde(default)]
    pub saint_title: String,
    #[serde(default)]
    pub saint_name: String,
    /// Primary calendar heading: `Today`, or `Tomorrow` after 18:00
    /// when nothing later remains on the calendar day.
    #[serde(default = "default_today_title")]
    pub today_title: String,
    /// Day ordinal painted in red after `Tomorrow` (`20th`). Empty on Today.
    #[serde(default)]
    pub today_ordinal: String,
    pub events_today: Vec<CalendarEvent>,
    pub events_coming: Vec<CalendarEvent>,
    pub todos: Vec<TodoItem>,
    /// Open tasks that did not fit under Tube + House. Shown as “+ N other todos”.
    #[serde(default)]
    pub todos_more: usize,
    pub rooms: Vec<RoomClimate>,
    pub weather: Weather,
    pub tube: Vec<StatusLine>,
    #[serde(default)]
    pub history: Vec<HistoryFact>,
    #[serde(default)]
    pub joke: Option<Joke>,
    #[serde(default)]
    pub school: School,
    pub source_note: String,
    /// Homework / grades columns. Skipped in the layout hash JSON.
    #[serde(default, skip)]
    pub show_school_sections: bool,
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
        let saint = crate::sources::saints::of_date(date);
        Self {
            family_name: family_name.to_string(),
            weekday: date.format("%A").to_string(),
            day: date.format("%-d").to_string(),
            month: date.format("%B").to_string(),
            date_long: date.format("%-d %B").to_string(),
            date_iso: date.format("%Y-%m-%d").to_string(),
            saint_title: saint.title.to_string(),
            saint_name: saint.name.to_string(),
            today_title: default_today_title(),
            today_ordinal: String::new(),
            events_today: Vec::new(),
            events_coming: Vec::new(),
            todos: Vec::new(),
            todos_more: 0,
            rooms: Vec::new(),
            weather: Weather::default(),
            tube: Vec::new(),
            history: Vec::new(),
            joke: None,
            school: School::default(),
            source_note: String::new(),
            show_school_sections: false,
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
        self.set_refresh_at(now, refresh_until_at(now, next_secs), tz);
    }

    /// Paint last/next using the arrival time and the real next slot instant
    /// (so 18:00 stays 18:00).
    pub fn set_refresh_at(&mut self, last: DateTime<Utc>, next: DateTime<Utc>, tz: Tz) {
        let local = last.with_timezone(&tz);
        self.last_refresh = local.format("%H:%M").to_string();
        let next_local = next.with_timezone(&tz);
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
        Self::hide_school_sections_from_hash(&mut hashed);
        hashed
    }

    /// Canonical payload hashed so an unchanged family day skips Chromium
    /// and the Pico can skip the panel refresh.
    pub fn content_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(&self.for_layout_hash()).expect("dashboard json")
    }

    fn hide_school_sections_from_hash(hashed: &mut Self) {
        if hashed.show_school_sections {
            return;
        }
        hashed.school.homework.clear();
        hashed.school.grades.clear();
        hashed.school.average.clear();
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

    /// Keep Tube and House in full on the bottom row. To do and Joke share
    /// a quarter-column row. On this day sits above the school week; leftover
    /// height is packed with facts (up to three wrapped lines each).
    pub fn fit_sidebar_to_panel(&mut self) {
        self.fit_sidebar(self.show_school_sections && self.school.is_visible());
    }

    fn fit_sidebar(&mut self, school_on: bool) {
        let footer_px = sidebar_block_px(self.tube.len(), TUBE_ROW_PX)
            .max(sidebar_block_px(self.rooms.len(), ROOM_ROW_PX));
        if self
            .joke
            .as_ref()
            .is_some_and(|joke| joke_body_px(joke).is_none())
        {
            self.joke = None;
        }
        let week_on = !self.school.week.days.is_empty();
        let week_px = if week_on {
            week_block_px(&self.school.week)
        } else {
            0
        };
        let history_floor = if self.history.is_empty() {
            0
        } else {
            SECTION_HEAD_PX + HISTORY_ITEM_PAD_Y + HISTORY_LINE_PX + HISTORY_ITEM_BORDER_PX
        };
        let gaps = |history_on: bool| {
            2 + i32::from(school_on) + i32::from(week_on) + i32::from(history_on) - 1
        };

        let mut joke_px = joke_block_px(&self.joke);
        let pair_min = SECTION_HEAD_PX
            + if self.todos.is_empty() {
                EMPTY_SECTION_BODY_PX
            } else {
                TODO_PILL_TOP_PX + TODO_PILL_ROW_PX
            };
        let pair_reserve = pair_min.max(joke_px);

        if school_on {
            let school_budget = (SIDEBAR_PX
                - footer_px
                - pair_reserve
                - week_px
                - history_floor
                - SIDEBAR_GAP_PX * gaps(history_floor > 0))
            .max(0);
            let cap = max_rows_in(school_budget, SCHOOL_ROW_PX);
            self.school.homework.truncate(cap.min(MAX_HOMEWORK_ROWS));
            self.school.grades.truncate(cap.min(MAX_GRADE_ROWS));
        }

        let school_row = if school_on {
            sidebar_block_px(self.school.homework.len(), SCHOOL_ROW_PX)
                .max(sidebar_block_px(self.school.grades.len(), SCHOOL_ROW_PX))
        } else {
            0
        };
        let pair_budget = (SIDEBAR_PX
            - school_row
            - footer_px
            - week_px
            - history_floor
            - SIDEBAR_GAP_PX * gaps(history_floor > 0))
        .max(0);
        if joke_px > pair_budget {
            self.joke = None;
            joke_px = 0;
        }
        let wrap_px = if self.joke.is_some() {
            SIDEBAR_QUARTER_PX
        } else {
            SIDEBAR_INNER_PX
        };

        let total = self.todos.len();
        if total == 0 {
            self.todos_more = 0;
        } else {
            let body = pair_budget - SECTION_HEAD_PX;
            if todos_fitting_in(&self.todos, body, wrap_px) == total {
                self.todos_more = 0;
            } else {
                let shown = todos_fitting_in(&self.todos, body - TODOS_MORE_PX, wrap_px)
                    .min(total.saturating_sub(1));
                self.todos_more = total - shown;
                self.todos.truncate(shown);
            }
        }

        let pair_px = todos_block_px(&self.todos, self.todos_more, wrap_px).max(joke_px);
        let history_budget = SIDEBAR_PX
            - school_row
            - pair_px
            - week_px
            - footer_px
            - SIDEBAR_GAP_PX * gaps(history_floor > 0);
        self.history = if history_floor > 0 {
            pack_history(&self.history, history_budget)
        } else {
            Vec::new()
        };
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

fn max_rows_in(section_px: i32, row_px: i32) -> usize {
    let body = section_px - SECTION_HEAD_PX;
    if body < row_px {
        0
    } else {
        (body / row_px) as usize
    }
}

pub fn battery_level(pct: u16) -> &'static str {
    if pct < 25 {
        "low"
    } else {
        "ok"
    }
}

fn todo_pill_width(title: &str, wrap_px: i32) -> i32 {
    let text = (title.chars().count() as i32).saturating_mul(TODO_PILL_CHAR_PX);
    (TODO_PILL_PAD_X + TODO_PILL_BORDER_X + text).clamp(1, wrap_px)
}

/// How many leading to-dos wrap into `body_px` below the section heading.
fn todos_fitting_in(todos: &[TodoItem], body_px: i32, wrap_px: i32) -> usize {
    if body_px < TODO_PILL_TOP_PX + TODO_PILL_ROW_PX {
        return 0;
    }
    let mut rows = 0i32;
    let mut x = 0i32;
    let mut shown = 0usize;
    for todo in todos {
        let w = todo_pill_width(&todo.title, wrap_px);
        let new_row = rows == 0 || x + TODO_PILL_GAP_PX + w > wrap_px;
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

fn todos_block_px(todos: &[TodoItem], more: usize, wrap_px: i32) -> i32 {
    SECTION_HEAD_PX + todos_body_px(todos, wrap_px) + if more > 0 { TODOS_MORE_PX } else { 0 }
}

fn todos_body_px(todos: &[TodoItem], wrap_px: i32) -> i32 {
    if todos.is_empty() {
        return EMPTY_SECTION_BODY_PX;
    }
    let mut rows = 0i32;
    let mut x = 0i32;
    for todo in todos {
        let w = todo_pill_width(&todo.title, wrap_px);
        let new_row = rows == 0 || x + TODO_PILL_GAP_PX + w > wrap_px;
        if new_row {
            rows += 1;
            x = w;
        } else {
            x += TODO_PILL_GAP_PX + w;
        }
    }
    TODO_PILL_TOP_PX + rows * TODO_PILL_ROW_PX + (rows - 1).max(0) * TODO_PILL_GAP_PX
}

fn pack_history(facts: &[HistoryFact], section_px: i32) -> Vec<HistoryFact> {
    let body = section_px - SECTION_HEAD_PX;
    if body <= 0 {
        return Vec::new();
    }
    let mut facts = facts.to_vec();
    facts.sort_by_key(|fact| history_year_sort_key(&fact.year));
    let mut used = 0i32;
    let mut out = Vec::new();
    for fact in facts {
        if out.len() == MAX_HISTORY_FACTS {
            break;
        }
        let Some(height) = history_item_px(&fact.text) else {
            continue;
        };
        if used + height > body {
            continue;
        }
        used += height;
        out.push(fact);
    }
    out
}

fn history_year_sort_key(year: &str) -> i32 {
    if let Some(bc) = year.strip_suffix(" BC") {
        return -bc.parse::<i32>().unwrap_or(0);
    }
    year.parse().unwrap_or(0)
}

fn history_item_px(text: &str) -> Option<i32> {
    let lines = wrap_line_count(text, HISTORY_TEXT_MAX_PX, HISTORY_CHAR_PX);
    if lines == 0 || lines > HISTORY_MAX_LINES {
        return None;
    }
    Some(HISTORY_ITEM_PAD_Y + (lines as i32) * HISTORY_LINE_PX + HISTORY_ITEM_BORDER_PX)
}

fn joke_body_px(joke: &Joke) -> Option<i32> {
    let setup_lines = wrap_line_count(&joke.setup, JOKE_TEXT_MAX_PX, JOKE_CHAR_PX);
    let punch_lines = if joke.punchline.is_empty() {
        0
    } else {
        wrap_line_count(&joke.punchline, JOKE_TEXT_MAX_PX, JOKE_CHAR_PX)
    };
    let lines = setup_lines + punch_lines;
    if lines == 0 || lines > JOKE_MAX_LINES {
        return None;
    }
    let gap = if punch_lines > 0 {
        JOKE_PUNCH_GAP_PX
    } else {
        0
    };
    Some(JOKE_PAD_TOP_PX + (lines as i32) * JOKE_LINE_PX + gap)
}

pub fn joke_fits_panel(joke: &Joke) -> bool {
    joke_body_px(joke).is_some()
}

fn joke_block_px(joke: &Option<Joke>) -> i32 {
    match joke {
        Some(j) => SECTION_HEAD_PX + joke_body_px(j).unwrap_or(0),
        None => 0,
    }
}

fn week_block_px(week: &SchoolWeek) -> i32 {
    if week.days.is_empty() {
        return 0;
    }
    let bands = if week.bands > 0 {
        week.bands
    } else {
        week.times.len() as i32
    };
    SECTION_HEAD_PX + WEEK_GRID_TOP_PX + WEEK_DAY_LABEL_PX + bands * WEEK_SLOT_PX
}

fn wrap_line_count(text: &str, max_px: i32, char_px: i32) -> usize {
    if text.is_empty() || max_px <= 0 || char_px <= 0 {
        return 0;
    }
    let mut lines = 1usize;
    let mut x = 0i32;
    for word in text.split_whitespace() {
        let w = (word.chars().count() as i32)
            .saturating_mul(char_px)
            .max(char_px);
        if w > max_px {
            if x > 0 {
                lines += 1;
            }
            let chunks = ((w + max_px - 1) / max_px) as usize;
            lines += chunks.saturating_sub(1);
            x = w % max_px;
            continue;
        }
        let need = if x == 0 { w } else { char_px + w };
        if x > 0 && x + need > max_px {
            lines += 1;
            x = w;
        } else {
            x += need;
        }
    }
    lines
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
    fn mast_date_is_weekday_day_month_without_year() {
        let dash = Dashboard::empty("Family", NaiveDate::from_ymd_opt(2026, 9, 18).unwrap());
        assert_eq!(dash.weekday, "Friday");
        assert_eq!(dash.day, "18");
        assert_eq!(dash.month, "September");
        assert_eq!(dash.date_long, "18 September");
        assert_eq!(dash.saint_title, "Sainte");
        assert_eq!(dash.saint_name, "Nadège");
    }

    #[test]
    fn short_title_is_unchanged() {
        assert_eq!(
            truncate_event_title("Household waste collection"),
            "Household waste collection"
        );
        // 33 glyphs — fits the Today column at 33px Atkinson.
        assert_eq!(
            truncate_event_title("Household Waste and Recycling Cen"),
            "Household Waste and Recycling Cen"
        );
    }

    #[test]
    fn coming_next_fills_space_left_after_today() {
        assert_eq!(coming_event_capacity(0), 17);
        assert_eq!(coming_event_capacity(1), 17);
        assert_eq!(coming_event_capacity(2), 16);
        assert_eq!(coming_event_capacity(8), 8);
        assert_eq!(max_today_events(), 13);
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
        assert_eq!(dash.events_coming.len(), 17);
        assert_eq!(dash.events_coming[0].date, "2026-09-14");
        assert_eq!(dash.events_coming[16].date, "2026-09-30");
    }

    fn todo(title: &str) -> TodoItem {
        TodoItem {
            title: title.into(),
            done: false,
        }
    }

    fn history_fact(year: &str, text: &str) -> HistoryFact {
        HistoryFact {
            year: year.into(),
            text: text.into(),
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
            .map(|i| StatusLine {
                id: format!("l{i}"),
                name: format!("Line {i}"),
                status: "Good service".into(),
                severity: "good".into(),
                colour: "black".into(),
            })
            .collect()
    }

    fn school_hw(when: &str, subject: &str) -> SchoolItem {
        SchoolItem {
            when: when.into(),
            subject: subject.into(),
            detail: String::new(),
            level: String::new(),
        }
    }

    fn school_grade(when: &str, subject: &str, detail: &str, level: &str) -> SchoolItem {
        SchoolItem {
            when: when.into(),
            subject: subject.into(),
            detail: detail.into(),
            level: level.into(),
        }
    }

    #[test]
    fn sidebar_keeps_tube_and_house_and_trims_todo_pills() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 13).unwrap();
        let mut dash = Dashboard::empty("Family", today);
        dash.school.student = "Léa".into();
        dash.school.homework = (0..12)
            .map(|i| school_hw("Mon", &format!("Subject {i}")))
            .collect();
        dash.tube = tube_lines(4);
        dash.rooms = ["A", "B", "C", "D", "E"].into_iter().map(room).collect();
        dash.todos = (0..50).map(|_| todo("Milk")).collect();
        dash.fit_sidebar(true);
        assert_eq!(dash.tube.len(), 4);
        assert_eq!(dash.rooms.len(), 5);
        assert_eq!(dash.school.homework.len(), MAX_HOMEWORK_ROWS);
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
    fn sidebar_trims_grades_to_fit_above_house() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 13).unwrap();
        let mut dash = Dashboard::empty("Family", today);
        dash.school.student = "Léa".into();
        dash.school.grades = (0..12)
            .map(|i| school_grade("Fri", "Maths", &format!("{i}/20"), "mid"))
            .collect();
        dash.tube = tube_lines(4);
        dash.rooms = (0..12).map(|i| room(&format!("R{i}"))).collect();
        dash.fit_sidebar(true);
        assert_eq!(dash.rooms.len(), 12);
        assert_eq!(dash.tube.len(), 4);
        assert!(dash.school.grades.len() < 12);
        assert!(!dash.school.grades.is_empty());
    }

    #[test]
    fn sidebar_keeps_school_tube_and_house() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 13).unwrap();
        let mut dash = Dashboard::empty("Family", today);
        dash.school = School {
            student: "Léa".into(),
            average: "14.2".into(),
            homework: vec![school_hw("Today", "Maths")],
            grades: vec![school_grade("Fri", "French", "15/20", "high")],
            days: Vec::new(),
            week: SchoolWeek::default(),
        };
        dash.tube = tube_lines(4);
        dash.rooms = ["Kitchen"].into_iter().map(room).collect();
        dash.todos = (0..120).map(|_| todo("Milk")).collect();
        dash.fit_sidebar(true);
        assert_eq!(dash.school.homework.len(), 1);
        assert_eq!(dash.school.grades.len(), 1);
        assert_eq!(dash.tube.len(), 4);
        assert_eq!(dash.rooms.len(), 1);
        assert!(dash.todos_more > 0);
        assert_eq!(dash.todos.len() + dash.todos_more, 120);
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
    fn sidebar_adds_history_facts_that_fit_leftover() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 18).unwrap();
        let mut dash = Dashboard::empty("Family", today);
        dash.tube = tube_lines(4);
        dash.rooms = ["Kitchen"].into_iter().map(room).collect();
        dash.todos = vec![todo("Milk"), todo("Eggs")];
        dash.history = (1850..1862)
            .map(|year| {
                history_fact(
                    &year.to_string(),
                    "The New York Times is founded in New York City as the largest metropolitan newspaper in the United States and begins daily publication.",
                )
            })
            .collect();
        dash.fit_sidebar_to_panel();
        assert_eq!(dash.todos.len(), 2);
        assert_eq!(dash.history.len(), MAX_HISTORY_FACTS);
        assert_eq!(dash.history[0].year, "1850");
        assert_eq!(dash.history[4].year, "1854");
    }

    #[test]
    fn sidebar_skips_history_facts_that_need_four_lines() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 18).unwrap();
        let mut dash = Dashboard::empty("Family", today);
        dash.tube = tube_lines(4);
        dash.rooms = ["Kitchen"].into_iter().map(room).collect();
        dash.todos = vec![todo("Milk")];
        let too_long = "Word ".repeat(80);
        dash.history = vec![
            history_fact("1900", &too_long),
            history_fact("1851", "The New York Times is founded."),
        ];
        dash.fit_sidebar_to_panel();
        assert_eq!(dash.history.len(), 1);
        assert_eq!(dash.history[0].year, "1851");
    }

    #[test]
    fn sidebar_history_is_oldest_first_and_capped() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 18).unwrap();
        let mut dash = Dashboard::empty("Family", today);
        dash.tube = tube_lines(4);
        dash.rooms = ["Kitchen"].into_iter().map(room).collect();
        dash.todos = vec![todo("Milk")];
        dash.history = vec![
            history_fact("1964", "King Constantine II marries Princess Anne-Marie."),
            history_fact("44 BC", "Julius Caesar is born."),
            history_fact("1851", "The New York Times is founded."),
            history_fact("1879", "Blackpool Illuminations are switched on."),
            history_fact(
                "1948",
                "Australia's Invincibles complete their tour of England.",
            ),
            history_fact("2018", "A science museum opens a new gallery."),
        ];
        dash.fit_sidebar_to_panel();
        let years: Vec<&str> = dash.history.iter().map(|f| f.year.as_str()).collect();
        assert_eq!(years, ["44 BC", "1851", "1879", "1948", "1964"]);
    }

    #[test]
    fn sidebar_keeps_history_above_school_week() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 18).unwrap();
        let mut dash = Dashboard::empty("Family", today);
        dash.tube = tube_lines(4);
        dash.rooms = ["Kitchen"].into_iter().map(room).collect();
        dash.todos = vec![todo("Milk")];
        dash.joke = Some(Joke {
            setup: "Why don't scientists trust atoms?".into(),
            punchline: "Because they make up everything.".into(),
        });
        dash.history = vec![history_fact("1851", "The New York Times is founded.")];
        dash.school.week = SchoolWeek {
            title: "School - This week".into(),
            days: vec![SchoolWeekDay {
                label: "Fri 18".into(),
                today: true,
                col: 2,
                lessons: vec![SchoolLesson {
                    subject: "Maths".into(),
                    colour: "#8000FF".into(),
                    ink: "#ffffff".into(),
                    row_start: 2,
                    row_end: 3,
                }],
            }],
            times: vec![
                SchoolWeekTime {
                    label: "08:15".into(),
                    row: 2,
                    end: false,
                },
                SchoolWeekTime {
                    label: "09:10".into(),
                    row: 3,
                    end: false,
                },
            ],
            bands: 1,
        };
        dash.fit_sidebar_to_panel();
        assert_eq!(dash.history.len(), 1);
        assert_eq!(dash.history[0].year, "1851");
        assert_eq!(dash.school.week.title, "School - This week");
        assert!(dash.joke.is_some());
        assert_eq!(dash.todos.len(), 1);
    }

    #[test]
    fn sidebar_keeps_history_when_todos_overflow() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 13).unwrap();
        let mut dash = Dashboard::empty("Family", today);
        dash.tube = tube_lines(4);
        dash.rooms = (0..12).map(|i| room(&format!("R{i}"))).collect();
        dash.todos = (0..80).map(|_| todo("Milk")).collect();
        dash.history = vec![history_fact("1851", "The New York Times is founded.")];
        dash.fit_sidebar_to_panel();
        assert!(dash.todos_more > 0);
        assert_eq!(dash.history.len(), 1);
    }

    #[test]
    fn sidebar_keeps_joke_between_todos_and_history() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 18).unwrap();
        let mut dash = Dashboard::empty("Family", today);
        dash.tube = tube_lines(4);
        dash.rooms = ["Kitchen"].into_iter().map(room).collect();
        dash.todos = vec![todo("Milk")];
        dash.joke = Some(Joke {
            setup: "Why don't scientists trust atoms?".into(),
            punchline: "Because they make up everything.".into(),
        });
        dash.history = vec![history_fact("1851", "The New York Times is founded.")];
        dash.fit_sidebar_to_panel();
        assert!(dash.joke.is_some());
        assert_eq!(dash.history.len(), 1);
    }

    #[test]
    fn sidebar_keeps_joke_when_todos_fill_leftover() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 13).unwrap();
        let mut dash = Dashboard::empty("Family", today);
        dash.tube = tube_lines(4);
        dash.rooms = (0..12).map(|i| room(&format!("R{i}"))).collect();
        dash.todos = (0..80).map(|_| todo("Milk")).collect();
        dash.joke = Some(Joke {
            setup: "What do you call a fake noodle?".into(),
            punchline: "An impasta.".into(),
        });
        dash.history = vec![history_fact("1851", "The New York Times is founded.")];
        dash.fit_sidebar_to_panel();
        assert!(dash.joke.is_some());
        assert!(dash.todos_more > 0);
        assert_eq!(dash.history.len(), 1);
    }

    #[test]
    fn sidebar_drops_a_joke_that_needs_too_many_lines() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 18).unwrap();
        let mut dash = Dashboard::empty("Family", today);
        dash.tube = tube_lines(4);
        dash.rooms = ["Kitchen"].into_iter().map(room).collect();
        dash.joke = Some(Joke {
            setup: "Word ".repeat(80),
            punchline: String::new(),
        });
        dash.fit_sidebar_to_panel();
        assert!(dash.joke.is_none());
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
    fn hidden_school_sections_are_omitted_from_the_layout_hash() {
        let mut dash = Dashboard::empty("Family", NaiveDate::from_ymd_opt(2026, 9, 18).unwrap());
        dash.school = School {
            student: "Léa".into(),
            average: "14.2".into(),
            homework: vec![school_hw("Today", "Maths")],
            grades: vec![school_grade("Fri", "French", "15/20", "high")],
            days: Vec::new(),
            week: SchoolWeek::default(),
        };
        let before = dash.content_bytes();
        dash.school.homework.push(school_hw("Mon", "English"));
        dash.school
            .grades
            .push(school_grade("Thu", "Maths", "12/20", "mid"));
        dash.school.average = "13.8".into();
        assert_eq!(before, dash.content_bytes());
        dash.school.student = "Maya".into();
        assert_ne!(before, dash.content_bytes());
    }

    #[test]
    fn sidebar_leaves_homework_when_school_sections_are_hidden() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 13).unwrap();
        let mut dash = Dashboard::empty("Family", today);
        dash.school.student = "Léa".into();
        dash.school.homework = (0..12)
            .map(|i| school_hw("Mon", &format!("Subject {i}")))
            .collect();
        dash.tube = tube_lines(4);
        dash.rooms = ["Kitchen"].into_iter().map(room).collect();
        dash.todos = (0..8).map(|_| todo("Ok")).collect();
        dash.fit_sidebar_to_panel();
        assert_eq!(dash.school.homework.len(), 12);
        assert_eq!(dash.todos.len(), 8);
        assert_eq!(dash.todos_more, 0);
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

    #[test]
    fn refresh_window_clock_slot_is_not_floored_to_xx59() {
        let tz = chrono_tz::Europe::London;
        let now = tz
            .with_ymd_and_hms(2026, 9, 16, 16, 0, 3)
            .unwrap()
            .with_timezone(&Utc)
            + chrono::Duration::milliseconds(475);
        let slot = tz
            .with_ymd_and_hms(2026, 9, 16, 18, 0, 0)
            .unwrap()
            .with_timezone(&Utc);
        let mut dash = Dashboard::empty("Family", now.date_naive());
        dash.set_refresh_at(now, slot, tz);
        assert_eq!(dash.last_refresh, "16:00");
        assert_eq!(dash.next_refresh, "18:00");
        dash.set_refresh_window(now, crate::schedule::secs_until(now, slot), tz);
        assert_eq!(dash.next_refresh, "17:59");
    }

    #[test]
    fn refresh_window_linked_slot_keeps_arrival_time() {
        let tz = chrono_tz::Europe::London;
        let last = tz
            .with_ymd_and_hms(2026, 9, 20, 6, 57, 0)
            .unwrap()
            .with_timezone(&Utc);
        let next = tz
            .with_ymd_and_hms(2026, 9, 20, 10, 0, 0)
            .unwrap()
            .with_timezone(&Utc);
        let mut dash = Dashboard::empty("Family", last.date_naive());
        dash.set_refresh_at(last, next, tz);
        assert_eq!(dash.last_refresh, "06:57");
        assert_eq!(dash.next_refresh, "10:00");
    }

    fn event(date: NaiveDate, start: &str, title: &str) -> CalendarEvent {
        CalendarEvent {
            start: start.into(),
            title: title.into(),
            all_day: false,
            day_label: date.format("%a %-d").to_string(),
            date: date.format("%Y-%m-%d").to_string(),
            birthday: false,
            school: false,
            recurring: false,
            bin: false,
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
