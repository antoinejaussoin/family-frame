//! Family e-ink frame server library.
//!
//! The Pico is a dumb client: it POSTs a packed Spectra 6 frame request
//! (with battery diagnostics) and skips the panel refresh when unchanged.

/// Release version from the repo-root `VERSION` file (see `build.rs`).
pub const VERSION: &str = env!("FAMILY_FRAME_VERSION");

pub mod assets;
pub mod birthdays;
pub mod caldav;
pub mod config;
pub mod debug;
pub mod frame;
pub mod history;
pub mod http;
pub mod ics;
pub mod jokes;
pub mod meross;
pub mod model;
pub mod pack;
pub mod photo;
pub mod pictures;
pub mod pronote;
pub mod saints;
pub mod schedule;
pub mod screenshot;
pub mod sources;
pub mod template;
pub mod tfl;
pub mod todoist;
pub mod weather;

pub use config::Config;
pub use frame::{Frame, FrameCache};
pub use model::Dashboard;
pub use pack::{pack_png_to_spectra6, PANEL_BYTES, PANEL_HEIGHT, PANEL_WIDTH};

#[cfg(test)]
mod tests {
    #[test]
    fn version_matches_repo_file() {
        let file = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../VERSION"));
        assert_eq!(crate::VERSION, file.trim());
    }
}
