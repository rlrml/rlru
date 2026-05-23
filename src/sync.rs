use std::collections::HashSet;
use std::path::PathBuf;

use anyhow::{Context, Result};
use secrecy::ExposeSecret;
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::auth::AuthManager;
use crate::auth::EosTokenResponse;
use crate::config::{AccountConfig, Config};
use crate::paths::AppPaths;
use crate::psynet::{MatchEntry, PlayerId, PsyNetClient};
use crate::upload::{ReplayUploader, UploadCache, UploadOutcome};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncSummary {
    pub accounts_seen: usize,
    pub matches_seen: usize,
    pub uploaded: usize,
    pub duplicates: usize,
    pub cached: usize,
    pub skipped: usize,
    pub failed: usize,
    pub failed_match_ids: Vec<String>,
    pub failed_uploads: Vec<FailedUpload>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedUpload {
    pub target_name: String,
    pub match_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncOptions {
    pub include_online: bool,
    pub target_name: Option<String>,
    pub force: bool,
    pub match_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    pub account_name: String,
    pub match_id: String,
    pub record_start_timestamp: i64,
    pub map_name: String,
    pub playlist: i64,
    pub team0_score: i64,
    pub team1_score: i64,
    pub replay_url: String,
    pub upload_states: Vec<HistoryUploadState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryUploadState {
    pub target_name: String,
    pub upload_enabled: bool,
    pub cached: bool,
    pub location: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SyncService {
    paths: AppPaths,
    config: Config,
    psynet: PsyNetClient,
    uploader: ReplayUploader,
    http: reqwest::Client,
}

impl SyncService {
    pub fn new(paths: AppPaths, config: Config) -> Self {
        Self {
            paths,
            config,
            psynet: PsyNetClient::new(),
            uploader: ReplayUploader::new(),
            http: reqwest::Client::new(),
        }
    }

    pub async fn run_once(&self) -> Result<SyncSummary> {
        self.run_once_with_options(SyncOptions::default()).await
    }

    pub async fn run_once_with_options(&self, options: SyncOptions) -> Result<SyncSummary> {
        let mut summary = SyncSummary::default();

        let mut active_accounts = Vec::new();
        for account in self
            .config
            .accounts
            .iter()
            .filter(|account| account.sync_enabled)
        {
            let auth = AuthManager::for_account(&self.paths, account);
            let token = auth
                .restore_or_refresh()
                .await
                .with_context(|| format!("failed to restore auth for account {}", account.name))?;
            active_accounts.push(AuthenticatedAccount { account, token });
        }

        let accounts_to_upload = if options.include_online {
            active_accounts
        } else {
            self.filter_connected_accounts(active_accounts).await?
        };
        for account in accounts_to_upload {
            summary.accounts_seen += 1;
            let account_summary = self
                .sync_account(account.account, &account.token, &options)
                .await
                .with_context(|| format!("failed to sync account {}", account.account.name))?;
            summary.merge(account_summary);
        }
        Ok(summary)
    }

    pub async fn current_history(&self, target_name: Option<&str>) -> Result<Vec<HistoryEntry>> {
        let targets = self.upload_destinations(target_name)?;
        let caches = targets
            .iter()
            .map(|target| {
                let cache = UploadCache::load(
                    self.paths.upload_cache_path(&target.name),
                    self.config.accounts.len(),
                )?;
                Ok((target, cache))
            })
            .collect::<Result<Vec<_>>>()?;
        let mut entries = Vec::new();

        for account in self
            .config
            .accounts
            .iter()
            .filter(|account| account.sync_enabled)
        {
            let auth = AuthManager::for_account(&self.paths, account);
            let token = auth
                .restore_or_refresh()
                .await
                .with_context(|| format!("failed to restore auth for account {}", account.name))?;
            let rpc = self
                .psynet
                .auth_player(&token.account_id, token.access_token.expose_secret())
                .await?;
            let matches = rpc.get_match_history().await?;
            let _ = rpc.close().await;

            entries.extend(matches.into_iter().map(|entry| {
                let match_id = entry.match_info.match_guid.clone();
                HistoryEntry {
                    account_name: account.name.clone(),
                    match_id: match_id.clone(),
                    record_start_timestamp: entry.match_info.record_start_timestamp,
                    map_name: entry.match_info.map_name,
                    playlist: entry.match_info.playlist,
                    team0_score: entry.match_info.team0_score,
                    team1_score: entry.match_info.team1_score,
                    replay_url: entry.replay_url,
                    upload_states: caches
                        .iter()
                        .map(|(target, cache)| HistoryUploadState {
                            target_name: target.name.clone(),
                            upload_enabled: target.replay_upload.enabled,
                            cached: cache.contains(&match_id),
                            location: cache.location(&match_id).map(ToOwned::to_owned),
                        })
                        .collect(),
                }
            }));
        }

        entries.sort_by(|left, right| {
            right
                .record_start_timestamp
                .cmp(&left.record_start_timestamp)
                .then_with(|| left.account_name.cmp(&right.account_name))
        });
        Ok(entries)
    }

    async fn filter_connected_accounts<'a>(
        &self,
        accounts: Vec<AuthenticatedAccount<'a>>,
    ) -> Result<Vec<AuthenticatedAccount<'a>>> {
        if !self.config.behavior.no_upload_while_connected {
            return Ok(accounts);
        }

        let Some(presence_account) = self
            .config
            .accounts
            .iter()
            .find(|account| !account.sync_enabled)
        else {
            return Ok(accounts);
        };

        let auth = AuthManager::for_account(&self.paths, presence_account);
        let token = auth.restore_or_refresh().await.with_context(|| {
            format!(
                "failed to restore presence-check account auth for {}",
                presence_account.name
            )
        })?;
        let rpc = self
            .psynet
            .auth_player(&token.account_id, token.access_token.expose_secret())
            .await?;
        let profile_ids = accounts
            .iter()
            .map(|account| {
                PlayerId::new(account.account.platform.clone(), &account.token.account_id)
            })
            .collect::<Vec<_>>();
        let profiles = rpc.get_profiles(profile_ids).await?;
        let _ = rpc.close().await;

        let online = profiles
            .iter()
            .filter(|profile| profile.presence_state == "Online")
            .map(|profile| profile.player_id.as_str())
            .collect::<std::collections::HashSet<_>>();

        Ok(accounts
            .into_iter()
            .filter(|account| {
                let player_id =
                    PlayerId::new(account.account.platform.clone(), &account.token.account_id)
                        .to_string();
                let should_upload = !online.contains(player_id.as_str());
                if !should_upload {
                    tracing::info!(
                        account = %account.account.name,
                        "skipping upload because account is online"
                    );
                }
                should_upload
            })
            .collect())
    }

    async fn sync_account(
        &self,
        account: &AccountConfig,
        token: &EosTokenResponse,
        options: &SyncOptions,
    ) -> Result<SyncSummary> {
        let rpc = self
            .psynet
            .auth_player(&token.account_id, token.access_token.expose_secret())
            .await?;
        let profiles = rpc
            .get_profiles(vec![PlayerId::new(
                account.platform.clone(),
                &token.account_id,
            )])
            .await
            .unwrap_or_default();
        if let Some(profile) = profiles.first() {
            tracing::info!(
                player_name = %profile.player_name,
                presence = %profile.presence_state,
                "connected to PsyNet account"
            );
        }

        let matches = rpc.get_match_history().await?;
        let _ = rpc.close().await;
        self.upload_matches(matches, options).await
    }

    async fn upload_matches(
        &self,
        matches: Vec<MatchEntry>,
        options: &SyncOptions,
    ) -> Result<SyncSummary> {
        let mut summary = SyncSummary {
            matches_seen: selected_match_count(&matches, &options.match_ids),
            ..SyncSummary::default()
        };
        let requested_match_ids = normalized_match_ids(&options.match_ids);

        for target in self.upload_destinations(options.target_name.as_deref())? {
            if !target.replay_upload.enabled {
                summary.skipped += summary.matches_seen;
                continue;
            }

            let mut cache = UploadCache::load(
                self.paths.upload_cache_path(&target.name),
                self.config.accounts.len(),
            )?;

            if let Err(error) = target.auth.header_value() {
                let reason = error.to_string();
                for replay in &matches {
                    let match_id = &replay.match_info.match_guid;
                    if !requested_match_ids.is_empty()
                        && !requested_match_ids.contains(&normalize_match_id(match_id))
                    {
                        continue;
                    }
                    if !options.force && cache.contains(match_id) {
                        summary.cached += 1;
                        continue;
                    }
                    record_failed_upload(
                        &mut summary,
                        &target.name,
                        match_id,
                        upload_auth_unavailable_reason(&target.name, &reason),
                    );
                }
                tracing::warn!(
                    %reason,
                    target = %target.name,
                    "skipping replay uploads because upload auth is unavailable"
                );
                continue;
            }

            for replay in &matches {
                let match_id = &replay.match_info.match_guid;
                if !requested_match_ids.is_empty()
                    && !requested_match_ids.contains(&normalize_match_id(match_id))
                {
                    continue;
                }
                if !options.force && cache.contains(match_id) {
                    summary.cached += 1;
                    continue;
                }

                let replay_path = match self.download_replay(replay).await {
                    Ok(path) => path,
                    Err(error) => {
                        record_failed_upload(
                            &mut summary,
                            &target.name,
                            match_id,
                            format!("download failed: {}", error_chain(&error)),
                        );
                        tracing::warn!(%error, match_id, "failed to download replay");
                        continue;
                    }
                };

                let result = self
                    .uploader
                    .upload_replay_with_match_id(target, &replay_path, Some(match_id))
                    .await;
                let _ = fs::remove_file(&replay_path).await;

                match result {
                    Ok(result) if result.outcome == UploadOutcome::Uploaded => {
                        summary.uploaded += 1;
                        cache.add_with_location(match_id.clone(), result.location)?;
                    }
                    Ok(result) if result.outcome == UploadOutcome::Duplicate => {
                        summary.duplicates += 1;
                        cache.add_with_location(match_id.clone(), result.location)?;
                    }
                    Ok(_) => {
                        summary.skipped += 1;
                    }
                    Err(error) => {
                        record_failed_upload(
                            &mut summary,
                            &target.name,
                            match_id,
                            format!("upload failed: {}", error_chain(&error)),
                        );
                        tracing::warn!(
                            %error,
                            match_id,
                            target = %target.name,
                            "failed to upload replay"
                        );
                    }
                }
            }
        }

        Ok(summary)
    }

    fn upload_destinations(
        &self,
        target_name: Option<&str>,
    ) -> Result<Vec<&crate::config::UploadDestinationConfig>> {
        match target_name {
            Some(name) => {
                let target = self
                    .config
                    .upload_destination(name)
                    .with_context(|| format!("unknown upload destination {name:?}"))?;
                Ok(vec![target])
            }
            None => Ok(self
                .config
                .upload_destinations
                .iter()
                .filter(|target| target.replay_upload.enabled)
                .collect()),
        }
    }

    async fn download_replay(&self, replay: &MatchEntry) -> Result<PathBuf> {
        let response = self
            .http
            .get(&replay.replay_url)
            .send()
            .await
            .with_context(|| {
                format!("failed to download replay {}", replay.match_info.match_guid)
            })?;
        let status = response.status();
        if !status.is_success() {
            anyhow::bail!(
                "replay download for {} failed with {status}",
                replay.match_info.match_guid
            );
        }

        let path = self
            .paths
            .cache_dir
            .join(format!("{}.replay.part", replay.match_info.match_guid));
        let final_path = self
            .paths
            .cache_dir
            .join(format!("{}.replay", replay.match_info.match_guid));
        let bytes = response
            .bytes()
            .await
            .context("failed to read replay body")?;
        let mut file = fs::File::create(&path)
            .await
            .with_context(|| format!("failed to create replay temp file {}", path.display()))?;
        file.write_all(&bytes).await?;
        file.flush().await?;
        drop(file);
        fs::rename(&path, &final_path).await.with_context(|| {
            format!(
                "failed to move replay temp file {} to {}",
                path.display(),
                final_path.display()
            )
        })?;
        Ok(final_path)
    }
}

fn record_failed_upload(
    summary: &mut SyncSummary,
    target_name: &str,
    match_id: &str,
    reason: String,
) {
    summary.failed += 1;
    summary.failed_match_ids.push(match_id.to_string());
    summary.failed_uploads.push(FailedUpload {
        target_name: target_name.to_string(),
        match_id: match_id.to_string(),
        reason,
    });
}

fn upload_auth_unavailable_reason(target_name: &str, reason: &str) -> String {
    format!(
        "{target_name} upload auth is unavailable: {reason}. Set the configured token source before uploading."
    )
}

fn error_chain(error: &anyhow::Error) -> String {
    error
        .chain()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(": ")
}

#[derive(Debug)]
struct AuthenticatedAccount<'a> {
    account: &'a AccountConfig,
    token: EosTokenResponse,
}

impl SyncSummary {
    fn merge(&mut self, other: Self) {
        self.accounts_seen += other.accounts_seen;
        self.matches_seen += other.matches_seen;
        self.uploaded += other.uploaded;
        self.duplicates += other.duplicates;
        self.cached += other.cached;
        self.skipped += other.skipped;
        self.failed += other.failed;
        self.failed_match_ids.extend(other.failed_match_ids);
        self.failed_uploads.extend(other.failed_uploads);
    }
}

fn selected_match_count(matches: &[MatchEntry], match_ids: &[String]) -> usize {
    let requested_match_ids = normalized_match_ids(match_ids);
    if requested_match_ids.is_empty() {
        return matches.len();
    }
    matches
        .iter()
        .filter(|replay| {
            requested_match_ids.contains(&normalize_match_id(&replay.match_info.match_guid))
        })
        .count()
}

fn normalized_match_ids(match_ids: &[String]) -> HashSet<String> {
    match_ids
        .iter()
        .map(|match_id| normalize_match_id(match_id))
        .collect()
}

fn normalize_match_id(match_id: &str) -> String {
    match_id.trim().to_ascii_uppercase()
}
