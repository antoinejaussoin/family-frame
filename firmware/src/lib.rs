//! Host-testable pieces of the Pico client (URL, settings, HTTP, panel map).

#![cfg_attr(not(test), no_std)]

/// Release version from the repo-root `VERSION` file (see `build.rs`).
pub const VERSION: &str = env!("FAMILY_FRAME_VERSION");

pub mod config;
pub mod headers;
pub mod panel;
pub mod protocol;

#[cfg(test)]
mod tests {
    #[test]
    fn version_matches_repo_file() {
        let file = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../VERSION"));
        assert_eq!(crate::VERSION, file.trim());
    }
}
