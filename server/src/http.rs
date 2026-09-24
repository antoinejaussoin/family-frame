use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Form, Multipart, Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, put};
use axum::{Json, Router};

/// Phone camera JPEGs routinely exceed Axum's 2 MiB default body limit.
const UPLOAD_BODY_LIMIT: usize = 40 * 1024 * 1024;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

use crate::assets;
use crate::battery;
use crate::config::SettingsPatch;
use crate::debug::{page_from_polls_full, DebugExtras, DebugLog, Poll};
use crate::frame::{checksum_matches, FrameCache};
use crate::sources;
use crate::sources::meross;

#[derive(Clone)]
pub struct AppState {
    pub cache: Arc<FrameCache>,
    pub debug: Arc<DebugLog>,
}

#[derive(Debug, Deserialize, Default)]
pub struct FrameQuery {
    pub checksum: Option<String>,
    /// Layout simulator: skip the dashboard cache and re-run Chrome.
    #[serde(default)]
    pub fresh: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct PicoTelemetry {
    #[serde(default)]
    mv: u32,
    #[serde(default)]
    pct: u16,
    #[serde(default)]
    usb: u8,
    #[serde(default)]
    wake: String,
    /// Echo of the last `X-Wake-At`. Timer polls use this as the assigned slot.
    #[serde(default)]
    wake_at: Option<String>,
    /// LPOSC error versus 32.768 kHz, in tenths of a percent. Positive = slow.
    hw_drift: i32,
}

#[derive(Debug, Deserialize)]
struct RotateBody {
    rotate: Vec<String>,
}

pub fn router(state: AppState, ui_dir: Option<PathBuf>) -> Router {
    let api = Router::new()
        .route("/frame.bin", get(frame_bin_get).post(frame_bin_post))
        .route("/frame.png", get(frame_png))
        .route("/frame-dither.png", get(frame_dither))
        .route("/frame.json", get(frame_json))
        .route("/health", get(health))
        .route("/settings", get(get_settings).patch(patch_settings))
        .route("/pictures", get(list_pictures).post(upload_picture))
        .route("/pictures/rotate", put(put_rotate))
        .route("/pictures/{id}", axum::routing::delete(delete_picture))
        .route("/pictures/{id}/thumb.jpg", get(picture_thumb))
        .route("/pictures/{id}/dither.png", get(picture_dither))
        .route("/pictures/{id}/original", get(picture_original))
        .route("/debug", get(get_debug).delete(delete_debug))
        .route("/debug/frames/{id}", get(debug_frame))
        // Phone JPEGs routinely exceed Axum's 2 MiB default (multipart parse fails).
        .layer(DefaultBodyLimit::max(UPLOAD_BODY_LIMIT));

    let mut app = Router::new()
        .nest("/api", api)
        .route("/health", get(health))
        .route("/dashboard", get(dashboard))
        .route("/debug", get(legacy_debug_page))
        // Old bookmarks; frames also live under /api/debug/frames/.
        .route("/debug/frames/{checksum}", get(debug_frame))
        .route("/stats/frames/{checksum}", get(debug_frame))
        .route("/static/{*path}", get(static_asset))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    if let Some(dir) = ui_dir.filter(|d| d.join("index.html").exists()) {
        let index = ServeFile::new(dir.join("index.html"));
        let spa = ServeDir::new(dir).fallback(index);
        app = app.fallback_service(spa);
    } else {
        app = app
            .route("/", get(spa_missing))
            .route("/preview", get(spa_missing))
            .route("/stats", get(spa_missing))
            .route("/config", get(spa_missing));
    }

    app
}

async fn legacy_debug_page() -> Redirect {
    Redirect::permanent("/stats")
}

async fn spa_missing() -> impl IntoResponse {
    Html(
        r#"<!doctype html>
<html><head><meta charset="utf-8"><title>Family Frame</title></head>
<body style="font-family:system-ui;max-width:40rem;margin:2rem auto;padding:0 1rem">
  <h1>Family Frame</h1>
  <p>The family UI is not built yet.</p>
  <p>For hot reload, in another terminal: <code>cd ui &amp;&amp; npm run dev</code>
  then open <a href="http://127.0.0.1:5173/">http://127.0.0.1:5173/</a>
  (Vite proxies <code>/api</code> here).</p>
  <p>Or run <code>npm ci &amp;&amp; npm run build</code> in <code>server/ui</code>
  and refresh this page.</p>
  <p><a href="/preview">Layout simulator</a> · <a href="/stats">Stats</a> · <a href="/config">Setup</a></p>
</body></html>"#,
    )
}

async fn static_asset(Path(path): Path<String>) -> Response {
    let (body, content_type): (&[u8], &str) = match path.as_str() {
        "dashboard.css" => (assets::DASHBOARD_CSS.as_bytes(), "text/css; charset=utf-8"),
        "fonts/AtkinsonHyperlegible-Regular.woff2" => {
            (assets::FONT_ATKINSON_REGULAR_WOFF2, "font/woff2")
        }
        "fonts/AtkinsonHyperlegible-Bold.woff2" => (assets::FONT_ATKINSON_BOLD_WOFF2, "font/woff2"),
        "fonts/AtkinsonHyperlegible-Regular.ttf" => (assets::FONT_ATKINSON_REGULAR_TTF, "font/ttf"),
        "fonts/AtkinsonHyperlegible-Bold.ttf" => (assets::FONT_ATKINSON_BOLD_TTF, "font/ttf"),
        "fonts/TRMNL12-Regular.woff2" => (assets::FONT_12_REGULAR_WOFF2, "font/woff2"),
        "fonts/TRMNL12-Bold.woff2" => (assets::FONT_12_BOLD_WOFF2, "font/woff2"),
        "fonts/TRMNL12-Regular.ttf" => (assets::FONT_12_REGULAR_TTF, "font/ttf"),
        "fonts/TRMNL12-Bold.ttf" => (assets::FONT_12_BOLD_TTF, "font/ttf"),
        "fonts/TRMNL16-Regular.woff2" => (assets::FONT_16_REGULAR_WOFF2, "font/woff2"),
        "fonts/TRMNL16-Bold.woff2" => (assets::FONT_16_BOLD_WOFF2, "font/woff2"),
        "fonts/TRMNL16-Regular.ttf" => (assets::FONT_16_REGULAR_TTF, "font/ttf"),
        "fonts/TRMNL16-Bold.ttf" => (assets::FONT_16_BOLD_TTF, "font/ttf"),
        "fonts/TRMNL21-Regular.woff2" => (assets::FONT_21_REGULAR_WOFF2, "font/woff2"),
        "fonts/TRMNL21-Bold.woff2" => (assets::FONT_21_BOLD_WOFF2, "font/woff2"),
        "fonts/TRMNL21-Regular.ttf" => (assets::FONT_21_REGULAR_TTF, "font/ttf"),
        "fonts/TRMNL21-Bold.ttf" => (assets::FONT_21_BOLD_TTF, "font/ttf"),
        _ => {
            return (StatusCode::NOT_FOUND, "not found\n").into_response();
        }
    };
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, content_type.parse().unwrap());
    headers.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    (headers, body).into_response()
}

async fn dashboard(State(state): State<AppState>) -> impl IntoResponse {
    let cfg = state.cache.snapshot_config().await;
    match sources::load_dashboard(&cfg).await {
        Ok(mut dash) => {
            state.cache.stamp_status(&cfg, &mut dash).await;
            match state.cache.templates().render_dashboard(&dash) {
                Ok(html) => no_store_html(html),
                Err(err) => error_response(err),
            }
        }
        Err(err) => error_response(err),
    }
}

#[derive(Debug, Deserialize, Default)]
struct DebugQuery {
    page: Option<usize>,
}

async fn get_debug(
    State(state): State<AppState>,
    Query(q): Query<DebugQuery>,
) -> impl IntoResponse {
    let polls = state.debug.snapshot().await;
    let cfg = state.cache.snapshot_config().await;
    Json(page_from_polls_full(
        &polls,
        cfg.tz(),
        cfg.pico_drift,
        cfg.pico_overhead_secs,
        q.page.unwrap_or(1),
        state.debug.dir_bytes(),
        &DebugExtras {
            cell: cfg.battery_cell(),
            wakes_per_day: cfg.wakes_per_weekday(),
        },
        |c| state.debug.has_frame(c),
    ))
}

async fn delete_debug(State(state): State<AppState>) -> Response {
    match state.debug.clear().await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => error_response(err),
    }
}

async fn debug_frame(State(state): State<AppState>, Path(name): Path<String>) -> Response {
    let checksum = name.strip_suffix(".png").unwrap_or(name.as_str());
    match state.debug.frame_png(checksum) {
        Some(bytes) => {
            let mut headers = HeaderMap::new();
            headers.insert(header::CONTENT_TYPE, "image/png".parse().unwrap());
            headers.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
            (headers, Body::from(bytes)).into_response()
        }
        None => (StatusCode::NOT_FOUND, "not found\n").into_response(),
    }
}

async fn get_settings(State(state): State<AppState>) -> impl IntoResponse {
    Json(public_settings(&state).await)
}

async fn patch_settings(
    State(state): State<AppState>,
    Json(patch): Json<SettingsPatch>,
) -> Response {
    let rotate_ids = patch.rotate.clone();
    let mode_changing = patch.mode.is_some() || patch.rotate.is_some();
    let household_changing = patch.touches_household();

    {
        let pictures = state.cache.pictures();
        if let Some(ids) = &rotate_ids {
            if let Err(err) = pictures.validate_rotate_ids(ids) {
                return bad_request(err);
            }
        }
        let cfg = state.cache.snapshot_config().await;
        let next_mode = match patch.mode.as_deref() {
            Some(s) => match crate::config::FrameMode::parse(s) {
                Ok(m) => m,
                Err(err) => return bad_request(err),
            },
            None => cfg.mode,
        };
        if next_mode == crate::config::FrameMode::Picture {
            let ids = rotate_ids.as_ref().unwrap_or(&cfg.pictures.rotate);
            if ids.is_empty() {
                return bad_request(anyhow::anyhow!(
                    "picture mode needs at least one photo in the rotation"
                ));
            }
            if let Err(err) = pictures.validate_rotate_ids(ids) {
                return bad_request(err);
            }
        }
    }

    let result = {
        let cfg_lock = state.cache.config();
        let mut cfg = cfg_lock.write().await;
        cfg.apply_patch(patch)
    };

    match result {
        Ok(()) => {
            if mode_changing {
                let _ = state.cache.pictures().reset_index();
            }
            if mode_changing || household_changing {
                state.cache.invalidate().await;
            }
            Json(public_settings(&state).await).into_response()
        }
        Err(err) => bad_request(err),
    }
}

async fn list_pictures(State(state): State<AppState>) -> Response {
    let cfg = state.cache.snapshot_config().await;
    match state.cache.pictures().list(&cfg.pictures.rotate) {
        Ok(list) => {
            let rotate: Vec<String> = cfg
                .pictures
                .rotate
                .iter()
                .filter(|id| list.iter().any(|p| p.id == **id))
                .cloned()
                .collect();
            Json(serde_json::json!({
                "pictures": list,
                "rotate": rotate,
            }))
            .into_response()
        }
        Err(err) => error_response(err),
    }
}

async fn upload_picture(State(state): State<AppState>, mut multipart: Multipart) -> Response {
    let mut filename = String::from("upload.jpg");
    let mut bytes: Option<Vec<u8>> = None;
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(field)) => field,
            Ok(None) => break,
            Err(err) => {
                return bad_request(anyhow::anyhow!(
                    "upload failed (is the photo under 40 MB?): {err}"
                ));
            }
        };
        let name = field.name().unwrap_or("").to_string();
        if name == "file" || name == "photo" || name.is_empty() {
            if let Some(fname) = field.file_name().map(|s| s.to_string()) {
                filename = fname;
            }
            match field.bytes().await {
                Ok(b) => bytes = Some(b.to_vec()),
                Err(err) => {
                    return bad_request(anyhow::anyhow!(
                        "upload read failed (is the photo under 40 MB?): {err}"
                    ));
                }
            }
        }
    }
    let Some(bytes) = bytes else {
        return bad_request(anyhow::anyhow!("missing file field"));
    };
    if bytes.is_empty() {
        return bad_request(anyhow::anyhow!("empty upload"));
    }
    match state.cache.pictures().add(&filename, bytes).await {
        Ok(meta) => (StatusCode::CREATED, Json(meta)).into_response(),
        Err(err) => error_response(err),
    }
}

async fn delete_picture(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.cache.pictures().delete(&id).await {
        Ok(()) => {
            let cfg_lock = state.cache.config();
            let mut cfg = cfg_lock.write().await;
            let before = cfg.pictures.rotate.len();
            cfg.pictures.rotate.retain(|x| x != &id);
            if cfg.pictures.rotate.len() != before {
                if let Err(err) = cfg.persist_editable() {
                    return error_response(err);
                }
            }
            if cfg.mode == crate::config::FrameMode::Picture && cfg.pictures.rotate.is_empty() {
                cfg.mode = crate::config::FrameMode::Dashboard;
                if let Err(err) = cfg.persist_editable() {
                    return error_response(err);
                }
            }
            drop(cfg);
            let _ = state.cache.pictures().reset_index();
            state.cache.invalidate().await;
            StatusCode::NO_CONTENT.into_response()
        }
        Err(err) => {
            if err.to_string().contains("not found") {
                (StatusCode::NOT_FOUND, format!("{err:#}\n")).into_response()
            } else {
                error_response(err)
            }
        }
    }
}

async fn put_rotate(State(state): State<AppState>, Json(body): Json<RotateBody>) -> Response {
    if let Err(err) = state.cache.pictures().validate_rotate_ids(&body.rotate) {
        return bad_request(err);
    }
    let result = {
        let cfg_lock = state.cache.config();
        let mut cfg = cfg_lock.write().await;
        cfg.apply_patch(SettingsPatch {
            rotate: Some(body.rotate),
            ..Default::default()
        })
    };
    match result {
        Ok(()) => {
            let _ = state.cache.pictures().reset_index();
            state.cache.invalidate().await;
            Json(public_settings(&state).await).into_response()
        }
        Err(err) => bad_request(err),
    }
}

async fn picture_thumb(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    if let Err(err) = state.cache.pictures().ensure_cache(&id).await {
        return not_found_or_error(err);
    }
    file_response(state.cache.pictures().thumb_path(&id), "image/jpeg")
}

async fn picture_dither(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    if let Err(err) = state.cache.pictures().ensure_cache(&id).await {
        return not_found_or_error(err);
    }
    file_response(state.cache.pictures().dither_png_path(&id), "image/png")
}

async fn picture_original(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.cache.pictures().original_path(&id) {
        Ok(path) => {
            let ctype = match path.extension().and_then(|s| s.to_str()) {
                Some("png") => "image/png",
                Some("webp") => "image/webp",
                _ => "image/jpeg",
            };
            file_response(path, ctype)
        }
        Err(err) => not_found_or_error(err),
    }
}

fn file_response(path: PathBuf, content_type: &'static str) -> Response {
    match std::fs::read(&path) {
        Ok(bytes) => {
            let mut headers = HeaderMap::new();
            headers.insert(header::CONTENT_TYPE, content_type.parse().unwrap());
            headers.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
            (headers, Body::from(bytes)).into_response()
        }
        Err(_) => (StatusCode::NOT_FOUND, "not found\n").into_response(),
    }
}

fn not_found_or_error(err: anyhow::Error) -> Response {
    if err.to_string().contains("not found") {
        (StatusCode::NOT_FOUND, format!("{err:#}\n")).into_response()
    } else {
        error_response(err)
    }
}

async fn frame_bin_get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<FrameQuery>,
) -> impl IntoResponse {
    match state.cache.current().await {
        Ok(frame) => {
            let sleep_s = pico_sleep_secs(&state).await;
            if checksum_matches(&frame, offered_checksum(&headers, &q)) {
                return not_modified(&frame.checksum, Some(sleep_s));
            }
            binary(
                frame.bin,
                "application/octet-stream",
                &frame.checksum,
                "frame.bin",
                Some(sleep_s),
                None,
            )
        }
        Err(err) => error_response(err),
    }
}

async fn frame_bin_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(tel): Form<PicoTelemetry>,
) -> Response {
    let fresh = tel.wake.eq_ignore_ascii_case("button");
    if fresh {
        meross::invalidate_rooms();
        tracing::info!("Pico button wake — reloading dashboard sources");
    }
    let cfg = state.cache.snapshot_config().await;
    state
        .cache
        .note_pico_battery(battery::soc_pct(tel.mv, cfg.battery_cell().empty_mv))
        .await;
    let assigned = assigned_wake_from_telemetry(&tel.wake, tel.wake_at.as_deref());
    let frame_result = if fresh {
        state.cache.current_for_pico_fresh().await
    } else {
        state.cache.current_for_pico(assigned).await
    };
    match frame_result {
        Ok(frame) => {
            let offered = headers
                .get(header::IF_NONE_MATCH)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.trim().trim_matches('"').to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_default();
            let unchanged =
                checksum_matches(&frame, Some(offered.as_str()).filter(|s| !s.is_empty()));
            let status = if unchanged { 204 } else { 200 };
            update_pico_drift(&state, &tel).await;
            let now = Utc::now();
            let (sleep_s, wake_at) =
                pico_sleep_plan_for_wake(&state, &tel.wake, tel.wake_at.as_deref(), now).await;
            let echoed = tel
                .wake_at
                .as_deref()
                .and_then(crate::schedule::parse_wake_at_slot);
            let prev_next = state
                .debug
                .snapshot()
                .await
                .last()
                .and_then(|p| p.next_wake_at);
            let poll = Poll {
                t: now,
                scheduled_at: echoed.or(prev_next),
                next_wake_at: Some(wake_at),
                status,
                offered,
                checksum: frame.checksum.clone(),
                mv: tel.mv,
                pct: tel.pct,
                usb: tel.usb != 0,
                wake: tel.wake,
                sleep_s,
                hw_drift: f64::from(tel.hw_drift) / 1000.0,
            };
            let png = if status == 200 {
                Some(frame.preview_png.as_slice())
            } else {
                None
            };
            if let Err(err) = state.debug.record(poll, png).await {
                tracing::warn!(%err, "could not persist Pico poll");
            }
            if unchanged {
                no_content(&frame.checksum, sleep_s, Some(wake_at))
            } else {
                binary(
                    frame.bin,
                    "application/octet-stream",
                    &frame.checksum,
                    "frame.bin",
                    Some(sleep_s),
                    Some(wake_at),
                )
            }
        }
        Err(err) => error_response(err),
    }
}

async fn frame_png(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<FrameQuery>,
) -> impl IntoResponse {
    match state.cache.current().await {
        Ok(frame) => {
            if checksum_matches(&frame, offered_checksum(&headers, &q)) {
                return not_modified(&frame.checksum, None);
            }
            binary(
                frame.png,
                "image/png",
                &frame.checksum,
                "frame.png",
                None,
                None,
            )
        }
        Err(err) => error_response(err),
    }
}

async fn frame_dither(
    State(state): State<AppState>,
    Query(q): Query<FrameQuery>,
) -> impl IntoResponse {
    let result = if query_flag(q.fresh.as_deref()) {
        state.cache.current_dashboard_reraster().await
    } else {
        state.cache.current().await
    };
    match result {
        Ok(frame) => {
            let mut response = binary(
                frame.preview_png,
                "image/png",
                &frame.checksum,
                "frame-dither.png",
                None,
                None,
            );
            if query_flag(q.fresh.as_deref()) {
                response
                    .headers_mut()
                    .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
            }
            response
        }
        Err(err) => error_response(err),
    }
}

async fn frame_json(State(state): State<AppState>) -> impl IntoResponse {
    match state.cache.current().await {
        Ok(frame) => {
            let mut headers = HeaderMap::new();
            headers.insert(header::ETAG, frame.checksum.parse().unwrap());
            (headers, axum::Json(frame.info())).into_response()
        }
        Err(err) => error_response(err),
    }
}

async fn health() -> impl IntoResponse {
    axum::Json(serde_json::json!({ "ok": true, "version": crate::VERSION }))
}

fn no_store_html(html: String) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    (headers, Html(html)).into_response()
}

fn query_flag(value: Option<&str>) -> bool {
    matches!(
        value
            .map(str::trim)
            .map(|s| s.to_ascii_lowercase())
            .as_deref(),
        Some("") | Some("1") | Some("true") | Some("yes") | Some("on")
    )
}

fn offered_checksum<'a>(headers: &'a HeaderMap, q: &'a FrameQuery) -> Option<&'a str> {
    q.checksum.as_deref().or_else(|| {
        headers
            .get(header::IF_NONE_MATCH)
            .and_then(|v| v.to_str().ok())
    })
}

async fn public_settings(state: &AppState) -> crate::config::PublicSettings {
    let cfg = state.cache.snapshot_config().await;
    let polls = state.debug.snapshot().await;
    let assigned = polls.last().and_then(|p| p.next_wake_at);
    cfg.public_settings_for_assigned_wake(Utc::now(), assigned)
}

async fn pico_sleep_secs(state: &AppState) -> u64 {
    state
        .cache
        .snapshot_config()
        .await
        .pico_sleep_secs(Utc::now())
}

fn assigned_wake_from_telemetry(wake: &str, reported_slot: Option<&str>) -> Option<DateTime<Utc>> {
    if crate::schedule::is_timer_wake(wake) {
        reported_slot.and_then(crate::schedule::parse_wake_at_slot)
    } else {
        None
    }
}

async fn pico_sleep_plan_for_wake(
    state: &AppState,
    wake: &str,
    reported_slot: Option<&str>,
    now: DateTime<Utc>,
) -> (u64, DateTime<Utc>) {
    let cfg = state.cache.snapshot_config().await;
    cfg.pico_sleep_plan(now, assigned_wake_from_telemetry(wake, reported_slot))
}

async fn update_pico_drift(state: &AppState, tel: &PicoTelemetry) {
    let polls = state.debug.snapshot().await;
    let Some(prev) = polls.last() else {
        return;
    };
    let now = Utc::now();
    let scheduled_at = tel
        .wake_at
        .as_deref()
        .and_then(crate::schedule::parse_wake_at_slot)
        .or(prev.next_wake_at);
    match crate::schedule::timing_between_polls(
        &prev.wake,
        prev.usb,
        prev.sleep_s,
        prev.t,
        &tel.wake,
        tel.usb != 0,
        now,
        scheduled_at,
    ) {
        crate::schedule::TimingPair::Skip => {}
        crate::schedule::TimingPair::OutOfRange { asked, elapsed } => {
            tracing::warn!(
                asked,
                elapsed,
                "pico sleep gap exceeds drift+overhead envelope; leaving timing unchanged"
            );
        }
        crate::schedule::TimingPair::Sample { asked, elapsed } => {
            let mut samples = Vec::new();
            for window in polls.windows(2) {
                if let crate::schedule::TimingPair::Sample { asked, elapsed } =
                    crate::schedule::timing_between_polls(
                        &window[0].wake,
                        window[0].usb,
                        window[0].sleep_s,
                        window[0].t,
                        &window[1].wake,
                        window[1].usb,
                        window[1].t,
                        window[1].scheduled_at,
                    )
                {
                    samples.push((asked, elapsed));
                }
            }
            samples.push((asked, elapsed));
            crate::schedule::keep_recent_timing_samples(&mut samples);
            let cfg_lock = state.cache.config();
            let mut cfg = cfg_lock.write().await;
            let Some(measured) = crate::schedule::fit_pico_timing(&samples, cfg.pico_timing())
            else {
                return;
            };
            match cfg.record_pico_timing(measured) {
                Ok(true) => {
                    tracing::info!(
                        samples = samples.len(),
                        measured_drift = measured.drift,
                        measured_overhead = measured.overhead_secs,
                        stored_drift = cfg.pico_drift,
                        stored_overhead = cfg.pico_overhead_secs,
                        "updated pico timing from timer polls"
                    );
                }
                Ok(false) => {}
                Err(err) => tracing::warn!(%err, "could not persist pico timing"),
            }
        }
    }
}

fn insert_sleep_header(headers: &mut HeaderMap, sleep_s: u64) {
    headers.insert("x-sleep-seconds", sleep_s.to_string().parse().unwrap());
}

fn insert_wake_at_header(headers: &mut HeaderMap, wake_at: DateTime<Utc>) {
    headers.insert(
        "x-wake-at",
        crate::schedule::format_wake_at_slot(wake_at)
            .parse()
            .unwrap(),
    );
}

fn insert_pico_headers(
    headers: &mut HeaderMap,
    sleep_s: Option<u64>,
    wake_at: Option<DateTime<Utc>>,
) {
    if let Some(sleep_s) = sleep_s {
        insert_sleep_header(headers, sleep_s);
    }
    if let Some(wake_at) = wake_at {
        insert_wake_at_header(headers, wake_at);
    }
}

fn not_modified(etag: &str, sleep_s: Option<u64>) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(header::ETAG, etag.parse().unwrap());
    headers.insert("x-frame-checksum", etag.parse().unwrap());
    insert_pico_headers(&mut headers, sleep_s, None);
    (StatusCode::NOT_MODIFIED, headers).into_response()
}

fn no_content(etag: &str, sleep_s: u64, wake_at: Option<DateTime<Utc>>) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(header::ETAG, etag.parse().unwrap());
    headers.insert("x-frame-checksum", etag.parse().unwrap());
    insert_pico_headers(&mut headers, Some(sleep_s), wake_at);
    (StatusCode::NO_CONTENT, headers).into_response()
}

fn binary(
    bytes: Vec<u8>,
    content_type: &'static str,
    etag: &str,
    filename: &str,
    sleep_s: Option<u64>,
    wake_at: Option<DateTime<Utc>>,
) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, content_type.parse().unwrap());
    headers.insert(header::ETAG, etag.parse().unwrap());
    headers.insert("x-frame-checksum", etag.parse().unwrap());
    insert_pico_headers(&mut headers, sleep_s, wake_at);
    headers.insert(
        header::CONTENT_DISPOSITION,
        format!("inline; filename=\"{filename}\"").parse().unwrap(),
    );
    headers.insert(
        header::CACHE_CONTROL,
        "max-age=10, must-revalidate".parse().unwrap(),
    );
    (headers, Body::from(bytes)).into_response()
}

fn error_response(err: anyhow::Error) -> Response {
    tracing::error!(%err, "request failed");
    (StatusCode::INTERNAL_SERVER_ERROR, format!("{err:#}\n")).into_response()
}

fn bad_request(err: anyhow::Error) -> Response {
    (StatusCode::BAD_REQUEST, format!("{err:#}\n")).into_response()
}

/// Resolve the built SPA directory (`EINK_UI`, else `/app/ui`, else `ui/dist` under the crate).
pub fn ui_dir() -> Option<PathBuf> {
    if let Ok(from_env) = std::env::var("EINK_UI") {
        let trimmed = from_env.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed));
        }
    }
    for candidate in [
        PathBuf::from("/app/ui"),
        crate::config::asset_root().join("ui/dist"),
    ] {
        if candidate.join("index.html").exists() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::query_flag;

    #[test]
    fn fresh_query_accepts_common_truthy_flags() {
        assert!(query_flag(Some("1")));
        assert!(query_flag(Some("true")));
        assert!(query_flag(Some("YES")));
        assert!(query_flag(Some("")));
        assert!(!query_flag(Some("0")));
        assert!(!query_flag(None));
    }
}
