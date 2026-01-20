use axum::{Router, extract::Query, response::Html, routing::get};
use axum_server::tls_rustls::RustlsConfig;
use rcgen::{CertifiedKey, generate_simple_self_signed};
use serde::Deserialize;
use std::net::SocketAddr;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::oneshot;

use ndl_core::OAUTH_SCOPES;
pub use ndl_core::TokenResponse;

const OAUTH_PORT: u16 = 1337;

#[derive(Debug, Deserialize)]
pub struct CallbackParams {
    pub code: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

pub struct OAuthConfig {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

impl OAuthConfig {
    pub fn new(client_id: String, client_secret: String) -> Self {
        Self {
            client_id,
            client_secret,
            redirect_uri: format!("https://localhost:{}/callback", OAUTH_PORT),
        }
    }

    pub fn authorization_url(&self) -> String {
        format!(
            "https://threads.net/oauth/authorize?client_id={}&redirect_uri={}&scope={}&response_type=code",
            self.client_id,
            urlencoding::encode(&self.redirect_uri),
            OAUTH_SCOPES
        )
    }

    /// Exchange an authorization code for an access token, then upgrade to long-lived
    pub async fn exchange_code(&self, code: &str) -> Result<TokenResponse, OAuthError> {
        // First, exchange code for short-lived token
        let short_lived = ndl_core::exchange_code(
            &self.client_id,
            &self.client_secret,
            &self.redirect_uri,
            code,
        )
        .await
        .map_err(|e| OAuthError::TokenExchange(e.to_string()))?;

        // Then, exchange short-lived token for long-lived token (60 days)
        ndl_core::exchange_for_long_lived_token(&self.client_secret, &short_lived.access_token)
            .await
            .map_err(|e| OAuthError::TokenExchange(e.to_string()))
    }
}

/// Generate a self-signed certificate for localhost
pub fn generate_localhost_cert() -> Result<CertifiedKey, rcgen::Error> {
    let subject_alt_names = vec!["localhost".to_string(), "127.0.0.1".to_string()];
    generate_simple_self_signed(subject_alt_names)
}

/// Start the OAuth callback server and wait for the authorization code
pub async fn wait_for_callback() -> Result<String, OAuthError> {
    let (tx, rx) = oneshot::channel::<Result<String, OAuthError>>();
    let tx = Arc::new(std::sync::Mutex::new(Some(tx)));

    let tx_clone = Arc::clone(&tx);
    let app = Router::new()
        .route(
            "/callback",
            get(move |params: Query<CallbackParams>| {
                let tx = Arc::clone(&tx_clone);
                async move {
                    let result = if let Some(code) = params.code.clone() {
                        Ok(code)
                    } else {
                        Err(OAuthError::AuthorizationDenied(
                            params.error_description.clone().unwrap_or_else(|| {
                                params.error.clone().unwrap_or("Unknown error".to_string())
                            }),
                        ))
                    };

                    if let Some(tx) = tx.lock().unwrap().take() {
                        let _ = tx.send(result);
                    }

                    Html(CALLBACK_HTML)
                }
            }),
        )
        .route("/deauthorize", get(|| async { Html("Deauthorized") }))
        .route("/delete", get(|| async { Html("Deleted") }));

    // Generate self-signed cert
    let cert = generate_localhost_cert().map_err(|e| OAuthError::CertGeneration(e.to_string()))?;

    let config = RustlsConfig::from_pem(
        cert.cert.pem().into_bytes(),
        cert.key_pair.serialize_pem().into_bytes(),
    )
    .await
    .map_err(|e| OAuthError::TlsConfig(e.to_string()))?;

    let addr = SocketAddr::from(([127, 0, 0, 1], OAUTH_PORT));

    // Spawn the server
    let server = axum_server::bind_rustls(addr, config).serve(app.into_make_service());

    tokio::select! {
        result = rx => {
            result.map_err(|_| OAuthError::ChannelClosed)?
        }
        _ = server => {
            Err(OAuthError::ServerShutdown)
        }
    }
}

#[derive(Debug, Error)]
pub enum OAuthError {
    #[error("Failed to generate certificate: {0}")]
    CertGeneration(String),
    #[error("TLS configuration error: {0}")]
    TlsConfig(String),
    #[error("Authorization denied: {0}")]
    AuthorizationDenied(String),
    #[error("Internal channel closed unexpectedly")]
    ChannelClosed,
    #[error("OAuth server shut down unexpectedly")]
    ServerShutdown,
    #[error("Token exchange failed: {0}")]
    TokenExchange(String),
    #[error("Hosted auth error: {0}")]
    HostedAuth(String),
    #[error("Auth session timeout")]
    SessionTimeout,
}

/// Run the complete OAuth login flow
pub async fn login(client_id: &str, client_secret: &str) -> Result<TokenResponse, OAuthError> {
    let config = OAuthConfig::new(client_id.to_string(), client_secret.to_string());
    let auth_url = config.authorization_url();

    println!("Opening browser for authorization...");
    println!("If it doesn't open, visit:\n{}", auth_url);
    println!();
    println!("Note: You may need to accept the self-signed certificate warning.");

    // Open browser (don't fail if it doesn't work - user can visit URL manually)
    if let Err(e) = open::that(&auth_url) {
        eprintln!("Could not open browser automatically: {}", e);
    }

    // Wait for callback
    println!("Waiting for authorization...");
    let code = wait_for_callback().await?;

    // Exchange code for token
    println!("Exchanging code for access token...");
    let token = config.exchange_code(&code).await?;

    println!("Login successful!");
    Ok(token)
}

const CALLBACK_HTML: &str = r#"
<!DOCTYPE html>
<html>
<head>
    <title>ndl - Authorization Complete</title>
    <style>
        body {
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            display: flex;
            justify-content: center;
            align-items: center;
            height: 100vh;
            margin: 0;
            background: #0a0a0a;
            color: #fff;
        }
        .container {
            text-align: center;
            padding: 2rem;
        }
        h1 { color: #00d4aa; }
        p { color: #888; }
    </style>
</head>
<body>
    <div class="container">
        <h1>Authorization Complete</h1>
        <p>You can close this window and return to ndl.</p>
    </div>
</body>
</html>
"#;

// ============================================================================
// Hosted Auth Client (for use with ndld server)
// ============================================================================

use serde::Serialize;

#[derive(Debug, Deserialize)]
pub struct StartAuthResponse {
    pub session_id: String,
    pub auth_url: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PollStatus {
    Pending,
    Completed { access_token: String },
    Failed { error: String },
}

#[derive(Debug, Serialize)]
struct EmptyBody {}

/// Run OAuth login flow using a hosted auth server
pub async fn hosted_login(auth_server: &str) -> Result<TokenResponse, OAuthError> {
    let client = reqwest::Client::new();

    // Step 1: Start auth session
    println!("Connecting to auth server...");
    let start_url = format!("{}/auth/start", auth_server);
    let response = client
        .post(&start_url)
        .json(&EmptyBody {})
        .send()
        .await
        .map_err(|e| OAuthError::HostedAuth(format!("Failed to start auth: {}", e)))?;

    if !response.status().is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(OAuthError::HostedAuth(format!("Server error: {}", body)));
    }

    let start_resp: StartAuthResponse = response
        .json()
        .await
        .map_err(|e| OAuthError::HostedAuth(format!("Invalid response: {}", e)))?;

    // Step 2: Show auth URL to user
    println!("Opening browser for authorization...");
    println!("If it doesn't open, visit:\n{}", start_resp.auth_url);

    // Open browser (don't fail if it doesn't work - user can visit URL manually)
    if let Err(e) = open::that(&start_resp.auth_url) {
        eprintln!("Could not open browser automatically: {}", e);
    }

    // Step 3: Poll for completion
    println!("Waiting for authorization...");
    let poll_url = format!("{}/auth/poll/{}", auth_server, start_resp.session_id);

    // Poll every 2 seconds for up to 5 minutes
    for _ in 0..150 {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let response = client
            .get(&poll_url)
            .send()
            .await
            .map_err(|e| OAuthError::HostedAuth(format!("Poll failed: {}", e)))?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(OAuthError::SessionTimeout);
        }

        if !response.status().is_success() {
            continue; // Retry on server errors
        }

        let poll_resp: PollStatus = response
            .json()
            .await
            .map_err(|e| OAuthError::HostedAuth(format!("Invalid poll response: {}", e)))?;

        match poll_resp {
            PollStatus::Pending => continue,
            PollStatus::Completed { access_token } => {
                println!("Login successful!");
                // Return a TokenResponse for compatibility
                return Ok(TokenResponse {
                    access_token,
                    user_id: None,
                    expires_in: Some(60 * 24 * 60 * 60), // Assume 60 days for long-lived token
                });
            }
            PollStatus::Failed { error } => {
                return Err(OAuthError::AuthorizationDenied(error));
            }
        }
    }

    Err(OAuthError::SessionTimeout)
}
