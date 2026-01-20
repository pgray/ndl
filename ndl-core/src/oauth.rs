use serde::{Deserialize, Deserializer, de};
use thiserror::Error;

pub const TOKEN_URL: &str = "https://graph.threads.net/oauth/access_token";
pub const OAUTH_SCOPES: &str =
    "threads_basic,threads_read_replies,threads_manage_replies,threads_content_publish";

/// Deserialize user_id from either a string or number (Threads API returns both), or None if missing
fn deserialize_user_id_opt<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrNumber {
        String(String),
        Number(u64),
    }

    let opt: Option<StringOrNumber> = Option::deserialize(deserializer)?;
    match opt {
        Some(StringOrNumber::String(s)) => s.parse().map(Some).map_err(de::Error::custom),
        Some(StringOrNumber::Number(n)) => Ok(Some(n)),
        None => Ok(None),
    }
}

#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    #[allow(dead_code)]
    #[serde(default, deserialize_with = "deserialize_user_id_opt")]
    pub user_id: Option<u64>,
    /// Number of seconds until the token expires (3600 for short-lived, 5184000 for long-lived)
    #[serde(default)]
    pub expires_in: Option<u64>,
}

#[derive(Debug, Error)]
pub enum TokenExchangeError {
    #[error("Request failed: {0}")]
    Request(String),
    #[error("HTTP {status}: {body}")]
    Http { status: u16, body: String },
    #[error("Parse error: {0}")]
    Parse(String),
}

/// Exchange an authorization code for an access token
pub async fn exchange_code(
    client_id: &str,
    client_secret: &str,
    redirect_uri: &str,
    code: &str,
) -> Result<TokenResponse, TokenExchangeError> {
    let client = reqwest::Client::new();

    let params = [
        ("client_id", client_id),
        ("client_secret", client_secret),
        ("grant_type", "authorization_code"),
        ("redirect_uri", redirect_uri),
        ("code", code),
    ];

    let response = client
        .post(TOKEN_URL)
        .form(&params)
        .send()
        .await
        .map_err(|e| TokenExchangeError::Request(e.to_string()))?;

    if !response.status().is_success() {
        let status = response.status().as_u16();
        let body = response.text().await.unwrap_or_default();
        return Err(TokenExchangeError::Http { status, body });
    }

    let body = response
        .text()
        .await
        .map_err(|e| TokenExchangeError::Parse(e.to_string()))?;

    serde_json::from_str(&body).map_err(|e| TokenExchangeError::Parse(format!("{}: {}", e, body)))
}

/// Exchange a short-lived access token for a long-lived one (60 days)
pub async fn exchange_for_long_lived_token(
    client_secret: &str,
    short_lived_token: &str,
) -> Result<TokenResponse, TokenExchangeError> {
    let client = reqwest::Client::new();

    let url = format!(
        "https://graph.threads.net/access_token?grant_type=th_exchange_token&client_secret={}&access_token={}",
        client_secret, short_lived_token
    );

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| TokenExchangeError::Request(e.to_string()))?;

    if !response.status().is_success() {
        let status = response.status().as_u16();
        let body = response.text().await.unwrap_or_default();
        return Err(TokenExchangeError::Http { status, body });
    }

    let body = response
        .text()
        .await
        .map_err(|e| TokenExchangeError::Parse(e.to_string()))?;

    serde_json::from_str(&body).map_err(|e| TokenExchangeError::Parse(format!("{}: {}", e, body)))
}

/// Refresh a long-lived access token (extends validity by another 60 days)
pub async fn refresh_access_token(
    long_lived_token: &str,
) -> Result<TokenResponse, TokenExchangeError> {
    let client = reqwest::Client::new();

    let url = format!(
        "https://graph.threads.net/refresh_access_token?grant_type=th_refresh_token&access_token={}",
        long_lived_token
    );

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| TokenExchangeError::Request(e.to_string()))?;

    if !response.status().is_success() {
        let status = response.status().as_u16();
        let body = response.text().await.unwrap_or_default();
        return Err(TokenExchangeError::Http { status, body });
    }

    let body = response
        .text()
        .await
        .map_err(|e| TokenExchangeError::Parse(e.to_string()))?;

    serde_json::from_str(&body).map_err(|e| TokenExchangeError::Parse(format!("{}: {}", e, body)))
}
