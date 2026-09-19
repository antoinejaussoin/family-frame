//! Dashboard HTML/CSS/fonts compiled into the binary (the 1600×1200 panel).
//!
//! Family UI pages live in `ui/` and are served as the SPA. These assets
//! ship in the Docker image; they are not mounted at deploy time.
//!
//! Two panel faces (both SIL OFL 1.1):
//! - TRMNL12 / 16 / 21 — pixel-grid cuts for dense small UI (native px only).
//! - Atkinson Hyperlegible — mast, saint, and Today event rows.

pub const DASHBOARD_HTML: &str = include_str!("../templates/dashboard.html");
pub const WX_SPRITE_HTML: &str = include_str!("../templates/wx-sprite.html");
pub const DASHBOARD_CSS: &str = include_str!("../static/dashboard.css");

pub const FONT_ATKINSON_REGULAR_WOFF2: &[u8] =
    include_bytes!("../static/fonts/AtkinsonHyperlegible-Regular.woff2");
pub const FONT_ATKINSON_BOLD_WOFF2: &[u8] =
    include_bytes!("../static/fonts/AtkinsonHyperlegible-Bold.woff2");
pub const FONT_ATKINSON_REGULAR_TTF: &[u8] =
    include_bytes!("../static/fonts/AtkinsonHyperlegible-Regular.ttf");
pub const FONT_ATKINSON_BOLD_TTF: &[u8] =
    include_bytes!("../static/fonts/AtkinsonHyperlegible-Bold.ttf");

pub const FONT_12_REGULAR_WOFF2: &[u8] = include_bytes!("../static/fonts/TRMNL12-Regular.woff2");
pub const FONT_12_BOLD_WOFF2: &[u8] = include_bytes!("../static/fonts/TRMNL12-Bold.woff2");
pub const FONT_12_REGULAR_TTF: &[u8] = include_bytes!("../static/fonts/TRMNL12-Regular.ttf");
pub const FONT_12_BOLD_TTF: &[u8] = include_bytes!("../static/fonts/TRMNL12-Bold.ttf");

pub const FONT_16_REGULAR_WOFF2: &[u8] = include_bytes!("../static/fonts/TRMNL16-Regular.woff2");
pub const FONT_16_BOLD_WOFF2: &[u8] = include_bytes!("../static/fonts/TRMNL16-Bold.woff2");
pub const FONT_16_REGULAR_TTF: &[u8] = include_bytes!("../static/fonts/TRMNL16-Regular.ttf");
pub const FONT_16_BOLD_TTF: &[u8] = include_bytes!("../static/fonts/TRMNL16-Bold.ttf");

pub const FONT_21_REGULAR_WOFF2: &[u8] = include_bytes!("../static/fonts/TRMNL21-Regular.woff2");
pub const FONT_21_BOLD_WOFF2: &[u8] = include_bytes!("../static/fonts/TRMNL21-Bold.woff2");
pub const FONT_21_REGULAR_TTF: &[u8] = include_bytes!("../static/fonts/TRMNL21-Regular.ttf");
pub const FONT_21_BOLD_TTF: &[u8] = include_bytes!("../static/fonts/TRMNL21-Bold.ttf");

/// CSS plus bundled faces — anything that changes the Spectra raster.
pub fn layout_bytes() -> Vec<u8> {
    let mut bytes = DASHBOARD_CSS.as_bytes().to_vec();
    for face in [
        FONT_ATKINSON_REGULAR_WOFF2,
        FONT_ATKINSON_BOLD_WOFF2,
        FONT_ATKINSON_REGULAR_TTF,
        FONT_ATKINSON_BOLD_TTF,
        FONT_12_REGULAR_WOFF2,
        FONT_12_BOLD_WOFF2,
        FONT_12_REGULAR_TTF,
        FONT_12_BOLD_TTF,
        FONT_16_REGULAR_WOFF2,
        FONT_16_BOLD_WOFF2,
        FONT_16_REGULAR_TTF,
        FONT_16_BOLD_TTF,
        FONT_21_REGULAR_WOFF2,
        FONT_21_BOLD_WOFF2,
        FONT_21_REGULAR_TTF,
        FONT_21_BOLD_TTF,
    ] {
        bytes.extend_from_slice(face);
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_fonts_are_well_formed() {
        for (label, woff2) in [
            ("atk-r", FONT_ATKINSON_REGULAR_WOFF2),
            ("atk-b", FONT_ATKINSON_BOLD_WOFF2),
            ("12r", FONT_12_REGULAR_WOFF2),
            ("12b", FONT_12_BOLD_WOFF2),
            ("16r", FONT_16_REGULAR_WOFF2),
            ("16b", FONT_16_BOLD_WOFF2),
            ("21r", FONT_21_REGULAR_WOFF2),
            ("21b", FONT_21_BOLD_WOFF2),
        ] {
            assert!(
                woff2.starts_with(b"wOF2"),
                "{label} woff2 should start with wOF2"
            );
        }
        for (label, ttf) in [
            ("atk-r", FONT_ATKINSON_REGULAR_TTF),
            ("atk-b", FONT_ATKINSON_BOLD_TTF),
            ("12r", FONT_12_REGULAR_TTF),
            ("12b", FONT_12_BOLD_TTF),
            ("16r", FONT_16_REGULAR_TTF),
            ("16b", FONT_16_BOLD_TTF),
            ("21r", FONT_21_REGULAR_TTF),
            ("21b", FONT_21_BOLD_TTF),
        ] {
            assert!(
                ttf.starts_with(b"\x00\x01\x00\x00"),
                "{label} ttf should be a TrueType sfnt"
            );
        }
        assert!(DASHBOARD_CSS.contains("Atkinson Hyperlegible"));
        assert!(DASHBOARD_CSS.contains("TRMNL12"));
        assert!(DASHBOARD_CSS.contains("TRMNL16"));
        assert!(DASHBOARD_CSS.contains("TRMNL21"));
    }
}
