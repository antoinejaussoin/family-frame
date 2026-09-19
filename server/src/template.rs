use anyhow::{Context, Result};
use minijinja::Environment;

use crate::assets;
use crate::model::Dashboard;

pub struct Templates {
    env: Environment<'static>,
}

impl Templates {
    pub fn load() -> Result<Self> {
        let mut env = Environment::new();
        env.add_template("dashboard.html", assets::DASHBOARD_HTML)
            .context("templates/dashboard.html")?;
        env.add_template("wx-sprite.html", assets::WX_SPRITE_HTML)
            .context("templates/wx-sprite.html")?;
        Ok(Self { env })
    }

    pub fn render_dashboard(&self, dash: &Dashboard) -> Result<String> {
        let tmpl = self
            .env
            .get_template("dashboard.html")
            .context("templates/dashboard.html")?;
        Ok(tmpl.render(minijinja::context! {
            show_school_sections => dash.show_school_sections,
            ..minijinja::Value::from_serialize(dash),
        })?)
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;
    use crate::sources::weather;

    #[test]
    fn dashboard_uses_today_title() {
        let mut dash = Dashboard::empty("Family", NaiveDate::from_ymd_opt(2026, 9, 19).unwrap());
        dash.today_title = "Tomorrow 20th".into();
        let html = Templates::load().unwrap().render_dashboard(&dash).unwrap();
        assert!(html.contains("Tomorrow 20th"));
        assert!(html.contains("Nothing on the family calendar tomorrow."));
        assert!(!html.contains("Nothing on the family calendar today."));
    }

    #[test]
    fn dashboard_includes_weather_slots() {
        let mut dash = Dashboard::empty("Family", NaiveDate::from_ymd_opt(2026, 9, 12).unwrap());
        dash.weather = weather::demo_weather();
        dash.tube = crate::sources::tfl::demo_tube();
        let html = Templates::load().unwrap().render_dashboard(&dash).unwrap();
        assert!(html.contains("wx-sun"));
        assert!(html.contains("18°"));
        assert!(html.contains("06:33"));
        assert!(html.contains("19:18"));
        assert!(html.contains("pollen-low"));
        assert!(html.contains("class=\"coming\""));
        assert!(html.contains("icon-today"));
        assert!(html.contains("icon-house"));
        assert!(html.contains("Northern"));
        assert!(html.contains("Circle"));
        assert!(html.contains("District"));
        assert!(html.contains("Victoria"));
        assert!(html.contains("day-meta"));
        assert!(!html.contains("aria-label=\"Forecast\""));
        assert!(!html.contains("class=\"weather\""));
        assert!(html.contains("/static/fonts/AtkinsonHyperlegible-Bold.woff2"));
        assert!(html.contains("/static/fonts/TRMNL16-Bold.woff2"));
        assert!(html.contains("/static/fonts/TRMNL21-Bold.woff2"));
        assert!(!html.contains("class=\"battery\""));
        assert!(!html.contains("class=\"refresh\""));
        assert!(html.contains("class=\"frame-meta\""));
        assert!(html.contains("class=\"day-num\""));
        assert!(!html.contains("class=\"kicker\""));
        assert!(!html.contains("class=\"date\""));
    }

    #[test]
    fn dashboard_mast_is_weekday_day_month() {
        let dash = Dashboard::empty("Famille", NaiveDate::from_ymd_opt(2026, 9, 18).unwrap());
        let html = Templates::load().unwrap().render_dashboard(&dash).unwrap();
        assert!(html.contains("<title>Famille frame</title>"));
        assert!(html.contains("class=\"day-num\">18</span>"));
        assert!(html.contains(">Friday <span class=\"day-num\">18</span> September</h1>"));
        assert!(html.contains("class=\"saint\""));
        assert!(html.contains("Sainte <span class=\"saint-name\">Nadège</span>"));
        assert!(!html.contains("class=\"kicker\""));
        assert!(!html.contains("Famille</p>"));
        assert!(!html.contains("2026"));
    }

    #[test]
    fn dashboard_keeps_school_markup_hidden() {
        let mut dash = Dashboard::empty("Family", NaiveDate::from_ymd_opt(2026, 9, 18).unwrap());
        dash.school =
            crate::sources::pronote::demo_school(NaiveDate::from_ymd_opt(2026, 9, 18).unwrap());
        let html = Templates::load().unwrap().render_dashboard(&dash).unwrap();
        assert!(html.contains("icon-school"));
        assert!(html.contains("icon-grades"));
        assert!(!html.contains("class=\"panel with-school\""));
        assert!(!html.contains("class=\"homework\""));
        assert!(!html.contains("class=\"grades\""));
        assert!(!html.contains("Homework"));
        assert!(!html.contains("Grades"));
        assert!(!html.contains("Léa"));
        assert!(!html.contains("14.2"));
        assert!(!html.contains("15.5&#x2f;20"));
        assert!(!html.contains("grade-high"));
        assert!(html.contains("class=\"todos\""));
        assert!(html.contains("class=\"panel no-history no-joke\""));
        assert!(!html.contains("On this day"));
        assert!(!html.contains("Joke of the day"));
        assert!(!html.contains("No history for today."));
    }

    #[test]
    fn dashboard_shows_school_day_hours() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 18).unwrap();
        let mut dash = Dashboard::empty("Family", today);
        dash.events_today.push(crate::model::CalendarEvent {
            start: "08:30".into(),
            title: "School: Léa (finishes at 16:30)".into(),
            all_day: false,
            day_label: "Today".into(),
            date: "2026-09-18".into(),
            birthday: false,
            school: true,
            recurring: false,
            bin: false,
        });
        dash.events_coming.push(crate::model::CalendarEvent {
            start: "08:15".into(),
            title: "School: Léa (finishes at 15:45)".into(),
            all_day: false,
            day_label: "Mon 21".into(),
            date: "2026-09-21".into(),
            birthday: false,
            school: true,
            recurring: false,
            bin: false,
        });
        let html = Templates::load().unwrap().render_dashboard(&dash).unwrap();
        assert!(html.contains("class=\"school-day\""));
        assert!(html.contains("School: Léa (finishes at 16:30)"));
        assert!(html.contains("School: Léa (finishes at 15:45)"));
        assert!(html.contains("08:30"));
        assert!(html.contains("Mon 21 08:15"));
        assert!(!html.contains(">School<"));
    }

    #[test]
    fn dashboard_shows_battery_and_refresh_times() {
        let mut dash = Dashboard::empty("Family", NaiveDate::from_ymd_opt(2026, 9, 17).unwrap());
        dash.set_battery(62);
        dash.last_refresh = "17:53".into();
        dash.next_refresh = "18:53".into();
        let html = Templates::load().unwrap().render_dashboard(&dash).unwrap();
        assert!(html.contains("battery-ok"));
        assert!(html.contains("62%"));
        assert!(html.contains("17:53"));
        assert!(html.contains("18:53"));
        assert!(html.contains("class=\"refresh-arrow\""));
        assert!(html.contains("class=\"refresh-dash\""));
        assert!(html.contains("class=\"refresh\""));
        dash.set_battery(24);
        let html = Templates::load().unwrap().render_dashboard(&dash).unwrap();
        assert!(html.contains("battery-low"));
        assert!(!html.contains("battery-critical"));
        dash.set_battery(10);
        let html = Templates::load().unwrap().render_dashboard(&dash).unwrap();
        assert!(html.contains("battery-low"));
        assert!(html.contains("10%"));
    }

    #[test]
    fn dashboard_shows_birthday_name_and_age() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 14).unwrap();
        let mut dash = Dashboard::empty("Family", today);
        dash.events_today.push(crate::model::CalendarEvent {
            start: String::new(),
            title: "Maya turns 8".into(),
            all_day: true,
            day_label: "Today".into(),
            date: "2026-09-14".into(),
            birthday: true,
            school: false,
            recurring: false,
            bin: false,
        });
        dash.events_coming.push(crate::model::CalendarEvent {
            start: String::new(),
            title: "Sam turns 11".into(),
            all_day: true,
            day_label: "Mon 28".into(),
            date: "2026-09-28".into(),
            birthday: true,
            school: false,
            recurring: false,
            bin: false,
        });
        let html = Templates::load().unwrap().render_dashboard(&dash).unwrap();
        assert!(html.contains("class=\"birthday\""));
        assert!(html.contains("icon-present"));
        assert!(html.contains("Maya turns 8"));
        assert!(html.contains("Sam turns 11"));
        assert!(html.contains("Birthday"));
        assert!(html.contains("Mon 28"));
        assert!(!html.contains("class=\"one-off\""));
        assert!(!html.contains("class=\"all-day\""));
    }

    #[test]
    fn dashboard_marks_all_day_calendar_rows() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 18).unwrap();
        let mut dash = Dashboard::empty("Family", today);
        dash.events_today.push(crate::model::CalendarEvent {
            start: String::new(),
            title: "Test eink".into(),
            all_day: true,
            day_label: "Today".into(),
            date: "2026-09-18".into(),
            birthday: false,
            school: false,
            recurring: false,
            bin: false,
        });
        dash.events_coming.push(crate::model::CalendarEvent {
            start: String::new(),
            title: "Swim".into(),
            all_day: true,
            day_label: "Tomorrow".into(),
            date: "2026-09-19".into(),
            birthday: false,
            school: false,
            recurring: true,
            bin: false,
        });
        let html = Templates::load().unwrap().render_dashboard(&dash).unwrap();
        assert_eq!(html.matches("class=\"all-day\"").count(), 2);
        assert!(html.contains("All day"));
        assert!(html.contains("Test eink"));
        assert!(html.contains("Swim"));
        assert!(!html.contains("class=\"one-off\""));
        assert!(!html.contains("class=\"birthday\""));
    }

    #[test]
    fn dashboard_marks_one_off_calendar_rows() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 18).unwrap();
        let mut dash = Dashboard::empty("Family", today);
        dash.events_today.push(crate::model::CalendarEvent {
            start: "18:30".into(),
            title: "Dinner at Sam’s".into(),
            all_day: false,
            day_label: "Today".into(),
            date: "2026-09-18".into(),
            birthday: false,
            school: false,
            recurring: false,
            bin: false,
        });
        dash.events_coming.push(crate::model::CalendarEvent {
            start: "15:15".into(),
            title: "Pick-up Armand".into(),
            all_day: false,
            day_label: "Thu 24".into(),
            date: "2026-09-24".into(),
            birthday: false,
            school: false,
            recurring: true,
            bin: false,
        });
        let html = Templates::load().unwrap().render_dashboard(&dash).unwrap();
        assert!(html.contains("class=\"one-off\""));
        assert!(html.contains("Dinner at Sam’s"));
        assert!(html.contains("Pick-up Armand"));
        assert!(!html.contains("class=\"birthday\""));
        assert!(!html.contains("class=\"school-day\""));
        assert_eq!(html.matches("class=\"one-off\"").count(), 1);
    }

    #[test]
    fn dashboard_marks_bin_day_calendar_rows() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 19).unwrap();
        let mut dash = Dashboard::empty("Family", today);
        dash.events_today.push(crate::model::CalendarEvent {
            start: String::new(),
            title: "Bins collection".into(),
            all_day: true,
            day_label: "Today".into(),
            date: "2026-09-19".into(),
            birthday: false,
            school: false,
            recurring: false,
            bin: true,
        });
        dash.events_coming.push(crate::model::CalendarEvent {
            start: String::new(),
            title: "Bins collection".into(),
            all_day: true,
            day_label: "Wed 23".into(),
            date: "2026-09-23".into(),
            birthday: false,
            school: false,
            recurring: false,
            bin: true,
        });
        let html = Templates::load().unwrap().render_dashboard(&dash).unwrap();
        assert_eq!(html.matches("class=\"bin-day\"").count(), 2);
        assert!(html.contains("Bins collection"));
        assert!(html.contains("All day"));
        assert!(html.contains("Wed 23</span>"));
        assert!(!html.contains("09:17"));
        assert!(!html.contains("class=\"one-off\""));
        assert!(!html.contains("class=\"all-day\""));
    }

    #[test]
    fn dashboard_shows_other_todos_count() {
        let mut dash = Dashboard::empty("Family", NaiveDate::from_ymd_opt(2026, 9, 12).unwrap());
        dash.todos = vec![crate::model::TodoItem {
            title: "Buy milk".into(),
            done: false,
        }];
        dash.todos_more = 4;
        let html = Templates::load().unwrap().render_dashboard(&dash).unwrap();
        assert!(html.contains("todo-pill"));
        assert!(html.contains("Buy milk"));
        assert!(html.contains("+ 4 other todos"));
        assert!(!html.contains("No open family tasks"));
    }

    #[test]
    fn dashboard_shows_on_this_day_facts() {
        let mut dash = Dashboard::empty("Family", NaiveDate::from_ymd_opt(2026, 9, 18).unwrap());
        dash.history = crate::sources::history::demo_history();
        let html = Templates::load().unwrap().render_dashboard(&dash).unwrap();
        assert!(html.contains("class=\"history\""));
        assert!(html.contains("On this day"));
        assert!(html.contains("icon-history"));
        assert!(html.contains("1851"));
        assert!(html.contains("The New York Times is founded."));
        assert!(!html.contains("No history for today."));
        assert!(!html.contains("no-history"));
        assert!(html.contains("no-joke"));
    }

    #[test]
    fn dashboard_shows_joke_of_the_day() {
        let mut dash = Dashboard::empty("Family", NaiveDate::from_ymd_opt(2026, 9, 18).unwrap());
        dash.joke = Some(crate::sources::jokes::demo_joke());
        let html = Templates::load().unwrap().render_dashboard(&dash).unwrap();
        assert!(html.contains("class=\"joke\""));
        assert!(html.contains("Joke of the day"));
        assert!(html.contains("icon-joke"));
        assert!(html.contains("Why don&#x27;t scientists trust atoms?"));
        assert!(html.contains("Because they make up everything."));
        assert!(!html.contains("no-joke"));
    }
}
