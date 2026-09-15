use anyhow::{Context, Result};
use minijinja::Environment;

use crate::assets;
use crate::debug::DebugPage;
use crate::model::Dashboard;

pub struct Templates {
    env: Environment<'static>,
}

impl Templates {
    pub fn load() -> Result<Self> {
        let mut env = Environment::new();
        env.add_template("dashboard.html", assets::DASHBOARD_HTML)
            .context("templates/dashboard.html")?;
        env.add_template("preview.html", assets::PREVIEW_HTML)
            .context("templates/preview.html")?;
        env.add_template("debug.html", assets::DEBUG_HTML)
            .context("templates/debug.html")?;
        Ok(Self { env })
    }

    pub fn render_dashboard(&self, dash: &Dashboard) -> Result<String> {
        let tmpl = self
            .env
            .get_template("dashboard.html")
            .context("templates/dashboard.html")?;
        Ok(tmpl.render(dash)?)
    }

    pub fn render_preview(&self, dash: &Dashboard) -> Result<String> {
        let tmpl = self
            .env
            .get_template("preview.html")
            .context("templates/preview.html")?;
        Ok(tmpl.render(dash)?)
    }

    pub fn render_debug(&self, page: &DebugPage) -> Result<String> {
        let tmpl = self
            .env
            .get_template("debug.html")
            .context("templates/debug.html")?;
        Ok(tmpl.render(page)?)
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
        assert!(html.contains("Coming next"));
        assert!(html.contains("icon-today"));
        assert!(html.contains("icon-house"));
        assert!(html.contains("Northern"));
        assert!(html.contains("Circle"));
        assert!(html.contains("District"));
        assert!(html.contains("Victoria"));
        assert!(html.contains("day-meta"));
        assert!(!html.contains("aria-label=\"Forecast\""));
        assert!(!html.contains("class=\"weather\""));
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
        });
        dash.events_coming.push(crate::model::CalendarEvent {
            start: String::new(),
            title: "Sam turns 11".into(),
            who: String::new(),
            all_day: true,
            day_label: "Mon 28".into(),
            date: "2026-09-28".into(),
            birthday: true,
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
    fn debug_page_renders_battery_and_log() {
        let polls = vec![crate::debug::Poll {
            t: chrono::Utc::now(),
            status: 200,
            offered: String::new(),
            checksum: "deadbeef".into(),
            mv: 3850,
            pct: 72,
            usb: false,
            wake: "timer".into(),
        }];
        let page = crate::debug::page_from_polls(&polls, chrono_tz::Europe::London, |_| true);
        let html = Templates::load().unwrap().render_debug(&page).unwrap();
        assert!(html.contains("72"));
        assert!(html.contains("3850"));
        assert!(html.contains("200 new frame"));
        assert!(html.contains("/debug/frames/deadbeef.png"));
        assert!(html.contains("<svg"));
    }

    #[test]
    fn debug_page_empty_state() {
        let page = crate::debug::page_from_polls(&[], chrono_tz::Europe::London, |_| false);
        let html = Templates::load().unwrap().render_debug(&page).unwrap();
        assert!(html.contains("No Pico has checked in yet"));
        assert!(!html.contains("<svg"));
    }
}
