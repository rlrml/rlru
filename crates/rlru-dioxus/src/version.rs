//! Build/version metadata for the GUI, embedded at compile time by
//! `build.rs` (which shares `../../build_version.rs` with the `rlru` crate).
//!
//! Self-contained so it works on every target, including the `wasm32` web
//! build where the `rlru` library is not a dependency.

/// Semantic version from `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Full git commit this build was made from, or `"unknown"`.
pub const GIT_COMMIT: &str = env!("RLRU_GIT_COMMIT");

/// Whether a real git commit was recorded at build time.
pub fn has_git_commit() -> bool {
    GIT_COMMIT != "unknown"
}

/// Abbreviated git commit (up to 12 leading characters).
pub fn git_commit_short() -> &'static str {
    let end = GIT_COMMIT
        .char_indices()
        .nth(12)
        .map(|(idx, _)| idx)
        .unwrap_or(GIT_COMMIT.len());
    &GIT_COMMIT[..end]
}

/// Compile target triple this build was produced for.
pub const TARGET: &str = env!("RLRU_BUILD_TARGET");
