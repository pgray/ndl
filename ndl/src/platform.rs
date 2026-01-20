use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

/// Errors that can occur when interacting with social platforms
#[derive(Debug, Error)]
pub enum PlatformError {
    #[error("HTTP request failed: {0}")]
    Request(String),
    #[error("Authentication failed: {0}")]
    Auth(String),
    #[error("API error: {0}")]
    Api(String),
}

/// Platform identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Platform {
    Threads,
    Bluesky,
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Platform::Threads => write!(f, "Threads"),
            Platform::Bluesky => write!(f, "Bluesky"),
        }
    }
}

/// Platform-agnostic post representation
#[derive(Debug, Clone)]
pub struct Post {
    pub id: String,
    pub text: Option<String>,
    pub author_handle: Option<String>,
    pub timestamp: Option<String>,
    pub permalink: Option<String>,
    /// Media type (e.g., "REPOST_FACADE", "IMAGE", "VIDEO", "CAROUSEL_ALBUM")
    pub media_type: Option<String>,
}

/// Platform-agnostic reply thread (recursive structure)
#[derive(Debug, Clone)]
pub struct ReplyThread {
    pub post: Post,
    pub replies: Vec<ReplyThread>,
}

/// Common trait for all social media platform clients
#[async_trait]
pub trait SocialClient: Send + Sync {
    /// Get the authenticated user's posts/timeline
    async fn get_posts(&self, limit: Option<u32>) -> Result<Vec<Post>, PlatformError>;

    /// Get replies to a specific post (with nested replies)
    async fn get_post_replies(
        &self,
        post_id: &str,
        depth: u8,
    ) -> Result<Vec<ReplyThread>, PlatformError>;

    /// Create a new post
    async fn create_post(&self, text: &str) -> Result<(), PlatformError>;

    /// Reply to a post
    async fn reply_to_post(&self, post_id: &str, text: &str) -> Result<(), PlatformError>;
}

// Helper to convert from platform-specific errors
impl From<reqwest::Error> for PlatformError {
    fn from(err: reqwest::Error) -> Self {
        PlatformError::Request(err.to_string())
    }
}

impl From<crate::api::ApiError> for PlatformError {
    fn from(err: crate::api::ApiError) -> Self {
        match err {
            crate::api::ApiError::Request(e) => PlatformError::Request(e.to_string()),
            crate::api::ApiError::Api(e) => PlatformError::Api(e),
        }
    }
}
