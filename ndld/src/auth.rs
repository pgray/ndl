use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use uuid::Uuid;

const SESSION_TTL: Duration = Duration::from_secs(300); // 5 minutes
const TOKEN_URL: &str = "https://graph.threads.net/oauth/access_token";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum AuthState {
    Pending,
    Completed { access_token: String },
    Failed { error: String },
}

#[derive(Debug)]
pub struct AuthSession {
    pub id: String,
    pub state: RwLock<AuthState>,
    pub created_at: Instant,
}

impl AuthSession {
    pub fn new() -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            state: RwLock::new(AuthState::Pending),
            created_at: Instant::now(),
        }
    }

    pub fn is_expired(&self) -> bool {
        self.created_at.elapsed() > SESSION_TTL
    }
}

#[derive(Clone)]
pub struct SessionStore {
    sessions: Arc<DashMap<String, Arc<AuthSession>>>,
}

impl SessionStore {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(DashMap::new()),
        }
    }

    pub fn create_session(&self) -> Arc<AuthSession> {
        let session = Arc::new(AuthSession::new());
        self.sessions
            .insert(session.id.clone(), Arc::clone(&session));
        session
    }

    pub fn get_session(&self, id: &str) -> Option<Arc<AuthSession>> {
        self.sessions.get(id).map(|r| Arc::clone(r.value()))
    }

    pub fn remove_session(&self, id: &str) {
        self.sessions.remove(id);
    }

    /// Remove expired sessions
    pub fn cleanup_expired(&self) {
        self.sessions.retain(|_, session| !session.is_expired());
    }
}

#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    #[allow(dead_code)]
    pub user_id: u64,
}

#[derive(Clone)]
pub struct OAuthConfig {
    pub client_id: String,
    pub client_secret: String,
    pub public_url: String,
}

impl OAuthConfig {
    pub fn redirect_uri(&self) -> String {
        format!("{}/auth/callback", self.public_url)
    }

    pub fn authorization_url(&self, state: &str) -> String {
        format!(
            "https://threads.net/oauth/authorize?client_id={}&redirect_uri={}&scope=threads_basic,threads_read_replies,threads_manage_replies,threads_content_publish&response_type=code&state={}",
            self.client_id,
            urlencoding::encode(&self.redirect_uri()),
            state
        )
    }

    /// Exchange an authorization code for an access token
    pub async fn exchange_code(&self, code: &str) -> Result<TokenResponse, String> {
        let client = reqwest::Client::new();
        let redirect_uri = self.redirect_uri();

        let params = [
            ("client_id", self.client_id.as_str()),
            ("client_secret", self.client_secret.as_str()),
            ("grant_type", "authorization_code"),
            ("redirect_uri", redirect_uri.as_str()),
            ("code", code),
        ];

        let response = client
            .post(TOKEN_URL)
            .form(&params)
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("HTTP {}: {}", status, body));
        }

        response
            .json::<TokenResponse>()
            .await
            .map_err(|e| format!("Parse error: {}", e))
    }
}

/// Spawn a background task to periodically clean up expired sessions
pub fn spawn_cleanup_task(store: SessionStore) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            store.cleanup_expired();
            tracing::debug!("Cleaned up expired auth sessions");
        }
    });
}
