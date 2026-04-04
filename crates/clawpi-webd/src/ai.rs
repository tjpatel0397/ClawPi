use base64::Engine;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use clawpi_core::Layout;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::Duration;
use tokio::runtime::{Builder, Runtime};
use zeroclaw::providers::{create_provider_with_options, ProviderRuntimeOptions};

pub const OPENAI_CODEX_PROVIDER: &str = "openai-codex";
const OPENAI_DEFAULT_PROFILE: &str = "default";
const OPENAI_OAUTH_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const OPENAI_OAUTH_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const OPENAI_OAUTH_DEVICE_CODE_URL: &str = "https://auth.openai.com/oauth/device/code";
const AUTH_PROFILE_SCHEMA_VERSION: u32 = 1;
const VALIDATION_TIMEOUT_SECS: u64 = 25;
const VALIDATION_PROMPT: &str = "Reply with OK only.";
const VALIDATION_SYSTEM_PROMPT: &str =
    "You are validating an AI provider setup for ClawPi. Reply with OK only.";
const OPENAI_SCOPE: &str = "openid profile email offline_access";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PendingOpenAiLogin {
    pub provider: String,
    pub model: String,
    pub profile_name: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: Option<String>,
    pub device_code: String,
    pub interval_secs: u64,
    pub expires_at: String,
    pub message: Option<String>,
}

impl PendingOpenAiLogin {
    pub fn expires_at_datetime(&self) -> Option<DateTime<Utc>> {
        DateTime::parse_from_rfc3339(&self.expires_at)
            .ok()
            .map(|value| value.with_timezone(&Utc))
    }

    pub fn is_expired(&self) -> bool {
        self.expires_at_datetime()
            .is_some_and(|value| value <= Utc::now())
    }
}

#[derive(Debug)]
pub enum OpenAiLoginPollResult {
    Missing,
    Pending(PendingOpenAiLogin),
    Complete { provider: String, model: String },
    Error(String),
}

#[derive(Debug, Deserialize)]
struct OpenAiDeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    #[serde(default)]
    verification_uri_complete: Option<String>,
    expires_in: u64,
    #[serde(default)]
    interval: Option<u64>,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiTokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default)]
    scope: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiOauthErrorResponse {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

#[derive(Debug, Clone)]
struct TokenSet {
    access_token: String,
    refresh_token: Option<String>,
    id_token: Option<String>,
    expires_at: Option<DateTime<Utc>>,
    token_type: Option<String>,
    scope: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PersistedAuthProfiles {
    #[serde(default = "default_schema_version")]
    schema_version: u32,
    #[serde(default = "default_now_rfc3339")]
    updated_at: String,
    #[serde(default)]
    active_profiles: BTreeMap<String, String>,
    #[serde(default)]
    profiles: BTreeMap<String, PersistedAuthProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PersistedAuthProfile {
    provider: String,
    profile_name: String,
    kind: String,
    #[serde(default)]
    account_id: Option<String>,
    #[serde(default)]
    workspace_id: Option<String>,
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    expires_at: Option<String>,
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default = "default_now_rfc3339")]
    created_at: String,
    #[serde(default = "default_now_rfc3339")]
    updated_at: String,
    #[serde(default)]
    metadata: BTreeMap<String, String>,
}

fn default_schema_version() -> u32 {
    AUTH_PROFILE_SCHEMA_VERSION
}

fn default_now_rfc3339() -> String {
    Utc::now().to_rfc3339()
}

fn runtime() -> io::Result<Runtime> {
    Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| io::Error::other(err.to_string()))
}

fn provider_error(err: impl ToString) -> io::Error {
    io::Error::other(err.to_string())
}

fn zeroclaw_state_dir(layout: &Layout) -> PathBuf {
    layout.state_dir().join("zeroclaw")
}

fn auth_profile_path(layout: &Layout) -> PathBuf {
    zeroclaw_state_dir(layout).join("auth-profiles.json")
}

fn pending_openai_login_path(layout: &Layout) -> PathBuf {
    zeroclaw_state_dir(layout).join("clawpi-openai-device-login.json")
}

fn openai_profile_id() -> String {
    format!("{OPENAI_CODEX_PROVIDER}:{OPENAI_DEFAULT_PROFILE}")
}

fn validate_provider_async(
    layout: &Layout,
    provider: &str,
    model: &str,
    api_key: Option<&str>,
) -> io::Result<()> {
    let options = ProviderRuntimeOptions {
        zeroclaw_dir: Some(zeroclaw_state_dir(layout)),
        reasoning_effort: Some(String::from("low")),
        provider_timeout_secs: Some(VALIDATION_TIMEOUT_SECS),
        ..ProviderRuntimeOptions::default()
    };

    let provider_client =
        create_provider_with_options(provider, api_key, &options).map_err(provider_error)?;

    runtime()?.block_on(async move {
        provider_client
            .chat_with_system(
                Some(VALIDATION_SYSTEM_PROMPT),
                VALIDATION_PROMPT,
                model,
                0.0,
            )
            .await
            .map(|_| ())
            .map_err(provider_error)
    })
}

pub fn validate_provider(
    layout: &Layout,
    provider: &str,
    model: &str,
    api_key: Option<&str>,
) -> io::Result<()> {
    let provider = provider.trim();
    let model = model.trim();
    if provider.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Choose a provider to continue.",
        ));
    }
    if model.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Choose a model to continue.",
        ));
    }

    validate_provider_async(layout, provider, model, api_key)
}

pub fn openai_codex_auth_profile_exists(layout: &Layout) -> io::Result<bool> {
    let path = auth_profile_path(layout);
    if !path.exists() {
        return Ok(false);
    }

    let content = fs::read_to_string(path)?;
    if content.trim().is_empty() {
        return Ok(false);
    }

    let profiles: PersistedAuthProfiles =
        serde_json::from_str(&content).map_err(|err| provider_error(err.to_string()))?;
    let profile_id = openai_profile_id();
    Ok(profiles
        .profiles
        .get(&profile_id)
        .is_some_and(|profile| profile.kind == "oauth" && profile.access_token.is_some()))
}

pub fn load_pending_openai_codex_login(layout: &Layout) -> io::Result<Option<PendingOpenAiLogin>> {
    let path = pending_openai_login_path(layout);
    match fs::read_to_string(path) {
        Ok(content) => {
            let pending = serde_json::from_str(&content).map_err(|err| {
                provider_error(format!("invalid pending OpenAI login state: {err}"))
            })?;
            Ok(Some(pending))
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

pub fn clear_pending_openai_codex_login(layout: &Layout) -> io::Result<()> {
    match fs::remove_file(pending_openai_login_path(layout)) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

pub fn start_openai_codex_device_login(
    layout: &Layout,
    model: &str,
) -> io::Result<PendingOpenAiLogin> {
    let model = model.trim();
    if model.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Choose a model before starting OpenAI login.",
        ));
    }

    let pending = runtime()?.block_on(async move {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(provider_error)?;

        let response = client
            .post(OPENAI_OAUTH_DEVICE_CODE_URL)
            .form(&[
                ("client_id", OPENAI_OAUTH_CLIENT_ID),
                ("scope", OPENAI_SCOPE),
            ])
            .send()
            .await
            .map_err(provider_error)?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(provider_error(format!(
                "failed to start OpenAI login ({status}): {body}"
            )));
        }

        let device: OpenAiDeviceCodeResponse = response.json().await.map_err(provider_error)?;

        Ok(PendingOpenAiLogin {
            provider: String::from(OPENAI_CODEX_PROVIDER),
            model: model.to_string(),
            profile_name: String::from(OPENAI_DEFAULT_PROFILE),
            user_code: device.user_code,
            verification_uri: device.verification_uri,
            verification_uri_complete: device.verification_uri_complete,
            device_code: device.device_code,
            interval_secs: device.interval.unwrap_or(5).max(1),
            expires_at: (Utc::now() + ChronoDuration::seconds(device.expires_in as i64))
                .to_rfc3339(),
            message: device.message,
        })
    })?;

    let path = pending_openai_login_path(layout);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        path,
        serde_json::to_vec_pretty(&pending).map_err(provider_error)?,
    )?;

    Ok(pending)
}

pub fn poll_openai_codex_device_login(layout: &Layout) -> io::Result<OpenAiLoginPollResult> {
    let mut pending = match load_pending_openai_codex_login(layout)? {
        Some(pending) => pending,
        None => return Ok(OpenAiLoginPollResult::Missing),
    };

    if pending.is_expired() {
        clear_pending_openai_codex_login(layout)?;
        return Ok(OpenAiLoginPollResult::Error(String::from(
            "OpenAI login expired. Start the login again.",
        )));
    }

    match poll_openai_device_token_once(&pending)? {
        PollOpenAiTokenResult::Pending => Ok(OpenAiLoginPollResult::Pending(pending)),
        PollOpenAiTokenResult::SlowDown(interval_secs) => {
            pending.interval_secs = interval_secs.max(pending.interval_secs + 1);
            fs::write(
                pending_openai_login_path(layout),
                serde_json::to_vec_pretty(&pending).map_err(provider_error)?,
            )?;
            Ok(OpenAiLoginPollResult::Pending(pending))
        }
        PollOpenAiTokenResult::Complete(token_set) => {
            store_openai_auth_tokens(layout, &token_set)?;
            clear_pending_openai_codex_login(layout)?;
            match validate_provider(layout, OPENAI_CODEX_PROVIDER, &pending.model, None) {
                Ok(()) => Ok(OpenAiLoginPollResult::Complete {
                    provider: pending.provider,
                    model: pending.model,
                }),
                Err(err) => Ok(OpenAiLoginPollResult::Error(format!(
                    "OpenAI login completed, but provider validation failed: {err}"
                ))),
            }
        }
        PollOpenAiTokenResult::Denied => {
            clear_pending_openai_codex_login(layout)?;
            Ok(OpenAiLoginPollResult::Error(String::from(
                "OpenAI login was denied.",
            )))
        }
        PollOpenAiTokenResult::Expired => {
            clear_pending_openai_codex_login(layout)?;
            Ok(OpenAiLoginPollResult::Error(String::from(
                "OpenAI login expired. Start the login again.",
            )))
        }
        PollOpenAiTokenResult::Error(message) => {
            clear_pending_openai_codex_login(layout)?;
            Ok(OpenAiLoginPollResult::Error(message))
        }
    }
}

enum PollOpenAiTokenResult {
    Pending,
    SlowDown(u64),
    Complete(TokenSet),
    Denied,
    Expired,
    Error(String),
}

fn poll_openai_device_token_once(
    pending: &PendingOpenAiLogin,
) -> io::Result<PollOpenAiTokenResult> {
    let device_code = pending.device_code.clone();
    runtime()?.block_on(async move {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(provider_error)?;

        let response = client
            .post(OPENAI_OAUTH_TOKEN_URL)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("device_code", device_code.as_str()),
                ("client_id", OPENAI_OAUTH_CLIENT_ID),
            ])
            .send()
            .await
            .map_err(provider_error)?;

        if response.status().is_success() {
            let token: OpenAiTokenResponse = response.json().await.map_err(provider_error)?;
            let expires_at = token
                .expires_in
                .filter(|seconds| *seconds > 0)
                .map(|seconds| Utc::now() + ChronoDuration::seconds(seconds))
                .or_else(|| extract_expiry_from_jwt(&token.access_token));
            return Ok(PollOpenAiTokenResult::Complete(TokenSet {
                access_token: token.access_token,
                refresh_token: token.refresh_token,
                id_token: token.id_token,
                expires_at,
                token_type: token.token_type,
                scope: token.scope,
            }));
        }

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if let Ok(err) = serde_json::from_str::<OpenAiOauthErrorResponse>(&body) {
            return Ok(match err.error.as_str() {
                "authorization_pending" => PollOpenAiTokenResult::Pending,
                "slow_down" => PollOpenAiTokenResult::SlowDown(pending.interval_secs + 5),
                "access_denied" => PollOpenAiTokenResult::Denied,
                "expired_token" => PollOpenAiTokenResult::Expired,
                _ => PollOpenAiTokenResult::Error(format!(
                    "OpenAI login failed ({status}): {}",
                    err.error_description.unwrap_or(err.error)
                )),
            });
        }

        Ok(PollOpenAiTokenResult::Error(format!(
            "OpenAI login failed ({status}): {body}"
        )))
    })
}

fn store_openai_auth_tokens(layout: &Layout, token_set: &TokenSet) -> io::Result<()> {
    let path = auth_profile_path(layout);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut persisted = if path.exists() {
        let content = fs::read_to_string(&path)?;
        if content.trim().is_empty() {
            PersistedAuthProfiles::default()
        } else {
            serde_json::from_str(&content).map_err(provider_error)?
        }
    } else {
        PersistedAuthProfiles::default()
    };

    let now = Utc::now().to_rfc3339();
    let profile_id = openai_profile_id();
    let created_at = persisted
        .profiles
        .get(&profile_id)
        .map(|profile| profile.created_at.clone())
        .unwrap_or_else(|| now.clone());

    persisted.schema_version = AUTH_PROFILE_SCHEMA_VERSION;
    persisted.updated_at = now.clone();
    persisted
        .active_profiles
        .insert(String::from(OPENAI_CODEX_PROVIDER), profile_id.clone());
    persisted.profiles.insert(
        profile_id,
        PersistedAuthProfile {
            provider: String::from(OPENAI_CODEX_PROVIDER),
            profile_name: String::from(OPENAI_DEFAULT_PROFILE),
            kind: String::from("oauth"),
            account_id: extract_account_id_from_jwt(&token_set.access_token),
            workspace_id: None,
            access_token: Some(token_set.access_token.clone()),
            refresh_token: token_set.refresh_token.clone(),
            id_token: token_set.id_token.clone(),
            token: None,
            expires_at: token_set.expires_at.map(|value| value.to_rfc3339()),
            token_type: token_set.token_type.clone(),
            scope: token_set.scope.clone(),
            created_at,
            updated_at: now,
            metadata: BTreeMap::new(),
        },
    );

    fs::write(
        path,
        serde_json::to_vec_pretty(&persisted).map_err(provider_error)?,
    )
}

fn extract_account_id_from_jwt(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&decoded).ok()?;

    for key in [
        "account_id",
        "accountId",
        "acct",
        "sub",
        "https://api.openai.com/account_id",
    ] {
        if let Some(value) = claims.get(key).and_then(|raw| raw.as_str()) {
            if !value.trim().is_empty() {
                return Some(value.to_string());
            }
        }
    }

    None
}

fn extract_expiry_from_jwt(token: &str) -> Option<DateTime<Utc>> {
    let payload = token.split('.').nth(1)?;
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
    let exp = claims.get("exp").and_then(|raw| raw.as_i64())?;
    DateTime::<Utc>::from_timestamp(exp, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clawpi_core::Layout;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_layout(label: &str) -> Layout {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        Layout::from_root(std::env::temp_dir().join(format!("clawpi-webd-ai-{label}-{unique}")))
    }

    #[test]
    fn openai_auth_profile_upsert_creates_expected_active_profile() {
        let layout = test_layout("auth-profile");
        let token_set = TokenSet {
            access_token: String::from("header.eyJhY2NvdW50X2lkIjoiYWNjdC0xIn0.sig"),
            refresh_token: Some(String::from("refresh-token")),
            id_token: None,
            expires_at: Some(Utc::now() + ChronoDuration::minutes(30)),
            token_type: Some(String::from("Bearer")),
            scope: Some(String::from(OPENAI_SCOPE)),
        };

        store_openai_auth_tokens(&layout, &token_set).unwrap();

        let raw = fs::read_to_string(auth_profile_path(&layout)).unwrap();
        let persisted: PersistedAuthProfiles = serde_json::from_str(&raw).unwrap();
        let profile_id = openai_profile_id();

        assert_eq!(
            persisted.active_profiles.get(OPENAI_CODEX_PROVIDER),
            Some(&profile_id)
        );
        assert_eq!(
            persisted
                .profiles
                .get(&profile_id)
                .and_then(|profile| profile.account_id.as_deref()),
            Some("acct-1")
        );
    }

    #[test]
    fn pending_openai_login_round_trips() {
        let layout = test_layout("pending");
        let pending = PendingOpenAiLogin {
            provider: String::from(OPENAI_CODEX_PROVIDER),
            model: String::from("gpt-5-codex"),
            profile_name: String::from(OPENAI_DEFAULT_PROFILE),
            user_code: String::from("ABCD-EFGH"),
            verification_uri: String::from("https://auth.openai.com/activate"),
            verification_uri_complete: Some(String::from(
                "https://auth.openai.com/activate?user_code=ABCD-EFGH",
            )),
            device_code: String::from("device-code"),
            interval_secs: 5,
            expires_at: (Utc::now() + ChronoDuration::minutes(10)).to_rfc3339(),
            message: Some(String::from("Finish sign-in in your browser.")),
        };

        let path = pending_openai_login_path(&layout);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, serde_json::to_vec_pretty(&pending).unwrap()).unwrap();

        let loaded = load_pending_openai_codex_login(&layout).unwrap().unwrap();
        assert_eq!(loaded.user_code, "ABCD-EFGH");
        assert_eq!(loaded.model, "gpt-5-codex");
    }
}
