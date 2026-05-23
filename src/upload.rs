use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use reqwest::header::{AUTHORIZATION, LOCATION};
use reqwest::StatusCode;
use serde_json::Value;

use crate::config::{TargetAuth, UploadDestinationConfig};

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

        let response = self
            .request_with_auth(
                self.http.get(target.endpoint_url(&target.ping.path)?),
                &target.auth,
            )?
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
        if !target.replay_upload.enabled {
            return Ok(UploadResult {
                outcome: UploadOutcome::Skipped,
                location: None,
            });
        }

        self.ping(target).await?;

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
        let form =
            reqwest::multipart::Form::new().part(target.replay_upload.file_field.clone(), part);

        let response = self
            .request_with_auth(
                self.http
                    .post(target.endpoint_url(&target.replay_upload.path)?)
                    .multipart(form),
                &target.auth,
            )?
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

    fn request_with_auth(
        &self,
        request: reqwest::RequestBuilder,
        auth: &TargetAuth,
    ) -> Result<reqwest::RequestBuilder> {
        Ok(match auth.header_value()? {
            Some(value) => request.header(AUTHORIZATION, value),
            None => request,
        })
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
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&self.path)
            .with_context(|| format!("failed to write upload cache {}", self.path.display()))?;
        for item in &self.items {
            writeln!(file, "{}", item.serialize())?;
        }
        Ok(())
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
