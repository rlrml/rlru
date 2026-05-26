use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::config::{BehaviorConfig, Config};
use crate::paths::AppPaths;
use crate::sync::{SyncService, SyncSummary};

pub async fn run_sync_daemon(
    paths: AppPaths,
    config_path: &Path,
    report_summary: impl Fn(&SyncSummary),
) -> Result<()> {
    let config = Config::load_or_default(config_path)?;
    let mut state = load_daemon_sync_state(&paths);
    if config.behavior.upload_on_launch {
        run_daemon_sync_cycle(&paths, config, &mut state, &report_summary).await;
    } else {
        ensure_next_sync_scheduled(&paths, &config, &mut state);
    }

    loop {
        let config = Config::load_or_default(config_path)?;
        let mut state = load_daemon_sync_state(&paths);

        if config.behavior.auto_upload {
            ensure_next_sync_scheduled(&paths, &config, &mut state);
            let delay = state
                .next_sync_after_at
                .map(duration_until)
                .unwrap_or(Duration::ZERO);

            if delay.is_zero() {
                run_daemon_sync_cycle(&paths, config, &mut state, &report_summary).await;
                continue;
            }

            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    println!("stopping");
                    return Ok(());
                }
                _ = tokio::time::sleep(delay) => {}
            }
        } else {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    println!("stopping");
                    return Ok(());
                }
                _ = tokio::time::sleep(Duration::from_secs(60)) => {}
            }
        }
    }
}

fn daemon_sleep_duration(behavior: &BehaviorConfig) -> Duration {
    behavior.auto_upload_interval.max(Duration::from_secs(60))
        + jitter_duration(behavior.auto_upload_jitter_max)
}

fn jitter_duration(max: Duration) -> Duration {
    let max_secs = max.as_secs();
    if max_secs == 0 {
        return Duration::ZERO;
    }

    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    Duration::from_secs((seed % (u128::from(max_secs) + 1)) as u64)
}

async fn run_daemon_sync_cycle(
    paths: &AppPaths,
    config: Config,
    state: &mut DaemonSyncState,
    report_summary: &impl Fn(&SyncSummary),
) {
    let behavior = config.behavior.clone();
    state.last_started_at = Some(Utc::now());
    state.last_error = None;
    save_daemon_sync_state(paths, state);

    match SyncService::new(paths.clone(), config).run_once().await {
        Ok(summary) => {
            state.last_completed_at = Some(Utc::now());
            state.last_error = None;
            report_summary(&summary);
        }
        Err(error) => {
            state.last_error = Some(error.to_string());
            tracing::warn!(%error, "sync cycle failed");
        }
    }
    state.next_sync_after_at = Some(next_sync_after(&behavior));
    save_daemon_sync_state(paths, state);
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct DaemonSyncState {
    last_started_at: Option<DateTime<Utc>>,
    last_completed_at: Option<DateTime<Utc>>,
    next_sync_after_at: Option<DateTime<Utc>>,
    last_error: Option<String>,
}

fn ensure_next_sync_scheduled(paths: &AppPaths, config: &Config, state: &mut DaemonSyncState) {
    if state.next_sync_after_at.is_none() {
        state.next_sync_after_at = Some(next_sync_after(&config.behavior));
        save_daemon_sync_state(paths, state);
    }
}

fn next_sync_after(behavior: &BehaviorConfig) -> DateTime<Utc> {
    Utc::now()
        + chrono::Duration::from_std(daemon_sleep_duration(behavior))
            .expect("daemon sync interval fits in chrono::Duration")
}

fn duration_until(time: DateTime<Utc>) -> Duration {
    time.signed_duration_since(Utc::now())
        .to_std()
        .unwrap_or(Duration::ZERO)
}

fn load_daemon_sync_state(paths: &AppPaths) -> DaemonSyncState {
    let path = paths.sync_state_file();
    match fs::read_to_string(&path) {
        Ok(content) => match toml::from_str(&content) {
            Ok(state) => state,
            Err(error) => {
                tracing::warn!(%error, path = %path.display(), "failed to parse daemon sync state");
                DaemonSyncState::default()
            }
        },
        Err(error) if error.kind() == ErrorKind::NotFound => DaemonSyncState::default(),
        Err(error) => {
            tracing::warn!(%error, path = %path.display(), "failed to read daemon sync state");
            DaemonSyncState::default()
        }
    }
}

fn save_daemon_sync_state(paths: &AppPaths, state: &DaemonSyncState) {
    if let Err(error) = write_daemon_sync_state(paths, state) {
        tracing::warn!(%error, "failed to write daemon sync state");
    }
}

fn write_daemon_sync_state(paths: &AppPaths, state: &DaemonSyncState) -> Result<()> {
    fs::create_dir_all(&paths.data_dir)
        .with_context(|| format!("failed to create {}", paths.data_dir.display()))?;
    let path = paths.sync_state_file();
    let temp_path = path.with_extension("toml.part");
    let content = toml::to_string_pretty(state).context("failed to serialize daemon sync state")?;
    fs::write(&temp_path, content)
        .with_context(|| format!("failed to write daemon sync state {}", temp_path.display()))?;
    fs::rename(&temp_path, &path).with_context(|| {
        format!(
            "failed to move daemon sync state {} to {}",
            temp_path.display(),
            path.display()
        )
    })
}
