//! Spectra 6 comparison grid (not used by the server).
//!
//! How to regenerate, serve, and apply a winner: [`examples/README.md`](./README.md).
//!
//! The gold “now” cell tracks [`eink_frame::photo::PHOTO_DITHER_MODE`] and
//! [`eink_frame::photo::PHOTO_PUNCH`].

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::{bail, Context, Result};
use eink_frame::pack::{PANEL_HEIGHT, PANEL_WIDTH};
use eink_frame::photo::{self, PHOTO_DITHER_MODE, PHOTO_PUNCH};
use epaper_dithering_core::enums::{DitherMode, GamutCompression, ToneCompression};
use epaper_dithering_core::measured_palettes::SPECTRA_7_3_6COLOR_V2;
use epaper_dithering_core::palettes::ColorScheme;
use epaper_dithering_core::types::ImageBuffer;
use epaper_dithering_core::{dither_with_canonical, DitherConfig};
use image::codecs::jpeg::JpegEncoder;
use image::{imageops, ExtendedColorType, ImageEncoder, Rgba, RgbaImage};

/// Contrast (sRGB, pivot 0.5) and OKLab saturation, as a shared multiplier.
/// Includes the live [`PHOTO_PUNCH`] so the gold cell can land on the current recipe.
const PUNCHES: [f64; 10] = [0.55, 0.70, 0.80, 0.90, 1.00, 1.15, 1.30, 1.45, 1.60, 1.80];

const MODES: [Mode; 5] = [
    Mode {
        slug: "atkinson",
        label: "Atkinson",
        hint: "Local diffusion, less muddy.",
        mode: DitherMode::Atkinson,
    },
    Mode {
        slug: "floyd-steinberg",
        label: "Floyd–Steinberg",
        hint: "Classic error diffusion, more grain.",
        mode: DitherMode::FloydSteinberg,
    },
    Mode {
        slug: "burkes",
        label: "Burkes",
        hint: "OpenDisplay default. Wider kernel.",
        mode: DitherMode::Burkes,
    },
    Mode {
        slug: "jarvis",
        label: "Jarvis–Judice–Ninke",
        hint: "Widest kernel. Smoothest, slowest.",
        mode: DitherMode::JarvisJudiceNinke,
    },
    Mode {
        slug: "ordered",
        label: "Ordered (Bayer 4×4)",
        hint: "Patterned. No error diffusion.",
        mode: DitherMode::Ordered,
    },
];

const PHOTOS: [Photo; 5] = [
    Photo {
        id: "ed003713-56ed-49d0-9ae2-1322756ba778",
        title: "Wheat Field with Cypresses",
    },
    Photo {
        id: "c4dcbdbb-a780-4b2a-b09c-3f2392d5421a",
        title: "The Starry Night",
    },
    Photo {
        id: "44b6cd36-8329-40c5-a868-d8b13987b1ae",
        title: "Beach (photo)",
    },
    Photo {
        id: "6f3c87d2-7f93-46d6-b012-d31208f768a5",
        title: "Beach (Hergé)",
    },
    Photo {
        id: "fe40b25b-5e07-4768-8f46-3ffc2500317f",
        title: "Mountain portrait",
    },
];

const CRATE_TO_NIBBLE: [u8; 6] = [0, 1, 2, 3, 5, 6];
const MEASURED_RGB: [(u8, [u8; 3]); 6] = [
    (0, [31, 24, 41]),
    (1, [168, 180, 182]),
    (2, [180, 173, 0]),
    (3, [113, 24, 19]),
    (5, [36, 70, 139]),
    (6, [50, 84, 60]),
];
const IDEAL_RGB: [(u8, [u8; 3]); 6] = [
    (0, [0, 0, 0]),
    (1, [255, 255, 255]),
    (2, [255, 255, 0]),
    (3, [255, 0, 0]),
    (5, [0, 0, 255]),
    (6, [0, 255, 0]),
];
const PREVIEW_LIFT: f32 = 0.45;
const PREVIEW_WIDTH: u32 = 800;
const PREVIEW_HEIGHT: u32 = 600;

#[derive(Clone, Copy)]
struct Mode {
    slug: &'static str,
    label: &'static str,
    hint: &'static str,
    mode: DitherMode,
}

#[derive(Clone, Copy)]
struct Photo {
    id: &'static str,
    title: &'static str,
}

struct Job {
    punch: f64,
    mode: Mode,
    rgb: Arc<Vec<u8>>,
    out: PathBuf,
}

fn main() -> Result<()> {
    // Error-diffusion is serial; keep Rayon from fighting the outer thread pool.
    std::env::set_var("RAYON_NUM_THREADS", "1");

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let pictures = root.join("pictures");
    let out = root.join("out/dither-matrix");
    fs::create_dir_all(&out)?;

    let started = Instant::now();
    let mut jobs = Vec::with_capacity(PHOTOS.len() * PUNCHES.len() * MODES.len());

    for photo in PHOTOS {
        let src = pictures.join(format!("{}.jpg", photo.id));
        eprintln!("prepare {} ({})", photo.title, src.display());
        let bytes = fs::read(&src).with_context(|| format!("reading {}", src.display()))?;
        let img = image::load_from_memory(&bytes)
            .with_context(|| format!("decoding {}", src.display()))?
            .to_rgba8();
        let panel = photo::prepare_panel_rgba(&img);
        let dir = out.join(photo.id);
        fs::create_dir_all(&dir)?;
        write_jpeg(&panel, &dir.join("original.jpg"), 88)?;

        for punch in PUNCHES {
            let contrasted = photo::apply_contrast_srgb(&panel, punch as f32);
            let rgb = Arc::new(rgba_to_rgb_flat(&contrasted));
            for mode in MODES {
                jobs.push(Job {
                    punch,
                    mode,
                    rgb: rgb.clone(),
                    out: dir.join(format!("{}_{}.jpg", mode.slug, punch_slug(punch))),
                });
            }
        }
    }

    let total = jobs.len();
    eprintln!("dithering {total} variants…");
    let done = AtomicUsize::new(0);
    let errors = Mutex::new(Vec::new());
    parallel_for(&jobs, |job| {
        if let Err(err) = render_job(job) {
            errors
                .lock()
                .unwrap()
                .push(format!("{}: {err:#}", job.out.display()));
        }
        let n = done.fetch_add(1, Ordering::Relaxed) + 1;
        if n % 10 == 0 || n == total {
            eprintln!("  {n}/{total}");
        }
    });

    let errors = errors.into_inner().unwrap();
    if !errors.is_empty() {
        for err in &errors {
            eprintln!("error: {err}");
        }
        bail!("{} jobs failed", errors.len());
    }

    let html_path = out.join("index.html");
    fs::write(&html_path, index_html())?;
    eprintln!(
        "wrote {} in {:.1}s\nopen {}",
        html_path.display(),
        started.elapsed().as_secs_f32(),
        html_path.display()
    );
    Ok(())
}

fn render_job(job: &Job) -> Result<()> {
    let preview = dither_preview(job.rgb.as_slice(), job.mode.mode, job.punch)?;
    write_jpeg(&preview, &job.out, 82)
}

fn dither_preview(rgb: &[u8], mode: DitherMode, saturation: f64) -> Result<RgbaImage> {
    let buf = ImageBuffer::new(rgb, PANEL_WIDTH as usize);
    let config = DitherConfig {
        mode,
        serpentine: true,
        exposure: 1.05,
        saturation,
        shadows: 0.15,
        highlights: 0.3,
        tone: ToneCompression::Auto,
        gamut: GamutCompression::Auto,
    };
    let indices = dither_with_canonical(
        &buf,
        &SPECTRA_7_3_6COLOR_V2,
        ColorScheme::Bwgbry,
        config,
    );
    if indices.len() != (PANEL_WIDTH * PANEL_HEIGHT) as usize {
        bail!(
            "dither returned {} indices, expected {}",
            indices.len(),
            PANEL_WIDTH * PANEL_HEIGHT
        );
    }
    let mut img = RgbaImage::new(PANEL_WIDTH, PANEL_HEIGHT);
    for (i, &crate_idx) in indices.iter().enumerate() {
        let mut n = CRATE_TO_NIBBLE
            .get(crate_idx as usize)
            .copied()
            .unwrap_or(0);
        // Same green → black remap as photo.rs.
        if n == 6 {
            n = 0;
        }
        let x = (i as u32) % PANEL_WIDTH;
        let y = (i as u32) / PANEL_WIDTH;
        img.put_pixel(x, y, measured_rgba(n));
    }
    Ok(img)
}

fn rgba_to_rgb_flat(img: &RgbaImage) -> Vec<u8> {
    let mut out = Vec::with_capacity((img.width() * img.height() * 3) as usize);
    for px in img.pixels() {
        let Rgba([r, g, b, a]) = *px;
        if a < 128 {
            out.extend_from_slice(&[255, 255, 255]);
        } else {
            out.extend_from_slice(&[r, g, b]);
        }
    }
    out
}

fn measured_rgba(idx: u8) -> Rgba<u8> {
    if idx == 0 {
        return Rgba([0, 0, 0, 255]);
    }
    let measured = MEASURED_RGB
        .iter()
        .find(|(i, _)| *i == idx)
        .map(|(_, rgb)| *rgb)
        .unwrap_or([0, 0, 0]);
    let ideal = IDEAL_RGB
        .iter()
        .find(|(i, _)| *i == idx)
        .map(|(_, rgb)| *rgb)
        .unwrap_or(measured);
    let t = PREVIEW_LIFT;
    let blend = |m: u8, i: u8| -> u8 {
        ((m as f32) * (1.0 - t) + (i as f32) * t)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    Rgba([
        blend(measured[0], ideal[0]),
        blend(measured[1], ideal[1]),
        blend(measured[2], ideal[2]),
        255,
    ])
}

fn write_jpeg(img: &RgbaImage, path: &Path, quality: u8) -> Result<()> {
    let resized = if img.width() == PREVIEW_WIDTH && img.height() == PREVIEW_HEIGHT {
        img.clone()
    } else {
        imageops::resize(
            img,
            PREVIEW_WIDTH,
            PREVIEW_HEIGHT,
            imageops::FilterType::Triangle,
        )
    };
    let rgb = image::DynamicImage::ImageRgba8(resized).to_rgb8();
    let mut file = fs::File::create(path).with_context(|| format!("creating {}", path.display()))?;
    let encoder = JpegEncoder::new_with_quality(&mut file, quality);
    encoder
        .write_image(
            rgb.as_raw(),
            PREVIEW_WIDTH,
            PREVIEW_HEIGHT,
            ExtendedColorType::Rgb8,
        )
        .with_context(|| format!("jpeg {}", path.display()))?;
    Ok(())
}

fn punch_slug(punch: f64) -> String {
    format!("p{punch:.2}").replace('.', "")
}

fn parallel_for<T: Sync>(items: &[T], f: impl Fn(&T) + Sync) {
    if items.is_empty() {
        return;
    }
    let n = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .min(items.len());
    let chunk = items.len().div_ceil(n);
    std::thread::scope(|s| {
        for slice in items.chunks(chunk) {
            let f = &f;
            s.spawn(move || {
                for item in slice {
                    f(item);
                }
            });
        }
    });
}

fn index_html() -> String {
    let mut tabs = String::new();
    let mut panels = String::new();
    for (pi, photo) in PHOTOS.iter().enumerate() {
        let hidden = if pi == 0 { "" } else { " hidden" };
        tabs.push_str(&format!(
            r#"<button class="tab" role="tab" aria-selected="{sel}" data-photo="{id}">
  <img src="{id}/original.jpg" alt="">
  <span>{title}</span>
</button>"#,
            sel = if pi == 0 { "true" } else { "false" },
            id = photo.id,
            title = esc(photo.title),
        ));
        let mut rows = String::new();
        for (mi, mode) in MODES.iter().enumerate() {
            let mut cells = String::new();
            for (ci, punch) in PUNCHES.iter().enumerate() {
                let is_current =
                    mode.mode == PHOTO_DITHER_MODE && (*punch - PHOTO_PUNCH).abs() < 1e-9;
                let src = format!("{}/{}_{}.jpg", photo.id, mode.slug, punch_slug(*punch));
                cells.push_str(&format!(
                    r#"<td><button type="button" class="cell{cur}" data-photo="{pid}" data-pi="{pi}" data-mi="{mi}" data-ci="{ci}" data-src="{src}" data-mode="{mode}" data-punch="{punch:.2}" data-current="{is_current}" title="{mode} · {punch:.2}×">
  <img src="{src}" alt="{mode} {punch:.2}×" loading="lazy" decoding="async" width="800" height="600">
  {badge}
</button></td>"#,
                    cur = if is_current { " current" } else { "" },
                    pid = photo.id,
                    mode = esc(mode.label),
                    punch = punch,
                    is_current = is_current,
                    badge = if is_current {
                        "<span class=\"badge\" aria-hidden=\"true\">now</span>"
                    } else {
                        ""
                    },
                ));
            }
            rows.push_str(&format!(
                r#"<tr>
  <th scope="row"><div class="mode-name">{label}</div><div class="mode-hint">{hint}</div></th>
  {cells}
</tr>"#,
                label = esc(mode.label),
                hint = esc(mode.hint),
            ));
        }
        let mut heads = String::from(r#"<th class="corner">Mode</th>"#);
        for punch in PUNCHES {
            let now = if (punch - PHOTO_PUNCH).abs() < 1e-9 {
                r#" <span class="now">current</span>"#
            } else {
                ""
            };
            heads.push_str(&format!(
                r#"<th><div class="punch">{punch:.2}×</div><div class="punch-sub">contrast {punch:.2} · sat {punch:.2}</div>{now}</th>"#
            ));
        }
        panels.push_str(&format!(
            r#"<section class="panel"{hidden} data-photo="{id}">
  <div class="orig">
    <img src="{id}/original.jpg" alt="Prepared original">
    <div><strong>{title}</strong><span>Cover-cropped 4:3, unsharp 0.6, no dither. Click a cell to zoom. Star favourites to copy a shortlist.</span></div>
  </div>
  <div class="table-wrap">
    <table>
      <thead><tr>{heads}</tr></thead>
      <tbody>{rows}</tbody>
    </table>
  </div>
</section>"#,
            hidden = hidden,
            id = photo.id,
            title = esc(photo.title),
        ));
    }

    let recipe_label = MODES
        .iter()
        .find(|m| m.mode == PHOTO_DITHER_MODE)
        .map(|m| m.label)
        .unwrap_or("?");
    let current_slug = MODES
        .iter()
        .find(|m| m.mode == PHOTO_DITHER_MODE)
        .map(|m| m.slug)
        .unwrap_or("");

    format!(
        r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Spectra 6 dither matrix</title>
<style>
:root {{
  --bg: #121417;
  --panel: #1b1e23;
  --line: #2c313a;
  --text: #ece7dc;
  --muted: #9aa3b2;
  --gold: #e2b340;
  --green: #7dba7a;
  --accent: #6ea8ff;
}}
* {{ box-sizing: border-box; }}
html, body {{ margin: 0; background: var(--bg); color: var(--text); font: 13px/1.4 ui-sans-serif, system-ui, sans-serif; }}
header {{
  position: sticky; top: 0; z-index: 20;
  background: color-mix(in srgb, var(--bg) 92%, transparent);
  backdrop-filter: blur(10px);
  border-bottom: 1px solid var(--line);
  padding: 12px 16px 0;
}}
h1 {{ margin: 0 0 4px; font-size: 18px; font-weight: 650; }}
.lede {{ color: var(--muted); margin: 0 0 10px; max-width: 90rem; }}
.lede code {{ color: var(--text); background: #0003; padding: 1px 5px; border-radius: 4px; }}
.tabs {{ display: flex; gap: 8px; overflow-x: auto; padding-bottom: 12px; }}
.tab {{
  display: flex; align-items: center; gap: 8px;
  background: var(--panel); color: var(--text);
  border: 1px solid var(--line); border-radius: 10px;
  padding: 6px 10px 6px 6px; cursor: pointer; flex: 0 0 auto;
}}
.tab img {{ width: 56px; height: 42px; object-fit: cover; border-radius: 6px; }}
.tab[aria-selected="true"] {{ border-color: var(--gold); box-shadow: 0 0 0 1px var(--gold); }}
.panel {{ padding: 12px 16px 80px; }}
.orig {{ display: flex; gap: 12px; align-items: center; margin-bottom: 12px; color: var(--muted); }}
.orig img {{ width: 160px; height: 120px; object-fit: cover; border-radius: 8px; border: 1px solid var(--line); }}
.orig strong {{ display: block; color: var(--text); font-size: 15px; margin-bottom: 2px; }}
.table-wrap {{ overflow: auto; max-height: calc(100dvh - 220px); border: 1px solid var(--line); border-radius: 12px; }}
table {{ border-collapse: separate; border-spacing: 0; min-width: 1680px; }}
th, td {{ border-bottom: 1px solid var(--line); border-right: 1px solid var(--line); padding: 0; background: var(--panel); }}
thead th {{
  position: sticky; top: 0; z-index: 3;
  background: #232830; padding: 8px 6px; font-weight: 600; text-align: center;
}}
.corner, tbody th {{
  position: sticky; left: 0; z-index: 2;
  width: 168px; min-width: 168px; max-width: 168px;
  text-align: left; padding: 10px; background: #232830; vertical-align: middle;
}}
thead .corner {{ z-index: 4; }}
.punch {{ font-variant-numeric: tabular-nums; }}
.punch-sub {{ font-size: 10px; color: var(--muted); font-weight: 400; }}
.now {{ color: var(--gold); font-size: 10px; font-weight: 700; letter-spacing: .04em; }}
.mode-name {{ font-size: 13px; }}
.mode-hint {{ font-size: 11px; color: var(--muted); font-weight: 400; margin-top: 2px; }}
.cell {{
  position: relative; display: block; width: 100%; padding: 0;
  border: 0; background: #000; cursor: zoom-in;
}}
.cell img {{ display: block; width: 100%; height: auto; aspect-ratio: 4/3; object-fit: cover; }}
.cell:hover, .cell:focus-visible {{ outline: 2px solid var(--accent); outline-offset: -2px; }}
.cell.current {{ outline: 2px solid var(--gold); outline-offset: -2px; }}
.cell.picked {{ outline: 2px solid var(--green); outline-offset: -2px; }}
.cell.active {{ outline: 2px solid #fff; outline-offset: -2px; }}
.badge {{
  position: absolute; top: 6px; left: 6px;
  background: var(--gold); color: #1a1406; font-size: 10px; font-weight: 800;
  padding: 1px 6px; border-radius: 999px; letter-spacing: .04em;
}}
.dock {{
  position: fixed; left: 0; right: 0; bottom: 0; z-index: 25;
  background: #16191ecc; backdrop-filter: blur(12px);
  border-top: 1px solid var(--line); padding: 8px 16px;
  display: flex; gap: 16px; align-items: flex-start; justify-content: space-between;
}}
.dock h2 {{ margin: 0 0 4px; font-size: 12px; text-transform: uppercase; letter-spacing: .08em; color: var(--muted); }}
.picks {{ flex: 1; min-width: 0; }}
#pick-list {{ margin: 0; padding-left: 18px; max-height: 88px; overflow: auto; }}
#pick-list:empty::before {{ content: "None yet — click a cell, then Star. Favourites persist in this browser."; color: var(--muted); }}
.dock-actions {{ display: flex; gap: 8px; flex-wrap: wrap; }}
.dock button, .zoom-bar button {{
  background: var(--panel); color: var(--text); border: 1px solid var(--line);
  border-radius: 8px; padding: 8px 12px; cursor: pointer;
}}
dialog {{
  border: 1px solid var(--line); border-radius: 14px; padding: 0;
  background: #0e1013; color: var(--text); max-width: min(1100px, 96vw);
}}
dialog::backdrop {{ background: #0008; }}
.zoom-bar {{
  display: flex; justify-content: space-between; align-items: center; gap: 12px;
  padding: 10px 12px; border-bottom: 1px solid var(--line);
}}
#zoom-meta {{ font-variant-numeric: tabular-nums; }}
dialog img {{ display: block; width: 100%; height: auto; }}
kbd {{ font: 11px ui-monospace, monospace; background: #0006; padding: 1px 5px; border-radius: 4px; border: 1px solid var(--line); }}
</style>
</head>
<body>
<header>
  <h1>Spectra 6 dither matrix</h1>
  <p class="lede">
    X = pre-dither <strong>contrast + saturation</strong> (same multiplier).
    Gold <strong>now</strong> is the live recipe
    (<code>{recipe_label} × {recipe_punch:.2}</code> from <code>photo.rs</code>).
    Y = dither kernel. Constants held fixed:
    <code>exposure 1.05</code>, <code>shadows 0.15</code>, <code>highlights 0.3</code>,
    <code>tone auto</code>, <code>gamut auto</code>, green ink remapped to black.
    Keys: <kbd>1</kbd>–<kbd>5</kbd> photos, arrows move, <kbd>Enter</kbd> zoom, <kbd>F</kbd> star.
  </p>
  <div class="tabs" role="tablist">{tabs}</div>
</header>
{panels}
<div class="dock">
  <div class="picks">
    <h2>Shortlist</h2>
    <ol id="pick-list"></ol>
  </div>
  <div class="dock-actions">
    <button type="button" id="copy-picks">Copy shortlist</button>
    <button type="button" id="clear-picks">Clear</button>
  </div>
</div>
<dialog id="zoom">
  <div class="zoom-bar">
    <div id="zoom-meta"></div>
    <div>
      <button type="button" id="star">Star</button>
      <button type="button" id="close-zoom">Close</button>
    </div>
  </div>
  <img id="zoom-img" alt="">
</dialog>
<script>
const PHOTOS = {photos_json};
const MODES = {modes_json};
const PUNCHES = {punches_json};
const CURRENT_SLUG = {current_slug:?};
const CURRENT_PUNCH = {recipe_punch};
const KEY = "dither-matrix-picks-v1";
let photo = 0;
let mi = Math.max(0, MODES.findIndex(m => m.slug === CURRENT_SLUG));
let ci = PUNCHES.findIndex(p => Math.abs(p - CURRENT_PUNCH) < 1e-6);
if (ci < 0) ci = 0;
const picks = new Set(JSON.parse(localStorage.getItem(KEY) || "[]"));

function cellKey(pi, m, c) {{ return pi + ":" + m + ":" + c; }}
function cellEl(pi, m, c) {{
  return document.querySelector('.cell[data-pi="'+pi+'"][data-mi="'+m+'"][data-ci="'+c+'"]');
}}
function showPhoto(i) {{
  photo = i;
  document.querySelectorAll(".tab").forEach((t, idx) => t.setAttribute("aria-selected", idx === i));
  document.querySelectorAll(".panel").forEach((p, idx) => p.toggleAttribute("hidden", idx !== i));
  highlight();
}}
function highlight() {{
  document.querySelectorAll(".cell.active").forEach(el => el.classList.remove("active"));
  const el = cellEl(photo, mi, ci);
  if (el) el.classList.add("active");
}}
function renderPicks() {{
  const ol = document.getElementById("pick-list");
  ol.innerHTML = "";
  [...picks].sort().forEach(k => {{
    const [pi, m, c] = k.split(":").map(Number);
    const li = document.createElement("li");
    li.textContent = PHOTOS[pi].title + " · " + MODES[m].label + " · " + PUNCHES[c].toFixed(2) + "× contrast/sat";
    ol.appendChild(li);
  }});
  document.querySelectorAll(".cell").forEach(el => {{
    const k = cellKey(+el.dataset.pi, +el.dataset.mi, +el.dataset.ci);
    el.classList.toggle("picked", picks.has(k));
  }});
  localStorage.setItem(KEY, JSON.stringify([...picks]));
}}
function togglePick(pi, m, c) {{
  const k = cellKey(pi, m, c);
  if (picks.has(k)) picks.delete(k); else picks.add(k);
  renderPicks();
}}
function openZoom() {{
  const el = cellEl(photo, mi, ci);
  if (!el) return;
  document.getElementById("zoom-img").src = el.dataset.src;
  const cur = el.dataset.current === "true" ? "  ·  current recipe" : "";
  document.getElementById("zoom-meta").textContent =
    PHOTOS[photo].title + "  ·  " + MODES[mi].label +
    "  ·  contrast " + PUNCHES[ci].toFixed(2) +
    "  ·  saturation " + PUNCHES[ci].toFixed(2) + cur;
  document.getElementById("zoom").showModal();
}}
document.querySelectorAll(".tab").forEach((t, i) => t.addEventListener("click", () => showPhoto(i)));
document.querySelectorAll(".cell").forEach(el => {{
  el.addEventListener("click", () => {{
    photo = +el.dataset.pi; mi = +el.dataset.mi; ci = +el.dataset.ci;
    highlight(); openZoom();
  }});
}});
document.getElementById("close-zoom").onclick = () => document.getElementById("zoom").close();
document.getElementById("star").onclick = () => togglePick(photo, mi, ci);
document.getElementById("clear-picks").onclick = () => {{ picks.clear(); renderPicks(); }};
document.getElementById("copy-picks").onclick = async () => {{
  const lines = [...picks].sort().map(k => {{
    const [pi, m, c] = k.split(":").map(Number);
    return PHOTOS[pi].title + " | " + MODES[m].label + " | contrast " + PUNCHES[c].toFixed(2) + " | saturation " + PUNCHES[c].toFixed(2);
  }});
  const text = lines.length ? lines.join("\\n") : "(no shortlist yet)";
  await navigator.clipboard.writeText(text);
  document.getElementById("copy-picks").textContent = "Copied";
  setTimeout(() => document.getElementById("copy-picks").textContent = "Copy shortlist", 1200);
}};
document.addEventListener("keydown", (e) => {{
  if (e.target.closest("input,textarea")) return;
  if (e.key >= "1" && e.key <= "5") {{ showPhoto(+e.key - 1); return; }}
  if (e.key === "ArrowRight") {{ ci = Math.min(PUNCHES.length-1, ci+1); highlight(); if (document.getElementById("zoom").open) openZoom(); e.preventDefault(); }}
  if (e.key === "ArrowLeft") {{ ci = Math.max(0, ci-1); highlight(); if (document.getElementById("zoom").open) openZoom(); e.preventDefault(); }}
  if (e.key === "ArrowDown") {{ mi = Math.min(MODES.length-1, mi+1); highlight(); if (document.getElementById("zoom").open) openZoom(); e.preventDefault(); }}
  if (e.key === "ArrowUp") {{ mi = Math.max(0, mi-1); highlight(); if (document.getElementById("zoom").open) openZoom(); e.preventDefault(); }}
  if (e.key === "Enter") {{ openZoom(); }}
  if (e.key === "Escape") {{ document.getElementById("zoom").close(); }}
  if (e.key === "f" || e.key === "F") {{ togglePick(photo, mi, ci); }}
}});
highlight();
renderPicks();
</script>
</body>
</html>
"##,
        tabs = tabs,
        panels = panels,
        recipe_label = recipe_label,
        recipe_punch = PHOTO_PUNCH,
        current_slug = current_slug,
        photos_json = serde_json::to_string(
            &PHOTOS
                .iter()
                .map(|p| serde_json::json!({"id": p.id, "title": p.title}))
                .collect::<Vec<_>>(),
        )
        .unwrap(),
        modes_json = serde_json::to_string(
            &MODES
                .iter()
                .map(|m| serde_json::json!({"slug": m.slug, "label": m.label}))
                .collect::<Vec<_>>(),
        )
        .unwrap(),
        punches_json = serde_json::to_string(&PUNCHES).unwrap(),
    )
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
