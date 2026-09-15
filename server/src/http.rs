use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Form, Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use chrono::Utc;
use chrono_tz::Tz;
use serde::Deserialize;
use tower_http::trace::TraceLayer;

use crate::assets;
use crate::debug::{page_from_polls, DebugLog, Poll};
use crate::frame::{checksum_matches, FrameCache};
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

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(preview))
        .route("/preview", get(preview))
        .route("/dashboard", get(dashboard))
        .route("/debug", get(debug_page))
        .route("/debug/frames/{checksum}", get(debug_frame))
        .route("/frame.bin", get(frame_bin_get).post(frame_bin_post))
        .route("/frame.png", get(frame_png))
        .route("/frame-dither.png", get(frame_dither))
        .route("/frame.json", get(frame_json))
        .route("/health", get(health))
        .route("/static/{name}", get(static_asset))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn static_asset(Path(name): Path<String>) -> Response {
    let (body, content_type) = match name.as_str() {
        "dashboard.css" => (assets::DASHBOARD_CSS, "text/css; charset=utf-8"),
        "preview.css" => (assets::PREVIEW_CSS, "text/css; charset=utf-8"),
        "preview.js" => (assets::PREVIEW_JS, "application/javascript; charset=utf-8"),
        "debug.css" => (assets::DEBUG_CSS, "text/css; charset=utf-8"),
        _ => {
            return (StatusCode::NOT_FOUND, "not found\n").into_response();
        }
    };
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, content_type.parse().unwrap());
    headers.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    (headers, body).into_response()
}

async fn preview(State(state): State<AppState>) -> impl IntoResponse {
    match sources::load_dashboard(state.cache.config()).await {
        Ok(dash) => match state.cache.templates().render_preview(&dash) {
            Ok(html) => no_store_html(html),
            Err(err) => error_response(err),
        },
        Err(err) => error_response(err),
    }
}

async fn dashboard(State(state): State<AppState>) -> impl IntoResponse {
    match sources::load_dashboard(state.cache.config()).await {
        Ok(dash) => match state.cache.templates().render_dashboard(&dash) {
            Ok(html) => no_store_html(html),
            Err(err) => error_response(err),
        },
        Err(err) => error_response(err),
    }
}

async fn debug_page(State(state): State<AppState>) -> impl IntoResponse {
    let polls = state.debug.snapshot().await;
    let tz: Tz = state
        .cache
        .config()
        .timezone
        .parse()
        .unwrap_or(chrono_tz::Europe::London);
    let page = page_from_polls(&polls, tz, |c| state.debug.has_frame(c));
    match state.cache.templates().render_debug(&page) {
        Ok(html) => no_store_html(html),
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

async fn frame_bin_get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<FrameQuery>,
) -> impl IntoResponse {
    match state.cache.current().await {
        Ok(frame) => {
            if checksum_matches(&frame, offered_checksum(&headers, &q)) {
                return not_modified(&frame.checksum);
            }
            binary(
                frame.bin,
                "application/octet-stream",
                &frame.checksum,
                "frame.bin",
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
    match state.cache.current().await {
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
            let poll = Poll {
                t: Utc::now(),
                status,
                offered,
                checksum: frame.checksum.clone(),
                mv: tel.mv,
                pct: tel.pct,
                usb: tel.usb != 0,
                wake: tel.wake,
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
                no_content(&frame.checksum)
            } else {
                binary(
                    frame.bin,
                    "application/octet-stream",
                    &frame.checksum,
                    "frame.bin",
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
                return not_modified(&frame.checksum);
            }
            binary(frame.png, "image/png", &frame.checksum, "frame.png")
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
    axum::Json(serde_json::json!({ "ok": true }))
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

fn not_modified(etag: &str) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(header::ETAG, etag.parse().unwrap());
    headers.insert("x-frame-checksum", etag.parse().unwrap());
    (StatusCode::NOT_MODIFIED, headers).into_response()
}

fn no_content(etag: &str) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(header::ETAG, etag.parse().unwrap());
    headers.insert("x-frame-checksum", etag.parse().unwrap());
    (StatusCode::NO_CONTENT, headers).into_response()
}

fn binary(bytes: Vec<u8>, content_type: &'static str, etag: &str, filename: &str) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, content_type.parse().unwrap());
    headers.insert(header::ETAG, etag.parse().unwrap());
    headers.insert("x-frame-checksum", etag.parse().unwrap());
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
