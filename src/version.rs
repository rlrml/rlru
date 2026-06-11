//! Build/version metadata embedded at compile time.
//!
//! The git commit is resolved by the build script (`build.rs` ->
//! `build_version.rs`): it prefers the `RLRU_GIT_COMMIT` env var supplied by
//! the Nix flake and otherwise falls back to `git rev-parse HEAD`, so the
//! commit is recorded even for binaries built through Nix where the `.git`
//! directory is unavailable.

/// Semantic version from `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Full git commit this binary was built from, or `"unknown"`.
///
/// May carry a `-dirty` suffix for local builds with uncommitted changes.
pub const GIT_COMMIT: &str = env!("RLRU_GIT_COMMIT");

/// Human-friendly version string, e.g. `0.1.7 (abc123def456)`.
pub const LONG_VERSION: &str = env!("RLRU_VERSION_LONG");

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
