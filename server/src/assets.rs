//! Dashboard HTML/CSS/fonts compiled into the binary (the 1600×1200 panel).
//!
//! Family UI pages live in `ui/` and are served as the SPA. These assets
//! ship in the Docker image; they are not mounted at deploy time.
//!
//! The panel typeface is Fusion Pixel 12px proportional (SIL OFL 1.1),
//! used at 12×n so glyph pixels land on the Spectra grid.

pub const DASHBOARD_HTML: &str = include_str!("../templates/dashboard.html");
pub const WX_SPRITE_HTML: &str = include_str!("../templates/wx-sprite.html");
pub const WEATHER_ICONS_HTML: &str = include_str!("../templates/weather-icons.html");
pub const WEATHER_ICONS_VIEW_HTML: &str = include_str!("../templates/weather-icons-view.html");
pub const DASHBOARD_CSS: &str = include_str!("../static/dashboard.css");

pub const FONT_WOFF2: &[u8] = include_bytes!("../static/fonts/FusionPixel12-Regular.woff2");
pub const FONT_OTF: &[u8] = include_bytes!("../static/fonts/FusionPixel12-Regular.otf");

/// CSS plus bundled faces — anything that changes the Spectra raster.
pub fn layout_bytes() -> Vec<u8> {
    let mut bytes = DASHBOARD_CSS.as_bytes().to_vec();
    bytes.extend_from_slice(FONT_WOFF2);
    bytes.extend_from_slice(FONT_OTF);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_fonts_are_well_formed() {
        assert!(FONT_WOFF2.starts_with(b"wOF2"));
        assert!(FONT_OTF.starts_with(b"OTTO"));
        assert!(DASHBOARD_CSS.contains("Fusion Pixel"));
    }
}
