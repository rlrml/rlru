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
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncOptions {
    pub include_online: bool,
    pub target_name: Option<String>,
    pub force: bool,
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
            .filter(|account| !account.unused)
        {
            let auth = AuthManager::new(&self.paths, account.profile_id);
            let token = auth.restore_or_refresh().await.with_context(|| {
                format!(
                    "failed to restore auth for account {} (profile {})",
                    account.name, account.profile_id
                )
            })?;
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
                .with_context(|| {
                    format!(
                        "failed to sync account {} (profile {})",
                        account.account.name, account.account.profile_id
                    )
                })?;
            summary.merge(account_summary);
        }
        Ok(summary)
    }

    pub async fn current_history(&self, target_name: Option<&str>) -> Result<Vec<HistoryEntry>> {
        let targets = self.upload_targets(target_name)?;
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
            .filter(|account| !account.unused)
        {
            let auth = AuthManager::new(&self.paths, account.profile_id);
            let token = auth.restore_or_refresh().await.with_context(|| {
                format!(
                    "failed to restore auth for account {} (profile {})",
                    account.name, account.profile_id
                )
            })?;
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

        let Some(unused_account) = self.config.accounts.iter().find(|account| account.unused)
        else {
            return Ok(accounts);
        };

        let auth = AuthManager::new(&self.paths, unused_account.profile_id);
        let token = auth.restore_or_refresh().await.with_context(|| {
            format!(
                "failed to restore unused account auth for profile {}",
                unused_account.profile_id
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
            matches_seen: matches.len(),
            ..SyncSummary::default()
        };

        for target in self.upload_targets(options.target_name.as_deref())? {
            if !target.replay_upload.enabled {
                summary.skipped += matches.len();
                continue;
            }

            let mut cache = UploadCache::load(
                self.paths.upload_cache_path(&target.name),
                self.config.accounts.len(),
            )?;

            for replay in &matches {
                let match_id = &replay.match_info.match_guid;
                if !options.force && cache.contains(match_id) {
                    summary.cached += 1;
                    continue;
                }

                let replay_path = match self.download_replay(replay).await {
                    Ok(path) => path,
                    Err(error) => {
                        summary.failed += 1;
                        tracing::warn!(%error, match_id, "failed to download replay");
                        continue;
                    }
                };

                let outcome = self.uploader.upload_replay(target, &replay_path).await;
                let _ = fs::remove_file(&replay_path).await;

                match outcome {
                    Ok(UploadOutcome::Uploaded) => {
                        summary.uploaded += 1;
                        cache.add(match_id.clone())?;
                    }
                    Ok(UploadOutcome::Duplicate) => {
                        summary.duplicates += 1;
                        cache.add(match_id.clone())?;
                    }
                    Ok(UploadOutcome::Skipped) => {
                        summary.skipped += 1;
                    }
                    Err(error) => {
                        summary.failed += 1;
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

    fn upload_targets(
        &self,
        target_name: Option<&str>,
    ) -> Result<Vec<&crate::config::StorageConfig>> {
        match target_name {
            Some(name) => {
                let target = self
                    .config
                    .target(name)
                    .with_context(|| format!("unknown storage target {name:?}"))?;
                Ok(vec![target])
            }
            None => Ok(self
                .config
                .storage
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
    }
}
