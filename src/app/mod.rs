use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

mod format;
mod model;

pub use format::*;
pub use model::*;

use format::{auth_label, format_record_start_timestamp, platform_label};

use crate::auth::AuthManager;
use crate::config::{AccountConfig, BehaviorConfig, PlayerPlatform, WindowDecorationsConfig};
use crate::paths::AppPaths;
use crate::state_file::write_atomically;
use crate::sync::{SyncOptions, SyncService};
use crate::Config;

pub const MAX_CONCURRENT_UPLOADS: usize = crate::sync::MAX_CONCURRENT_UPLOADS;

#[derive(Debug, Default, serde::Deserialize, serde::Serialize)]
struct UploadFailureState {
    failed_uploads: Vec<ReplayUploadRequest>,
}

struct AppContext {
    paths: AppPaths,
    config_path: std::path::PathBuf,
    config: Config,
}

impl AppContext {
    fn load(ensure_paths: bool) -> Result<Self, String> {
        let paths = AppPaths::discover().map_err(|error| error.to_string())?;
        if ensure_paths {
            paths.ensure().map_err(|error| error.to_string())?;
        }
        let config_path = paths.config_file();
        let config = Config::load_or_default(&config_path).map_err(|error| error.to_string())?;
        Ok(Self {
            paths,
            config_path,
            config,
        })
    }

    fn sync_service(self) -> SyncService {
        SyncService::new(self.paths, self.config)
    }

    fn save_config(&self) -> Result<(), String> {
        self.config
            .save(&self.config_path)
            .map_err(|error| error.to_string())
    }
}

pub fn load_summary() -> AppSummary {
    match AppContext::load(false) {
        Ok(context) => summary_from_config(&context.paths, &context.config_path, &context.config),
        Err(error) => unavailable_summary(error),
    }
}

pub fn load_persisted_failed_uploads() -> Result<Vec<ReplayUploadRequest>, String> {
    let context = AppContext::load(true)?;
    read_failed_upload_state(&context.paths.upload_failures_file())
}

pub fn save_persisted_failed_uploads(failed_uploads: &[ReplayUploadRequest]) -> Result<(), String> {
    let context = AppContext::load(true)?;
    write_failed_upload_state(&context.paths.upload_failures_file(), failed_uploads)
}

fn read_failed_upload_state(path: &Path) -> Result<Vec<ReplayUploadRequest>, String> {
    match std::fs::read_to_string(path) {
        Ok(contents) => {
            let state: UploadFailureState = toml::from_str(&contents)
                .map_err(|error| format!("failed to parse {}: {error}", path.display()))?;
            Ok(dedupe_upload_requests(state.failed_uploads))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(format!("failed to read {}: {error}", path.display())),
    }
}

fn write_failed_upload_state(
    path: &Path,
    failed_uploads: &[ReplayUploadRequest],
) -> Result<(), String> {
    let state = UploadFailureState {
        failed_uploads: dedupe_upload_requests(failed_uploads.to_vec()),
    };
    let contents = toml::to_string_pretty(&state)
        .map_err(|error| format!("failed to serialize upload failures: {error}"))?;
    write_atomically(path, contents).map_err(|error| error.to_string())
}

fn summary_from_config(
    paths: &AppPaths,
    config_path: &std::path::Path,
    config: &Config,
) -> AppSummary {
    let selected_account = config.behavior.selected_account.clone();
    let selected_upload_destination = config.behavior.selected_upload_destination.clone();
    let auto_upload = config.behavior.auto_upload;
    AppSummary {
        config_path: config_path.display().to_string(),
        accounts: config
            .accounts
            .iter()
            .map(|account| AccountSummary {
                id: account.id,
                name: account.name.clone(),
                platform: platform_label(&account.platform).to_string(),
                sync_enabled: account.sync_enabled,
                selected: selected_account.as_ref() == Some(&account.name),
                saved_auth: account.platform == PlayerPlatform::Epic
                    && AuthManager::for_account(paths, account)
                        .has_saved_login()
                        .unwrap_or(false),
            })
            .collect(),
        upload_destinations: config
            .upload_destinations
            .iter()
            .map(|target| UploadDestinationSummary {
                name: target.name.clone(),
                url: target.url.to_string(),
                upload_enabled: target.replay_upload.enabled,
                automatic: auto_upload
                    && target.replay_upload.enabled
                    && selected_upload_destination
                        .as_ref()
                        .is_none_or(|selected| selected == &target.name),
                auth: auth_label(&target.auth),
            })
            .collect(),
        auto_upload,
        upload_on_launch: config.behavior.upload_on_launch,
        no_upload_while_connected: config.behavior.no_upload_while_connected,
        window_decorations: window_decorations_value(config.behavior.window_decorations)
            .to_string(),
        selected_account,
        selected_upload_destination,
        auto_upload_interval_minutes: config.behavior.auto_upload_interval.as_secs() / 60,
        auto_upload_jitter_minutes: config.behavior.auto_upload_jitter_max.as_secs() / 60,
        interval: format!(
            "Every {} minutes",
            config.behavior.auto_upload_interval.as_secs() / 60
        ),
        jitter: format!(
            "{} minutes",
            config.behavior.auto_upload_jitter_max.as_secs() / 60
        ),
        status: "Ready for auth, sync, and uploader runs".to_string(),
    }
}

fn unavailable_summary(error: String) -> AppSummary {
    AppSummary {
        config_path: error,
        accounts: Vec::new(),
        upload_destinations: Vec::new(),
        auto_upload: false,
        upload_on_launch: false,
        no_upload_while_connected: false,
        window_decorations: window_decorations_value(WindowDecorationsConfig::Auto).to_string(),
        selected_account: None,
        selected_upload_destination: None,
        auto_upload_interval_minutes: 45,
        auto_upload_jitter_minutes: 15,
        interval: "Unavailable".to_string(),
        jitter: "Unavailable".to_string(),
        status: "Could not load local app state".to_string(),
    }
}

pub async fn load_history() -> Result<Vec<HistoryRow>, String> {
    let entries = AppContext::load(false)?
        .sync_service()
        .current_history(None)
        .await
        .map_err(|error| error.to_string())?;

    Ok(entries
        .into_iter()
        .map(|entry| {
            let upload_destinations = entry
                .upload_states
                .into_iter()
                .map(|state| {
                    let label = if state.cached {
                        "Uploaded"
                    } else if !state.upload_enabled {
                        "Disabled"
                    } else {
                        "Not uploaded"
                    };
                    HistoryUploadDestination {
                        target_name: state.target_name,
                        state: label.to_string(),
                        uploaded: state.cached,
                        upload_enabled: state.upload_enabled,
                        location: state.location,
                    }
                })
                .collect();
            HistoryRow {
                account: entry.account_name,
                match_id: entry.match_id,
                timestamp: format_record_start_timestamp(entry.record_start_timestamp),
                map_name: entry.map_name,
                playlist: entry.playlist.to_string(),
                score: format!("{}-{}", entry.team0_score, entry.team1_score),
                upload_destinations,
            }
        })
        .collect())
}

pub async fn backfill_upload_destinations() -> Result<BackfillSummary, String> {
    let summary = AppContext::load(true)?
        .sync_service()
        .run_once_with_options(SyncOptions {
            include_online: true,
            target_name: None,
            force: false,
            match_ids: Vec::new(),
        })
        .await
        .map_err(|error| error.to_string())?;

    Ok(summary.into())
}

pub async fn upload_history_replay(
    request: ReplayUploadRequest,
) -> Result<BackfillSummary, String> {
    upload_history_replays(vec![request]).await
}

pub async fn upload_history_replays(
    requests: Vec<ReplayUploadRequest>,
) -> Result<BackfillSummary, String> {
    if requests.is_empty() {
        return Ok(BackfillSummary::default());
    }

    let context = AppContext::load(true)?;
    let mut aggregate = BackfillSummary::default();

    for (target_name, match_ids) in grouped_upload_requests(requests) {
        let summary = match SyncService::new(context.paths.clone(), context.config.clone())
            .run_once_with_options(SyncOptions {
                include_online: true,
                target_name: Some(target_name.clone()),
                force: true,
                match_ids: match_ids.clone(),
            })
            .await
        {
            Ok(summary) => summary,
            Err(error) => {
                tracing::warn!(
                    target_name = %target_name,
                    match_ids = ?match_ids,
                    %error,
                    "upload request failed while running sync service"
                );
                return Err(error.to_string());
            }
        };

        if summary.matches_seen == 0 {
            tracing::warn!(
                target_name = %target_name,
                match_ids = ?match_ids,
                "upload request did not match any replay in current RL API history"
            );
            return Err("No matching replay was found in current RL API history".to_string());
        }

        merge_backfill_summary(&mut aggregate, summary.into());
    }

    Ok(aggregate)
}

fn grouped_upload_requests(requests: Vec<ReplayUploadRequest>) -> BTreeMap<String, Vec<String>> {
    let mut grouped = BTreeMap::<String, BTreeSet<String>>::new();
    for request in requests {
        grouped
            .entry(request.target_name)
            .or_default()
            .insert(request.match_id);
    }
    grouped
        .into_iter()
        .map(|(target_name, match_ids)| (target_name, match_ids.into_iter().collect()))
        .collect()
}

fn merge_backfill_summary(summary: &mut BackfillSummary, next: BackfillSummary) {
    summary.uploaded += next.uploaded;
    summary.duplicates += next.duplicates;
    summary.cached += next.cached;
    summary.failed += next.failed;
    summary.failed_match_ids.extend(next.failed_match_ids);
    for failed_upload in next.failed_uploads {
        upsert_failed_upload(&mut summary.failed_uploads, failed_upload);
    }
}

pub fn add_account(input: AccountFormData) -> Result<AppSummary, String> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err("Account name is required".to_string());
    }

    let platform = parse_platform(&input.platform)?;

    let mut context = AppContext::load(true)?;
    if context
        .config
        .accounts
        .iter()
        .any(|account| account.name == name)
    {
        return Err(format!("Account {name:?} already exists"));
    }

    let account = if !context.config_path.exists()
        && context.config.accounts == vec![AccountConfig::default()]
    {
        context.config.accounts[0] =
            AccountConfig::new(0, name.to_string(), platform, input.sync_enabled);
        context.config.accounts[0].clone()
    } else {
        let next_id = context
            .config
            .accounts
            .iter()
            .map(|account| account.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        let account = AccountConfig::new(next_id, name.to_string(), platform, input.sync_enabled);
        context.config.accounts.push(account.clone());
        account
    };

    context.config.behavior.selected_account = Some(account.name);
    context.save_config()?;
    Ok(load_summary())
}

pub fn begin_account_auth(account_id: u32) -> Result<AccountAuthPrompt, String> {
    let context = AppContext::load(true)?;
    let account = context
        .config
        .accounts
        .iter()
        .find(|account| account.id == account_id)
        .ok_or_else(|| format!("Account ID {account_id} no longer exists"))?;

    if account.platform != PlayerPlatform::Epic {
        return Err("Only Epic accounts can be authenticated".to_string());
    }

    let auth = AuthManager::for_account(&context.paths, account);
    Ok(AccountAuthPrompt {
        account_id,
        account_name: account.name.clone(),
        login_url: auth.login_url(),
    })
}

pub async fn finish_account_auth(
    prompt: AccountAuthPrompt,
    code: String,
) -> Result<String, String> {
    let context = AppContext::load(true)?;
    let account = context
        .config
        .accounts
        .iter()
        .find(|account| account.id == prompt.account_id)
        .ok_or_else(|| format!("Account ID {} no longer exists", prompt.account_id))?;
    let code = code.trim();
    if code.is_empty() {
        return Err("Epic authorization code is required".to_string());
    }

    let auth = AuthManager::for_account(&context.paths, account);
    let eos = auth
        .authenticate_with_code(code)
        .await
        .map_err(|error| error.to_string())?;
    Ok(format!(
        "Authenticated {} as Epic account {}",
        prompt.account_name, eos.account_id
    ))
}

pub fn remove_account(account_id: u32) -> Result<AppSummary, String> {
    update_config(|config| {
        if config.accounts.len() <= 1 {
            return Err("Config must keep at least one account".to_string());
        }

        let Some(index) = config
            .accounts
            .iter()
            .position(|account| account.id == account_id)
        else {
            return Err(format!("Account ID {account_id} no longer exists"));
        };

        let removed = config.accounts.remove(index);
        if config.behavior.selected_account.as_ref() == Some(&removed.name) {
            config.behavior.selected_account = None;
        }
        Ok(())
    })
}

pub fn save_auto_upload(enabled: bool) -> Result<AppSummary, String> {
    update_behavior(|behavior| behavior.auto_upload = enabled)
}

pub fn save_overview_config(input: OverviewConfigFormData) -> Result<AppSummary, String> {
    let interval_minutes = parse_minutes(
        "sync interval",
        &input.auto_upload_interval_minutes,
        Some(1),
    )?;
    let jitter_minutes = parse_minutes("jitter max", &input.auto_upload_jitter_minutes, None)?;
    let window_decorations = parse_window_decorations(&input.window_decorations)?;

    update_behavior(|behavior| {
        behavior.auto_upload_interval = Duration::from_secs(interval_minutes * 60);
        behavior.auto_upload_jitter_max = Duration::from_secs(jitter_minutes * 60);
        behavior.upload_on_launch = input.upload_on_launch;
        behavior.no_upload_while_connected = input.no_upload_while_connected;
        behavior.window_decorations = window_decorations;
    })
}

fn update_config(
    mut update: impl FnMut(&mut Config) -> Result<(), String>,
) -> Result<AppSummary, String> {
    let mut context = AppContext::load(true)?;
    update(&mut context.config)?;
    context.save_config()?;
    Ok(load_summary())
}

fn update_behavior(mut update: impl FnMut(&mut BehaviorConfig)) -> Result<AppSummary, String> {
    update_config(|config| {
        update(&mut config.behavior);
        Ok(())
    })
}

fn window_decorations_value(config: WindowDecorationsConfig) -> &'static str {
    match config {
        WindowDecorationsConfig::Auto => "auto",
        WindowDecorationsConfig::System => "system",
        WindowDecorationsConfig::Hidden => "hidden",
    }
}

fn parse_window_decorations(value: &str) -> Result<WindowDecorationsConfig, String> {
    match value {
        "auto" => Ok(WindowDecorationsConfig::Auto),
        "system" => Ok(WindowDecorationsConfig::System),
        "hidden" => Ok(WindowDecorationsConfig::Hidden),
        _ => Err(format!("Unsupported window decorations setting {value:?}")),
    }
}

fn parse_platform(value: &str) -> Result<PlayerPlatform, String> {
    match value {
        "epic" => Ok(PlayerPlatform::Epic),
        "steam" => Ok(PlayerPlatform::Steam),
        "play_station" => Ok(PlayerPlatform::PlayStation),
        "xbox" => Ok(PlayerPlatform::Xbox),
        "nintendo" => Ok(PlayerPlatform::Nintendo),
        _ => Err(format!("Unsupported platform {value:?}")),
    }
}

fn parse_minutes(label: &str, value: &str, minimum: Option<u64>) -> Result<u64, String> {
    let minutes = value
        .trim()
        .parse::<u64>()
        .map_err(|_| format!("{label} must be a whole number of minutes"))?;

    if let Some(minimum) = minimum {
        if minutes < minimum {
            return Err(format!("{label} must be at least {minimum} minute"));
        }
    }

    Ok(minutes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedupes_failed_uploads_by_target_and_match() {
        let requests = vec![
            ReplayUploadRequest {
                target_name: "Rocket Sense".to_string(),
                match_id: "abc".to_string(),
                reason: Some("old".to_string()),
            },
            ReplayUploadRequest {
                target_name: "Rocket Sense".to_string(),
                match_id: "abc".to_string(),
                reason: Some("new".to_string()),
            },
        ];

        assert_eq!(
            dedupe_upload_requests(requests),
            vec![ReplayUploadRequest {
                target_name: "Rocket Sense".to_string(),
                match_id: "abc".to_string(),
                reason: Some("new".to_string()),
            }]
        );
    }

    #[test]
    fn missing_failed_upload_state_loads_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("upload-failures.toml");

        assert_eq!(read_failed_upload_state(&path).unwrap(), Vec::new());
    }

    #[test]
    fn failed_upload_state_round_trips_deduped_requests() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("upload-failures.toml");
        let old_request = ReplayUploadRequest {
            target_name: "Rocket Sense".to_string(),
            match_id: "abc".to_string(),
            reason: Some("old".to_string()),
        };
        let new_request = ReplayUploadRequest {
            target_name: "Rocket Sense".to_string(),
            match_id: "abc".to_string(),
            reason: Some("new".to_string()),
        };

        write_failed_upload_state(&path, &[old_request, new_request.clone()]).unwrap();

        assert_eq!(read_failed_upload_state(&path).unwrap(), vec![new_request]);
    }

    #[test]
    fn formats_failed_backfill_message_with_first_issue() {
        let message = format_backfill_message(
            "Backfill complete: 0 uploaded, 0 duplicates, 0 cached, 1 failed".to_string(),
            &[ReplayUploadRequest {
                target_name: "Rocket Sense".to_string(),
                match_id: "123456789".to_string(),
                reason: Some("token missing".to_string()),
            }],
        );

        assert_eq!(
            message,
            "Backfill complete: 0 uploaded, 0 duplicates, 0 cached, 1 failed; first issue: 12345678 to Rocket Sense: token missing"
        );
    }

    #[test]
    fn sync_run_state_transitions_preserve_previous_context() {
        let previous = SyncRunState {
            running: false,
            last_started_at: Some("old-start".to_string()),
            last_completed_at: Some("old-complete".to_string()),
            last_summary: Some(BackfillSummary {
                uploaded: 1,
                duplicates: 2,
                cached: 3,
                failed: 4,
                failed_match_ids: vec!["old-match".to_string()],
                failed_uploads: Vec::new(),
            }),
            last_error: Some("old-error".to_string()),
        };

        let running = previous.started("new-start".to_string());
        assert!(running.running);
        assert_eq!(running.last_started_at.as_deref(), Some("new-start"));
        assert_eq!(running.last_completed_at.as_deref(), Some("old-complete"));
        assert!(running.last_summary.is_some());
        assert_eq!(running.last_error, None);

        let completed = running.completed(
            "new-complete".to_string(),
            BackfillSummary {
                uploaded: 5,
                duplicates: 0,
                cached: 0,
                failed: 0,
                failed_match_ids: Vec::new(),
                failed_uploads: Vec::new(),
            },
        );
        assert!(!completed.running);
        assert_eq!(completed.last_started_at.as_deref(), Some("new-start"));
        assert_eq!(completed.last_completed_at.as_deref(), Some("new-complete"));
        assert_eq!(
            completed
                .last_summary
                .as_ref()
                .map(|summary| summary.uploaded),
            Some(5)
        );
        assert_eq!(completed.last_error, None);

        let failed = completed.failed("failed-at".to_string(), "boom".to_string());
        assert!(!failed.running);
        assert_eq!(failed.last_completed_at.as_deref(), Some("failed-at"));
        assert_eq!(
            failed.last_summary.as_ref().map(|summary| summary.uploaded),
            Some(5)
        );
        assert_eq!(failed.last_error.as_deref(), Some("boom"));
    }
}
