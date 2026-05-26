use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use chrono::{DateTime, Utc};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use tokio::time::sleep;

use crate::config::AccountConfig;
use crate::paths::AppPaths;
use crate::state_file::write_private_atomically;

const EGS_USER_AGENT: &str =
    "UELauncher/11.0.1-14907503+++Portal+Release-Live Windows/10.0.19041.1.256.64bit";
const EGS_CLIENT_ID: &str = "34a02cf8f4414e29b15921876da36f9a";
const EGS_CLIENT_SECRET: &str = "daafbccc737745039dffe53d94fc76cf";
const EGS_OAUTH_HOST: &str = "account-public-service-prod03.ol.epicgames.com";
const EOS_DEPLOYMENT_ID: &str = "da32ae9c12ae40e8a112c52e1f17f3ba";
const EOS_CLIENT_ID: &str = "xyza7891p5D7s9R6Gm6moTHWGloerp7B";
const EOS_SECRET: &str = "Knh18du4NVlFs+3uQ+ZPpDCVto0WYf4yXP8+OcwVt1o";
const REFRESH_MARGIN: Duration = Duration::from_secs(15 * 60);

#[derive(Debug, Clone)]
pub struct AuthManager {
    client: EpicClient,
    store: TokenStore,
}

impl AuthManager {
    pub fn new(paths: &AppPaths, account_id: u32) -> Self {
        Self {
            client: EpicClient::new(),
            store: TokenStore::new(paths.tokens_dir(), account_id, None),
        }
    }

    pub fn for_legacy_profile(paths: &AppPaths, profile_id: u32) -> Self {
        Self {
            client: EpicClient::new(),
            store: TokenStore::new(paths.tokens_dir(), profile_id, Some(profile_id)),
        }
    }

    pub fn for_account(paths: &AppPaths, account: &AccountConfig) -> Self {
        Self {
            client: EpicClient::new(),
            store: TokenStore::new(paths.tokens_dir(), account.id, account.legacy_profile_id()),
        }
    }

    pub fn login_url(&self) -> String {
        self.client.login_url()
    }

    pub async fn authenticate_with_code(&self, code: &str) -> Result<EosTokenResponse> {
        let egs = self.client.authenticate_with_code(code.trim()).await?;
        self.store.save_egs_refresh(&egs.refresh_token)?;

        let exchange_code = self
            .client
            .exchange_code(egs.access_token.expose_secret())
            .await?;
        let eos = self.client.exchange_eos_token(&exchange_code).await?;
        self.store.save_eos_refresh(&eos.refresh_token)?;
        Ok(eos)
    }

    pub async fn begin_device_auth(&self) -> Result<DeviceAuthResponse> {
        self.client.begin_device_auth().await
    }

    pub async fn wait_for_device_auth(
        &self,
        device: &DeviceAuthResponse,
    ) -> Result<EosTokenResponse> {
        let eos = self.client.wait_for_device_auth(device).await?;
        self.store.save_eos_refresh(&eos.refresh_token)?;
        Ok(eos)
    }

    pub async fn restore_or_refresh(&self) -> Result<EosTokenResponse> {
        if let Some(refresh_token) = self.store.read_egs_refresh()? {
            let egs = self
                .client
                .authenticate_with_refresh_token(refresh_token.expose_secret())
                .await
                .context("failed to refresh EGS token from local store")?;
            self.store.save_egs_refresh(&egs.refresh_token)?;
            let exchange_code = self
                .client
                .exchange_code(egs.access_token.expose_secret())
                .await?;
            let eos = self.client.exchange_eos_token(&exchange_code).await?;
            self.store.save_eos_refresh(&eos.refresh_token)?;
            return Ok(eos);
        }

        if let Some(refresh_token) = self.store.read_eos_refresh()? {
            let eos = self
                .client
                .refresh_eos_token(refresh_token.expose_secret())
                .await
                .context("failed to refresh EOS token from local store")?;
            self.store.save_eos_refresh(&eos.refresh_token)?;
            return Ok(eos);
        }

        bail!("no saved auth token for account {}", self.store.account_id)
    }

    pub fn clear(&self) -> Result<()> {
        self.store.clear()
    }
}

#[derive(Debug, Clone)]
pub struct EpicClient {
    http: reqwest::Client,
}

impl EpicClient {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("valid reqwest client"),
        }
    }

    pub fn login_url(&self) -> String {
        let redirect = format!(
            "https://www.epicgames.com/id/api/redirect?clientId={EGS_CLIENT_ID}&responseType=code"
        );
        format!(
            "https://www.epicgames.com/id/login?redirectUrl={}",
            url::form_urlencoded::byte_serialize(redirect.as_bytes()).collect::<String>()
        )
    }

    pub async fn authenticate_with_code(&self, code: &str) -> Result<TokenResponse> {
        self.request_egs_token(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("token_type", "eg1"),
        ])
        .await
    }

    pub async fn authenticate_with_refresh_token(
        &self,
        refresh_token: &str,
    ) -> Result<TokenResponse> {
        self.request_egs_token(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("token_type", "eg1"),
        ])
        .await
    }

    pub async fn exchange_code(&self, access_token: &str) -> Result<String> {
        let response = self
            .http
            .get(format!(
                "https://{EGS_OAUTH_HOST}/account/api/oauth/exchange"
            ))
            .header(USER_AGENT, EGS_USER_AGENT)
            .header(AUTHORIZATION, format!("bearer {access_token}"))
            .send()
            .await
            .context("failed to request EGS exchange code")?;

        let status = response.status();
        let body = response
            .text()
            .await
            .context("failed to read exchange response")?;
        if !status.is_success() {
            bail!("EGS exchange failed with {status}: {body}");
        }

        #[derive(Deserialize)]
        struct ExchangeCode {
            code: String,
        }

        let parsed: ExchangeCode =
            serde_json::from_str(&body).context("failed to parse EGS exchange response")?;
        Ok(parsed.code)
    }

    pub async fn exchange_eos_token(&self, exchange_code: &str) -> Result<EosTokenResponse> {
        self.request_eos_token(&[
            ("grant_type", "exchange_code"),
            ("exchange_code", exchange_code),
        ])
        .await
    }

    pub async fn refresh_eos_token(&self, refresh_token: &str) -> Result<EosTokenResponse> {
        self.request_eos_token(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ])
        .await
    }

    pub async fn begin_device_auth(&self) -> Result<DeviceAuthResponse> {
        let response = self
            .http
            .post("https://api.epicgames.dev/epic/oauth/v2/deviceAuthorization")
            .header(USER_AGENT, EGS_USER_AGENT)
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .form(&[("client_id", EOS_CLIENT_ID)])
            .send()
            .await
            .context("failed to begin Epic device authorization")?;

        parse_json_response(response, "Epic device authorization").await
    }

    pub async fn wait_for_device_auth(
        &self,
        device: &DeviceAuthResponse,
    ) -> Result<EosTokenResponse> {
        if device.interval == 0 || device.expires_in == 0 {
            bail!("invalid device authorization polling interval");
        }

        let attempts = device.expires_in / device.interval;
        for _ in 0..attempts {
            match self
                .request_eos_token(&[
                    ("grant_type", "device_code"),
                    ("device_code", device.device_code.expose_secret()),
                ])
                .await
            {
                Ok(token) => return Ok(token),
                Err(error) => {
                    tracing::debug!(%error, "device authorization not complete yet");
                    sleep(Duration::from_secs(device.interval)).await;
                }
            }
        }

        bail!("Epic device authorization timed out")
    }

    async fn request_egs_token(&self, form: &[(&str, &str)]) -> Result<TokenResponse> {
        let auth = basic_auth(EGS_CLIENT_ID, EGS_CLIENT_SECRET);
        let response = self
            .http
            .post(format!("https://{EGS_OAUTH_HOST}/account/api/oauth/token"))
            .header(USER_AGENT, EGS_USER_AGENT)
            .header(AUTHORIZATION, auth)
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .form(form)
            .send()
            .await
            .context("failed to request EGS token")?;

        parse_json_response(response, "EGS token request").await
    }

    async fn request_eos_token(&self, form: &[(&str, &str)]) -> Result<EosTokenResponse> {
        let mut owned_form: Vec<(&str, &str)> = Vec::with_capacity(form.len() + 2);
        owned_form.extend_from_slice(form);
        owned_form.push(("deployment_id", EOS_DEPLOYMENT_ID));
        owned_form.push(("scope", "basic_profile"));

        let response = self
            .http
            .post("https://api.epicgames.dev/epic/oauth/v2/token")
            .header(USER_AGENT, EGS_USER_AGENT)
            .header(AUTHORIZATION, basic_auth(EOS_CLIENT_ID, EOS_SECRET))
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .form(&owned_form)
            .send()
            .await
            .context("failed to request EOS token")?;

        parse_json_response(response, "EOS token request").await
    }
}

impl Default for EpicClient {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct TokenResponse {
    pub access_token: SecretString,
    pub refresh_token: SecretString,
    pub expires_at: String,
    pub account_id: String,
    #[serde(default, rename = "displayName")]
    pub display_name: String,
}

impl TokenResponse {
    pub fn expires_soon(&self) -> bool {
        expires_soon(&self.expires_at)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct EosTokenResponse {
    pub access_token: SecretString,
    pub refresh_token: SecretString,
    pub expires_at: String,
    pub refresh_expires_at: String,
    pub account_id: String,
    #[serde(default)]
    pub selected_account_id: String,
}

impl EosTokenResponse {
    pub fn expires_soon(&self) -> bool {
        expires_soon(&self.expires_at)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceAuthResponse {
    pub user_code: String,
    pub device_code: SecretString,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Debug, Clone)]
struct TokenStore {
    dir: PathBuf,
    account_id: u32,
    legacy_profile_id: Option<u32>,
}

impl TokenStore {
    fn new(dir: PathBuf, account_id: u32, legacy_profile_id: Option<u32>) -> Self {
        Self {
            dir,
            account_id,
            legacy_profile_id,
        }
    }

    fn read_egs_refresh(&self) -> Result<Option<SecretString>> {
        read_secret_with_legacy(&self.egs_path(), self.legacy_egs_path().as_ref())
    }

    fn read_eos_refresh(&self) -> Result<Option<SecretString>> {
        read_secret_with_legacy(&self.eos_path(), self.legacy_eos_path().as_ref())
    }

    fn save_egs_refresh(&self, refresh_token: &SecretString) -> Result<()> {
        write_secret(&self.egs_path(), refresh_token)
    }

    fn save_eos_refresh(&self, refresh_token: &SecretString) -> Result<()> {
        write_secret(&self.eos_path(), refresh_token)
    }

    fn clear(&self) -> Result<()> {
        remove_if_exists(&self.egs_path())?;
        remove_if_exists(&self.eos_path())?;
        if let Some(path) = self.legacy_egs_path() {
            remove_if_exists(&path)?;
        }
        if let Some(path) = self.legacy_eos_path() {
            remove_if_exists(&path)?;
        }
        Ok(())
    }

    fn egs_path(&self) -> PathBuf {
        self.dir
            .join(format!("account-{}-egs.refresh", self.account_id))
    }

    fn eos_path(&self) -> PathBuf {
        self.dir
            .join(format!("account-{}-eos.refresh", self.account_id))
    }

    fn legacy_egs_path(&self) -> Option<PathBuf> {
        self.legacy_profile_id
            .map(|profile_id| self.dir.join(format!("profile-{profile_id}-egs.refresh")))
    }

    fn legacy_eos_path(&self) -> Option<PathBuf> {
        self.legacy_profile_id
            .map(|profile_id| self.dir.join(format!("profile-{profile_id}-eos.refresh")))
    }
}

async fn parse_json_response<T: for<'de> Deserialize<'de>>(
    response: reqwest::Response,
    context: &str,
) -> Result<T> {
    let status = response.status();
    let body = response
        .text()
        .await
        .with_context(|| format!("failed to read {context} response"))?;
    if !status.is_success() {
        return Err(anyhow!("{context} failed with {status}: {body}"));
    }

    serde_json::from_str(&body).with_context(|| format!("failed to parse {context} response"))
}

fn basic_auth(client_id: &str, client_secret: &str) -> String {
    let credentials = format!("{client_id}:{client_secret}");
    let encoded = base64::engine::general_purpose::STANDARD.encode(credentials);
    format!("Basic {encoded}")
}

fn read_secret(path: &PathBuf) -> Result<Option<SecretString>> {
    if !path.exists() {
        return Ok(None);
    }
    let value = fs::read_to_string(path)
        .with_context(|| format!("failed to read token {}", path.display()))?
        .trim()
        .to_string();
    if value.is_empty() {
        Ok(None)
    } else {
        Ok(Some(SecretString::from(value)))
    }
}

fn read_secret_with_legacy(
    path: &PathBuf,
    legacy_path: Option<&PathBuf>,
) -> Result<Option<SecretString>> {
    if let Some(secret) = read_secret(path)? {
        return Ok(Some(secret));
    }

    if let Some(legacy_path) = legacy_path {
        return read_secret(legacy_path);
    }

    Ok(None)
}

fn write_secret(path: &Path, value: &SecretString) -> Result<()> {
    write_private_atomically(path, format!("{}\n", value.expose_secret()))
        .with_context(|| format!("failed to write token {}", path.display()))
}

fn remove_if_exists(path: &PathBuf) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("failed to remove {}", path.display())),
    }
}

fn expires_soon(expires_at: &str) -> bool {
    let Ok(expires_at) = DateTime::parse_from_rfc3339(expires_at) else {
        return true;
    };
    let refresh_margin = chrono::Duration::from_std(REFRESH_MARGIN).unwrap_or_default();
    Utc::now() >= expires_at.with_timezone(&Utc) - refresh_margin
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_url_points_at_epic_code_flow() {
        let url = EpicClient::new().login_url();

        assert!(url.starts_with("https://www.epicgames.com/id/login?redirectUrl="));
        assert!(url.contains(EGS_CLIENT_ID));
    }

    #[test]
    fn token_store_uses_account_specific_files() {
        let tmp = tempfile::tempdir().unwrap();
        let store = TokenStore::new(tmp.path().to_path_buf(), 42, None);

        store
            .save_eos_refresh(&SecretString::from("refresh-token".to_string()))
            .unwrap();

        assert_eq!(
            store.read_eos_refresh().unwrap().unwrap().expose_secret(),
            "refresh-token"
        );
        assert!(store.read_egs_refresh().unwrap().is_none());
    }

    #[test]
    fn token_store_reads_legacy_profile_files() {
        let tmp = tempfile::tempdir().unwrap();
        let legacy_path = tmp.path().join("profile-7-eos.refresh");
        fs::write(&legacy_path, "legacy-refresh\n").unwrap();
        let store = TokenStore::new(tmp.path().to_path_buf(), 42, Some(7));

        assert_eq!(
            store.read_eos_refresh().unwrap().unwrap().expose_secret(),
            "legacy-refresh"
        );

        store
            .save_eos_refresh(&SecretString::from("new-refresh".to_string()))
            .unwrap();

        assert_eq!(
            fs::read_to_string(tmp.path().join("account-42-eos.refresh")).unwrap(),
            "new-refresh\n"
        );
    }
}
