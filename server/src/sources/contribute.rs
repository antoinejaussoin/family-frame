use chrono::NaiveDate;

use crate::model::{
    CalendarEvent, Dashboard, HistoryFact, Joke, RoomClimate, School, StatusLine, TodoItem, Weather,
};

/// What a source is allowed to add. No variant clears another source's calendar.
pub enum Contribution {
    None,
    Calendar(Vec<CalendarEvent>),
    Todos(Vec<TodoItem>),
    Rooms(Vec<RoomClimate>),
    Weather(Weather),
    Transit(Vec<StatusLine>),
    School(School),
    Joke(Joke),
    History(Vec<HistoryFact>),
    Mast {
        saint_title: String,
        saint_name: String,
    },
}

pub enum SourceStatus {
    Live,
    Demo,
    Unavailable,
}

pub struct SourceOutcome {
    pub note: String,
    pub contribution: Contribution,
    pub status: SourceStatus,
}

impl SourceOutcome {
    pub fn live(note: impl Into<String>, contribution: Contribution) -> Self {
        Self {
            note: note.into(),
            contribution,
            status: SourceStatus::Live,
        }
    }

    pub fn demo(note: impl Into<String>, contribution: Contribution) -> Self {
        Self {
            note: note.into(),
            contribution,
            status: SourceStatus::Demo,
        }
    }

    pub fn unavailable(note: impl Into<String>, contribution: Contribution) -> Self {
        Self {
            note: note.into(),
            contribution,
            status: SourceStatus::Unavailable,
        }
    }
}

pub fn apply(
    dash: &mut Dashboard,
    calendar: &mut Vec<CalendarEvent>,
    contribution: Contribution,
    today: NaiveDate,
) {
    match contribution {
        Contribution::None => {}
        Contribution::Calendar(events) => calendar.extend(events),
        Contribution::Todos(todos) => dash.todos = todos,
        Contribution::Rooms(rooms) => dash.rooms = rooms,
        Contribution::Weather(weather) => dash.weather = weather,
        Contribution::Transit(lines) => dash.tube = lines,
        Contribution::School(school) => {
            calendar.extend(super::pronote::school_day_events(
                &school.student,
                &school.days,
                today,
            ));
            dash.school = school;
        }
        Contribution::Joke(joke) => dash.joke = Some(joke),
        Contribution::History(facts) => dash.history = facts,
        Contribution::Mast {
            saint_title,
            saint_name,
        } => {
            dash.saint_title = saint_title;
            dash.saint_name = saint_name;
        }
    }
}
