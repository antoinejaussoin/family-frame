use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Parser;
use eink_frame::config::Config;
use eink_frame::debug::DebugLog;
use eink_frame::frame::FrameCache;
use eink_frame::http::{self, AppState};
use eink_frame::pictures::PictureStore;
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tracing::{info, warn};

mod watch;

#[derive(Parser, Debug)]
#[command(name = "eink-frame", about = "Family e-ink frame server")]
struct Cli {
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long)]
    bind: Option<String>,
    /// Rebuild and restart when source, templates, or config change.
    /// Local development only — do not use in production.
    #[arg(long)]
    watch: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "eink_frame=info,tower_http=info".into()),
        )
        .init();

    let cli = Cli::parse();
    if cli.watch {
        watch::run(cli.config, cli.bind).await
    } else {
        serve(cli.config, cli.bind).await
    }
}

async fn serve(config: Option<PathBuf>, bind: Option<String>) -> Result<()> {
    let mut cfg = Config::load_or_default(config.as_deref())?;
    if let Some(bind) = bind {
        cfg.bind = bind;
    }
    let addr: SocketAddr = cfg.bind.parse().context("bind address")?;
    let pictures = PictureStore::open(&cfg.config_dir)?;
    let cfg = Arc::new(RwLock::new(cfg));
    let cache = FrameCache::new(cfg.clone(), pictures, addr.port())?;
    let debug = {
        let guard = cfg.read().await;
        DebugLog::open(&guard.config_dir)?
    };
    let polls = debug.snapshot().await;
    if let Some(last) = polls.last() {
        cache.note_pico_battery(last.pct).await;
    }
    let ui = http::ui_dir();
    let app = http::router(AppState { cache, debug }, ui.clone());

    let listener = TcpListener::bind(addr).await?;
    info!(%addr, "eink-frame listening");
    if ui.as_ref().is_some_and(|d| d.join("index.html").exists()) {
        info!("family UI         http://{addr}/");
    } else {
        warn!("family UI not built — npm run build in ui/, or npm run dev on :5173");
    }
    if cfg!(debug_assertions) {
        info!("family UI (HMR)   http://127.0.0.1:5173/  — cd ui && npm run dev");
    }
    info!("layout simulator  http://{addr}/preview");
    info!("debug dashboard   http://{addr}/debug");
    info!("dashboard only    http://{addr}/dashboard");
    info!("Pico endpoint     POST http://{addr}/api/frame.bin");
    {
        let guard = cfg.read().await;
        if !guard.icloud_enabled() {
            warn!("no iCloud credentials — serving demo calendar unless ICS URLs are set");
        }
        if !guard.todoist_enabled() {
            warn!("no Todoist token — serving demo to-dos");
        }
        if !guard.meross_enabled() {
            warn!("no Meross credentials — house temperatures will be demo rooms");
        }
        if !guard.weather_enabled() {
            warn!("no BBC weather location_id — serving demo forecast");
        }
    }
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_parses_watch() {
        let cli = Cli::try_parse_from(["eink-frame", "--watch"]).unwrap();
        assert!(cli.watch);
    }
}
