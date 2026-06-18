use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use hmac::{Hmac, Mac};
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, USER_AGENT};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::value::RawValue;
use sha2::Sha256;
use tokio::net::TcpStream;
use tokio::sync::{oneshot, Mutex};
use tokio::time::{sleep, timeout};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::HeaderName;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

const BASE_URL: &str = "https://api.rlpp.psynet.gg/rpc";
const GAME_VERSION: &str = "260602.75104.519749";
const FEATURE_SET: &str = "PrimeUpdate59";
const PSY_BUILD_ID: &str = "939334844";
const PSY_SIG_KEY: &str = "c338bd36fb8c42b1a431d30add939fc7";
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(15);

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;
type WsWrite = futures_util::stream::SplitSink<WsStream, Message>;

#[derive(Debug, Clone)]
pub struct PsyNetClient {
    http: reqwest::Client,
    request_ids: Arc<RequestIdCounter>,
}

impl PsyNetClient {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("valid reqwest client"),
            request_ids: Arc::new(RequestIdCounter::default()),
        }
    }

    pub async fn auth_player(&self, account_id: &str, access_token: &str) -> Result<PsyNetRpc> {
        let player_id = PlayerId::new(PlayerPlatform::Epic, account_id);
        let request = AuthPlayerRequest {
            platform: "Epic".to_string(),
            player_name: String::new(),
            player_id: account_id.to_string(),
            language: "INT".to_string(),
            auth_ticket: access_token.to_string(),
            build_region: String::new(),
            feature_set: FEATURE_SET.to_string(),
            device: "PC".to_string(),
            local_first_player_id: player_id.to_string(),
            skip_auth: false,
            set_as_primary_account: true,
            epic_auth_ticket: access_token.to_string(),
            epic_account_id: account_id.to_string(),
        };

        let response: AuthPlayerResponse = self
            .post_json(&["Auth", "AuthPlayer", "v2"], &request)
            .await?;

        self.establish_socket(
            &response.per_con_url_v2,
            player_id,
            response.psy_token,
            response.session_id,
        )
        .await
    }

    async fn establish_socket(
        &self,
        url: &str,
        local_player_id: PlayerId,
        psy_token: String,
        session_id: String,
    ) -> Result<PsyNetRpc> {
        let mut request = url
            .into_client_request()
            .with_context(|| format!("invalid PsyNet websocket URL {url:?}"))?;
        request.headers_mut().insert(
            HeaderName::from_static("psybuildid"),
            HeaderValue::from_static(PSY_BUILD_ID),
        );
        request.headers_mut().insert(
            USER_AGENT,
            HeaderValue::from_str(&format!("RL Win/{GAME_VERSION} gzip"))?,
        );
        request.headers_mut().insert(
            HeaderName::from_static("psyenvironment"),
            HeaderValue::from_static("Prod"),
        );
        request.headers_mut().insert(
            HeaderName::from_static("psytoken"),
            HeaderValue::from_str(&psy_token)?,
        );
        request.headers_mut().insert(
            HeaderName::from_static("psysessionid"),
            HeaderValue::from_str(&session_id)?,
        );

        let (ws, _) = connect_async(request)
            .await
            .context("failed to establish PsyNet websocket")?;
        let (write, mut read) = ws.split();
        let write = Arc::new(Mutex::new(write));
        let pending: PendingRequests = Arc::new(Mutex::new(HashMap::new()));
        let read_pending = Arc::clone(&pending);

        tokio::spawn(async move {
            while let Some(message) = read.next().await {
                match message {
                    Ok(Message::Text(text)) => {
                        if text.starts_with("PsyPong:") {
                            continue;
                        }
                        match parse_message(&text) {
                            Ok(response) if !response.response_id.is_empty() => {
                                let sender =
                                    read_pending.lock().await.remove(&response.response_id);
                                if let Some(sender) = sender {
                                    let _ = sender.send(response);
                                }
                            }
                            Ok(_) => {
                                tracing::debug!("received unmatched PsyNet message");
                            }
                            Err(error) => {
                                tracing::debug!(%error, message = %text, "failed to parse PsyNet message");
                            }
                        }
                    }
                    Ok(Message::Close(_)) => break,
                    Ok(_) => {}
                    Err(error) => {
                        tracing::debug!(%error, "PsyNet websocket read failed");
                        break;
                    }
                }
            }
        });

        Ok(PsyNetRpc {
            write,
            pending,
            request_ids: Arc::clone(&self.request_ids),
            local_player_id,
        })
    }

    async fn post_json<T: for<'de> Deserialize<'de>>(
        &self,
        path: &[&str],
        params: &impl Serialize,
    ) -> Result<T> {
        let url = format!("{BASE_URL}/{}", path.join("/"));
        let body = serde_json::to_vec(params).context("failed to serialize PsyNet request")?;
        let mut headers = HeaderMap::new();
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/x-www-form-urlencoded"),
        );
        headers.insert(
            USER_AGENT,
            HeaderValue::from_str(&format!(
                "RL Win/{GAME_VERSION} gzip (x86_64-pc-win32) curl-7.67.0 Schannel"
            ))?,
        );
        headers.insert(
            HeaderName::from_static("psybuildid"),
            HeaderValue::from_static(PSY_BUILD_ID),
        );
        headers.insert(
            HeaderName::from_static("psyenvironment"),
            HeaderValue::from_static("Prod"),
        );
        headers.insert(
            HeaderName::from_static("psyrequestid"),
            HeaderValue::from_str(&self.request_ids.next())?,
        );
        headers.insert(
            HeaderName::from_static("psysig"),
            HeaderValue::from_str(&generate_psy_sig(&body))?,
        );

        let response = self
            .http
            .post(&url)
            .headers(headers)
            .body(body)
            .send()
            .await
            .with_context(|| format!("failed to send PsyNet request to {url}"))?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            bail!("PsyNet request to {url} failed with {status}: {body}");
        }

        let wrapper: PsyHttpWrapper =
            serde_json::from_str(&body).context("failed to parse PsyNet HTTP response")?;
        if let Some(error) = wrapper.error {
            bail!("PsyNet error {}: {}", error.kind, error.message);
        }
        let result = wrapper
            .result
            .context("PsyNet response did not include Result")?;
        serde_json::from_str(result.get()).context("failed to parse PsyNet Result")
    }
}

impl Default for PsyNetClient {
    fn default() -> Self {
        Self::new()
    }
}

type PendingRequests = Arc<Mutex<HashMap<String, oneshot::Sender<PsyResponse>>>>;

#[derive(Debug, Clone)]
pub struct PsyNetRpc {
    write: Arc<Mutex<WsWrite>>,
    pending: PendingRequests,
    request_ids: Arc<RequestIdCounter>,
    local_player_id: PlayerId,
}

impl PsyNetRpc {
    pub async fn close(&self) -> Result<()> {
        self.write
            .lock()
            .await
            .send(Message::Close(None))
            .await
            .context("failed to close PsyNet websocket")
    }

    pub async fn get_match_history(&self) -> Result<Vec<MatchEntry>> {
        let request = GetMatchHistoryRequest {
            player_id: self.local_player_id.clone(),
        };
        let response: GetMatchHistoryResponse = self
            .send_request("Matches/GetMatchHistory v1", &request)
            .await?;
        Ok(response.matches)
    }

    pub async fn get_profiles(&self, player_ids: Vec<PlayerId>) -> Result<Vec<PlayerData>> {
        let request = GetProfileRequest { player_ids };
        let response: GetProfileResponse =
            self.send_request("Players/GetProfile v1", &request).await?;
        Ok(response.player_data)
    }

    /// Fetches the *current* per-playlist skill snapshot for each player via
    /// `Skills/GetPlayersSkills v1`. Unlike the rank metadata embedded in match
    /// history, this carries `WinStreak`, `MatchesPlayed`, and
    /// `PlacementMatchesPlayed`, but it is a point-in-time value as of the call
    /// (no before/after).
    pub async fn get_players_skills(
        &self,
        player_ids: Vec<PlayerId>,
    ) -> Result<Vec<PlayerWithSkills>> {
        let request = GetPlayersSkillsRequest { player_ids };
        let response: GetPlayersSkillsResponse = self
            .send_request("Skills/GetPlayersSkills v1", &request)
            .await?;
        Ok(response.players)
    }

    async fn send_request<T: for<'de> Deserialize<'de>>(
        &self,
        service: &str,
        data: &impl Serialize,
    ) -> Result<T> {
        let request_id = self.request_ids.next();
        let (sender, receiver) = oneshot::channel();

        let mut headers = HashMap::new();
        headers.insert("PsyService".to_string(), service.to_string());
        headers.insert("PsyRequestID".to_string(), request_id.clone());
        let message = build_message(&headers, Some(data))?;

        self.pending.lock().await.insert(request_id.clone(), sender);
        let send_result = self
            .write
            .lock()
            .await
            .send(Message::Text(message.into()))
            .await;

        if let Err(error) = send_result {
            self.pending.lock().await.remove(&request_id);
            return Err(error).context("failed to send PsyNet websocket request");
        }

        let response = timeout(RESPONSE_TIMEOUT, receiver)
            .await
            .context("timed out waiting for PsyNet response")?
            .context("PsyNet response channel closed")?;
        if let Some(error) = response.error {
            bail!("PsyNet error {}: {}", error.kind, error.message);
        }
        let result = response
            .result
            .context("PsyNet response did not include Result")?;
        serde_json::from_str(result.get()).context("failed to parse PsyNet response Result")
    }
}

impl Drop for PsyNetRpc {
    fn drop(&mut self) {
        let write = Arc::clone(&self.write);
        tokio::spawn(async move {
            let _ = write.lock().await.send(Message::Close(None)).await;
            sleep(Duration::from_millis(10)).await;
        });
    }
}

#[derive(Debug, Default)]
struct RequestIdCounter {
    next_id: AtomicU64,
}

impl RequestIdCounter {
    fn next(&self) -> String {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        format!("PsyNetMessage_X_{id}")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct PlayerId(String);

impl PlayerId {
    pub fn new(platform: PlayerPlatform, id: &str) -> Self {
        Self(format!("{}|{id}|0", platform.as_psynet_platform()))
    }

    /// Wraps a raw PsyNet PlayerID string (`Platform|id|splitscreen`), e.g. the
    /// `PlayerID` carried on each [`MatchPlayer`].
    pub fn from_psynet(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }
}

impl std::fmt::Display for PlayerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlayerPlatform {
    Epic,
    Steam,
    PlayStation,
    Xbox,
    Nintendo,
}

impl PlayerPlatform {
    fn as_psynet_platform(&self) -> &'static str {
        match self {
            Self::Epic => "Epic",
            Self::Steam => "Steam",
            Self::PlayStation => "PS4",
            Self::Xbox => "XboxOne",
            Self::Nintendo => "Switch",
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct MatchEntry {
    #[serde(rename = "ReplayUrl")]
    pub replay_url: String,
    #[serde(rename = "Match")]
    pub match_info: Match,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Match {
    #[serde(rename = "MatchGUID")]
    pub match_guid: String,
    #[serde(rename = "RecordStartTimestamp")]
    pub record_start_timestamp: i64,
    #[serde(rename = "MapName")]
    pub map_name: String,
    #[serde(rename = "Playlist")]
    pub playlist: i64,
    #[serde(rename = "Team0Score")]
    pub team0_score: i64,
    #[serde(rename = "Team1Score")]
    pub team1_score: i64,
    #[serde(rename = "Players", default)]
    pub players: Vec<MatchPlayer>,
}

/// Per-player record carried in each `Matches/GetMatchHistory v1` match,
/// including the rank metadata (`Skills`) that PsyNet reports for the match.
#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct MatchPlayer {
    #[serde(rename = "PlayerID")]
    pub player_id: String,
    #[serde(rename = "PlayerName")]
    pub player_name: String,
    #[serde(rename = "LastTeam")]
    pub last_team: i64,
    #[serde(rename = "TeamColor")]
    pub team_color: String,
    #[serde(rename = "Score")]
    pub score: i64,
    #[serde(rename = "Goals")]
    pub goals: i64,
    #[serde(rename = "Assists")]
    pub assists: i64,
    #[serde(rename = "Saves")]
    pub saves: i64,
    #[serde(rename = "Shots")]
    pub shots: i64,
    #[serde(rename = "Skills", deserialize_with = "deserialize_null_default")]
    pub skills: MatchSkills,
}

/// Rank/skill snapshot for a player in a single match. `Tier`/`Division` are the
/// values *after* the match; `Prev*` are the values from *before* it. `Mu`/`Sigma`
/// are the underlying TrueSkill values (display MMR is `Mu * 20 + 100`).
#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct MatchSkills {
    #[serde(rename = "Mu", deserialize_with = "deserialize_null_default")]
    pub mu: f64,
    #[serde(rename = "Sigma", deserialize_with = "deserialize_null_default")]
    pub sigma: f64,
    #[serde(rename = "Tier", deserialize_with = "deserialize_null_default")]
    pub tier: i64,
    #[serde(rename = "Division", deserialize_with = "deserialize_null_default")]
    pub division: i64,
    #[serde(rename = "PrevMu", deserialize_with = "deserialize_null_default")]
    pub prev_mu: f64,
    #[serde(rename = "PrevSigma", deserialize_with = "deserialize_null_default")]
    pub prev_sigma: f64,
    #[serde(rename = "PrevTier", deserialize_with = "deserialize_null_default")]
    pub prev_tier: i64,
    #[serde(rename = "PrevDivision", deserialize_with = "deserialize_null_default")]
    pub prev_division: i64,
    #[serde(rename = "bValid", deserialize_with = "deserialize_null_default")]
    pub valid: bool,
}

impl MatchSkills {
    /// Display MMR after the match, derived from the TrueSkill mean.
    pub fn mmr(&self) -> f64 {
        Self::mu_to_mmr(self.mu)
    }

    /// Display MMR before the match, derived from the previous TrueSkill mean.
    pub fn prev_mmr(&self) -> f64 {
        Self::mu_to_mmr(self.prev_mu)
    }

    fn mu_to_mmr(mu: f64) -> f64 {
        mu * 20.0 + 100.0
    }
}

/// A player and their current per-playlist skills, as returned by
/// `Skills/GetPlayersSkills v1`.
#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct PlayerWithSkills {
    #[serde(rename = "PlayerID")]
    pub player_id: String,
    #[serde(rename = "Skills")]
    pub skills: Vec<PlayerSkill>,
}

/// Current skill snapshot for a single playlist from `Skills/GetPlayersSkills`.
/// `Mu`/`Sigma`/`Tier`/`Division` mirror the match-history values, but this also
/// reports cumulative counters (`WinStreak`, `MatchesPlayed`,
/// `PlacementMatchesPlayed`) and PsyNet's own `MMR` field. These are
/// point-in-time as of the request, not tied to any particular match.
#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct PlayerSkill {
    #[serde(rename = "Playlist", deserialize_with = "deserialize_null_default")]
    pub playlist: i64,
    #[serde(rename = "Mu", deserialize_with = "deserialize_null_default")]
    pub mu: f64,
    #[serde(rename = "Sigma", deserialize_with = "deserialize_null_default")]
    pub sigma: f64,
    #[serde(rename = "Tier", deserialize_with = "deserialize_null_default")]
    pub tier: i64,
    #[serde(rename = "Division", deserialize_with = "deserialize_null_default")]
    pub division: i64,
    #[serde(rename = "MMR", deserialize_with = "deserialize_null_default")]
    pub mmr: f64,
    #[serde(rename = "WinStreak", deserialize_with = "deserialize_null_default")]
    pub win_streak: i64,
    #[serde(
        rename = "MatchesPlayed",
        deserialize_with = "deserialize_null_default"
    )]
    pub matches_played: i64,
    #[serde(
        rename = "PlacementMatchesPlayed",
        deserialize_with = "deserialize_null_default"
    )]
    pub placement_matches_played: i64,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct PlayerData {
    #[serde(rename = "PlayerID")]
    pub player_id: String,
    #[serde(rename = "PlayerName")]
    pub player_name: String,
    #[serde(rename = "PresenceState")]
    pub presence_state: String,
    #[serde(rename = "PresenceInfo")]
    pub presence_info: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
struct AuthPlayerRequest {
    platform: String,
    player_name: String,
    #[serde(rename = "PlayerID")]
    player_id: String,
    language: String,
    auth_ticket: String,
    build_region: String,
    feature_set: String,
    device: String,
    #[serde(rename = "LocalFirstPlayerID")]
    local_first_player_id: String,
    #[serde(rename = "bSkipAuth")]
    skip_auth: bool,
    #[serde(rename = "bSetAsPrimaryAccount")]
    set_as_primary_account: bool,
    epic_auth_ticket: String,
    #[serde(rename = "EpicAccountID")]
    epic_account_id: String,
}

#[derive(Debug, Deserialize)]
struct AuthPlayerResponse {
    #[serde(rename = "SessionID")]
    session_id: String,
    #[serde(rename = "PerConURLv2")]
    per_con_url_v2: String,
    #[serde(rename = "PsyToken")]
    psy_token: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
struct GetMatchHistoryRequest {
    #[serde(rename = "PlayerID")]
    player_id: PlayerId,
}

#[derive(Debug, Deserialize)]
struct GetMatchHistoryResponse {
    #[serde(rename = "Matches")]
    matches: Vec<MatchEntry>,
}

#[derive(Debug, Serialize)]
struct GetProfileRequest {
    #[serde(rename = "PlayerIDs")]
    player_ids: Vec<PlayerId>,
}

#[derive(Debug, Deserialize)]
struct GetProfileResponse {
    #[serde(rename = "PlayerData")]
    player_data: Vec<PlayerData>,
}

#[derive(Debug, Serialize)]
struct GetPlayersSkillsRequest {
    #[serde(rename = "PlayerIDs")]
    player_ids: Vec<PlayerId>,
}

#[derive(Debug, Deserialize)]
struct GetPlayersSkillsResponse {
    #[serde(rename = "Players", default)]
    players: Vec<PlayerWithSkills>,
}

#[derive(Debug, Deserialize)]
struct PsyError {
    #[serde(rename = "Type")]
    kind: String,
    #[serde(rename = "Message")]
    message: String,
}

#[derive(Debug, Deserialize)]
struct PsyHttpWrapper {
    #[serde(rename = "Result")]
    result: Option<Box<RawValue>>,
    #[serde(rename = "Error")]
    error: Option<PsyError>,
}

#[derive(Debug)]
struct PsyResponse {
    response_id: String,
    result: Option<Box<RawValue>>,
    error: Option<PsyError>,
}

#[derive(Debug, Deserialize)]
struct PsyResponsePayload {
    #[serde(rename = "Result")]
    result: Option<Box<RawValue>>,
    #[serde(rename = "Error")]
    error: Option<PsyError>,
}

fn generate_psy_sig(body: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(PSY_SIG_KEY.as_bytes())
        .expect("HMAC accepts keys of any size");
    mac.update(b"-");
    mac.update(body);
    base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes())
}

fn build_message(
    headers: &HashMap<String, String>,
    body: Option<&impl Serialize>,
) -> Result<String> {
    let body = match body {
        Some(body) => Some(serde_json::to_vec(body).context("failed to serialize PsyNet body")?),
        None => None,
    };
    let mut headers = headers.clone();
    if let Some(body) = body.as_deref() {
        headers.insert("PsySig".to_string(), generate_psy_sig(body));
    }

    let mut message = String::new();
    let mut keys = headers.keys().collect::<Vec<_>>();
    keys.sort();
    for key in keys {
        let value = headers.get(key).expect("key collected from map");
        message.push_str(key);
        message.push_str(": ");
        message.push_str(value);
        message.push_str("\r\n");
    }
    message.push_str("\r\n");
    if let Some(body) = body {
        message.push_str(std::str::from_utf8(&body).context("PsyNet body was not UTF-8 JSON")?);
    }
    Ok(message)
}

fn parse_message(message: &str) -> Result<PsyResponse> {
    let (headers, payload) = message
        .split_once("\r\n\r\n")
        .ok_or_else(|| anyhow!("message does not contain expected delimiter"))?;
    let mut response_id = String::new();
    for line in headers.split("\r\n") {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        if key.trim() == "PsyResponseID" {
            response_id = value.trim().to_string();
        }
    }
    let payload: PsyResponsePayload =
        serde_json::from_str(payload).context("failed to parse PsyNet payload")?;
    Ok(PsyResponse {
        response_id,
        result: payload.result,
        error: payload.error,
    })
}

fn deserialize_null_default<'de, D, T>(deserializer: D) -> std::result::Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_ids_match_rlapi_shape() {
        let ids = RequestIdCounter::default();

        assert_eq!(ids.next(), "PsyNetMessage_X_0");
        assert_eq!(ids.next(), "PsyNetMessage_X_1");
    }

    #[test]
    fn player_id_uses_psynet_platform_names() {
        assert_eq!(
            PlayerId::new(PlayerPlatform::Epic, "abc").to_string(),
            "Epic|abc|0"
        );
        assert_eq!(
            PlayerId::new(PlayerPlatform::Xbox, "abc").to_string(),
            "XboxOne|abc|0"
        );
    }

    #[test]
    fn parse_psynet_message_extracts_response_id_and_result() {
        let message = "PsyTime: 1\r\nPsySig: sig\r\nPsyResponseID: PsyNetMessage_X_1\r\n\r\n{\"Result\":{\"ok\":true}}";

        let response = parse_message(message).unwrap();

        assert_eq!(response.response_id, "PsyNetMessage_X_1");
        assert_eq!(response.result.unwrap().get(), "{\"ok\":true}");
    }

    #[test]
    fn match_history_accepts_null_skill_values() {
        let response: GetMatchHistoryResponse = serde_json::from_str(
            r#"{
                "Matches": [
                    {
                        "ReplayUrl": "https://example.com/replay.replay",
                        "Match": {
                            "MatchGUID": "match-1",
                            "RecordStartTimestamp": 1,
                            "MapName": "Stadium_P",
                            "Playlist": 11,
                            "Team0Score": 1,
                            "Team1Score": 2,
                            "Players": [
                                {
                                    "PlayerID": "Epic|abc|0",
                                    "PlayerName": "player",
                                    "LastTeam": 0,
                                    "TeamColor": "Blue",
                                    "Score": 100,
                                    "Goals": 1,
                                    "Assists": 0,
                                    "Saves": 0,
                                    "Shots": 2,
                                    "Skills": {
                                        "Mu": null,
                                        "Sigma": null,
                                        "Tier": null,
                                        "Division": null,
                                        "PrevMu": null,
                                        "PrevSigma": null,
                                        "PrevTier": null,
                                        "PrevDivision": null,
                                        "bValid": null
                                    }
                                }
                            ]
                        }
                    }
                ]
            }"#,
        )
        .unwrap();

        let skills = &response.matches[0].match_info.players[0].skills;
        assert_eq!(skills.mu, 0.0);
        assert_eq!(skills.prev_sigma, 0.0);
        assert_eq!(skills.tier, 0);
        assert!(!skills.valid);
    }

    #[test]
    fn build_message_adds_signature_and_json_body() {
        let mut headers = HashMap::new();
        headers.insert("PsyRequestID".to_string(), "PsyNetMessage_X_1".to_string());

        let message =
            build_message(&headers, Some(&serde_json::json!({"PlayerID": "Epic|a|0"}))).unwrap();

        assert!(message.contains("PsyRequestID: PsyNetMessage_X_1\r\n"));
        assert!(message.contains("PsySig: "));
        assert!(message.ends_with("{\"PlayerID\":\"Epic|a|0\"}"));
    }
}
