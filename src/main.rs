use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use clap::{Parser, Subcommand};
use rlru::auth::AuthManager;
use rlru::config::Config;
use rlru::paths::AppPaths;
use rlru::sync::{SyncOptions, SyncService};
use rlru::upload::{ReplayUploader, UploadCache, UploadOutcome};
use serde::{Deserialize, Serialize};

#[derive(Debug, Parser)]
#[command(version, about)]
struct Cli {
    #[arg(long)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    Auth {
        #[arg(long)]
        account: Option<String>,
        #[arg(long, hide = true)]
        profile_id: Option<u32>,
        #[command(subcommand)]
        command: AuthCommand,
    },
    UploadFile {
        #[arg(long)]
        target: String,
        #[arg(long)]
        path: PathBuf,
        #[arg(long)]
        match_id: Option<String>,
    },
    Sync {
        #[command(subcommand)]
        command: SyncCommand,
    },
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    Path,
    Defaults,
    Init {
        #[arg(long)]
        force: bool,
    },
    Validate,
}

#[derive(Debug, Subcommand)]
enum AuthCommand {
    LoginUrl {
        #[arg(long)]
        open: bool,
    },
    Code {
        code: String,
    },
    Device {
        #[arg(long)]
        open: bool,
        #[arg(long)]
        wait: bool,
    },
    Status,
    Clear,
}

#[derive(Debug, Subcommand)]
enum SyncCommand {
    Once,
    Backfill {
        #[arg(long)]
        target: Option<String>,
        #[arg(long)]
        rocket_sense: bool,
        #[arg(long)]
        all_targets: bool,
        #[arg(long)]
        respect_online_guard: bool,
        #[arg(long)]
        force: bool,
        #[arg(long = "match-id")]
        match_ids: Vec<String>,
    },
    Daemon,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .without_time()
        .init();

    let cli = Cli::parse();
    let paths = AppPaths::discover()?;
    paths.ensure()?;
    let config_path = cli.config.unwrap_or_else(|| paths.config_file());

    match cli.command {
        Command::Config { command } => handle_config_command(command, &config_path),
        Command::Auth {
            account,
            profile_id,
            command,
        } => handle_auth_command(command, &paths, &config_path, account, profile_id).await,
        Command::UploadFile {
            target,
            path,
            match_id,
        } => handle_upload_file(&paths, &config_path, &target, &path, match_id).await,
        Command::Sync { command } => handle_sync_command(command, paths, &config_path).await,
    }
}

fn handle_config_command(command: ConfigCommand, config_path: &Path) -> Result<()> {
    match command {
        ConfigCommand::Path => {
            println!("{}", config_path.display());
            Ok(())
        }
        ConfigCommand::Defaults => {
            print!("{}", Config::default().to_pretty_toml()?);
            Ok(())
        }
        ConfigCommand::Init { force } => {
            if config_path.exists() && !force {
                anyhow::bail!(
                    "config already exists at {}; pass --force to overwrite it",
                    config_path.display()
                );
            }
            Config::default().save(config_path)?;
            println!("wrote {}", config_path.display());
            Ok(())
        }
        ConfigCommand::Validate => {
            let config = Config::load(config_path)?;
            config.validate()?;
            println!("valid {}", config_path.display());
            Ok(())
        }
    }
}

async fn handle_auth_command(
    command: AuthCommand,
    paths: &AppPaths,
    config_path: &Path,
    account: Option<String>,
    profile_id: Option<u32>,
) -> Result<()> {
    let (auth, account_label) = resolve_auth_manager(paths, config_path, account, profile_id)?;
    match command {
        AuthCommand::LoginUrl { open } => {
            let url = auth.login_url();
            println!("{url}");
            if open {
                webbrowser::open(&url).context("failed to open browser")?;
            }
            Ok(())
        }
        AuthCommand::Code { code } => {
            let eos = auth.authenticate_with_code(&code).await?;
            println!("authenticated Epic account {}", eos.account_id);
            Ok(())
        }
        AuthCommand::Device { open, wait } => {
            let device = auth.begin_device_auth().await?;
            println!("visit {}", device.verification_uri);
            println!("enter code {}", device.user_code);
            if open {
                webbrowser::open(&device.verification_uri).context("failed to open browser")?;
            }
            if wait {
                let eos = auth.wait_for_device_auth(&device).await?;
                println!("authenticated Epic account {}", eos.account_id);
            }
            Ok(())
        }
        AuthCommand::Status => {
            let eos = auth.restore_or_refresh().await?;
            println!("authenticated Epic account {}", eos.account_id);
            println!("access token expires at {}", eos.expires_at);
            println!("refresh token expires at {}", eos.refresh_expires_at);
            Ok(())
        }
        AuthCommand::Clear => {
            auth.clear()?;
            println!("cleared tokens for {account_label}");
            Ok(())
        }
    }
}

fn resolve_auth_manager(
    paths: &AppPaths,
    config_path: &Path,
    account: Option<String>,
    profile_id: Option<u32>,
) -> Result<(AuthManager, String)> {
    if let Some(profile_id) = profile_id {
        return Ok((
            AuthManager::for_legacy_profile(paths, profile_id),
            format!("legacy profile {profile_id}"),
        ));
    }

    let config = Config::load_or_default(config_path)?;
    let account_config = match account {
        Some(account) => {
            let account_id = account.parse::<u32>().ok();
            config
                .accounts
                .iter()
                .find(|candidate| {
                    candidate.name == account.as_str() || account_id == Some(candidate.id)
                })
                .with_context(|| format!("unknown account {account:?}"))?
        }
        None => {
            if let Some(selected) = config.behavior.selected_account.as_deref() {
                config
                    .accounts
                    .iter()
                    .find(|candidate| candidate.name == selected)
                    .with_context(|| format!("selected account {selected:?} does not exist"))?
            } else {
                config
                    .accounts
                    .first()
                    .context("config must define at least one account")?
            }
        }
    };

    Ok((
        AuthManager::for_account(paths, account_config),
        format!("account {}", account_config.name),
    ))
}

async fn handle_upload_file(
    paths: &AppPaths,
    config_path: &Path,
    target_name: &str,
    file_path: &Path,
    match_id: Option<String>,
) -> Result<()> {
    let config = Config::load_or_default(config_path)?;
    let target = config
        .upload_destination(target_name)
        .with_context(|| format!("unknown upload destination {target_name:?}"))?;

    let mut cache =
        UploadCache::load(paths.upload_cache_path(&target.name), config.accounts.len())?;
    if let Some(match_id) = match_id.as_deref() {
        if cache.contains(match_id) {
            println!("skipped cached replay {match_id}");
            return Ok(());
        }
    }

    let uploader = ReplayUploader::new();
    let result = uploader
        .upload_replay_with_match_id(target, file_path, match_id.as_deref())
        .await?;
    if matches!(
        result.outcome,
        UploadOutcome::Uploaded | UploadOutcome::Duplicate
    ) {
        if let Some(match_id) = match_id {
            cache.add_with_location(match_id, result.location.clone())?;
        }
    }
    println!("{:?}", result.outcome);
    if let Some(location) = result.location {
        println!("location: {location}");
    }
    Ok(())
}

async fn handle_sync_command(
    command: SyncCommand,
    paths: AppPaths,
    config_path: &Path,
) -> Result<()> {
    match command {
        SyncCommand::Once => {
            let config = Config::load_or_default(config_path)?;
            let summary = SyncService::new(paths, config).run_once().await?;
            print_sync_summary(&summary);
            Ok(())
        }
        SyncCommand::Backfill {
            target,
            rocket_sense,
            all_targets,
            respect_online_guard,
            force,
            match_ids,
        } => {
            let config = Config::load_or_default(config_path)?;
            let target_name = if all_targets {
                None
            } else if rocket_sense {
                Some("Rocket Sense".to_string())
            } else if target.is_some() {
                target
            } else if config.upload_destination("Rocket Sense").is_some() {
                Some("Rocket Sense".to_string())
            } else {
                None
            };
            let summary = SyncService::new(paths, config)
                .run_once_with_options(SyncOptions {
                    include_online: !respect_online_guard,
                    target_name,
                    force,
                    match_ids,
                })
                .await?;
            print_sync_summary(&summary);
            Ok(())
        }
        SyncCommand::Daemon => run_sync_daemon(paths, config_path).await,
    }
}

async fn run_sync_daemon(paths: AppPaths, config_path: &Path) -> Result<()> {
    let config = Config::load_or_default(config_path)?;
    let mut state = load_daemon_sync_state(&paths);
    if config.behavior.upload_on_launch {
        run_daemon_sync_cycle(&paths, config, &mut state).await;
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
                run_daemon_sync_cycle(&paths, config, &mut state).await;
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

fn daemon_sleep_duration(behavior: &rlru::config::BehaviorConfig) -> Duration {
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

async fn run_daemon_sync_cycle(paths: &AppPaths, config: Config, state: &mut DaemonSyncState) {
    let behavior = config.behavior.clone();
    state.last_started_at = Some(Utc::now());
    state.last_error = None;
    save_daemon_sync_state(paths, state);

    match SyncService::new(paths.clone(), config).run_once().await {
        Ok(summary) => {
            state.last_completed_at = Some(Utc::now());
            state.last_error = None;
            print_sync_summary(&summary);
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

fn next_sync_after(behavior: &rlru::config::BehaviorConfig) -> DateTime<Utc> {
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

fn print_sync_summary(summary: &rlru::sync::SyncSummary) {
    println!("accounts: {}", summary.accounts_seen);
    println!("matches: {}", summary.matches_seen);
    println!("uploaded: {}", summary.uploaded);
    println!("duplicates: {}", summary.duplicates);
    println!("cached: {}", summary.cached);
    println!("skipped: {}", summary.skipped);
    println!("failed: {}", summary.failed);
    if !summary.failed_match_ids.is_empty() {
        println!("failed_match_ids: {}", summary.failed_match_ids.join(","));
    }
    for failure in &summary.failed_uploads {
        println!(
            "failed_upload: {} {}: {}",
            failure.target_name, failure.match_id, failure.reason
        );
    }
}
