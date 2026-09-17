//! Stamp `FAMILY_FRAME_VERSION` from the repo-root `VERSION` file (or the
//! `FAMILY_FRAME_VERSION` env, which Docker/CI set). Cargo.toml stays a
//! placeholder so a bump is one file, not a commit that rewrites manifests.

fn main() {
    inject_family_frame_version();
}

fn inject_family_frame_version() {
    println!("cargo:rerun-if-env-changed=FAMILY_FRAME_VERSION");
    let version = std::env::var("FAMILY_FRAME_VERSION")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(read_repo_version)
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
    assert!(
        !version.chars().any(|c| c.is_control() || c == '\0'),
        "VERSION must be a single line of printable text"
    );
    println!("cargo:rustc-env=FAMILY_FRAME_VERSION={version}");
}

fn read_repo_version() -> Option<String> {
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").ok()?);
    let path = manifest.join("../VERSION");
    println!("cargo:rerun-if-changed={}", path.display());
    let raw = std::fs::read_to_string(path).ok()?;
    let v = raw.trim().to_string();
    (!v.is_empty()).then_some(v)
}
