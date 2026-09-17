//! Dashboard HTML/CSS/fonts compiled into the binary (the 1600×1200 panel).
//!
//! Family UI pages live in `ui/` and are served as the SPA. These assets
//! ship in the Docker image; they are not mounted at deploy time.
//!
//! The panel typeface is Atkinson Hyperlegible (SIL OFL 1.1). Microsoft
//! Verdana cannot be redistributed, and Debian images do not ship it.

pub const DASHBOARD_HTML: &str = include_str!("../templates/dashboard.html");
pub const WX_SPRITE_HTML: &str = include_str!("../templates/wx-sprite.html");
pub const WEATHER_ICONS_HTML: &str = include_str!("../templates/weather-icons.html");
pub const WEATHER_ICONS_VIEW_HTML: &str = include_str!("../templates/weather-icons-view.html");
pub const DASHBOARD_CSS: &str = include_str!("../static/dashboard.css");

pub const FONT_REGULAR_WOFF2: &[u8] =
    include_bytes!("../static/fonts/AtkinsonHyperlegible-Regular.woff2");
pub const FONT_BOLD_WOFF2: &[u8] =
    include_bytes!("../static/fonts/AtkinsonHyperlegible-Bold.woff2");
pub const FONT_REGULAR_TTF: &[u8] =
    include_bytes!("../static/fonts/AtkinsonHyperlegible-Regular.ttf");
pub const FONT_BOLD_TTF: &[u8] = include_bytes!("../static/fonts/AtkinsonHyperlegible-Bold.ttf");

/// CSS plus bundled faces — anything that changes the Spectra raster.
pub fn layout_bytes() -> Vec<u8> {
    let mut bytes = DASHBOARD_CSS.as_bytes().to_vec();
    bytes.extend_from_slice(FONT_REGULAR_WOFF2);
    bytes.extend_from_slice(FONT_BOLD_WOFF2);
    bytes.extend_from_slice(FONT_REGULAR_TTF);
    bytes.extend_from_slice(FONT_BOLD_TTF);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_fonts_are_well_formed() {
        assert!(FONT_REGULAR_WOFF2.starts_with(b"wOF2"));
        assert!(FONT_BOLD_WOFF2.starts_with(b"wOF2"));
        assert!(FONT_REGULAR_TTF.starts_with(b"\x00\x01\x00\x00"));
        assert!(FONT_BOLD_TTF.starts_with(b"\x00\x01\x00\x00"));
        assert!(DASHBOARD_CSS.contains("Atkinson Hyperlegible"));
    }
}
