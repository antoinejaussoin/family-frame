//! Dashboard HTML/CSS compiled into the binary (the 1600×1200 panel).
//!
//! Family UI pages live in `ui/` and are served as the SPA. These assets
//! ship in the Docker image; they are not mounted at deploy time.

pub const DASHBOARD_HTML: &str = include_str!("../templates/dashboard.html");
pub const DASHBOARD_CSS: &str = include_str!("../static/dashboard.css");
