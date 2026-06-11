use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use rlru::auth::AuthManager;
use rlru::config::Config;
use rlru::daemon::run_sync_daemon;
use rlru::paths::AppPaths;
use rlru::sync::{SyncOptions, SyncService};
use rlru::upload::{ReplayUploader, UploadCache, UploadOutcome};

#[derive(Debug, Parser)]
#[command(version = rlru::version::LONG_VERSION, about)]
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

pub async fn run() -> Result<()> {
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
            print!("{summary}");
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
            let target_name = resolve_backfill_target(&config, target, rocket_sense, all_targets);
            let summary = SyncService::new(paths, config)
                .run_once_with_options(SyncOptions {
                    include_online: !respect_online_guard,
                    target_name,
                    force,
                    match_ids,
                })
                .await?;
            print!("{summary}");
            Ok(())
        }
        SyncCommand::Daemon => {
            run_sync_daemon(paths, config_path, |summary| print!("{summary}")).await
        }
    }
}

fn resolve_backfill_target(
    config: &Config,
    target: Option<String>,
    rocket_sense: bool,
    all_targets: bool,
) -> Option<String> {
    if all_targets {
        None
    } else if rocket_sense {
        Some("Rocket Sense".to_string())
    } else if target.is_some() {
        target
    } else if config.upload_destination("Rocket Sense").is_some() {
        Some("Rocket Sense".to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backfill_defaults_to_rocket_sense_when_available() {
        let config = Config::default();

        assert_eq!(
            resolve_backfill_target(&config, None, false, false),
            Some("Rocket Sense".to_string())
        );
    }

    #[test]
    fn backfill_all_targets_overrides_other_target_choices() {
        let config = Config::default();

        assert_eq!(
            resolve_backfill_target(&config, Some("Ballchasing".to_string()), true, true),
            None
        );
    }

    #[test]
    fn explicit_backfill_target_is_used_when_not_all_targets() {
        let config = Config::default();

        assert_eq!(
            resolve_backfill_target(&config, Some("Ballchasing".to_string()), false, false),
            Some("Ballchasing".to_string())
        );
    }
}
