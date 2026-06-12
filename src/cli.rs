use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use rlru::auth::AuthManager;
use rlru::config::{AccountConfig, Config, PlayerPlatform};
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
    Account {
        #[command(subcommand)]
        command: AccountCommand,
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
enum AccountCommand {
    List,
    Add {
        name: String,
        #[arg(long, value_enum, default_value_t = AccountPlatform::Epic)]
        platform: AccountPlatform,
        #[arg(long)]
        no_sync: bool,
        #[arg(long = "no-select", action = clap::ArgAction::SetFalse, default_value_t = true)]
        select: bool,
        #[arg(long, alias = "auth")]
        authenticate: bool,
        #[arg(long, requires = "authenticate")]
        open: bool,
    },
    Remove {
        account: String,
    },
    Select {
        account: String,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum AccountPlatform {
    Epic,
    Steam,
    PlayStation,
    Xbox,
    Nintendo,
}

impl AccountPlatform {
    fn into_player_platform(self) -> PlayerPlatform {
        match self {
            Self::Epic => PlayerPlatform::Epic,
            Self::Steam => PlayerPlatform::Steam,
            Self::PlayStation => PlayerPlatform::PlayStation,
            Self::Xbox => PlayerPlatform::Xbox,
            Self::Nintendo => PlayerPlatform::Nintendo,
        }
    }
}

impl std::fmt::Display for AccountPlatform {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Epic => "epic",
            Self::Steam => "steam",
            Self::PlayStation => "play-station",
            Self::Xbox => "xbox",
            Self::Nintendo => "nintendo",
        };
        formatter.write_str(value)
    }
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
        Command::Account { command } => handle_account_command(command, &paths, &config_path).await,
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

async fn handle_account_command(
    command: AccountCommand,
    paths: &AppPaths,
    config_path: &Path,
) -> Result<()> {
    match command {
        AccountCommand::List => {
            let config = Config::load_or_default(config_path)?;
            for account in &config.accounts {
                let selected = if config.behavior.selected_account.as_ref() == Some(&account.name) {
                    " selected"
                } else {
                    ""
                };
                let sync = if account.sync_enabled {
                    "sync"
                } else {
                    "sync disabled"
                };
                println!(
                    "{}\t{}\t{:?}\t{}{selected}",
                    account.id, account.name, account.platform, sync
                );
            }
            Ok(())
        }
        AccountCommand::Add {
            name,
            platform,
            no_sync,
            select,
            authenticate,
            open,
        } => {
            let platform = platform.into_player_platform();
            if authenticate && platform != PlayerPlatform::Epic {
                bail!("only Epic accounts can be authenticated");
            }
            let account = add_config_account(config_path, name, platform, !no_sync, select)?;
            println!("added account {} ({})", account.name, account.id);
            if authenticate {
                authenticate_account(paths, &account, open).await?;
            } else {
                println!(
                    "authenticate it with: rlru auth --account {} device --open --wait",
                    shell_quote(&account.name)
                );
            }
            Ok(())
        }
        AccountCommand::Remove { account } => {
            let mut config = Config::load(config_path)?;
            let index = resolve_account_index(&config, &account)?;
            let removed = config.accounts.remove(index);
            if config.behavior.selected_account.as_ref() == Some(&removed.name) {
                config.behavior.selected_account = None;
            }
            config.save(config_path)?;
            println!("removed account {}", removed.name);
            Ok(())
        }
        AccountCommand::Select { account } => {
            let mut config = Config::load(config_path)?;
            let index = resolve_account_index(&config, &account)?;
            let selected = config.accounts[index].name.clone();
            config.behavior.selected_account = Some(selected.clone());
            config.save(config_path)?;
            println!("selected account {selected}");
            Ok(())
        }
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

fn add_config_account(
    config_path: &Path,
    name: String,
    platform: PlayerPlatform,
    sync_enabled: bool,
    select: bool,
) -> Result<AccountConfig> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        bail!("account name cannot be empty");
    }

    let mut config = Config::load_or_default(config_path)?;
    if config
        .accounts
        .iter()
        .any(|account| account.name == trimmed)
    {
        bail!("account {trimmed:?} already exists");
    }

    let account = if !config_path.exists() && config.accounts == vec![AccountConfig::default()] {
        config.accounts[0] = AccountConfig::new(0, trimmed.to_string(), platform, sync_enabled);
        config.accounts[0].clone()
    } else {
        let next_id = config
            .accounts
            .iter()
            .map(|account| account.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        let account = AccountConfig::new(next_id, trimmed.to_string(), platform, sync_enabled);
        config.accounts.push(account.clone());
        account
    };

    if select {
        config.behavior.selected_account = Some(account.name.clone());
    }
    config.save(config_path)?;
    Ok(account)
}

async fn authenticate_account(paths: &AppPaths, account: &AccountConfig, open: bool) -> Result<()> {
    let auth = AuthManager::for_account(paths, account);
    let device = auth.begin_device_auth().await?;
    println!("visit {}", device.verification_uri);
    println!("enter code {}", device.user_code);
    if open {
        webbrowser::open(&device.verification_uri).context("failed to open browser")?;
    }
    let eos = auth.wait_for_device_auth(&device).await?;
    println!("authenticated Epic account {}", eos.account_id);
    Ok(())
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

fn resolve_account_index(config: &Config, account: &str) -> Result<usize> {
    let account_id = account.parse::<u32>().ok();
    config
        .accounts
        .iter()
        .position(|candidate| candidate.name == account || account_id == Some(candidate.id))
        .with_context(|| format!("unknown account {account:?}"))
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

fn shell_quote(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/' | ':'))
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', r"'\''"))
    }
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
    fn account_add_replaces_default_account_when_creating_config() {
        let tmp = tempfile::tempdir().unwrap();
        let config_path = tmp.path().join("config.toml");

        let account = add_config_account(
            &config_path,
            "Main".to_string(),
            PlayerPlatform::Epic,
            true,
            true,
        )
        .unwrap();
        let config = Config::load(&config_path).unwrap();

        assert_eq!(account.id, 0);
        assert_eq!(config.accounts.len(), 1);
        assert_eq!(config.accounts[0].name, "Main");
        assert_eq!(config.behavior.selected_account.as_deref(), Some("Main"));
    }

    #[test]
    fn account_add_appends_to_existing_config() {
        let tmp = tempfile::tempdir().unwrap();
        let config_path = tmp.path().join("config.toml");
        Config::default().save(&config_path).unwrap();

        let account = add_config_account(
            &config_path,
            "Alt".to_string(),
            PlayerPlatform::Epic,
            false,
            false,
        )
        .unwrap();
        let config = Config::load(&config_path).unwrap();

        assert_eq!(account.id, 1);
        assert_eq!(config.accounts.len(), 2);
        assert_eq!(config.accounts[1].name, "Alt");
        assert!(!config.accounts[1].sync_enabled);
        assert_eq!(config.behavior.selected_account, None);
    }

    #[test]
    fn shell_quote_handles_spaces() {
        assert_eq!(shell_quote("Main Account"), "'Main Account'");
    }

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
