//! Dashboard HTML/CSS/JS compiled into the binary.
//!
//! These ship in the Docker image; they are not mounted at deploy time.

pub const DASHBOARD_HTML: &str = include_str!("../templates/dashboard.html");
pub const PREVIEW_HTML: &str = include_str!("../templates/preview.html");
pub const DEBUG_HTML: &str = include_str!("../templates/debug.html");
pub const DASHBOARD_CSS: &str = include_str!("../static/dashboard.css");
pub const PREVIEW_CSS: &str = include_str!("../static/preview.css");
pub const PREVIEW_JS: &str = include_str!("../static/preview.js");
pub const DEBUG_CSS: &str = include_str!("../static/debug.css");
