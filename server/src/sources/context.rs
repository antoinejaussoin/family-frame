use std::path::Path;
use std::time::Duration;

use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;

use crate::config::Config;

/// Shared inputs for every [`super::DataSource::load`] call.
pub struct SourceContext<'a> {
    pub cfg: &'a Config,
    pub tz: Tz,
    pub today: NaiveDate,
    pub now: DateTime<Utc>,
    pub http: reqwest::Client,
    pub config_dir: &'a Path,
}

impl<'a> SourceContext<'a> {
    pub fn from_config(cfg: &'a Config) -> Self {
        let tz: Tz = cfg.timezone.parse().unwrap_or(chrono_tz::Europe::London);
        let now = Utc::now();
        let today = now.with_timezone(&tz).date_naive();
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            cfg,
            tz,
            today,
            now,
            http,
            config_dir: &cfg.config_dir,
        }
    }
}
