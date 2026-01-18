use axum::{
    Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Json},
    routing::{get, post},
};
use maud::{DOCTYPE, Markup, html};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::auth::{AuthState, OAuthConfig, SessionStore};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const GIT_VERSION: &str = env!("NDLD_GIT_VERSION");

#[derive(Clone)]
pub struct AppState {
    pub sessions: SessionStore,
    pub oauth: OAuthConfig,
}

// Request/Response types

#[derive(Serialize)]
pub struct StartAuthResponse {
    pub session_id: String,
    pub auth_url: String,
}

#[derive(Deserialize)]
pub struct CallbackParams {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

#[derive(Serialize)]
pub struct PollResponse {
    #[serde(flatten)]
    pub state: AuthState,
}

#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

// Route handlers

/// POST /auth/start - Create a new auth session
pub async fn start_auth(State(state): State<Arc<AppState>>) -> Json<StartAuthResponse> {
    let session = state.sessions.create_session();
    let auth_url = state.oauth.authorization_url(&session.id);

    tracing::info!(session_id = %session.id, "Created new auth session");

    Json(StartAuthResponse {
        session_id: session.id.clone(),
        auth_url,
    })
}

/// GET /auth/callback - OAuth callback from Threads
pub async fn auth_callback(
    State(state): State<Arc<AppState>>,
    Query(params): Query<CallbackParams>,
) -> impl IntoResponse {
    // The state parameter contains our session_id
    let session_id = match params.state {
        Some(id) => id,
        None => {
            return Html(error_html("Missing state parameter")).into_response();
        }
    };

    let session = match state.sessions.get_session(&session_id) {
        Some(s) => s,
        None => {
            return Html(error_html("Session not found or expired")).into_response();
        }
    };

    // Check for OAuth error
    if let Some(error) = params.error {
        let error_msg = params.error_description.unwrap_or(error);
        *session.state.write().await = AuthState::Failed {
            error: error_msg.clone(),
        };
        tracing::warn!(session_id = %session_id, error = %error_msg, "OAuth error");
        return Html(error_html(&error_msg)).into_response();
    }

    // Exchange code for token
    let code = match params.code {
        Some(c) => c,
        None => {
            let error = "Missing authorization code";
            *session.state.write().await = AuthState::Failed {
                error: error.to_string(),
            };
            return Html(error_html(error)).into_response();
        }
    };

    tracing::info!(session_id = %session_id, "Exchanging code for token");

    match state.oauth.exchange_code(&code).await {
        Ok(token) => {
            *session.state.write().await = AuthState::Completed {
                access_token: token.access_token,
            };
            tracing::info!(session_id = %session_id, "Token exchange successful");
            Html(success_html()).into_response()
        }
        Err(e) => {
            *session.state.write().await = AuthState::Failed { error: e.clone() };
            tracing::error!(session_id = %session_id, error = %e, "Token exchange failed");
            Html(error_html(&e)).into_response()
        }
    }
}

/// GET /auth/poll/:session_id - Poll for auth status
pub async fn poll_auth(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> Result<Json<PollResponse>, (StatusCode, Json<ErrorResponse>)> {
    let session = state.sessions.get_session(&session_id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Session not found or expired".to_string(),
            }),
        )
    })?;

    let auth_state = session.state.read().await.clone();

    // Clean up completed/failed sessions after polling
    if matches!(
        auth_state,
        AuthState::Completed { .. } | AuthState::Failed { .. }
    ) {
        state.sessions.remove_session(&session_id);
    }

    Ok(Json(PollResponse { state: auth_state }))
}

/// GET /health - Health check with version info
pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        version: VERSION,
        git: GIT_VERSION,
    })
}

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub version: &'static str,
    pub git: &'static str,
}

/// GET / - Landing page
pub async fn index() -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "ndld - OAuth server for ndl" }
                style {
                    (LANDING_CSS)
                }
            }
            body {
                div.container {
                    h1 { "ndld" }
                    p.tagline { "OAuth authentication server for ndl (needle)" }

                    div.links {
                        a.button href="https://github.com/pgray/ndl" target="_blank" {
                            "GitHub"
                        }
                        a.button href="https://threads.net" target="_blank" {
                            "Threads"
                        }
                    }

                    div.about {
                        h2 { "What is this?" }
                        p {
                            "This server handles OAuth authentication for "
                            a href="https://github.com/pgray/ndl" { "ndl" }
                            ", a minimal TUI client for Threads."
                        }
                        p {
                            "It keeps your Threads API credentials secure by handling "
                            "the OAuth flow server-side."
                        }
                    }

                    div.about {
                        h2 { "Privacy" }
                        p {
                            "We don't track, collect, or store any personal information. "
                            "Access tokens are returned directly to your client and never stored server-side. "
                            a href="/privacy-policy" { "Read our privacy policy" }
                            "."
                        }
                        p {
                            a href="/tos" { "Terms of Service" }
                        }
                    }

                    div.deps {
                        h2 { "Built with ❤️ using" }
                        ul {
                            li { "❤️ " a href="https://github.com/tokio-rs/axum" { "axum" } " - web framework" }
                            li { "❤️ " a href="https://github.com/tokio-rs/tokio" { "tokio" } " - async runtime" }
                            li { "❤️ " a href="https://github.com/seanmonstar/reqwest" { "reqwest" } " - HTTP client" }
                            li { "❤️ " a href="https://github.com/xacrimon/dashmap" { "dashmap" } " - concurrent hashmap" }
                            li { "❤️ " a href="https://github.com/lambda-fairy/maud" { "maud" } " - HTML templating" }
                            li { "❤️ " a href="https://github.com/uuid-rs/uuid" { "uuid" } " - unique IDs" }
                            li { "❤️ " a href="https://github.com/serde-rs/serde" { "serde" } " - serialization" }
                            li { "❤️ " a href="https://github.com/tokio-rs/tracing" { "tracing" } " - logging" }
                            li { "❤️ " a href="https://github.com/rustls/rustls" { "rustls" } " - TLS" }
                        }
                    }

                    footer.version {
                        "ndld " (VERSION) " (" (GIT_VERSION) ") · "
                        a href="https://github.com/pgray/ndl/blob/main/LICENSE" { "MIT License" }
                    }
                }
            }
        }
    }
}

const LANDING_CSS: &str = r#"
    * { box-sizing: border-box; margin: 0; padding: 0; }
    body {
        font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
        background: linear-gradient(135deg, #0a0a0a 0%, #1a1a2e 100%);
        color: #fff;
        min-height: 100vh;
        padding: 2rem;
    }
    .container {
        max-width: 600px;
        margin: 0 auto;
    }
    h1 {
        font-size: 3rem;
        color: #00d4aa;
        margin-bottom: 0.5rem;
    }
    .tagline {
        color: #888;
        margin-bottom: 2rem;
        font-size: 1.1rem;
    }
    .links {
        display: flex;
        gap: 1rem;
        margin-bottom: 3rem;
    }
    .button {
        display: inline-block;
        padding: 0.75rem 1.5rem;
        background: #00d4aa;
        color: #0a0a0a;
        text-decoration: none;
        border-radius: 6px;
        font-weight: 600;
        transition: transform 0.2s, background 0.2s;
    }
    .button:hover {
        background: #00f5c4;
        transform: translateY(-2px);
    }
    .about, .deps {
        background: rgba(255,255,255,0.05);
        border-radius: 12px;
        padding: 1.5rem;
        margin-bottom: 2rem;
    }
    h2 {
        font-size: 1.2rem;
        color: #00d4aa;
        margin-bottom: 1rem;
    }
    p {
        color: #ccc;
        line-height: 1.6;
        margin-bottom: 0.75rem;
    }
    a { color: #00d4aa; }
    ul {
        list-style: none;
    }
    li {
        padding: 0.4rem 0;
        color: #aaa;
    }
    li a {
        color: #fff;
        text-decoration: none;
    }
    li a:hover {
        text-decoration: underline;
    }
    .version {
        margin-top: 2rem;
        padding-top: 1rem;
        border-top: 1px solid rgba(255,255,255,0.1);
        color: #666;
        font-size: 0.85rem;
    }
    .version a {
        color: #888;
    }
"#;

/// GET /privacy-policy - Privacy policy page
pub async fn privacy_policy() -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "Privacy Policy - ndld" }
                style { (LANDING_CSS) }
            }
            body {
                div.container {
                    h1 { "Privacy Policy" }
                    p.tagline { "ndl and ndld" }

                    div.about {
                        h2 { "What we don't do" }
                        ul {
                            li { "We don't collect analytics or usage data" }
                            li { "We don't track users" }
                            li { "We don't store access tokens on the server" }
                            li { "We don't log IP addresses or user agents" }
                            li { "We don't use cookies" }
                            li { "We don't share any data with third parties" }
                        }
                    }

                    div.about {
                        h2 { "What happens during authentication" }
                        p { "When you use ndld to authenticate:" }
                        ol {
                            li { "You're redirected to Threads (threads.net) to authorize the app" }
                            li { "Threads sends an authorization code back to ndld" }
                            li { "ndld exchanges that code for an access token" }
                            li { "The token is immediately returned to your local ndl client" }
                            li { "ndld discards the token - nothing is stored server-side" }
                        }
                        p {
                            "Your access token is only stored locally on your machine in "
                            code { "~/.config/ndl/config.toml" }
                            "."
                        }
                    }

                    div.about {
                        h2 { "Data deletion" }
                        p {
                            "Since we don't store any data, there's nothing to delete. "
                            "You can revoke ndl's access to your Threads account at any time through your Threads settings."
                        }
                    }

                    div.about {
                        h2 { "Contact" }
                        p {
                            "Questions? Open an issue at "
                            a href="https://github.com/pgray/ndl" { "github.com/pgray/ndl" }
                        }
                    }

                    div.links {
                        a.button href="/" { "Back to home" }
                    }
                }
            }
        }
    }
}

/// GET /tos - Terms of Service page
pub async fn tos() -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "Terms of Service - ndld" }
                style { (LANDING_CSS) }
            }
            body {
                div.container {
                    h1 { "Terms of Service" }
                    p.tagline { "ndl and ndld" }

                    div.about {
                        h2 { "Agreement" }
                        p {
                            "By using ndl or ndld, you agree to these terms. "
                            "If you don't agree, don't use the service."
                        }
                    }

                    div.about {
                        h2 { "What ndl/ndld does" }
                        ul {
                            li { strong { "ndl" } " is a terminal client for viewing and interacting with Threads" }
                            li { strong { "ndld" } " is an OAuth authentication server that helps ndl connect to your Threads account" }
                        }
                    }

                    div.about {
                        h2 { "Your responsibilities" }
                        ul {
                            li { "You must have a valid Threads account" }
                            li { "You're responsible for keeping your access token secure" }
                            li { "Don't use ndl/ndld for spam, harassment, or anything that violates Threads' terms of service" }
                            li { "Don't attempt to abuse, overload, or attack the ndld server" }
                        }
                    }

                    div.about {
                        h2 { "Our responsibilities" }
                        ul {
                            li { "We provide ndl and ndld \"as is\" with no warranties" }
                            li { "We don't guarantee uptime or availability of the ndld server" }
                            li { "We're not responsible for any content you post through ndl" }
                            li { "We're not affiliated with Meta or Threads" }
                        }
                    }

                    div.about {
                        h2 { "Limitations" }
                        ul {
                            li { "ndl/ndld are provided free of charge" }
                            li { "We may discontinue the service at any time" }
                            li { "We may block access to users who abuse the service" }
                        }
                    }

                    div.about {
                        h2 { "Intellectual property" }
                        ul {
                            li { "ndl and ndld are open source under the MIT license" }
                            li { "Threads is a trademark of Meta Platforms, Inc." }
                        }
                    }

                    div.about {
                        h2 { "Changes" }
                        p {
                            "We may update these terms. Continued use means you accept the new terms."
                        }
                    }

                    div.about {
                        h2 { "Contact" }
                        p {
                            "Questions? Open an issue at "
                            a href="https://github.com/pgray/ndl" { "github.com/pgray/ndl" }
                        }
                    }

                    div.links {
                        a.button href="/" { "Back to home" }
                    }
                }
            }
        }
    }
}

/// Build the router
pub fn create_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/privacy-policy", get(privacy_policy))
        .route("/tos", get(tos))
        .route("/auth/start", post(start_auth))
        .route("/auth/callback", get(auth_callback))
        .route("/auth/poll/{session_id}", get(poll_auth))
        .route("/health", get(health))
        .with_state(state)
}

// HTML responses

fn success_html() -> &'static str {
    r#"<!DOCTYPE html>
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
</html>"#
}

fn error_html(error: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <title>ndl - Authorization Failed</title>
    <style>
        body {{
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            display: flex;
            justify-content: center;
            align-items: center;
            height: 100vh;
            margin: 0;
            background: #0a0a0a;
            color: #fff;
        }}
        .container {{
            text-align: center;
            padding: 2rem;
        }}
        h1 {{ color: #ff4444; }}
        p {{ color: #888; }}
        .error {{ color: #ff8888; margin-top: 1rem; }}
    </style>
</head>
<body>
    <div class="container">
        <h1>Authorization Failed</h1>
        <p>Something went wrong during authentication.</p>
        <p class="error">{}</p>
    </div>
</body>
</html>"#,
        error
    )
}
