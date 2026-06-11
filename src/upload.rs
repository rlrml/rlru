use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use reqwest::header::{AUTHORIZATION, LOCATION};
use reqwest::StatusCode;
use serde_json::Value;

use crate::config::{RankUploadConfig, UploadDestinationConfig};
use crate::state_file::write_atomically;

const HISTORY_PER_ACCOUNT: usize = 20;
const MAX_CACHE_SIZE_FACTOR: usize = 2;

#[derive(Debug, Clone)]
pub struct ReplayUploader {
    http: reqwest::Client,
}

impl ReplayUploader {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::new(),
        }
    }

    pub async fn ping(&self, target: &UploadDestinationConfig) -> Result<()> {
        if !target.ping.enabled {
            return Ok(());
        }
        let auth_header = target.auth.header_value()?;
        self.ping_with_auth_header(target, auth_header).await
    }

    async fn ping_with_auth_header(
        &self,
        target: &UploadDestinationConfig,
        auth_header: Option<String>,
    ) -> Result<()> {
        if !target.ping.enabled {
            return Ok(());
        }

        let response = self
            .request_with_auth_header(
                self.http.get(target.endpoint_url(&target.ping.path)?),
                auth_header,
            )
            .send()
            .await
            .with_context(|| format!("failed to ping {}", target.name))?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();

        if !status.is_success() {
            bail!("{} ping failed with {status}: {body}", target.name);
        }

        Ok(())
    }

    pub async fn upload_replay(
        &self,
        target: &UploadDestinationConfig,
        file_path: &Path,
    ) -> Result<UploadResult> {
        self.upload_replay_with_match_id(target, file_path, None)
            .await
    }

    pub async fn upload_replay_with_match_id(
        &self,
        target: &UploadDestinationConfig,
        file_path: &Path,
        match_id: Option<&str>,
    ) -> Result<UploadResult> {
        let auth_header = target.auth.header_value()?;
        self.upload_replay_with_auth_header(target, file_path, match_id, auth_header, None)
            .await
    }

    pub async fn upload_replay_with_auth_header(
        &self,
        target: &UploadDestinationConfig,
        file_path: &Path,
        match_id: Option<&str>,
        auth_header: Option<String>,
        ranks_bundle: Option<&RankBundle>,
    ) -> Result<UploadResult> {
        if !target.replay_upload.enabled {
            return Ok(UploadResult {
                outcome: UploadOutcome::Skipped,
                location: None,
            });
        }

        self.ping_with_auth_header(target, auth_header.clone())
            .await?;

        let file_name = match match_id {
            Some(match_id) => format!("{match_id}.replay"),
            None => file_path
                .file_name()
                .and_then(|value| value.to_str())
                .context("replay path has no UTF-8 file name")?
                .to_string(),
        };
        let bytes = fs::read(file_path)
            .with_context(|| format!("failed to read replay {}", file_path.display()))?;
        let part = reqwest::multipart::Part::bytes(bytes).file_name(file_name);
        let mut form =
            reqwest::multipart::Form::new().part(target.replay_upload.file_field.clone(), part);

        // Bundle player ranks into the same upload when the destination supports
        // it (Rocket Sense). Replay files do not carry ranks, so this is how the
        // metadata travels alongside the file in a single request.
        if let (RankUploadConfig::Bundled { field }, Some(bundle)) =
            (&target.rank_upload, ranks_bundle)
        {
            if !bundle.players.is_empty() {
                let json =
                    serde_json::to_string(bundle).context("failed to serialize rank bundle")?;
                form = form.text(field.clone(), json);
            }
        }

        let response = self
            .request_with_auth_header(
                self.http
                    .post(target.endpoint_url(&target.replay_upload.path)?)
                    .multipart(form),
                auth_header,
            )
            .send()
            .await
            .with_context(|| format!("failed to upload replay to {}", target.name))?;

        let status = response.status();
        let location_header = response
            .headers()
            .get(LOCATION)
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned);
        let body = response.text().await.unwrap_or_default();
        let location = upload_location(target, location_header.as_deref(), &body);

        classify_upload_response(target, status, location, &body)
    }

    pub async fn upload_mmr(
        &self,
        target: &UploadDestinationConfig,
        payload: &MmrUpload,
        match_id: &str,
    ) -> Result<()> {
        let auth_header = target.auth.header_value()?;
        self.upload_mmr_with_auth_header(target, payload, match_id, auth_header)
            .await
    }

    /// Posts per-match player rank metadata, mirroring the BakkesMod
    /// AutoReplayUploader plugin's separate `POST /api/v1/mmr` request. No-op
    /// unless the destination uses an MMR endpoint and there is data to send.
    pub async fn upload_mmr_with_auth_header(
        &self,
        target: &UploadDestinationConfig,
        payload: &MmrUpload,
        match_id: &str,
        auth_header: Option<String>,
    ) -> Result<()> {
        let RankUploadConfig::Endpoint { path } = &target.rank_upload else {
            return Ok(());
        };
        if payload.players.is_empty() {
            return Ok(());
        }

        let response = self
            .request_with_auth_header(
                self.http
                    .post(target.endpoint_url_without_query(path)?)
                    .json(payload),
                auth_header,
            )
            .send()
            .await
            .with_context(|| format!("failed to upload MMR for {match_id} to {}", target.name))?;

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            bail!(
                "{} MMR upload for {match_id} failed with {status}: {body}",
                target.name
            );
        }
        Ok(())
    }

    fn request_with_auth_header(
        &self,
        request: reqwest::RequestBuilder,
        auth_header: Option<String>,
    ) -> reqwest::RequestBuilder {
        match auth_header {
            Some(value) => request.header(AUTHORIZATION, value),
            None => request,
        }
    }
}

impl Default for ReplayUploader {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadResult {
    pub outcome: UploadOutcome,
    pub location: Option<String>,
}

/// Player rank payload posted alongside a replay, matching the JSON schema the
/// BakkesMod AutoReplayUploader plugin sends to `ballchasing.com/api/v1/mmr`.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct MmrUpload {
    /// Match GUID the ranks belong to (the plugin calls this field `game`).
    pub game: String,
    pub players: Vec<MmrPlayer>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct MmrPlayer {
    /// Rocket League `OnlinePlatform` enum value (Steam=1, PS4=2, Xbox=4,
    /// Switch=7, Epic=11).
    pub platform_id: i64,
    /// Platform-specific player id (the middle component of the PsyNet PlayerID).
    pub id: String,
    pub before: MmrSkill,
    pub after: MmrSkill,
    /// Free-form debug field; the plugin stores the player name here.
    pub debug: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct MmrSkill {
    pub tier: i64,
    pub division: i64,
    pub matches_played: i64,
    pub mmr: f64,
}

/// Rich player rank payload bundled into the replay upload for destinations
/// that accept it (Rocket Sense). Unlike [`MmrUpload`], this carries the full
/// PsyNet skill snapshot — raw TrueSkill `mu`/`sigma` plus the before/after
/// tier/division/mmr — so the server can store everything available.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct RankBundle {
    pub players: Vec<RankBundlePlayer>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct RankBundlePlayer {
    /// Platform-specific online id (the middle component of the PsyNet PlayerID).
    pub platform_player_id: String,
    /// PsyNet platform name (e.g. `Epic`, `Steam`, `PS4`); the server normalizes.
    pub platform: String,
    /// Playlist (queue) the skill is for.
    pub playlist: i64,
    /// Whether the skill is ranked/meaningful (PsyNet `bValid`).
    pub valid: bool,
    /// Rank after the match (current rank).
    pub after: RankSnapshot,
    /// Rank before the match.
    pub before: RankSnapshot,
    /// Current per-playlist skill snapshot from `Skills/GetPlayersSkills`,
    /// carrying counters the match-history rank metadata lacks (`win_streak`,
    /// `matches_played`, `placement_matches_played`) plus PsyNet's own `mmr`.
    /// Point-in-time as of the sync, NOT the match moment — accurate for fresh
    /// uploads, stale for backfilled replays. Absent when PsyNet returned no
    /// skill for this player/playlist.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<CurrentSkill>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct CurrentSkill {
    pub mmr: f64,
    pub win_streak: i64,
    pub matches_played: i64,
    pub placement_matches_played: i64,
    /// Unix epoch (seconds) when this snapshot was fetched from PsyNet — always
    /// a moment *after* the match was played. The server compares it against the
    /// match timestamp to gauge staleness: a small gap means the counters still
    /// reflect the match; a large gap (backfilled replay) means they don't.
    pub fetched_at: i64,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct RankSnapshot {
    pub tier: i64,
    pub division: i64,
    pub mu: f64,
    pub sigma: f64,
    pub mmr: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UploadOutcome {
    Uploaded,
    Duplicate,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UploadCacheEntry {
    replay_id: String,
    location: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UploadCache {
    path: PathBuf,
    items: Vec<UploadCacheEntry>,
    index: HashMap<String, Option<String>>,
    max: usize,
}

impl UploadCache {
    pub fn load(path: PathBuf, account_count: usize) -> Result<Self> {
        let max = account_count
            .max(1)
            .saturating_mul(HISTORY_PER_ACCOUNT)
            .saturating_mul(MAX_CACHE_SIZE_FACTOR);
        let mut cache = Self {
            path,
            items: Vec::new(),
            index: HashMap::new(),
            max,
        };

        if !cache.path.exists() {
            return Ok(cache);
        }

        let file = fs::File::open(&cache.path)
            .with_context(|| format!("failed to open upload cache {}", cache.path.display()))?;
        for line in BufReader::new(file).lines() {
            let line = line?;
            if let Some(entry) = UploadCacheEntry::parse(&line) {
                let replay_id = entry.replay_id.clone();
                if let Entry::Vacant(index_entry) = cache.index.entry(replay_id) {
                    index_entry.insert(entry.location.clone());
                    cache.items.push(entry);
                }
            }
        }
        cache.ensure_capacity();
        Ok(cache)
    }

    pub fn contains(&self, replay_id: &str) -> bool {
        self.index.contains_key(replay_id)
    }

    pub fn location(&self, replay_id: &str) -> Option<&str> {
        self.index.get(replay_id).and_then(|value| value.as_deref())
    }

    pub fn add(&mut self, replay_id: impl Into<String>) -> Result<bool> {
        self.add_with_location(replay_id, None)
    }

    pub fn add_with_location(
        &mut self,
        replay_id: impl Into<String>,
        location: Option<String>,
    ) -> Result<bool> {
        let replay_id = replay_id.into();
        if let Some(cached_location) = self.index.get_mut(&replay_id) {
            let updated = location.is_some() && cached_location != &location;
            if updated {
                *cached_location = location.clone();
                if let Some(entry) = self
                    .items
                    .iter_mut()
                    .find(|entry| entry.replay_id == replay_id)
                {
                    entry.location = location;
                }
                self.save()?;
            }
            return Ok(updated);
        }
        self.index.insert(replay_id.clone(), location.clone());
        self.items.push(UploadCacheEntry {
            replay_id,
            location,
        });
        self.ensure_capacity();
        self.save()?;
        Ok(true)
    }

    pub fn save(&self) -> Result<()> {
        let mut content = String::new();
        for item in &self.items {
            content.push_str(&item.serialize());
            content.push('\n');
        }
        write_atomically(&self.path, content)
            .with_context(|| format!("failed to write upload cache {}", self.path.display()))
    }

    fn ensure_capacity(&mut self) {
        while self.items.len() > self.max {
            if let Some(oldest) = self.items.first().cloned() {
                self.items.remove(0);
                self.index.remove(&oldest.replay_id);
            }
        }
    }
}

impl UploadCacheEntry {
    fn parse(line: &str) -> Option<Self> {
        let line = line.trim();
        if line.is_empty() {
            return None;
        }
        let (replay_id, location) = line
            .split_once('\t')
            .map(|(replay_id, location)| {
                let location = (!location.trim().is_empty()).then(|| location.trim().to_string());
                (replay_id.trim().to_string(), location)
            })
            .unwrap_or_else(|| (line.to_string(), None));
        (!replay_id.is_empty()).then_some(Self {
            replay_id,
            location,
        })
    }

    fn serialize(&self) -> String {
        match &self.location {
            Some(location) => format!("{}\t{}", self.replay_id, location),
            None => self.replay_id.clone(),
        }
    }
}

fn upload_location(
    target: &UploadDestinationConfig,
    location_header: Option<&str>,
    body: &str,
) -> Option<String> {
    location_header
        .and_then(|location| absolutize_location(target, location))
        .or_else(|| {
            location_from_body(body).and_then(|location| absolutize_location(target, &location))
        })
        .or_else(|| rocket_sense_location(target, body))
}

fn absolutize_location(target: &UploadDestinationConfig, location: &str) -> Option<String> {
    if location.trim().is_empty() {
        return None;
    }
    target
        .url
        .join(location)
        .ok()
        .map(|url| url.to_string())
        .or_else(|| Some(location.to_string()))
}

fn location_from_body(body: &str) -> Option<String> {
    let value: Value = serde_json::from_str(body).ok()?;
    ["location", "link", "url"]
        .into_iter()
        .find_map(|field| value.get(field).and_then(Value::as_str))
        .map(ToOwned::to_owned)
}

fn rocket_sense_location(target: &UploadDestinationConfig, body: &str) -> Option<String> {
    if target.name != "Rocket Sense" {
        return None;
    }
    let value: Value = serde_json::from_str(body).ok()?;
    let replay_id = value
        .get("replay")
        .and_then(|replay| replay.get("id"))
        .and_then(Value::as_str)?;
    let mut base = target.url.clone();
    if base.path().ends_with("/api/v1") {
        let stripped = base.path().trim_end_matches("/api/v1").to_string();
        base.set_path(&stripped);
    }
    base.join(&format!("replays/{replay_id}"))
        .ok()
        .map(|url| url.to_string())
}

fn classify_upload_response(
    target: &UploadDestinationConfig,
    status: StatusCode,
    location: Option<String>,
    body: &str,
) -> Result<UploadResult> {
    let code = status.as_u16();
    if target.replay_upload.success_statuses.contains(&code) {
        Ok(UploadResult {
            outcome: UploadOutcome::Uploaded,
            location,
        })
    } else if target.replay_upload.duplicate_statuses.contains(&code)
        || rocket_sense_deduplicated(target, status, body)
    {
        Ok(UploadResult {
            outcome: UploadOutcome::Duplicate,
            location,
        })
    } else {
        bail!("{} upload failed with {status}: {body}", target.name)
    }
}

fn rocket_sense_deduplicated(
    target: &UploadDestinationConfig,
    status: StatusCode,
    body: &str,
) -> bool {
    if target.name != "Rocket Sense" || status != StatusCode::OK {
        return false;
    }
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        return false;
    };
    value
        .get("deduplicated")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upload_cache_dedupes_and_persists() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("uploaded.txt");
        let mut cache = UploadCache::load(path.clone(), 1).unwrap();

        assert!(cache.add("match-1").unwrap());
        assert!(!cache.add("match-1").unwrap());

        let restored = UploadCache::load(path, 1).unwrap();
        assert!(restored.contains("match-1"));
    }

    #[test]
    fn upload_cache_persists_locations() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("uploaded.txt");
        let mut cache = UploadCache::load(path.clone(), 1).unwrap();

        cache
            .add_with_location("match-1", Some("https://example.com/replay/1".to_string()))
            .unwrap();

        let restored = UploadCache::load(path, 1).unwrap();
        assert_eq!(
            restored.location("match-1"),
            Some("https://example.com/replay/1")
        );
    }

    #[test]
    fn upload_cache_reads_legacy_id_only_lines() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("uploaded.txt");
        fs::write(&path, "match-1\n").unwrap();

        let restored = UploadCache::load(path, 1).unwrap();

        assert!(restored.contains("match-1"));
        assert_eq!(restored.location("match-1"), None);
    }

    #[test]
    fn upload_cache_save_uses_temporary_file_without_leaving_part_files() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("uploaded.txt");
        let mut cache = UploadCache::load(path.clone(), 1).unwrap();

        cache.add("match-1").unwrap();

        let entries = fs::read_dir(tmp.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(entries, vec!["uploaded.txt"]);
    }

    #[test]
    fn rocket_sense_http_200_deduplicated_is_duplicate() {
        let target = UploadDestinationConfig::rocket_sense();
        let result = classify_upload_response(
            &target,
            StatusCode::OK,
            Some("https://rocket-sense.duckdns.org/replays/replay-1".to_string()),
            r#"{"deduplicated":true,"replay":{"id":"replay-1"}}"#,
        )
        .unwrap();

        assert_eq!(result.outcome, UploadOutcome::Duplicate);
        assert_eq!(
            result.location,
            Some("https://rocket-sense.duckdns.org/replays/replay-1".to_string())
        );
    }
}
