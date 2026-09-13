use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use eink_frame::config::Config;
use eink_frame::frame::FrameCache;
use eink_frame::http::{self, AppState};
use tokio::net::TcpListener;
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
    let cache = FrameCache::new(cfg.clone(), addr.port())?;
    let app = http::router(AppState { cache });

    let listener = TcpListener::bind(addr).await?;
    info!(%addr, "eink-frame listening");
    info!("layout simulator  http://{addr}/preview");
    info!("dashboard only    http://{addr}/dashboard");
    info!("Pico endpoint     http://{addr}/frame.bin");
    if !cfg.icloud_enabled() {
        warn!("no iCloud credentials — serving demo calendar unless ICS URLs are set");
    }
    if !cfg.todoist_enabled() {
        warn!("no Todoist token — serving demo to-dos");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watch_flag_parses() {
        let cli = Cli::try_parse_from(["eink-frame", "--watch"]).unwrap();
        assert!(cli.watch);
        let cli = Cli::try_parse_from(["eink-frame"]).unwrap();
        assert!(!cli.watch);
    }
}
