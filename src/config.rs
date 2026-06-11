use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use anyhow::{bail, Context, Result};
pub use psynet::PlayerPlatform;
use serde::{Deserialize, Deserializer, Serialize};
use url::Url;

use crate::state_file::write_atomically;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub behavior: BehaviorConfig,
    pub accounts: Vec<AccountConfig>,
    #[serde(alias = "storage")]
    pub upload_destinations: Vec<UploadDestinationConfig>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            behavior: BehaviorConfig::default(),
            accounts: vec![AccountConfig::default()],
            upload_destinations: vec![
                UploadDestinationConfig::rocky(),
                UploadDestinationConfig::ballchasing(),
                UploadDestinationConfig::rocket_sense(),
            ],
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("failed to read config {}", path.display()))?;
        let config: Self = toml::from_str(&content)
            .with_context(|| format!("failed to parse TOML config {}", path.display()))?;
        config.validate()?;
        Ok(config)
    }

    pub fn load_or_default(path: &Path) -> Result<Self> {
        if path.exists() {
            Self::load(path)
        } else {
            let config = Self::default();
            config.validate()?;
            Ok(config)
        }
    }

    pub fn to_pretty_toml(&self) -> Result<String> {
        toml::to_string_pretty(self).context("failed to serialize config as TOML")
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        write_atomically(path, self.to_pretty_toml()?)
            .with_context(|| format!("failed to write config {}", path.display()))
    }

    pub fn upload_destination(&self, name: &str) -> Option<&UploadDestinationConfig> {
        self.upload_destinations
            .iter()
            .find(|target| target.name == name)
    }

    pub fn validate(&self) -> Result<()> {
        self.behavior.validate()?;

        if self.accounts.is_empty() {
            bail!("config must define at least one account");
        }

        let mut account_ids = HashSet::new();
        let mut auth_ids = HashSet::new();
        for account in &self.accounts {
            account.validate()?;
            if !account_ids.insert(account.id) {
                bail!("duplicate account id {}", account.id);
            }
            if !auth_ids.insert(account.auth_id()) {
                bail!("duplicate account auth id {}", account.auth_id());
            }
        }

        if self.upload_destinations.is_empty() {
            bail!("config must define at least one upload destination");
        }

        let mut target_names = HashSet::new();
        for target in &self.upload_destinations {
            target.validate()?;
            if !target_names.insert(target.name.as_str()) {
                bail!("duplicate upload destination {:?}", target.name);
            }
        }

        if let Some(selected) = &self.behavior.selected_account {
            if !self
                .accounts
                .iter()
                .any(|account| &account.name == selected)
            {
                bail!("selected account {selected:?} does not exist");
            }
        }

        if let Some(selected) = &self.behavior.selected_upload_destination {
            if !self
                .upload_destinations
                .iter()
                .any(|target| &target.name == selected)
            {
                bail!("selected upload destination {selected:?} does not exist");
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct BehaviorConfig {
    pub auto_upload: bool,
    pub exit_in_tray: bool,
    pub start_in_tray: bool,
    pub upload_on_launch: bool,
    pub no_upload_while_connected: bool,
    #[serde(with = "humantime_serde")]
    pub auto_upload_interval: Duration,
    #[serde(with = "humantime_serde")]
    pub auto_upload_jitter_max: Duration,
    pub selected_account: Option<String>,
    #[serde(alias = "selected_storage")]
    pub selected_upload_destination: Option<String>,
}

impl Default for BehaviorConfig {
    fn default() -> Self {
        Self {
            auto_upload: true,
            exit_in_tray: true,
            start_in_tray: false,
            upload_on_launch: false,
            no_upload_while_connected: true,
            auto_upload_interval: Duration::from_secs(45 * 60),
            auto_upload_jitter_max: Duration::from_secs(15 * 60),
            selected_account: None,
            selected_upload_destination: None,
        }
    }
}

impl BehaviorConfig {
    pub fn validate(&self) -> Result<()> {
        if self.auto_upload_interval < Duration::from_secs(60) {
            bail!("auto_upload_interval must be at least 60s");
        }
        if self.auto_upload_jitter_max > self.auto_upload_interval {
            bail!("auto_upload_jitter_max cannot exceed auto_upload_interval");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AccountConfig {
    pub id: u32,
    pub name: String,
    #[serde(
        default,
        rename = "profile_id",
        skip_serializing_if = "Option::is_none"
    )]
    legacy_profile_id: Option<u32>,
    pub platform: PlayerPlatform,
    #[serde(skip_serializing_if = "is_true")]
    pub sync_enabled: bool,
}

impl Default for AccountConfig {
    fn default() -> Self {
        Self::new(0, "Primary".to_string(), PlayerPlatform::Epic, true)
    }
}

impl AccountConfig {
    pub fn new(id: u32, name: String, platform: PlayerPlatform, sync_enabled: bool) -> Self {
        Self {
            id,
            name,
            legacy_profile_id: None,
            platform,
            sync_enabled,
        }
    }

    pub fn auth_id(&self) -> u32 {
        self.legacy_profile_id.unwrap_or(self.id)
    }

    pub fn legacy_profile_id(&self) -> Option<u32> {
        self.legacy_profile_id
    }

    pub fn validate(&self) -> Result<()> {
        validate_name("account name", &self.name)
    }
}

impl<'de> Deserialize<'de> for AccountConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let input = AccountConfigInput::deserialize(deserializer)?;
        let sync_enabled = input
            .sync_enabled
            .unwrap_or_else(|| !input.unused.unwrap_or(false));

        Ok(Self {
            id: input.id,
            name: input.name,
            legacy_profile_id: input.legacy_profile_id,
            platform: input.platform,
            sync_enabled,
        })
    }
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct AccountConfigInput {
    id: u32,
    name: String,
    #[serde(rename = "profile_id")]
    legacy_profile_id: Option<u32>,
    platform: PlayerPlatform,
    sync_enabled: Option<bool>,
    unused: Option<bool>,
}

impl Default for AccountConfigInput {
    fn default() -> Self {
        Self {
            id: 0,
            name: "Primary".to_string(),
            legacy_profile_id: None,
            platform: PlayerPlatform::Epic,
            sync_enabled: None,
            unused: None,
        }
    }
}

fn is_true(value: &bool) -> bool {
    *value
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct UploadDestinationConfig {
    pub name: String,
    pub url: Url,
    #[serde(default, skip_serializing, rename = "predefined")]
    _legacy_predefined: IgnoredLegacyBool,
    #[serde(default, skip_serializing, rename = "primary")]
    _legacy_primary: IgnoredLegacyBool,
    pub query: BTreeMap<String, String>,
    pub auth: TargetAuth,
    pub ping: PingConfig,
    pub replay_upload: ReplayUploadConfig,
    #[serde(default)]
    pub rank_upload: RankUploadConfig,
}

impl UploadDestinationConfig {
    pub fn rocky() -> Self {
        Self {
            name: "Rocky".to_string(),
            url: Url::parse("https://lexore.ca/rocky/api").expect("valid built-in Rocky URL"),
            _legacy_predefined: IgnoredLegacyBool,
            _legacy_primary: IgnoredLegacyBool,
            query: BTreeMap::new(),
            auth: TargetAuth::None,
            ping: PingConfig {
                enabled: false,
                path: "/".to_string(),
            },
            replay_upload: ReplayUploadConfig {
                enabled: true,
                path: "/upload".to_string(),
                file_field: "file".to_string(),
                success_statuses: vec![201],
                duplicate_statuses: vec![409],
            },
            rank_upload: RankUploadConfig::None,
        }
    }

    pub fn ballchasing() -> Self {
        Self {
            name: "Ballchasing".to_string(),
            url: Url::parse("https://ballchasing.com/api").expect("valid built-in Ballchasing URL"),
            _legacy_predefined: IgnoredLegacyBool,
            _legacy_primary: IgnoredLegacyBool,
            query: BTreeMap::from([("visibility".to_string(), "public".to_string())]),
            auth: TargetAuth::None,
            ping: PingConfig {
                enabled: true,
                path: "/".to_string(),
            },
            replay_upload: ReplayUploadConfig {
                enabled: true,
                path: "/v2/upload".to_string(),
                file_field: "file".to_string(),
                success_statuses: vec![201],
                duplicate_statuses: vec![409],
            },
            rank_upload: RankUploadConfig::Endpoint {
                path: "/v1/mmr".to_string(),
            },
        }
    }

    pub fn rocket_sense() -> Self {
        Self {
            name: "Rocket Sense".to_string(),
            url: Url::parse("https://rocket-sense.duckdns.org/api/v1")
                .expect("valid built-in Rocket Sense URL"),
            _legacy_predefined: IgnoredLegacyBool,
            _legacy_primary: IgnoredLegacyBool,
            query: BTreeMap::new(),
            auth: TargetAuth::BearerEnv {
                variable: "ROCKET_SENSE_TOKEN".to_string(),
            },
            ping: PingConfig {
                enabled: true,
                path: "/health".to_string(),
            },
            replay_upload: ReplayUploadConfig {
                enabled: true,
                path: "/replays".to_string(),
                file_field: "file".to_string(),
                success_statuses: vec![201],
                duplicate_statuses: vec![200, 409],
            },
            rank_upload: RankUploadConfig::Bundled {
                field: "ranks".to_string(),
            },
        }
    }

    pub fn validate(&self) -> Result<()> {
        validate_name("upload destination name", &self.name)?;
        validate_http_url(&self.url)?;
        self.auth.validate()?;
        self.ping.validate()?;
        self.replay_upload.validate()?;
        self.rank_upload.validate()?;
        Ok(())
    }

    pub fn endpoint_url(&self, path: &str) -> Result<Url> {
        self.build_endpoint_url(path, true)
    }

    /// Like [`Self::endpoint_url`] but without appending the destination's
    /// configured query parameters. Used for the MMR endpoint, which (matching
    /// the BakkesMod uploader) takes no `visibility`-style query parameters.
    pub fn endpoint_url_without_query(&self, path: &str) -> Result<Url> {
        self.build_endpoint_url(path, false)
    }

    fn build_endpoint_url(&self, path: &str, include_query: bool) -> Result<Url> {
        let mut url = self.url.clone();
        let base_path = url.path().trim_end_matches('/');
        let endpoint_path = path.trim_start_matches('/');
        url.set_path(&format!("{base_path}/{endpoint_path}"));
        if include_query && !self.query.is_empty() {
            let mut pairs = url.query_pairs_mut();
            for (key, value) in &self.query {
                pairs.append_pair(key, value);
            }
        }
        Ok(url)
    }
}

impl Default for UploadDestinationConfig {
    fn default() -> Self {
        Self::rocky()
    }
}

#[derive(Debug, Clone, Copy, Default, Eq)]
struct IgnoredLegacyBool;

impl PartialEq for IgnoredLegacyBool {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl<'de> Deserialize<'de> for IgnoredLegacyBool {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        bool::deserialize(deserializer)?;
        Ok(Self)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TargetAuth {
    #[default]
    None,
    AuthorizationHeader {
        value: String,
    },
    Bearer {
        token: String,
    },
    BearerEnv {
        variable: String,
    },
    BearerCommand {
        command: Vec<String>,
    },
}

impl TargetAuth {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::None => Ok(()),
            Self::AuthorizationHeader { value } => {
                if value.trim().is_empty() {
                    bail!("authorization header value cannot be empty");
                }
                Ok(())
            }
            Self::Bearer { token } => {
                if token.trim().is_empty() {
                    bail!("bearer token cannot be empty");
                }
                Ok(())
            }
            Self::BearerEnv { variable } => {
                validate_env_var_name("bearer token environment variable", variable)
            }
            Self::BearerCommand { command } => validate_token_command(command),
        }
    }

    pub fn header_value(&self) -> Result<Option<String>> {
        match self {
            Self::None => Ok(None),
            Self::AuthorizationHeader { value } => Ok(Some(value.clone())),
            Self::Bearer { token } => Ok(Some(format!("Bearer {token}"))),
            Self::BearerEnv { variable } => {
                let token = std::env::var(variable)
                    .with_context(|| format!("{variable} must be set for bearer auth"))?;
                bearer_header(token, variable)
            }
            Self::BearerCommand { command } => bearer_command_header(command),
        }
    }
}

fn bearer_header(token: impl AsRef<str>, source: &str) -> Result<Option<String>> {
    let token = token.as_ref().trim();
    if token.is_empty() {
        bail!("{source} did not provide a bearer token");
    }
    Ok(Some(format!("Bearer {token}")))
}

fn validate_token_command(command: &[String]) -> Result<()> {
    if command.is_empty() {
        bail!("bearer token command cannot be empty");
    }
    for part in command {
        if part.trim().is_empty() {
            bail!("bearer token command cannot contain empty arguments");
        }
    }
    Ok(())
}

fn bearer_command_header(command: &[String]) -> Result<Option<String>> {
    validate_token_command(command)?;
    let (program, args) = command.split_first().expect("validated non-empty command");
    let output = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("failed to run bearer token command {program:?}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr = stderr.trim();
        if stderr.is_empty() {
            bail!(
                "bearer token command {program:?} failed with {}",
                output.status
            );
        } else {
            bail!(
                "bearer token command {program:?} failed with {}: {stderr}",
                output.status
            );
        }
    }

    let token = String::from_utf8(output.stdout)
        .with_context(|| format!("bearer token command {program:?} did not output UTF-8"))?;
    bearer_header(token, "bearer token command")
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct PingConfig {
    pub enabled: bool,
    pub path: String,
}

impl Default for PingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            path: "/".to_string(),
        }
    }
}

impl PingConfig {
    pub fn validate(&self) -> Result<()> {
        validate_endpoint_path("ping.path", &self.path)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct ReplayUploadConfig {
    pub enabled: bool,
    pub path: String,
    pub file_field: String,
    pub success_statuses: Vec<u16>,
    pub duplicate_statuses: Vec<u16>,
}

impl Default for ReplayUploadConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            path: "/upload".to_string(),
            file_field: "file".to_string(),
            success_statuses: vec![201],
            duplicate_statuses: vec![409],
        }
    }
}

impl ReplayUploadConfig {
    pub fn validate(&self) -> Result<()> {
        validate_endpoint_path("replay_upload.path", &self.path)?;
        validate_name("replay_upload.file_field", &self.file_field)?;
        validate_statuses("replay_upload.success_statuses", &self.success_statuses)?;
        validate_statuses("replay_upload.duplicate_statuses", &self.duplicate_statuses)?;
        Ok(())
    }
}

/// How a destination accepts per-match player rank metadata.
///
/// Replay files do not carry ranks, so — mirroring the BakkesMod
/// AutoReplayUploader plugin — the uploader submits them out of band, either as
/// a separate POST to a ballchasing-style MMR endpoint or bundled into the
/// replay upload as a multipart field (the richer Rocket Sense payload). Off by
/// default; the built-in Ballchasing and Rocket Sense destinations enable it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum RankUploadConfig {
    /// No rank metadata is sent.
    #[default]
    None,
    /// Posted as a separate request to a ballchasing-style MMR endpoint, using
    /// the BakkesMod-shaped JSON payload.
    Endpoint { path: String },
    /// Included as a multipart field in the replay upload, using the richer
    /// Rocket Sense rank payload (full before/after skill snapshot).
    Bundled { field: String },
}

impl RankUploadConfig {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::None => Ok(()),
            Self::Endpoint { path } => validate_endpoint_path("rank_upload.path", path),
            Self::Bundled { field } => validate_name("rank_upload.field", field),
        }
    }
}

fn validate_name(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{label} cannot be empty");
    }
    if value.contains(['\n', '\r', '\0']) {
        bail!("{label} cannot contain control characters");
    }
    Ok(())
}

fn validate_env_var_name(label: &str, value: &str) -> Result<()> {
    validate_name(label, value)?;
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        bail!("{label} cannot be empty");
    };
    if !(first == '_' || first.is_ascii_alphabetic()) {
        bail!("{label} must start with an ASCII letter or underscore");
    }
    if !chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric()) {
        bail!("{label} must contain only ASCII letters, digits, and underscores");
    }
    Ok(())
}

fn validate_http_url(url: &Url) -> Result<()> {
    match url.scheme() {
        "http" | "https" => Ok(()),
        scheme => bail!("upload destination URL must use http or https, got {scheme:?}"),
    }
}

fn validate_endpoint_path(label: &str, value: &str) -> Result<()> {
    if !value.starts_with('/') {
        bail!("{label} must start with /");
    }
    if value.contains(['\n', '\r', '\0']) {
        bail!("{label} cannot contain control characters");
    }
    Ok(())
}

fn validate_statuses(label: &str, values: &[u16]) -> Result<()> {
    if values.is_empty() {
        bail!("{label} cannot be empty");
    }
    for value in values {
        if !(100..=599).contains(value) {
            bail!("{label} contains invalid HTTP status {value}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_round_trips_as_toml() {
        let config = Config::default();
        config.validate().unwrap();

        let toml = config.to_pretty_toml().unwrap();
        let parsed: Config = toml::from_str(&toml).unwrap();

        assert_eq!(parsed, config);
    }

    #[test]
    fn rejects_unknown_toml_fields() {
        let err = toml::from_str::<Config>("surprise = true").unwrap_err();

        assert!(err.to_string().contains("unknown field"));
    }

    #[test]
    fn rejects_duplicate_upload_destination_names() {
        let mut config = Config::default();
        config
            .upload_destinations
            .push(UploadDestinationConfig::rocky());

        let err = config.validate().unwrap_err();

        assert!(err.to_string().contains("duplicate upload destination"));
    }

    #[test]
    fn accepts_legacy_storage_field_names() {
        let mut config = Config::default();
        config.behavior.selected_upload_destination = Some("Rocket Sense".to_string());
        let toml = config.to_pretty_toml().unwrap();
        let legacy_toml = toml
            .replace("selected_upload_destination", "selected_storage")
            .replace("upload_destinations", "storage");

        let parsed: Config = toml::from_str(&legacy_toml).unwrap();

        assert_eq!(parsed, config);
    }

    #[test]
    fn accepts_legacy_upload_destination_badge_fields_without_reserializing_them() {
        let config = Config::default();
        let toml = config.to_pretty_toml().unwrap();
        let legacy_toml = toml.replacen(
            "query = {}",
            "predefined = true\nprimary = true\nquery = {}",
            1,
        );

        let parsed: Config = toml::from_str(&legacy_toml).unwrap();
        let serialized = parsed.to_pretty_toml().unwrap();

        assert_eq!(parsed, config);
        assert!(!serialized.contains("predefined ="));
        assert!(!serialized.contains("primary ="));
    }

    #[test]
    fn default_accounts_do_not_serialize_profile_ids() {
        let toml = Config::default().to_pretty_toml().unwrap();

        assert!(!toml.contains("profile_id"));
    }

    #[test]
    fn accepts_legacy_account_profile_id_as_auth_id() {
        let toml = Config::default().to_pretty_toml().unwrap();
        let legacy_toml = toml.replacen("id = 0", "id = 42\nprofile_id = 7", 1);

        let parsed: Config = toml::from_str(&legacy_toml).unwrap();

        assert_eq!(parsed.accounts[0].id, 42);
        assert_eq!(parsed.accounts[0].auth_id(), 7);
        assert!(parsed.to_pretty_toml().unwrap().contains("profile_id = 7"));
    }

    #[test]
    fn accepts_legacy_unused_account_field_as_sync_disabled() {
        let toml = Config::default().to_pretty_toml().unwrap();
        let legacy_toml = toml.replacen(
            "platform = \"epic\"",
            "platform = \"epic\"\nunused = true",
            1,
        );

        let parsed: Config = toml::from_str(&legacy_toml).unwrap();
        let serialized = parsed.to_pretty_toml().unwrap();

        assert!(!parsed.accounts[0].sync_enabled);
        assert!(serialized.contains("sync_enabled = false"));
        assert!(!serialized.contains("unused ="));
    }

    #[test]
    fn bearer_command_reads_token_from_stdout() {
        let auth = TargetAuth::BearerCommand {
            command: vec!["printf".to_string(), "token-from-command\n".to_string()],
        };

        assert_eq!(
            auth.header_value().unwrap(),
            Some("Bearer token-from-command".to_string())
        );
    }

    #[test]
    fn bearer_command_rejects_empty_stdout() {
        let auth = TargetAuth::BearerCommand {
            command: vec!["true".to_string()],
        };

        assert!(auth
            .header_value()
            .unwrap_err()
            .to_string()
            .contains("did not provide a bearer token"));
    }

    #[test]
    fn endpoint_url_keeps_base_path_and_query() {
        let target = UploadDestinationConfig::ballchasing();

        let url = target.endpoint_url(&target.replay_upload.path).unwrap();

        assert_eq!(
            url.as_str(),
            "https://ballchasing.com/api/v2/upload?visibility=public"
        );
    }

    #[test]
    fn ballchasing_uses_mmr_endpoint_without_query() {
        let target = UploadDestinationConfig::ballchasing();

        let RankUploadConfig::Endpoint { path } = &target.rank_upload else {
            panic!("ballchasing should use an MMR endpoint");
        };
        let url = target.endpoint_url_without_query(path).unwrap();
        assert_eq!(url.as_str(), "https://ballchasing.com/api/v1/mmr");
    }

    #[test]
    fn rocket_sense_bundles_ranks_with_upload() {
        assert_eq!(
            UploadDestinationConfig::rocket_sense().rank_upload,
            RankUploadConfig::Bundled {
                field: "ranks".to_string()
            }
        );
    }

    #[test]
    fn rocky_rank_upload_is_disabled_by_default() {
        assert_eq!(
            UploadDestinationConfig::rocky().rank_upload,
            RankUploadConfig::None
        );
    }

    #[test]
    fn rocket_sense_defaults_to_local_api_upload() {
        let target = UploadDestinationConfig::rocket_sense();

        assert_eq!(
            target
                .endpoint_url(&target.replay_upload.path)
                .unwrap()
                .as_str(),
            "https://rocket-sense.duckdns.org/api/v1/replays"
        );
        assert_eq!(
            target.endpoint_url(&target.ping.path).unwrap().as_str(),
            "https://rocket-sense.duckdns.org/api/v1/health"
        );
        assert_eq!(
            target.auth,
            TargetAuth::BearerEnv {
                variable: "ROCKET_SENSE_TOKEN".to_string()
            }
        );
        assert_eq!(target.replay_upload.duplicate_statuses, vec![200, 409]);
    }
}
