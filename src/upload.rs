use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use reqwest::header::AUTHORIZATION;

use crate::config::{StorageConfig, TargetAuth};

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

    pub async fn ping(&self, target: &StorageConfig) -> Result<()> {
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
        target: &StorageConfig,
        file_path: &Path,
    ) -> Result<UploadOutcome> {
        if !target.replay_upload.enabled {
            return Ok(UploadOutcome::Skipped);
        }

        self.ping(target).await?;

        let file_name = file_path
            .file_name()
            .and_then(|value| value.to_str())
            .context("replay path has no UTF-8 file name")?
            .to_string();
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
        let body = response.text().await.unwrap_or_default();
        let code = status.as_u16();

        if target.replay_upload.success_statuses.contains(&code) {
            Ok(UploadOutcome::Uploaded)
        } else if target.replay_upload.duplicate_statuses.contains(&code) {
            Ok(UploadOutcome::Duplicate)
        } else {
            bail!("{} upload failed with {status}: {body}", target.name)
        }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UploadOutcome {
    Uploaded,
    Duplicate,
    Skipped,
}

#[derive(Debug, Clone)]
pub struct UploadCache {
    path: PathBuf,
    items: Vec<String>,
    index: HashSet<String>,
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
            index: HashSet::new(),
            max,
        };

        if !cache.path.exists() {
            return Ok(cache);
        }

        let file = fs::File::open(&cache.path)
            .with_context(|| format!("failed to open upload cache {}", cache.path.display()))?;
        for line in BufReader::new(file).lines() {
            let id = line?.trim().to_string();
            if !id.is_empty() && cache.index.insert(id.clone()) {
                cache.items.push(id);
            }
        }
        cache.ensure_capacity();
        Ok(cache)
    }

    pub fn contains(&self, replay_id: &str) -> bool {
        self.index.contains(replay_id)
    }

    pub fn add(&mut self, replay_id: impl Into<String>) -> Result<bool> {
        let replay_id = replay_id.into();
        if !self.index.insert(replay_id.clone()) {
            return Ok(false);
        }
        self.items.push(replay_id);
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
            writeln!(file, "{item}")?;
        }
        Ok(())
    }

    fn ensure_capacity(&mut self) {
        while self.items.len() > self.max {
            if let Some(oldest) = self.items.first().cloned() {
                self.items.remove(0);
                self.index.remove(&oldest);
            }
        }
    }
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
}
