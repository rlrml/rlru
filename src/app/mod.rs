use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

mod format;
mod model;

pub use format::*;
pub use model::*;

use format::{auth_label, format_record_start_timestamp, platform_label};

use url::Url;

use crate::auth::AuthManager;
use crate::config::{
    AccountConfig, BehaviorConfig, PingConfig, PlayerPlatform, RankUploadConfig,
    ReplayUploadConfig, TargetAuth, UploadDestinationConfig, WindowDecorationsConfig,
};
use crate::paths::AppPaths;
use crate::state_file::write_atomically;
use crate::sync::{SyncOptions, SyncService};
use crate::Config;

pub const MAX_CONCURRENT_UPLOADS: usize = crate::sync::MAX_CONCURRENT_UPLOADS;

/// Serializes app operations that open PsyNet sessions (uploads, sync runs,
/// history loads). Concurrent logins — especially for the same account — trip
/// PsyNet's LoginBanned throttling and race the websocket ("Sending after
/// closing is not allowed").
static PSYNET_SESSION_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Renders an anyhow error together with its full cause chain, so the root
/// cause (e.g. the PsyNet/HTTP error behind "failed to sync account X") is
/// preserved when the error is flattened into a user-facing string. Plain
/// `error.to_string()` shows only the outermost context.
pub(crate) fn error_chain(error: &anyhow::Error) -> String {
    error
        .chain()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(": ")
}

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
    let _psynet_guard = PSYNET_SESSION_LOCK.lock().await;
    let entries = AppContext::load(false)?
        .sync_service()
        .current_history(None)
        .await
        .map_err(|error| error_chain(&error))?;

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
    let _psynet_guard = PSYNET_SESSION_LOCK.lock().await;
    let summary = AppContext::load(true)?
        .sync_service()
        .run_once_with_options(SyncOptions {
            include_online: true,
            target_name: None,
            force: false,
            match_ids: Vec::new(),
        })
        .await
        .map_err(|error| error_chain(&error))?;

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

    let _psynet_guard = PSYNET_SESSION_LOCK.lock().await;
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
                let reason = error_chain(&error);
                tracing::warn!(
                    target_name = %target_name,
                    match_ids = ?match_ids,
                    error = %reason,
                    "upload request failed while running sync service"
                );
                return Err(reason);
            }
        };

        if summary.matches_seen == 0 {
            // With per-account isolation, an account whose sync failed no longer
            // aborts the run — it lands in sync_errors and simply contributes no
            // matches. Surface that instead of the misleading "not found".
            if !summary.sync_errors.is_empty() {
                let reason = summary.sync_errors.join("; ");
                tracing::warn!(
                    target_name = %target_name,
                    match_ids = ?match_ids,
                    error = %reason,
                    "upload request could not run because account sync failed"
                );
                return Err(reason);
            }
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
    for sync_error in next.sync_errors {
        if !summary.sync_errors.contains(&sync_error) {
            summary.sync_errors.push(sync_error);
        }
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

pub fn add_upload_destination(input: UploadDestinationFormData) -> Result<AppSummary, String> {
    let destination = parse_upload_destination(&input)?;
    update_config(|config| add_destination_to_config(config, destination.clone()))
}

pub fn update_upload_destination(
    original_name: &str,
    input: UploadDestinationFormData,
) -> Result<AppSummary, String> {
    let destination = parse_upload_destination(&input)?;
    update_config(|config| update_destination_in_config(config, original_name, destination.clone()))
}

pub fn remove_upload_destination(name: &str) -> Result<AppSummary, String> {
    update_config(|config| remove_destination_from_config(config, name))
}

/// Loads the configured destination `name` as editable form data.
pub fn upload_destination_form(name: &str) -> Result<UploadDestinationFormData, String> {
    let context = AppContext::load(false)?;
    let destination = context
        .config
        .upload_destination(name)
        .ok_or_else(|| format!("Upload destination {name:?} no longer exists"))?;
    Ok(upload_destination_form_data(destination))
}

/// Form data prefilled from one of the built-in destination presets
/// (`rocket_sense`, `ballchasing`, `rocky`) or generic defaults (`custom`).
pub fn upload_destination_preset_form(preset: &str) -> Result<UploadDestinationFormData, String> {
    match preset {
        "rocket_sense" => {
            let mut form = upload_destination_form_data(&UploadDestinationConfig::rocket_sense());
            // The built-in constructor reads the token from ROCKET_SENSE_TOKEN,
            // but the common UI path is pasting a literal API token.
            form.auth_kind = "bearer".to_string();
            form.auth_value = String::new();
            Ok(form)
        }
        "ballchasing" => {
            let mut form = upload_destination_form_data(&UploadDestinationConfig::ballchasing());
            // Ballchasing authenticates with a raw Authorization header token.
            form.auth_kind = "authorization_header".to_string();
            form.auth_value = String::new();
            Ok(form)
        }
        "rocky" => Ok(upload_destination_form_data(
            &UploadDestinationConfig::rocky(),
        )),
        "custom" => {
            let mut form = upload_destination_form_data(&UploadDestinationConfig::new(
                String::new(),
                Url::parse("https://example.com/api").expect("valid placeholder URL"),
                BTreeMap::new(),
                TargetAuth::None,
                PingConfig::default(),
                ReplayUploadConfig::default(),
                RankUploadConfig::None,
            ));
            form.url = String::new();
            Ok(form)
        }
        other => Err(format!("Unsupported upload destination preset {other:?}")),
    }
}

fn upload_destination_form_data(
    destination: &UploadDestinationConfig,
) -> UploadDestinationFormData {
    let (auth_kind, auth_value) = match &destination.auth {
        TargetAuth::None => ("none", String::new()),
        TargetAuth::AuthorizationHeader { value } => ("authorization_header", value.clone()),
        TargetAuth::Bearer { token } => ("bearer", token.clone()),
        TargetAuth::BearerEnv { variable } => ("bearer_env", variable.clone()),
        TargetAuth::BearerCommand { command } => ("bearer_command", command.join(" ")),
    };
    let (rank_upload_mode, rank_upload_value) = match &destination.rank_upload {
        RankUploadConfig::None => ("none", String::new()),
        RankUploadConfig::Endpoint { path } => ("endpoint", path.clone()),
        RankUploadConfig::Bundled { field } => ("bundled", field.clone()),
    };

    UploadDestinationFormData {
        name: destination.name.clone(),
        url: destination.url.to_string(),
        auth_kind: auth_kind.to_string(),
        auth_value,
        query: destination
            .query
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join(", "),
        ping_enabled: destination.ping.enabled,
        ping_path: destination.ping.path.clone(),
        upload_enabled: destination.replay_upload.enabled,
        upload_path: destination.replay_upload.path.clone(),
        upload_file_field: destination.replay_upload.file_field.clone(),
        success_statuses: format_status_list(&destination.replay_upload.success_statuses),
        duplicate_statuses: format_status_list(&destination.replay_upload.duplicate_statuses),
        rank_upload_mode: rank_upload_mode.to_string(),
        rank_upload_value,
    }
}

fn parse_upload_destination(
    input: &UploadDestinationFormData,
) -> Result<UploadDestinationConfig, String> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err("Destination name is required".to_string());
    }
    let url = Url::parse(input.url.trim())
        .map_err(|error| format!("Destination URL is invalid: {error}"))?;

    let destination = UploadDestinationConfig::new(
        name.to_string(),
        url,
        parse_query_pairs(&input.query)?,
        parse_target_auth(&input.auth_kind, &input.auth_value)?,
        PingConfig {
            enabled: input.ping_enabled,
            path: input.ping_path.trim().to_string(),
        },
        ReplayUploadConfig {
            enabled: input.upload_enabled,
            path: input.upload_path.trim().to_string(),
            file_field: input.upload_file_field.trim().to_string(),
            success_statuses: parse_status_list("Success statuses", &input.success_statuses)?,
            duplicate_statuses: parse_status_list("Duplicate statuses", &input.duplicate_statuses)?,
        },
        parse_rank_upload(&input.rank_upload_mode, &input.rank_upload_value)?,
    );
    destination.validate().map_err(|error| error.to_string())?;
    Ok(destination)
}

fn parse_target_auth(kind: &str, value: &str) -> Result<TargetAuth, String> {
    let value = value.trim();
    match kind {
        "none" => Ok(TargetAuth::None),
        "bearer" => {
            if value.is_empty() {
                return Err("API token is required for bearer token auth".to_string());
            }
            Ok(TargetAuth::Bearer {
                token: value.to_string(),
            })
        }
        "bearer_env" => {
            if value.is_empty() {
                return Err("Environment variable name is required for bearer env auth".to_string());
            }
            Ok(TargetAuth::BearerEnv {
                variable: value.to_string(),
            })
        }
        "authorization_header" => {
            if value.is_empty() {
                return Err("Header value is required for authorization header auth".to_string());
            }
            Ok(TargetAuth::AuthorizationHeader {
                value: value.to_string(),
            })
        }
        "bearer_command" => {
            let command: Vec<String> = value.split_whitespace().map(str::to_string).collect();
            if command.is_empty() {
                return Err("Command is required for bearer command auth".to_string());
            }
            Ok(TargetAuth::BearerCommand { command })
        }
        other => Err(format!("Unsupported auth kind {other:?}")),
    }
}

fn parse_rank_upload(mode: &str, value: &str) -> Result<RankUploadConfig, String> {
    let value = value.trim();
    match mode {
        "none" => Ok(RankUploadConfig::None),
        "endpoint" => {
            if value.is_empty() {
                return Err("Endpoint path is required for rank endpoint uploads".to_string());
            }
            Ok(RankUploadConfig::Endpoint {
                path: value.to_string(),
            })
        }
        "bundled" => {
            if value.is_empty() {
                return Err("Multipart field is required for bundled rank uploads".to_string());
            }
            Ok(RankUploadConfig::Bundled {
                field: value.to_string(),
            })
        }
        other => Err(format!("Unsupported rank upload mode {other:?}")),
    }
}

fn parse_query_pairs(value: &str) -> Result<BTreeMap<String, String>, String> {
    let mut pairs = BTreeMap::new();
    for entry in value.split([',', '\n']) {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let Some((key, pair_value)) = entry.split_once('=') else {
            return Err(format!("Query parameter {entry:?} must be key=value"));
        };
        let key = key.trim();
        if key.is_empty() {
            return Err(format!("Query parameter {entry:?} is missing a key"));
        }
        pairs.insert(key.to_string(), pair_value.trim().to_string());
    }
    Ok(pairs)
}

fn parse_status_list(label: &str, value: &str) -> Result<Vec<u16>, String> {
    let mut statuses = Vec::new();
    for entry in value.split(',') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let status = entry.parse::<u16>().map_err(|_| {
            format!("{label} must be comma-separated HTTP status codes, got {entry:?}")
        })?;
        statuses.push(status);
    }
    if statuses.is_empty() {
        return Err(format!(
            "{label} must include at least one HTTP status code"
        ));
    }
    Ok(statuses)
}

fn format_status_list(statuses: &[u16]) -> String {
    statuses
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

fn add_destination_to_config(
    config: &mut Config,
    destination: UploadDestinationConfig,
) -> Result<(), String> {
    if config
        .upload_destinations
        .iter()
        .any(|target| target.name == destination.name)
    {
        return Err(format!(
            "Upload destination {:?} already exists",
            destination.name
        ));
    }
    config.upload_destinations.push(destination);
    Ok(())
}

fn update_destination_in_config(
    config: &mut Config,
    original_name: &str,
    destination: UploadDestinationConfig,
) -> Result<(), String> {
    let Some(index) = config
        .upload_destinations
        .iter()
        .position(|target| target.name == original_name)
    else {
        return Err(format!(
            "Upload destination {original_name:?} no longer exists"
        ));
    };

    let collides = config
        .upload_destinations
        .iter()
        .enumerate()
        .any(|(other, target)| other != index && target.name == destination.name);
    if collides {
        return Err(format!(
            "Upload destination {:?} already exists",
            destination.name
        ));
    }

    // Keep the behavior selection pointing at the same destination across a
    // rename.
    if config.behavior.selected_upload_destination.as_deref() == Some(original_name) {
        config.behavior.selected_upload_destination = Some(destination.name.clone());
    }
    config.upload_destinations[index] = destination;
    Ok(())
}

fn remove_destination_from_config(config: &mut Config, name: &str) -> Result<(), String> {
    if config.upload_destinations.len() <= 1 {
        return Err("Config must keep at least one upload destination".to_string());
    }

    let Some(index) = config
        .upload_destinations
        .iter()
        .position(|target| target.name == name)
    else {
        return Err(format!("Upload destination {name:?} no longer exists"));
    };

    config.upload_destinations.remove(index);
    if config.behavior.selected_upload_destination.as_deref() == Some(name) {
        config.behavior.selected_upload_destination = None;
    }
    Ok(())
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

    fn sample_destination(name: &str) -> UploadDestinationConfig {
        let mut form = upload_destination_preset_form("custom").unwrap();
        form.name = name.to_string();
        form.url = "https://example.com/api".to_string();
        parse_upload_destination(&form).unwrap()
    }

    #[test]
    fn add_destination_rejects_duplicate_names() {
        let mut config = Config::default();

        let error =
            add_destination_to_config(&mut config, sample_destination("Rocky")).unwrap_err();
        assert!(error.contains("already exists"), "{error}");

        add_destination_to_config(&mut config, sample_destination("Mine")).unwrap();
        config.validate().unwrap();
        assert!(config.upload_destination("Mine").is_some());
    }

    #[test]
    fn update_destination_rename_repoints_selected_destination() {
        let mut config = Config::default();
        config.behavior.selected_upload_destination = Some("Rocky".to_string());

        update_destination_in_config(&mut config, "Rocky", sample_destination("Rocky II")).unwrap();

        assert_eq!(
            config.behavior.selected_upload_destination.as_deref(),
            Some("Rocky II")
        );
        assert!(config.upload_destination("Rocky").is_none());
        config.validate().unwrap();
    }

    #[test]
    fn update_destination_rename_keeps_unrelated_selection() {
        let mut config = Config::default();
        config.behavior.selected_upload_destination = Some("Ballchasing".to_string());

        update_destination_in_config(&mut config, "Rocky", sample_destination("Rocky II")).unwrap();

        assert_eq!(
            config.behavior.selected_upload_destination.as_deref(),
            Some("Ballchasing")
        );
        config.validate().unwrap();
    }

    #[test]
    fn update_destination_rejects_collision_with_other_destination() {
        let mut config = Config::default();

        let error =
            update_destination_in_config(&mut config, "Rocky", sample_destination("Ballchasing"))
                .unwrap_err();

        assert!(error.contains("already exists"), "{error}");
    }

    #[test]
    fn update_destination_requires_existing_destination() {
        let mut config = Config::default();

        let error =
            update_destination_in_config(&mut config, "Missing", sample_destination("Renamed"))
                .unwrap_err();

        assert!(error.contains("no longer exists"), "{error}");
    }

    #[test]
    fn remove_destination_clears_selected_destination() {
        let mut config = Config::default();
        config.behavior.selected_upload_destination = Some("Rocky".to_string());

        remove_destination_from_config(&mut config, "Rocky").unwrap();

        assert_eq!(config.behavior.selected_upload_destination, None);
        assert!(config.upload_destination("Rocky").is_none());
        config.validate().unwrap();
    }

    #[test]
    fn remove_destination_keeps_unrelated_selection() {
        let mut config = Config::default();
        config.behavior.selected_upload_destination = Some("Ballchasing".to_string());

        remove_destination_from_config(&mut config, "Rocky").unwrap();

        assert_eq!(
            config.behavior.selected_upload_destination.as_deref(),
            Some("Ballchasing")
        );
        config.validate().unwrap();
    }

    #[test]
    fn remove_destination_keeps_at_least_one() {
        let mut config = Config::default();
        config.upload_destinations = vec![crate::config::UploadDestinationConfig::rocky()];

        let error = remove_destination_from_config(&mut config, "Rocky").unwrap_err();

        assert!(error.contains("at least one"), "{error}");
    }

    #[test]
    fn parse_upload_destination_round_trips_built_in_presets() {
        for destination in [
            UploadDestinationConfig::rocky(),
            UploadDestinationConfig::ballchasing(),
            UploadDestinationConfig::rocket_sense(),
        ] {
            let form = upload_destination_form_data(&destination);
            assert_eq!(parse_upload_destination(&form).unwrap(), destination);
        }
    }

    #[test]
    fn rocket_sense_preset_defaults_to_pasted_bearer_token() {
        let form = upload_destination_preset_form("rocket_sense").unwrap();
        assert_eq!(form.name, "Rocket Sense");
        assert_eq!(form.url, "https://rocket-sense.duckdns.org/api/v1");
        assert_eq!(form.auth_kind, "bearer");
        assert_eq!(form.auth_value, "");
        assert_eq!(form.upload_path, "/replays");
        assert_eq!(form.rank_upload_mode, "bundled");

        // The token itself is the one thing the user must supply.
        let error = parse_upload_destination(&form).unwrap_err();
        assert!(error.contains("token"), "{error}");

        let mut with_token = form;
        with_token.auth_value = "secret".to_string();
        let parsed = parse_upload_destination(&with_token).unwrap();
        assert_eq!(
            parsed.auth,
            TargetAuth::Bearer {
                token: "secret".to_string()
            }
        );
        assert_eq!(parsed.replay_upload.duplicate_statuses, vec![200, 409]);
    }

    #[test]
    fn parse_upload_destination_parses_text_fields() {
        let mut form = upload_destination_preset_form("custom").unwrap();
        form.name = "Custom".to_string();
        form.url = "https://example.com/api".to_string();
        form.query = "visibility=public, group=team".to_string();
        form.auth_kind = "bearer_command".to_string();
        form.auth_value = "pass show rocket".to_string();
        form.success_statuses = "200, 201".to_string();
        form.duplicate_statuses = "409".to_string();
        form.rank_upload_mode = "endpoint".to_string();
        form.rank_upload_value = "/v1/mmr".to_string();

        let parsed = parse_upload_destination(&form).unwrap();

        assert_eq!(
            parsed.query,
            BTreeMap::from([
                ("visibility".to_string(), "public".to_string()),
                ("group".to_string(), "team".to_string()),
            ])
        );
        assert_eq!(
            parsed.auth,
            TargetAuth::BearerCommand {
                command: vec!["pass".to_string(), "show".to_string(), "rocket".to_string()]
            }
        );
        assert_eq!(parsed.replay_upload.success_statuses, vec![200, 201]);
        assert_eq!(
            parsed.rank_upload,
            RankUploadConfig::Endpoint {
                path: "/v1/mmr".to_string()
            }
        );
    }

    #[test]
    fn parse_upload_destination_rejects_invalid_input() {
        let valid = {
            let mut form = upload_destination_preset_form("custom").unwrap();
            form.name = "Custom".to_string();
            form.url = "https://example.com/api".to_string();
            form
        };
        parse_upload_destination(&valid).unwrap();

        let mut empty_name = valid.clone();
        empty_name.name = "  ".to_string();
        assert!(parse_upload_destination(&empty_name)
            .unwrap_err()
            .contains("name is required"));

        let mut bad_scheme = valid.clone();
        bad_scheme.url = "ftp://example.com".to_string();
        assert!(parse_upload_destination(&bad_scheme)
            .unwrap_err()
            .contains("http or https"));

        let mut bad_status = valid.clone();
        bad_status.success_statuses = "created".to_string();
        assert!(parse_upload_destination(&bad_status)
            .unwrap_err()
            .contains("HTTP status codes"));

        let mut out_of_range_status = valid.clone();
        out_of_range_status.success_statuses = "99".to_string();
        assert!(parse_upload_destination(&out_of_range_status)
            .unwrap_err()
            .contains("invalid HTTP status"));

        let mut empty_statuses = valid.clone();
        empty_statuses.duplicate_statuses = " , ".to_string();
        assert!(parse_upload_destination(&empty_statuses)
            .unwrap_err()
            .contains("at least one"));

        let mut bad_query = valid.clone();
        bad_query.query = "visibility".to_string();
        assert!(parse_upload_destination(&bad_query)
            .unwrap_err()
            .contains("key=value"));

        let mut bad_ping = valid;
        bad_ping.ping_path = "health".to_string();
        assert!(parse_upload_destination(&bad_ping)
            .unwrap_err()
            .contains("must start with /"));
    }

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
                ..BackfillSummary::default()
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
                ..BackfillSummary::default()
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
