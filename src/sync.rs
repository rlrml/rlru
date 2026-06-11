use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};
use secrecy::ExposeSecret;
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::auth::AuthManager;
use crate::auth::EosTokenResponse;
use crate::config::{AccountConfig, Config, UploadDestinationConfig};
use crate::paths::AppPaths;
use crate::psynet::{MatchEntry, PlayerId, PsyNetClient};
use crate::upload::{MmrPlayer, MmrSkill, MmrUpload, ReplayUploader, UploadCache, UploadOutcome};

static DOWNLOAD_COUNTER: AtomicU64 = AtomicU64::new(0);

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
        let mut targets = self.upload_target_states(options.target_name.as_deref())?;

        for replay in &matches {
            let match_id = &replay.match_info.match_guid;
            if !requested_match_ids.is_empty()
                && !requested_match_ids.contains(&normalize_match_id(match_id))
            {
                continue;
            }

            let mut pending_target_indexes = Vec::new();
            for (index, target) in targets.iter_mut().enumerate() {
                if !target.config.replay_upload.enabled {
                    summary.skipped += 1;
                    continue;
                }

                if !options.force && target.cache.contains(match_id) {
                    summary.cached += 1;
                    continue;
                }

                match &target.auth {
                    UploadTargetAuth::Available(_) => pending_target_indexes.push(index),
                    UploadTargetAuth::Unavailable(reason) => record_failed_upload(
                        &mut summary,
                        &target.config.name,
                        match_id,
                        upload_auth_unavailable_reason(&target.config.name, reason),
                    ),
                }
            }

            if pending_target_indexes.is_empty() {
                continue;
            }

            let downloaded_replay = match self.download_replay(replay).await {
                Ok(path) => path,
                Err(error) => {
                    for index in pending_target_indexes {
                        record_failed_upload(
                            &mut summary,
                            &targets[index].config.name,
                            match_id,
                            format!("download failed: {}", error_chain(&error)),
                        );
                    }
                    tracing::warn!(%error, match_id, "failed to download replay");
                    continue;
                }
            };

            for index in pending_target_indexes {
                let target = &mut targets[index];
                let UploadTargetAuth::Available(auth_header) = &target.auth else {
                    continue;
                };
                let auth_header = auth_header.clone();
                let result = self
                    .uploader
                    .upload_replay_with_auth_header(
                        target.config,
                        downloaded_replay.path(),
                        Some(match_id),
                        auth_header.clone(),
                    )
                    .await;

                let stored = match result {
                    Ok(result) if result.outcome == UploadOutcome::Uploaded => {
                        summary.uploaded += 1;
                        target
                            .cache
                            .add_with_location(match_id.clone(), result.location)?;
                        true
                    }
                    Ok(result) if result.outcome == UploadOutcome::Duplicate => {
                        summary.duplicates += 1;
                        target
                            .cache
                            .add_with_location(match_id.clone(), result.location)?;
                        true
                    }
                    Ok(_) => {
                        summary.skipped += 1;
                        false
                    }
                    Err(error) => {
                        record_failed_upload(
                            &mut summary,
                            &target.config.name,
                            match_id,
                            format!("upload failed: {}", error_chain(&error)),
                        );
                        tracing::warn!(
                            %error,
                            match_id,
                            target = %target.config.name,
                            "failed to upload replay"
                        );
                        false
                    }
                };

                if stored && target.config.mmr_upload.enabled {
                    let payload = build_mmr_upload(replay);
                    if !payload.players.is_empty() {
                        if let Err(error) = self
                            .uploader
                            .upload_mmr_with_auth_header(
                                target.config,
                                &payload,
                                match_id,
                                auth_header,
                            )
                            .await
                        {
                            tracing::warn!(
                                %error,
                                match_id,
                                target = %target.config.name,
                                "failed to upload player rank metadata"
                            );
                        }
                    }
                }
            }
        }

        Ok(summary)
    }

    fn upload_target_states(
        &self,
        target_name: Option<&str>,
    ) -> Result<Vec<UploadTargetState<'_>>> {
        self.upload_destinations(target_name)?
            .into_iter()
            .map(|target| {
                let cache = UploadCache::load(
                    self.paths.upload_cache_path(&target.name),
                    self.config.accounts.len(),
                )?;
                let auth = match target.auth.header_value() {
                    Ok(header) => UploadTargetAuth::Available(header),
                    Err(error) => {
                        let reason = error.to_string();
                        tracing::warn!(
                            %reason,
                            target = %target.name,
                            "skipping replay uploads because upload auth is unavailable"
                        );
                        UploadTargetAuth::Unavailable(reason)
                    }
                };

                Ok(UploadTargetState {
                    config: target,
                    cache,
                    auth,
                })
            })
            .collect()
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

    async fn download_replay(&self, replay: &MatchEntry) -> Result<DownloadedReplay> {
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

        let download_stem = unique_replay_download_stem(&replay.match_info.match_guid);
        let path = self.paths.cache_dir.join(format!("{download_stem}.part"));
        let final_path = self.paths.cache_dir.join(download_stem);
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
        Ok(DownloadedReplay::new(final_path))
    }
}

/// Builds the per-match player rank payload from PsyNet match-history data,
/// mirroring what the BakkesMod plugin posts to Ballchasing's MMR endpoint.
/// Players without valid skill data (e.g. unranked playlists) are omitted.
fn build_mmr_upload(replay: &MatchEntry) -> MmrUpload {
    let players = replay
        .match_info
        .players
        .iter()
        .filter(|player| player.skills.valid)
        .filter_map(|player| {
            let (platform, id) = parse_player_id(&player.player_id)?;
            Some(MmrPlayer {
                platform_id: online_platform_id(platform),
                id,
                before: MmrSkill {
                    tier: player.skills.prev_tier,
                    division: player.skills.prev_division,
                    matches_played: 0,
                    mmr: player.skills.prev_mmr(),
                },
                after: MmrSkill {
                    tier: player.skills.tier,
                    division: player.skills.division,
                    matches_played: 0,
                    mmr: player.skills.mmr(),
                },
                debug: player.player_name.clone(),
            })
        })
        .collect();
    MmrUpload {
        game: replay.match_info.match_guid.clone(),
        players,
    }
}

/// Splits a PsyNet PlayerID (`Platform|id|splitscreen`) into its platform name
/// and platform-specific id components.
fn parse_player_id(player_id: &str) -> Option<(&str, String)> {
    let mut parts = player_id.split('|');
    let platform = parts.next()?;
    let id = parts.next()?;
    if platform.is_empty() || id.is_empty() {
        return None;
    }
    Some((platform, id.to_string()))
}

/// Maps a PsyNet platform name to its Rocket League `OnlinePlatform` enum value,
/// matching the integer Ballchasing expects in the `platform_id` field.
fn online_platform_id(platform: &str) -> i64 {
    match platform {
        "Steam" => 1,
        "PS4" | "PS3" => 2,
        "Dingo" | "XboxOne" => 4,
        "NNX" | "Switch" => 7,
        "PsyNet" => 8,
        "WeGame" => 10,
        "Epic" => 11,
        _ => 0,
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

#[derive(Debug)]
struct UploadTargetState<'a> {
    config: &'a UploadDestinationConfig,
    cache: UploadCache,
    auth: UploadTargetAuth,
}

#[derive(Debug)]
enum UploadTargetAuth {
    Available(Option<String>),
    Unavailable(String),
}

#[derive(Debug)]
struct DownloadedReplay {
    path: PathBuf,
}

impl DownloadedReplay {
    fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for DownloadedReplay {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_file(&self.path) {
            tracing::debug!(
                %error,
                path = %self.path.display(),
                "failed to remove downloaded replay cache file"
            );
        }
    }
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

impl std::fmt::Display for SyncSummary {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(formatter, "accounts: {}", self.accounts_seen)?;
        writeln!(formatter, "matches: {}", self.matches_seen)?;
        writeln!(formatter, "uploaded: {}", self.uploaded)?;
        writeln!(formatter, "duplicates: {}", self.duplicates)?;
        writeln!(formatter, "cached: {}", self.cached)?;
        writeln!(formatter, "skipped: {}", self.skipped)?;
        writeln!(formatter, "failed: {}", self.failed)?;
        if !self.failed_match_ids.is_empty() {
            writeln!(
                formatter,
                "failed_match_ids: {}",
                self.failed_match_ids.join(",")
            )?;
        }
        for failure in &self.failed_uploads {
            writeln!(
                formatter,
                "failed_upload: {} {}: {}",
                failure.target_name, failure.match_id, failure.reason
            )?;
        }
        Ok(())
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

fn unique_replay_download_stem(match_id: &str) -> String {
    let sequence = DOWNLOAD_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{match_id}-{}-{sequence}.replay", std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::psynet::{Match, MatchPlayer, MatchSkills};

    #[test]
    fn build_mmr_upload_maps_skills_and_platforms() {
        let mut replay = match_entry("MATCH-GUID");
        replay.match_info.players = vec![
            MatchPlayer {
                player_id: "Epic|epic-account|0".to_string(),
                player_name: "Blue".to_string(),
                skills: MatchSkills {
                    mu: 30.0,
                    tier: 12,
                    division: 3,
                    prev_mu: 29.0,
                    prev_tier: 12,
                    prev_division: 2,
                    valid: true,
                    ..MatchSkills::default()
                },
                ..MatchPlayer::default()
            },
            MatchPlayer {
                player_id: "Steam|76561198000000000|0".to_string(),
                player_name: "Orange".to_string(),
                skills: MatchSkills {
                    valid: false,
                    ..MatchSkills::default()
                },
                ..MatchPlayer::default()
            },
        ];

        let payload = build_mmr_upload(&replay);

        assert_eq!(payload.game, "MATCH-GUID");
        // The unranked (bValid == false) player is dropped.
        assert_eq!(payload.players.len(), 1);
        let player = &payload.players[0];
        assert_eq!(player.platform_id, 11);
        assert_eq!(player.id, "epic-account");
        assert_eq!(player.debug, "Blue");
        assert_eq!(player.after.tier, 12);
        assert_eq!(player.after.division, 3);
        assert_eq!(player.after.mmr, 30.0 * 20.0 + 100.0);
        assert_eq!(player.before.division, 2);
        assert_eq!(player.before.mmr, 29.0 * 20.0 + 100.0);
    }

    #[test]
    fn online_platform_id_maps_known_platforms() {
        assert_eq!(online_platform_id("Steam"), 1);
        assert_eq!(online_platform_id("PS4"), 2);
        assert_eq!(online_platform_id("XboxOne"), 4);
        assert_eq!(online_platform_id("Switch"), 7);
        assert_eq!(online_platform_id("Epic"), 11);
        assert_eq!(online_platform_id("Mystery"), 0);
    }

    #[test]
    fn parse_player_id_splits_platform_and_id() {
        assert_eq!(
            parse_player_id("Epic|abc123|0"),
            Some(("Epic", "abc123".to_string()))
        );
        assert_eq!(parse_player_id("Epic||0"), None);
        assert_eq!(parse_player_id("nonsense"), None);
    }

    #[test]
    fn selected_match_count_matches_case_insensitively() {
        let matches = vec![match_entry("ABC123"), match_entry("def456")];

        assert_eq!(
            selected_match_count(&matches, &[" abc123 ".to_string(), "DEF456".to_string()]),
            2
        );
        assert_eq!(selected_match_count(&matches, &["missing".to_string()]), 0);
    }

    #[test]
    fn sync_summary_display_matches_cli_output_shape() {
        let summary = SyncSummary {
            accounts_seen: 1,
            matches_seen: 2,
            uploaded: 3,
            duplicates: 4,
            cached: 5,
            skipped: 6,
            failed: 1,
            failed_match_ids: vec!["match-1".to_string()],
            failed_uploads: vec![FailedUpload {
                target_name: "Rocket Sense".to_string(),
                match_id: "match-1".to_string(),
                reason: "token missing".to_string(),
            }],
        };

        assert_eq!(
            summary.to_string(),
            concat!(
                "accounts: 1\n",
                "matches: 2\n",
                "uploaded: 3\n",
                "duplicates: 4\n",
                "cached: 5\n",
                "skipped: 6\n",
                "failed: 1\n",
                "failed_match_ids: match-1\n",
                "failed_upload: Rocket Sense match-1: token missing\n",
            )
        );
    }

    #[test]
    fn downloaded_replay_removes_file_on_drop() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("match.replay");
        std::fs::write(&path, "replay bytes").unwrap();

        let replay = DownloadedReplay::new(path.clone());
        assert!(replay.path().exists());
        drop(replay);

        assert!(!path.exists());
    }

    #[test]
    fn replay_download_stems_are_unique_and_keep_match_id() {
        let first = unique_replay_download_stem("match-1");
        let second = unique_replay_download_stem("match-1");

        assert_ne!(first, second);
        assert!(first.starts_with("match-1-"));
        assert!(first.ends_with(".replay"));
    }

    fn match_entry(match_guid: &str) -> MatchEntry {
        MatchEntry {
            replay_url: "https://example.com/replay".to_string(),
            match_info: Match {
                match_guid: match_guid.to_string(),
                record_start_timestamp: 0,
                map_name: "DFH Stadium".to_string(),
                playlist: 10,
                team0_score: 1,
                team1_score: 2,
                players: Vec::new(),
            },
        }
    }
}
