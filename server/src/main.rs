use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use eink_frame::config::Config;
use eink_frame::frame::FrameCache;
use eink_frame::http::{self, AppState};
use tokio::net::TcpListener;
use tracing::{info, warn};

#[derive(Parser, Debug)]
#[command(name = "eink-frame", about = "Family e-ink frame server")]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Serve the HTML simulator and the Pico frame endpoints.
    Serve {
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long)]
        bind: Option<String>,
    },
    /// Behave like the Pico: poll /frame.bin with the last checksum.
    PicoSim {
        #[arg(long, default_value = "http://127.0.0.1:8765")]
        url: String,
        #[arg(long, default_value_t = 5)]
        interval_secs: u64,
        #[arg(long)]
        save: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "eink_frame=info,tower_http=info".into()),
        )
        .init();

    match Cli::parse().cmd {
        Command::Serve { config, bind } => serve(config, bind).await,
        Command::PicoSim {
            url,
            interval_secs,
            save,
        } => pico_sim(url, interval_secs, save).await,
    }
}

async fn serve(config: Option<PathBuf>, bind: Option<String>) -> Result<()> {
    let mut cfg = Config::load_or_default(config.as_deref())?;
    if let Some(bind) = bind {
        cfg.bind = bind;
    }
    let addr: SocketAddr = cfg.bind.parse().context("bind address")?;
    let cache = FrameCache::new(cfg.clone(), addr.port())?;
    let app = http::router(AppState { cache });

    let listener = TcpListener::bind(addr).await?;
    info!(%addr, "eink-frame listening");
    info!("layout simulator  http://{addr}/preview");
    info!("dashboard only    http://{addr}/dashboard");
    info!("Pico endpoint     http://{addr}/frame.bin");
    if !cfg.icloud_enabled() {
        warn!("no iCloud credentials — serving demo / local JSON lists");
    }
    if !cfg.meross_enabled() {
        warn!("no Meross credentials — house temperatures will be demo rooms");
    }
    if !cfg.weather_enabled() {
        warn!("no BBC weather location_id — serving demo forecast");
    }
    axum::serve(listener, app).await?;
    Ok(())
}

async fn pico_sim(base: String, interval_secs: u64, save: Option<PathBuf>) -> Result<()> {
    let client = reqwest::Client::new();
    let mut checksum = String::new();
    loop {
        let url = format!(
            "{}/frame.bin{}",
            base.trim_end_matches('/'),
            if checksum.is_empty() {
                String::new()
            } else {
                format!("?checksum={checksum}")
            }
        );
        let resp = client.get(&url).send().await?;
        let status = resp.status();
        if status.as_u16() == 304 {
            info!(checksum, "304 not modified — Pico would skip the refresh");
        } else if status.is_success() {
            let etag = resp
                .headers()
                .get("x-frame-checksum")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();
            let bytes = resp.bytes().await?;
            checksum = etag;
            info!(checksum, bytes = bytes.len(), "200 new frame");
            if let Some(path) = &save {
                std::fs::write(path, &bytes)?;
            }
        } else {
            warn!(%status, "frame request failed");
        }
        tokio::time::sleep(std::time::Duration::from_secs(interval_secs)).await;
    }
}
