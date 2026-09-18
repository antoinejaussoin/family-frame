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
        env.add_template("weather-icons.html", assets::WEATHER_ICONS_HTML)
            .context("templates/weather-icons.html")?;
        Ok(Self { env })
    }

    pub fn render_dashboard(&self, dash: &Dashboard) -> Result<String> {
        let tmpl = self
            .env
            .get_template("dashboard.html")
            .context("templates/dashboard.html")?;
        Ok(tmpl.render(minijinja::context! {
            show_school_sections => crate::model::SHOW_SCHOOL_SECTIONS,
            ..minijinja::Value::from_serialize(dash),
        })?)
    }

    pub fn render_weather_icons(&self) -> Result<String> {
        let tmpl = self
            .env
            .get_template("weather-icons.html")
            .context("templates/weather-icons.html")?;
        Ok(tmpl.render(minijinja::context! { icons => crate::weather::ICONS })?)
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;
    use crate::weather;

    #[test]
    fn dashboard_includes_weather_slots() {
        let mut dash = Dashboard::empty("Family", NaiveDate::from_ymd_opt(2026, 9, 12).unwrap());
        dash.weather = weather::demo_weather();
        dash.tube = crate::tfl::demo_tube();
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
        assert!(html.contains("/static/fonts/AtkinsonHyperlegible-Regular.woff2"));
        assert!(html.contains("/static/fonts/AtkinsonHyperlegible-Bold.woff2"));
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
        dash.school = crate::pronote::demo_school(NaiveDate::from_ymd_opt(2026, 9, 18).unwrap());
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
    }

    #[test]
    fn dashboard_shows_school_day_hours() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 18).unwrap();
        let mut dash = Dashboard::empty("Family", today);
        dash.events_today.push(crate::model::CalendarEvent {
            start: "08:30".into(),
            title: "School: Léa (finishes at 16:30)".into(),
            who: String::new(),
            all_day: false,
            day_label: "Today".into(),
            date: "2026-09-18".into(),
            birthday: false,
            school: true,
        });
        dash.events_coming.push(crate::model::CalendarEvent {
            start: "08:15".into(),
            title: "School: Léa (finishes at 15:45)".into(),
            who: String::new(),
            all_day: false,
            day_label: "Mon 21".into(),
            date: "2026-09-21".into(),
            birthday: false,
            school: true,
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
            who: String::new(),
            all_day: true,
            day_label: "Today".into(),
            date: "2026-09-14".into(),
            birthday: true,
            school: false,
        });
        dash.events_coming.push(crate::model::CalendarEvent {
            start: String::new(),
            title: "Sam turns 11".into(),
            who: String::new(),
            all_day: true,
            day_label: "Mon 28".into(),
            date: "2026-09-28".into(),
            birthday: true,
            school: false,
        });
        let html = Templates::load().unwrap().render_dashboard(&dash).unwrap();
        assert!(html.contains("class=\"birthday\""));
        assert!(html.contains("icon-present"));
        assert!(html.contains("Maya turns 8"));
        assert!(html.contains("Sam turns 11"));
        assert!(html.contains("Birthday"));
        assert!(html.contains("Mon 28"));
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
    fn weather_icons_sheet_lists_every_symbol() {
        let html = Templates::load().unwrap().render_weather_icons().unwrap();
        for icon in crate::weather::ICONS {
            assert!(
                html.contains(&format!("href=\"#wx-{}\"", icon.id)),
                "{}",
                icon.id
            );
        }
        assert_eq!(
            html.matches("class=\"at-80\"").count(),
            crate::weather::ICONS.len()
        );
        assert!(html.contains("class=\"at-40\""));
        assert!(html.contains("/static/dashboard.css"));
    }
}
