use serde::Deserialize;
use thiserror::Error;

pub const TOKEN_URL: &str = "https://graph.threads.net/oauth/access_token";
pub const OAUTH_SCOPES: &str = "threads_basic,threads_read_replies,threads_manage_replies,threads_content_publish";

#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    #[allow(dead_code)]
    pub user_id: u64,
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

    response
        .json::<TokenResponse>()
        .await
        .map_err(|e| TokenExchangeError::Parse(e.to_string()))
}
