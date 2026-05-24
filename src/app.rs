use std::time::Duration;

use crate::config::{AccountConfig, BehaviorConfig, PlayerPlatform, TargetAuth};
use crate::paths::AppPaths;
use crate::sync::{SyncOptions, SyncService};
use crate::Config;

#[derive(Clone, Debug, PartialEq)]
pub struct AppSummary {
    pub config_path: String,
    pub accounts: Vec<AccountSummary>,
    pub upload_destinations: Vec<UploadDestinationSummary>,
    pub auto_upload: bool,
    pub upload_on_launch: bool,
    pub no_upload_while_connected: bool,
    pub selected_account: Option<String>,
    pub selected_upload_destination: Option<String>,
    pub auto_upload_interval_minutes: u64,
    pub auto_upload_jitter_minutes: u64,
    pub interval: String,
    pub jitter: String,
    pub status: String,
}

impl AppSummary {
    pub fn account_count(&self) -> usize {
        self.accounts.len()
    }

    pub fn upload_destination_count(&self) -> usize {
        self.upload_destinations.len()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AccountSummary {
    pub id: u32,
    pub name: String,
    pub platform: String,
    pub sync_enabled: bool,
    pub selected: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountFormData {
    pub name: String,
    pub platform: String,
    pub sync_enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OverviewConfigFormData {
    pub auto_upload_interval_minutes: String,
    pub auto_upload_jitter_minutes: String,
    pub upload_on_launch: bool,
    pub no_upload_while_connected: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UploadDestinationSummary {
    pub name: String,
    pub url: String,
    pub upload_enabled: bool,
    pub automatic: bool,
    pub auth: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryRow {
    pub account: String,
    pub match_id: String,
    pub timestamp: String,
    pub map_name: String,
    pub playlist: String,
    pub score: String,
    pub upload_destinations: Vec<HistoryUploadDestination>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryUploadDestination {
    pub target_name: String,
    pub state: String,
    pub uploaded: bool,
    pub upload_enabled: bool,
    pub location: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayUploadRequest {
    pub target_name: String,
    pub match_id: String,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SyncRunState {
    pub running: bool,
    pub last_started_at: Option<String>,
    pub last_completed_at: Option<String>,
    pub last_summary: Option<BackfillSummary>,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackfillSummary {
    pub uploaded: usize,
    pub duplicates: usize,
    pub cached: usize,
    pub failed: usize,
    pub failed_match_ids: Vec<String>,
    pub failed_uploads: Vec<ReplayUploadRequest>,
}

pub fn load_summary() -> AppSummary {
    match AppPaths::discover() {
        Ok(paths) => {
            let config_path = paths.config_file();
            let config = Config::load_or_default(&config_path).unwrap_or_default();
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
        Err(error) => AppSummary {
            config_path: error.to_string(),
            accounts: Vec::new(),
            upload_destinations: Vec::new(),
            auto_upload: false,
            upload_on_launch: false,
            no_upload_while_connected: false,
            selected_account: None,
            selected_upload_destination: None,
            auto_upload_interval_minutes: 45,
            auto_upload_jitter_minutes: 15,
            interval: "Unavailable".to_string(),
            jitter: "Unavailable".to_string(),
            status: "Could not discover local app paths".to_string(),
        },
    }
}

pub async fn load_history() -> Result<Vec<HistoryRow>, String> {
    let paths = AppPaths::discover().map_err(|error| error.to_string())?;
    let config_path = paths.config_file();
    let config = Config::load_or_default(&config_path).map_err(|error| error.to_string())?;
    let service = SyncService::new(paths, config);
    let entries = service
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
    let paths = AppPaths::discover().map_err(|error| error.to_string())?;
    paths.ensure().map_err(|error| error.to_string())?;
    let config_path = paths.config_file();
    let config = Config::load_or_default(&config_path).map_err(|error| error.to_string())?;
    let summary = SyncService::new(paths, config)
        .run_once_with_options(SyncOptions {
            include_online: true,
            target_name: None,
            force: false,
            match_ids: Vec::new(),
        })
        .await
        .map_err(|error| error.to_string())?;

    Ok(backfill_summary_from_sync_summary(summary))
}

pub async fn upload_history_replay(
    request: ReplayUploadRequest,
) -> Result<BackfillSummary, String> {
    let paths = AppPaths::discover().map_err(|error| error.to_string())?;
    paths.ensure().map_err(|error| error.to_string())?;
    let config_path = paths.config_file();
    let config = Config::load_or_default(&config_path).map_err(|error| error.to_string())?;
    let summary = SyncService::new(paths, config)
        .run_once_with_options(SyncOptions {
            include_online: true,
            target_name: Some(request.target_name),
            force: true,
            match_ids: vec![request.match_id],
        })
        .await
        .map_err(|error| error.to_string())?;

    if summary.matches_seen == 0 {
        return Err("No matching replay was found in current RL API history".to_string());
    }

    Ok(backfill_summary_from_sync_summary(summary))
}

pub fn add_account(input: AccountFormData) -> Result<AppSummary, String> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err("Account name is required".to_string());
    }

    let platform = parse_platform(&input.platform)?;

    update_config(|config| {
        if config.accounts.iter().any(|account| account.name == name) {
            return Err(format!("Account {name:?} already exists"));
        }

        let next_id = config
            .accounts
            .iter()
            .map(|account| account.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        config.accounts.push(AccountConfig::new(
            next_id,
            name.to_string(),
            platform.clone(),
            input.sync_enabled,
        ));
        Ok(())
    })
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

    update_behavior(|behavior| {
        behavior.auto_upload_interval = Duration::from_secs(interval_minutes * 60);
        behavior.auto_upload_jitter_max = Duration::from_secs(jitter_minutes * 60);
        behavior.upload_on_launch = input.upload_on_launch;
        behavior.no_upload_while_connected = input.no_upload_while_connected;
    })
}

pub fn short_match_id(match_id: &str) -> &str {
    match_id.get(..8).unwrap_or(match_id)
}

pub fn now_label() -> String {
    chrono::Local::now()
        .format("%Y-%m-%d %H:%M:%S %Z")
        .to_string()
}

pub fn format_backfill_message(message: String, failed_uploads: &[ReplayUploadRequest]) -> String {
    if failed_uploads.is_empty() {
        message
    } else {
        let suffix = if failed_uploads.len() == 1 {
            String::new()
        } else {
            format!("; {} total blocked/failed uploads", failed_uploads.len())
        };
        format!(
            "{message}; first issue: {}{suffix}",
            format_failed_upload(&failed_uploads[0])
        )
    }
}

pub fn dedupe_upload_requests(requests: Vec<ReplayUploadRequest>) -> Vec<ReplayUploadRequest> {
    let mut deduped = Vec::new();
    for request in requests {
        upsert_failed_upload(&mut deduped, request);
    }
    deduped
}

pub fn upsert_failed_upload(
    failed_uploads: &mut Vec<ReplayUploadRequest>,
    request: ReplayUploadRequest,
) {
    if let Some(existing) = failed_uploads
        .iter_mut()
        .find(|failure| is_same_upload_request(failure, &request))
    {
        *existing = request;
    } else {
        failed_uploads.push(request);
    }
}

pub fn is_same_upload_request(left: &ReplayUploadRequest, right: &ReplayUploadRequest) -> bool {
    is_same_upload(left, &right.target_name, &right.match_id)
}

pub fn is_same_upload(request: &ReplayUploadRequest, target_name: &str, match_id: &str) -> bool {
    request.target_name == target_name && request.match_id == match_id
}

pub fn failed_upload<'a>(
    failed_uploads: &'a [ReplayUploadRequest],
    target_name: &str,
    match_id: &str,
) -> Option<&'a ReplayUploadRequest> {
    failed_uploads
        .iter()
        .find(|failure| is_same_upload(failure, target_name, match_id))
}

pub fn upload_failure_reason(
    failed_uploads: &[ReplayUploadRequest],
    target_name: &str,
    match_id: &str,
) -> String {
    failed_upload(failed_uploads, target_name, match_id)
        .and_then(|failure| failure.reason.clone())
        .unwrap_or_default()
}

pub fn format_failed_upload(failure: &ReplayUploadRequest) -> String {
    let base = format!(
        "{} to {}",
        short_match_id(&failure.match_id),
        failure.target_name
    );
    match &failure.reason {
        Some(reason) => format!("{base}: {reason}"),
        None => base,
    }
}

pub fn format_failed_upload_retry_label(failure: &ReplayUploadRequest) -> String {
    format!("Retry {}", format_failed_upload(failure))
}

pub fn auto_upload_label(summary: &AppSummary) -> &'static str {
    if summary.auto_upload {
        "enabled"
    } else {
        "disabled"
    }
}

pub fn tray_sync_label(sync_run: &SyncRunState) -> String {
    if sync_run.running {
        return sync_run
            .last_started_at
            .as_ref()
            .map(|started| format!("Sync running since {started}"))
            .unwrap_or_else(|| "Sync running".to_string());
    }

    if let Some(error) = &sync_run.last_error {
        return sync_run
            .last_completed_at
            .as_ref()
            .map(|completed| format!("Last sync failed at {completed}: {error}"))
            .unwrap_or_else(|| format!("Last sync failed: {error}"));
    }

    match (&sync_run.last_completed_at, &sync_run.last_summary) {
        (Some(completed), Some(summary)) => format!(
            "Last sync {completed}: {} uploaded, {} duplicate, {} cached, {} failed",
            summary.uploaded, summary.duplicates, summary.cached, summary.failed
        ),
        (Some(completed), None) => format!("Last sync {completed}"),
        _ => "No sync run yet".to_string(),
    }
}

pub fn tray_tooltip(summary: &AppSummary, sync_run: &SyncRunState, failed_count: usize) -> String {
    format!(
        "rlru\n{}\nAuto upload: {}, {}\nFailed uploads: {}",
        tray_sync_label(sync_run),
        auto_upload_label(summary),
        summary.interval,
        failed_count
    )
}

fn update_config(
    mut update: impl FnMut(&mut Config) -> Result<(), String>,
) -> Result<AppSummary, String> {
    let paths = AppPaths::discover().map_err(|error| error.to_string())?;
    paths.ensure().map_err(|error| error.to_string())?;
    let config_path = paths.config_file();
    let mut config = Config::load_or_default(&config_path).map_err(|error| error.to_string())?;
    update(&mut config)?;
    config
        .save(&config_path)
        .map_err(|error| error.to_string())?;
    Ok(load_summary())
}

fn update_behavior(mut update: impl FnMut(&mut BehaviorConfig)) -> Result<AppSummary, String> {
    update_config(|config| {
        update(&mut config.behavior);
        Ok(())
    })
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

fn platform_label(platform: &PlayerPlatform) -> &'static str {
    match platform {
        PlayerPlatform::Epic => "Epic",
        PlayerPlatform::Steam => "Steam",
        PlayerPlatform::PlayStation => "PlayStation",
        PlayerPlatform::Xbox => "Xbox",
        PlayerPlatform::Nintendo => "Nintendo",
    }
}

fn auth_label(auth: &TargetAuth) -> String {
    match auth {
        TargetAuth::None => "No auth".to_string(),
        TargetAuth::AuthorizationHeader { .. } => "Authorization header".to_string(),
        TargetAuth::Bearer { .. } => "Bearer token".to_string(),
        TargetAuth::BearerEnv { variable } => {
            if std::env::var_os(variable).is_some() {
                format!("Bearer env token ({variable})")
            } else {
                format!("Bearer env token missing ({variable})")
            }
        }
        TargetAuth::BearerCommand { command } => command
            .first()
            .map(|program| format!("Bearer command token ({program})"))
            .unwrap_or_else(|| "Bearer command token missing command".to_string()),
    }
}

fn format_record_start_timestamp(timestamp: i64) -> String {
    use chrono::TimeZone;

    chrono::Local
        .timestamp_opt(timestamp, 0)
        .single()
        .map(|datetime| datetime.format("%Y-%m-%d %H:%M:%S %Z").to_string())
        .unwrap_or_else(|| timestamp.to_string())
}

fn backfill_summary_from_sync_summary(summary: crate::sync::SyncSummary) -> BackfillSummary {
    BackfillSummary {
        uploaded: summary.uploaded,
        duplicates: summary.duplicates,
        cached: summary.cached,
        failed: summary.failed,
        failed_match_ids: summary.failed_match_ids,
        failed_uploads: summary
            .failed_uploads
            .into_iter()
            .map(|failed| ReplayUploadRequest {
                target_name: failed.target_name,
                match_id: failed.match_id,
                reason: Some(failed.reason),
            })
            .collect(),
    }
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
}
