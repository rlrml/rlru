// Shared build-script helper for embedding version metadata.
//
// `include!`d from both the workspace-root `build.rs` (the `rlru` library/CLI)
// and `crates/rlru-dioxus/build.rs` (the desktop/web GUI) so every binary
// reports the same commit regardless of which crate it lives in.
//
// Commit resolution order:
//   1. `RLRU_GIT_COMMIT` env var (set by the Nix flake from `self.rev` /
//      `self.dirtyRev`, since Nix builds strip the `.git` directory).
//   2. `git rev-parse HEAD` for local `cargo` builds, with a `-dirty` suffix
//      when the working tree has uncommitted changes.
//   3. The literal `unknown` as a last resort.

use std::process::Command;

fn emit_version_env() {
    println!("cargo:rerun-if-env-changed=RLRU_GIT_COMMIT");

    let commit = std::env::var("RLRU_GIT_COMMIT")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(git_commit_from_repo)
        .unwrap_or_else(|| "unknown".to_string());

    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let short: String = commit.chars().take(12).collect();
    let long = if commit == "unknown" {
        version.clone()
    } else {
        format!("{version} ({short})")
    };

    let target = std::env::var("TARGET").unwrap_or_else(|_| "unknown".to_string());

    println!("cargo:rustc-env=RLRU_GIT_COMMIT={commit}");
    println!("cargo:rustc-env=RLRU_VERSION_LONG={long}");
    println!("cargo:rustc-env=RLRU_BUILD_TARGET={target}");
}

fn git_commit_from_repo() -> Option<String> {
    let head = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    if !head.status.success() {
        return None;
    }
    let mut commit = String::from_utf8(head.stdout).ok()?.trim().to_string();
    if commit.is_empty() {
        return None;
    }

    if working_tree_is_dirty() {
        commit.push_str("-dirty");
    }

    // Rebuild when the checked-out commit changes.
    if let Ok(git_dir) = Command::new("git").args(["rev-parse", "--git-dir"]).output() {
        if git_dir.status.success() {
            let dir = String::from_utf8_lossy(&git_dir.stdout).trim().to_string();
            if !dir.is_empty() {
                println!("cargo:rerun-if-changed={dir}/HEAD");
            }
        }
    }

    Some(commit)
}

fn working_tree_is_dirty() -> bool {
    Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .map(|output| output.status.success() && !output.stdout.is_empty())
        .unwrap_or(false)
}
