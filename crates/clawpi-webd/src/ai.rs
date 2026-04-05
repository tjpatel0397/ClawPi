use base64::Engine;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use clawpi_core::Layout;
use rand::{rngs::OsRng, RngCore};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
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
const OPENAI_OAUTH_AUTHORIZE_URL: &str = "https://auth.openai.com/oauth/authorize";
const OPENAI_OAUTH_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const OPENAI_OAUTH_REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
const OPENAI_SCOPE: &str = "openid profile email offline_access";
const AUTH_PROFILE_SCHEMA_VERSION: u32 = 1;
const VALIDATION_TIMEOUT_SECS: u64 = 25;
const VALIDATION_PROMPT: &str = "Reply with OK only.";
const VALIDATION_SYSTEM_PROMPT: &str =
    "You are validating an AI provider setup for ClawPi. Reply with OK only.";
const OPENAI_LOGIN_TTL_MINUTES: i64 = 15;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PendingOpenAiLogin {
    pub provider: String,
    pub model: String,
    pub profile_name: String,
    pub authorize_url: String,
    pub code_verifier: String,
    pub state: String,
    pub expires_at: String,
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
pub struct OpenAiLoginCompletion {
    pub provider: String,
    pub model: String,
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

#[derive(Debug, Clone)]
struct PkceState {
    code_verifier: String,
    code_challenge: String,
    state: String,
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

fn oauth_error(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn zeroclaw_state_dir(layout: &Layout) -> PathBuf {
    layout.state_dir().join("zeroclaw")
}

fn auth_profile_path(layout: &Layout) -> PathBuf {
    zeroclaw_state_dir(layout).join("auth-profiles.json")
}

fn pending_openai_login_path(layout: &Layout) -> PathBuf {
    zeroclaw_state_dir(layout).join("clawpi-openai-oauth-login.json")
}

fn legacy_pending_openai_login_path(layout: &Layout) -> PathBuf {
    zeroclaw_state_dir(layout).join("clawpi-openai-device-login.json")
}

fn openai_profile_id() -> String {
    format!("{OPENAI_CODEX_PROVIDER}:{OPENAI_DEFAULT_PROFILE}")
}

fn http_client() -> io::Result<Client> {
    Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(provider_error)
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
    for path in [
        pending_openai_login_path(layout),
        legacy_pending_openai_login_path(layout),
    ] {
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => return Err(err),
        }
    }
    Ok(())
}

pub fn start_openai_codex_oauth_login(
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

    let pkce = generate_pkce_state();
    let pending = PendingOpenAiLogin {
        provider: String::from(OPENAI_CODEX_PROVIDER),
        model: model.to_string(),
        profile_name: String::from(OPENAI_DEFAULT_PROFILE),
        authorize_url: build_authorize_url(&pkce)?,
        code_verifier: pkce.code_verifier,
        state: pkce.state,
        expires_at: (Utc::now() + ChronoDuration::minutes(OPENAI_LOGIN_TTL_MINUTES)).to_rfc3339(),
    };

    let path = pending_openai_login_path(layout);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    clear_pending_openai_codex_login(layout)?;
    fs::write(
        path,
        serde_json::to_vec_pretty(&pending).map_err(provider_error)?,
    )?;

    Ok(pending)
}

pub fn complete_openai_codex_oauth_login(
    layout: &Layout,
    redirect_input: &str,
) -> io::Result<OpenAiLoginCompletion> {
    let pending = load_pending_openai_codex_login(layout)?
        .ok_or_else(|| oauth_error("No OpenAI login is in progress. Start the login again."))?;

    if pending.is_expired() {
        clear_pending_openai_codex_login(layout)?;
        return Err(oauth_error("OpenAI login expired. Start the login again."));
    }

    let code = parse_code_from_redirect(redirect_input, Some(&pending.state))?;
    let token_set = exchange_openai_code_for_tokens(&code, &pending.code_verifier)?;
    store_openai_auth_tokens(layout, &token_set)?;
    clear_pending_openai_codex_login(layout)?;

    match validate_provider(layout, OPENAI_CODEX_PROVIDER, &pending.model, None) {
        Ok(()) => Ok(OpenAiLoginCompletion {
            provider: pending.provider,
            model: pending.model,
        }),
        Err(err) => Err(oauth_error(format!(
            "OpenAI login completed, but provider validation failed: {err}"
        ))),
    }
}

fn generate_pkce_state() -> PkceState {
    let code_verifier = random_base64url(64);
    let digest = Sha256::digest(code_verifier.as_bytes());
    let code_challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);

    PkceState {
        code_verifier,
        code_challenge,
        state: random_base64url(24),
    }
}

fn random_base64url(byte_len: usize) -> String {
    let mut bytes = vec![0_u8; byte_len];
    OsRng.fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn build_authorize_url(pkce: &PkceState) -> io::Result<String> {
    let mut url = Url::parse(OPENAI_OAUTH_AUTHORIZE_URL).map_err(provider_error)?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", OPENAI_OAUTH_CLIENT_ID)
        .append_pair("redirect_uri", OPENAI_OAUTH_REDIRECT_URI)
        .append_pair("scope", OPENAI_SCOPE)
        .append_pair("code_challenge", &pkce.code_challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", &pkce.state)
        .append_pair("codex_cli_simplified_flow", "true")
        .append_pair("id_token_add_organizations", "true");
    Ok(url.into())
}

fn exchange_openai_code_for_tokens(code: &str, code_verifier: &str) -> io::Result<TokenSet> {
    let code = code.trim().to_string();
    let code_verifier = code_verifier.trim().to_string();

    runtime()?.block_on(async move {
        let response = http_client()?
            .post(OPENAI_OAUTH_TOKEN_URL)
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", code.as_str()),
                ("client_id", OPENAI_OAUTH_CLIENT_ID),
                ("redirect_uri", OPENAI_OAUTH_REDIRECT_URI),
                ("code_verifier", code_verifier.as_str()),
            ])
            .send()
            .await
            .map_err(provider_error)?;

        parse_token_response(response).await
    })
}

async fn parse_token_response(response: reqwest::Response) -> io::Result<TokenSet> {
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if let Ok(err) = serde_json::from_str::<OpenAiOauthErrorResponse>(&body) {
            return Err(oauth_error(format!(
                "OpenAI OAuth token request failed ({status}): {}",
                err.error_description.unwrap_or(err.error)
            )));
        }
        return Err(oauth_error(format!(
            "OpenAI OAuth token request failed ({status}): {body}"
        )));
    }

    let token: OpenAiTokenResponse = response.json().await.map_err(provider_error)?;
    let expires_at = token
        .expires_in
        .filter(|seconds| *seconds > 0)
        .map(|seconds| Utc::now() + ChronoDuration::seconds(seconds))
        .or_else(|| extract_expiry_from_jwt(&token.access_token));

    Ok(TokenSet {
        access_token: token.access_token,
        refresh_token: token.refresh_token,
        id_token: token.id_token,
        expires_at,
        token_type: token.token_type,
        scope: token.scope,
    })
}

fn parse_code_from_redirect(input: &str, expected_state: Option<&str>) -> io::Result<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(oauth_error(
            "Paste the final OpenAI redirect URL or OAuth code.",
        ));
    }

    let query = if let Some((_, right)) = trimmed.split_once('?') {
        right
    } else {
        trimmed
    };
    let params = parse_query_params(query);
    let is_callback_payload = trimmed.contains('?')
        || params.contains_key("code")
        || params.contains_key("state")
        || params.contains_key("error");

    if let Some(err) = params.get("error") {
        let description = params
            .get("error_description")
            .cloned()
            .unwrap_or_else(|| String::from("OAuth authorization failed"));
        return Err(oauth_error(format!(
            "OpenAI OAuth error: {err} ({description})"
        )));
    }

    if let Some(expected_state) = expected_state {
        if let Some(got) = params.get("state") {
            if got != expected_state {
                return Err(oauth_error("OAuth state mismatch."));
            }
        } else if is_callback_payload {
            return Err(oauth_error("Missing OAuth state in callback."));
        }
    }

    if let Some(code) = params.get("code") {
        return Ok(code.clone());
    }

    if !is_callback_payload {
        return Ok(trimmed.to_string());
    }

    Err(oauth_error("Missing OAuth code in callback."))
}

fn parse_query_params(input: &str) -> BTreeMap<String, String> {
    input
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            (url_decode(key), url_decode(value))
        })
        .collect()
}

fn url_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let hex = &input[index + 1..index + 3];
                if let Ok(byte) = u8::from_str_radix(hex, 16) {
                    out.push(byte);
                    index += 3;
                    continue;
                }
                out.push(bytes[index]);
                index += 1;
            }
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }

    String::from_utf8_lossy(&out).to_string()
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
            model: String::from("gpt-5.4"),
            profile_name: String::from(OPENAI_DEFAULT_PROFILE),
            authorize_url: String::from("https://auth.openai.com/oauth/authorize?code=abc"),
            code_verifier: String::from("verifier"),
            state: String::from("state-123"),
            expires_at: (Utc::now() + ChronoDuration::minutes(10)).to_rfc3339(),
        };

        let path = pending_openai_login_path(&layout);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, serde_json::to_vec_pretty(&pending).unwrap()).unwrap();

        let loaded = load_pending_openai_codex_login(&layout).unwrap().unwrap();
        assert_eq!(loaded.state, "state-123");
        assert_eq!(loaded.model, "gpt-5.4");
    }

    #[test]
    fn parse_redirect_url_extracts_code() {
        let code = parse_code_from_redirect(
            "http://127.0.0.1:1455/auth/callback?code=abc123&state=xyz",
            Some("xyz"),
        )
        .unwrap();
        assert_eq!(code, "abc123");
    }

    #[test]
    fn parse_redirect_accepts_raw_code() {
        let code = parse_code_from_redirect("raw-code", None).unwrap();
        assert_eq!(code, "raw-code");
    }

    #[test]
    fn parse_redirect_rejects_state_mismatch() {
        let err = parse_code_from_redirect("/auth/callback?code=x&state=a", Some("b")).unwrap_err();
        assert!(err.to_string().contains("state mismatch"));
    }

    #[test]
    fn parse_redirect_rejects_error_without_code() {
        let err = parse_code_from_redirect(
            "/auth/callback?error=access_denied&error_description=user+cancelled",
            Some("xyz"),
        )
        .unwrap_err();
        assert!(err
            .to_string()
            .contains("OpenAI OAuth error: access_denied"));
    }
}
