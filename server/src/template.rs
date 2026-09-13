use anyhow::{Context, Result};
use minijinja::{Environment, path_loader};
use std::path::PathBuf;

use crate::model::Dashboard;

pub struct Templates {
    env: Environment<'static>,
}

impl Templates {
    pub fn load() -> Result<Self> {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("templates");
        let mut env = Environment::new();
        env.set_loader(path_loader(dir));
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
