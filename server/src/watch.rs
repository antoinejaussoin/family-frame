//! Dev-only rebuild/restart loop for `--watch`.
//!
//! Production (Docker `CMD`) is the binary with no flags and never enters this path.

use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::time::Duration;

use anyhow::{Context, Result};
use notify::event::ModifyKind;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::process::{Child, Command};
use tokio::sync::mpsc::UnboundedReceiver;
use tracing::{error, info, warn};

const DEBOUNCE: Duration = Duration::from_millis(400);
const PORT_RELEASE: Duration = Duration::from_millis(100);

enum LoopEvent {
    CtrlC,
    Exited(std::io::Result<ExitStatus>),
    Fs(Option<Event>),
}

const WATCH_DIRS: &[&str] = &["src", "templates", "static", "fixtures"];
const ROOT_FILES: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    "config.toml",
    "config.example.toml",
];

pub async fn run(config: Option<PathBuf>, bind: Option<String>) -> Result<()> {
    let root = crate_root()?;
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let _watcher = start_watcher(&root, tx)?;

    info!(
        dir = %root.display(),
        "watch mode — rebuild and restart on file changes (dev only)"
    );

    let mut child = Some(spawn_server(&root, config.as_deref(), bind.as_deref())?);

    loop {
        let event = tokio::select! {
            _ = tokio::signal::ctrl_c() => LoopEvent::CtrlC,
            status = wait_running_server(&mut child) => LoopEvent::Exited(status),
            ev = rx.recv() => LoopEvent::Fs(ev),
        };
        match event {
            LoopEvent::CtrlC => {
                info!("stopping watch mode");
                stop_server(&mut child).await;
                return Ok(());
            }
            LoopEvent::Exited(status) => match status {
                Ok(status) => {
                    warn!(%status, "server exited; waiting for a file change to rebuild")
                }
                Err(err) => {
                    warn!(%err, "could not wait for server; waiting for a file change")
                }
            },
            LoopEvent::Fs(None) => break,
            LoopEvent::Fs(Some(event)) => {
                if !event_should_reload(&root, &event) {
                    continue;
                }
                debounce_changes(&mut rx).await;
                info!("change detected — rebuilding");
                match rebuild(&cargo, &root).await {
                    Ok(()) => {
                        stop_server(&mut child).await;
                        match spawn_server(&root, config.as_deref(), bind.as_deref()) {
                            Ok(next) => {
                                info!("restarted server");
                                child = Some(next);
                            }
                            Err(err) => {
                                error!(%err, "could not start server after rebuild");
                            }
                        }
                    }
                    Err(err) => {
                        error!(%err, "rebuild failed — keeping the current server");
                    }
                }
            }
        }
    }

    stop_server(&mut child).await;
    Ok(())
}

fn crate_root() -> Result<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if !root.join("Cargo.toml").exists() {
        anyhow::bail!(
            "--watch is for local development only; the compiled production binary cannot rebuild itself"
        );
    }
    Ok(root)
}

fn start_watcher(
    root: &Path,
    tx: tokio::sync::mpsc::UnboundedSender<Event>,
) -> Result<RecommendedWatcher> {
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| match res {
        Ok(event) => {
            let _ = tx.send(event);
        }
        Err(err) => {
            warn!(%err, "watch error");
        }
    })?;

    watcher
        .watch(root, RecursiveMode::NonRecursive)
        .with_context(|| format!("watching {}", root.display()))?;

    for dir in WATCH_DIRS {
        let path = root.join(dir);
        if path.is_dir() {
            watcher
                .watch(&path, RecursiveMode::Recursive)
                .with_context(|| format!("watching {}", path.display()))?;
        }
    }
    for file in ROOT_FILES {
        let path = root.join(file);
        if path.is_file() {
            watcher
                .watch(&path, RecursiveMode::NonRecursive)
                .with_context(|| format!("watching {}", path.display()))?;
        }
    }

    Ok(watcher)
}

async fn debounce_changes(rx: &mut UnboundedReceiver<Event>) {
    loop {
        tokio::select! {
            ev = rx.recv() => {
                if ev.is_none() {
                    return;
                }
            }
            _ = tokio::time::sleep(DEBOUNCE) => return,
        }
    }
}

async fn rebuild(cargo: &str, root: &Path) -> Result<()> {
    let status = Command::new(cargo)
        .arg("build")
        .arg("--manifest-path")
        .arg(root.join("Cargo.toml"))
        .kill_on_drop(true)
        .status()
        .await
        .with_context(|| format!("running {cargo} build"))?;
    if !status.success() {
        anyhow::bail!("cargo build failed");
    }
    Ok(())
}

fn debug_bin(root: &Path) -> PathBuf {
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target"));
    let exe = if cfg!(windows) {
        "eink-frame.exe"
    } else {
        "eink-frame"
    };
    target.join("debug").join(exe)
}

fn spawn_server(root: &Path, config: Option<&Path>, bind: Option<&str>) -> Result<Child> {
    let bin = debug_bin(root);
    if !bin.exists() {
        anyhow::bail!("expected compiled server at {}", bin.display());
    }
    let mut cmd = Command::new(&bin);
    cmd.kill_on_drop(true);
    if let Some(config) = config {
        cmd.arg("--config").arg(config);
    }
    if let Some(bind) = bind {
        cmd.arg("--bind").arg(bind);
    }
    cmd.spawn()
        .with_context(|| format!("spawning {}", bin.display()))
}

async fn wait_running_server(child: &mut Option<Child>) -> std::io::Result<ExitStatus> {
    match child.as_mut() {
        Some(c) => {
            let status = c.wait().await;
            *child = None;
            status
        }
        None => std::future::pending().await,
    }
}

async fn stop_server(child: &mut Option<Child>) {
    if let Some(mut c) = child.take() {
        let _ = c.start_kill();
        let _ = c.wait().await;
        tokio::time::sleep(PORT_RELEASE).await;
    }
}

fn event_should_reload(root: &Path, event: &Event) -> bool {
    kind_is_content_change(event.kind)
        && event
            .paths
            .iter()
            .any(|path| should_trigger_reload(root, path))
}

fn kind_is_content_change(kind: EventKind) -> bool {
    match kind {
        EventKind::Create(_) | EventKind::Remove(_) => true,
        EventKind::Modify(ModifyKind::Metadata(_)) => false,
        EventKind::Modify(_) | EventKind::Any => true,
        _ => false,
    }
}

fn should_trigger_reload(root: &Path, path: &Path) -> bool {
    if is_noise_file(path) {
        return false;
    }
    let rel = match path.strip_prefix(root) {
        Ok(rel) => rel,
        Err(_) => return interesting_root_file(path),
    };
    if rel
        .components()
        .any(|c| matches!(c.as_os_str().to_str(), Some("target" | "out" | ".git")))
    {
        return false;
    }
    if rel.components().count() <= 1 {
        return interesting_root_file(path);
    }
    matches!(
        rel.components().next().and_then(|c| c.as_os_str().to_str()),
        Some("src" | "templates" | "static" | "fixtures")
    )
}

fn interesting_root_file(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|s| s.to_str()),
        Some(
            "Cargo.toml"
                | "Cargo.lock"
                | "rust-toolchain.toml"
                | "config.toml"
                | "config.example.toml"
        )
    )
}

fn is_noise_file(path: &Path) -> bool {
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    name.starts_with('.')
        || name.ends_with('~')
        || name.ends_with(".swp")
        || name.ends_with(".tmp")
        || name.ends_with(".bak")
        || name == "meross-creds.json"
        || name == "weather-cache.json"
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{CreateKind, DataChange, MetadataKind};

    fn root() -> &'static Path {
        Path::new("/proj")
    }

    #[test]
    fn source_templates_and_config_trigger() {
        assert!(should_trigger_reload(
            root(),
            Path::new("/proj/src/http.rs")
        ));
        assert!(should_trigger_reload(
            root(),
            Path::new("/proj/templates/dashboard.html")
        ));
        assert!(should_trigger_reload(
            root(),
            Path::new("/proj/static/dashboard.css")
        ));
        assert!(should_trigger_reload(
            root(),
            Path::new("/proj/fixtures/todos.json")
        ));
        assert!(should_trigger_reload(
            root(),
            Path::new("/proj/config.toml")
        ));
        assert!(should_trigger_reload(root(), Path::new("/proj/Cargo.toml")));
    }

    #[test]
    fn build_output_and_runtime_files_do_not_trigger() {
        assert!(!should_trigger_reload(
            root(),
            Path::new("/proj/target/debug/eink-frame")
        ));
        assert!(!should_trigger_reload(
            root(),
            Path::new("/proj/out/frame.png")
        ));
        assert!(!should_trigger_reload(
            root(),
            Path::new("/proj/meross-creds.json")
        ));
        assert!(!should_trigger_reload(
            root(),
            Path::new("/proj/weather-cache.json")
        ));
        assert!(!should_trigger_reload(
            root(),
            Path::new("/proj/Dockerfile")
        ));
        assert!(!should_trigger_reload(
            root(),
            Path::new("/proj/src/.http.rs.swp")
        ));
    }

    #[test]
    fn metadata_only_does_not_reload() {
        let event = Event::new(EventKind::Modify(ModifyKind::Metadata(MetadataKind::Any)))
            .add_path(PathBuf::from("/proj/src/lib.rs"));
        assert!(!event_should_reload(root(), &event));
    }

    #[test]
    fn content_write_does_reload() {
        let event = Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Content)))
            .add_path(PathBuf::from("/proj/src/lib.rs"));
        assert!(event_should_reload(root(), &event));

        let created = Event::new(EventKind::Create(CreateKind::File))
            .add_path(PathBuf::from("/proj/templates/dashboard.html"));
        assert!(event_should_reload(root(), &created));
    }
}
