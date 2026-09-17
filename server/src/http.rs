use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Form, Multipart, Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, put};
use axum::{Json, Router};

/// Phone camera JPEGs routinely exceed Axum's 2 MiB default body limit.
const UPLOAD_BODY_LIMIT: usize = 40 * 1024 * 1024;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

use crate::assets;
use crate::config::SettingsPatch;
use crate::debug::{page_from_polls_with_drift, DebugLog, Poll};
use crate::frame::{checksum_matches, FrameCache};
use crate::meross;
use crate::sources;

#[derive(Clone)]
pub struct AppState {
    pub cache: Arc<FrameCache>,
    pub debug: Arc<DebugLog>,
}

#[derive(Debug, Deserialize, Default)]
pub struct FrameQuery {
    pub checksum: Option<String>,
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
        .route("/debug", get(get_debug))
        .route("/debug/frames/{id}", get(debug_frame))
        // Phone JPEGs routinely exceed Axum's 2 MiB default (multipart parse fails).
        .layer(DefaultBodyLimit::max(UPLOAD_BODY_LIMIT));

    let mut app = Router::new()
        .nest("/api", api)
        .route("/health", get(health))
        .route("/dashboard", get(dashboard))
        .route("/weather-icons", get(weather_icons_view))
        .route("/weather-icons/sheet", get(weather_icons_sheet))
        .route("/weather-icons/dither.png", get(weather_icons_dither))
        .route("/weather-icons/chrome.png", get(weather_icons_chrome))
        // Old bookmarks; the SPA lives at /debug.
        .route("/debug/frames/{checksum}", get(debug_frame))
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
            .route("/debug", get(spa_missing));
    }

    app
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
  <p><a href="/preview">Layout simulator</a> · <a href="/debug">Debug</a></p>
</body></html>"#,
    )
}

async fn static_asset(Path(path): Path<String>) -> Response {
    let (body, content_type): (&[u8], &str) = match path.as_str() {
        "dashboard.css" => (assets::DASHBOARD_CSS.as_bytes(), "text/css; charset=utf-8"),
        "fonts/AtkinsonHyperlegible-Regular.woff2" => (assets::FONT_REGULAR_WOFF2, "font/woff2"),
        "fonts/AtkinsonHyperlegible-Bold.woff2" => (assets::FONT_BOLD_WOFF2, "font/woff2"),
        "fonts/AtkinsonHyperlegible-Regular.ttf" => (assets::FONT_REGULAR_TTF, "font/ttf"),
        "fonts/AtkinsonHyperlegible-Bold.ttf" => (assets::FONT_BOLD_TTF, "font/ttf"),
        _ => {
            return (StatusCode::NOT_FOUND, "not found\n").into_response();
        }
    };
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, content_type.parse().unwrap());
    headers.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    (headers, body).into_response()
}

async fn weather_icons_view() -> impl IntoResponse {
    no_store_html(crate::assets::WEATHER_ICONS_VIEW_HTML.to_string())
}

async fn weather_icons_sheet(State(state): State<AppState>) -> impl IntoResponse {
    match state.cache.templates().render_weather_icons() {
        Ok(html) => no_store_html(html),
        Err(err) => error_response(err),
    }
}

async fn weather_icons_dither(State(state): State<AppState>) -> Response {
    match state.cache.weather_icon_sheet().await {
        Ok((_, dither)) => png_bytes(dither, "weather-icons-dither.png"),
        Err(err) => error_response(err),
    }
}

async fn weather_icons_chrome(State(state): State<AppState>) -> Response {
    match state.cache.weather_icon_sheet().await {
        Ok((chrome, _)) => png_bytes(chrome, "weather-icons-chrome.png"),
        Err(err) => error_response(err),
    }
}

fn png_bytes(bytes: Vec<u8>, filename: &str) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, "image/png".parse().unwrap());
    headers.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    headers.insert(
        header::CONTENT_DISPOSITION,
        format!("inline; filename=\"{filename}\"").parse().unwrap(),
    );
    (headers, Body::from(bytes)).into_response()
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

async fn get_debug(State(state): State<AppState>) -> impl IntoResponse {
    let polls = state.debug.snapshot().await;
    let cfg = state.cache.snapshot_config().await;
    Json(page_from_polls_with_drift(
        &polls,
        cfg.tz(),
        cfg.pico_drift,
        |c| state.debug.has_frame(c),
    ))
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
    let cfg = state.cache.snapshot_config().await;
    Json(cfg.public_settings(Utc::now()))
}

async fn patch_settings(
    State(state): State<AppState>,
    Json(patch): Json<SettingsPatch>,
) -> Response {
    let rotate_ids = patch.rotate.clone();
    let mode_changing = patch.mode.is_some() || patch.rotate.is_some();

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
                state.cache.invalidate().await;
            }
            let cfg = state.cache.snapshot_config().await;
            Json(cfg.public_settings(Utc::now())).into_response()
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
            let cfg = state.cache.snapshot_config().await;
            Json(cfg.public_settings(Utc::now())).into_response()
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
    state.cache.note_pico_battery(tel.pct).await;
    let frame_result = if fresh {
        state.cache.current_for_pico_fresh().await
    } else {
        state.cache.current_for_pico().await
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
            let (sleep_s, wake_at) = pico_sleep_plan_for_wake(&state, &tel.wake, now).await;
            let poll = Poll {
                t: now,
                status,
                offered,
                checksum: frame.checksum.clone(),
                mv: tel.mv,
                pct: tel.pct,
                usb: tel.usb != 0,
                wake: tel.wake,
                sleep_s,
                wake_at: Some(wake_at),
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
                no_content(&frame.checksum, sleep_s)
            } else {
                binary(
                    frame.bin,
                    "application/octet-stream",
                    &frame.checksum,
                    "frame.bin",
                    Some(sleep_s),
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
            binary(frame.png, "image/png", &frame.checksum, "frame.png", None)
        }
        Err(err) => error_response(err),
    }
}

async fn frame_dither(State(state): State<AppState>) -> impl IntoResponse {
    match state.cache.current().await {
        Ok(frame) => binary(
            frame.preview_png,
            "image/png",
            &frame.checksum,
            "frame-dither.png",
            None,
        ),
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

fn offered_checksum<'a>(headers: &'a HeaderMap, q: &'a FrameQuery) -> Option<&'a str> {
    q.checksum.as_deref().or_else(|| {
        headers
            .get(header::IF_NONE_MATCH)
            .and_then(|v| v.to_str().ok())
    })
}

async fn pico_sleep_secs(state: &AppState) -> u64 {
    state
        .cache
        .snapshot_config()
        .await
        .pico_sleep_secs(Utc::now())
}

async fn pico_sleep_plan_for_wake(
    state: &AppState,
    wake: &str,
    now: DateTime<Utc>,
) -> (u64, DateTime<Utc>) {
    let cfg = state.cache.snapshot_config().await;
    let assigned = if crate::schedule::is_timer_wake(wake) {
        let polls = state.debug.snapshot().await;
        polls.last().and_then(|p| {
            crate::schedule::assigned_wake_from_poll(p.wake_at, p.t, p.sleep_s, cfg.pico_drift)
        })
    } else {
        None
    };
    cfg.pico_sleep_plan(now, assigned)
}

async fn update_pico_drift(state: &AppState, tel: &PicoTelemetry) {
    let polls = state.debug.snapshot().await;
    let Some(prev) = polls.last() else {
        return;
    };
    match crate::schedule::drift_between_polls(
        &prev.wake,
        prev.usb,
        prev.sleep_s,
        prev.t,
        &tel.wake,
        tel.usb != 0,
        Utc::now(),
    ) {
        crate::schedule::DriftSample::Skip => {}
        crate::schedule::DriftSample::OutOfRange {
            asked,
            elapsed,
            drift,
        } => {
            tracing::warn!(
                asked,
                elapsed,
                drift,
                "pico sleep drift exceeds 5%; leaving pico_drift unchanged"
            );
        }
        crate::schedule::DriftSample::Measured(measured) => {
            let cfg_lock = state.cache.config();
            let mut cfg = cfg_lock.write().await;
            match cfg.record_pico_drift(measured) {
                Ok(true) => {
                    tracing::info!(
                        measured,
                        stored = cfg.pico_drift,
                        "updated pico_drift from timer polls"
                    );
                }
                Ok(false) => {}
                Err(err) => tracing::warn!(%err, "could not persist pico_drift"),
            }
        }
    }
}

fn insert_sleep_header(headers: &mut HeaderMap, sleep_s: u64) {
    headers.insert("x-sleep-seconds", sleep_s.to_string().parse().unwrap());
}

fn not_modified(etag: &str, sleep_s: Option<u64>) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(header::ETAG, etag.parse().unwrap());
    headers.insert("x-frame-checksum", etag.parse().unwrap());
    if let Some(sleep_s) = sleep_s {
        insert_sleep_header(&mut headers, sleep_s);
    }
    (StatusCode::NOT_MODIFIED, headers).into_response()
}

fn no_content(etag: &str, sleep_s: u64) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(header::ETAG, etag.parse().unwrap());
    headers.insert("x-frame-checksum", etag.parse().unwrap());
    insert_sleep_header(&mut headers, sleep_s);
    (StatusCode::NO_CONTENT, headers).into_response()
}

fn binary(
    bytes: Vec<u8>,
    content_type: &'static str,
    etag: &str,
    filename: &str,
    sleep_s: Option<u64>,
) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, content_type.parse().unwrap());
    headers.insert(header::ETAG, etag.parse().unwrap());
    headers.insert("x-frame-checksum", etag.parse().unwrap());
    if let Some(sleep_s) = sleep_s {
        insert_sleep_header(&mut headers, sleep_s);
    }
    headers.insert(
        header::CONTENT_DISPOSITION,
        format!("inline; filename=\"{filename}\"").parse().unwrap(),
    );
    headers.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
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
