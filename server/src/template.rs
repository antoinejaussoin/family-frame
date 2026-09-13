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
        env.add_template("preview.html", assets::PREVIEW_HTML)
            .context("templates/preview.html")?;
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
        let html = Templates::load().unwrap().render_dashboard(&dash).unwrap();
        assert!(html.contains("Morning"));
        assert!(html.contains("Afternoon"));
        assert!(html.contains("Evening"));
        assert!(html.contains("wx-sun"));
        assert!(html.contains("18°"));
        assert!(html.contains("Tomorrow"));
        assert!(html.contains("icon-house"));
        assert!(html.contains("icon-today"));
    }
}
