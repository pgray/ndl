use async_trait::async_trait;
use futures::future::join_all;
use reqwest::Client;
use serde::Deserialize;
use std::sync::Arc;
use thiserror::Error;

use crate::platform::{
    Platform, PlatformError, Post, PostResult, ReplyThread as PlatformReplyThread, SocialClient,
    UserProfile as PlatformUserProfile,
};

const BASE_URL: &str = "https://graph.threads.net";

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("HTTP request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("API error: {0}")]
    Api(String),
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct UserProfile {
    pub id: String,
    pub username: Option<String>,
    pub name: Option<String>,
    pub threads_profile_picture_url: Option<String>,
    pub threads_biography: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Thread {
    pub id: String,
    pub text: Option<String>,
    pub username: Option<String>,
    pub timestamp: Option<String>,
    pub media_type: Option<String>,
    pub permalink: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct ThreadsResponse {
    pub data: Vec<Thread>,
    pub paging: Option<Paging>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct Paging {
    pub cursors: Option<Cursors>,
    pub next: Option<String>,
    pub previous: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct Cursors {
    pub before: Option<String>,
    pub after: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ContainerResponse {
    pub id: String,
}

/// A reply with nested replies
#[derive(Debug, Clone)]
pub struct ReplyThread {
    pub thread: Thread,
    pub replies: Vec<ReplyThread>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct PublishResponse {
    pub id: String,
}

#[derive(Clone)]
pub struct ThreadsClient {
    client: Client,
    access_token: Arc<String>,
}

impl ThreadsClient {
    pub fn new(access_token: String) -> Self {
        Self {
            client: Client::new(),
            access_token: Arc::new(access_token),
        }
    }

    /// Get the authenticated user's profile
    #[allow(dead_code)]
    pub async fn get_profile(&self) -> Result<UserProfile, ApiError> {
        let url = format!(
            "{}/me?fields=id,username,name,threads_profile_picture_url,threads_biography&access_token={}",
            BASE_URL, self.access_token
        );

        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(ApiError::Api(body));
        }

        Ok(response.json().await?)
    }

    /// Get the authenticated user's threads
    pub async fn get_threads(&self, limit: Option<u32>) -> Result<ThreadsResponse, ApiError> {
        let limit = limit.unwrap_or(25);
        let url = format!(
            "{}/me/threads?fields=id,text,username,timestamp,media_type,permalink&limit={}&access_token={}",
            BASE_URL, limit, self.access_token
        );

        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(ApiError::Api(body));
        }

        Ok(response.json().await?)
    }

    /// Get replies to the authenticated user's threads
    #[allow(dead_code)]
    pub async fn get_replies(&self, limit: Option<u32>) -> Result<ThreadsResponse, ApiError> {
        let limit = limit.unwrap_or(25);
        let url = format!(
            "{}/me/replies?fields=id,text,username,timestamp,media_type,permalink&limit={}&access_token={}",
            BASE_URL, limit, self.access_token
        );

        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(ApiError::Api(body));
        }

        Ok(response.json().await?)
    }

    /// Get a specific thread by ID
    #[allow(dead_code)]
    pub async fn get_thread(&self, thread_id: &str) -> Result<Thread, ApiError> {
        let url = format!(
            "{}/{}?fields=id,text,username,timestamp,media_type,permalink&access_token={}",
            BASE_URL, thread_id, self.access_token
        );

        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(ApiError::Api(body));
        }

        Ok(response.json().await?)
    }

    /// Get replies to a specific thread
    pub async fn get_thread_replies(&self, thread_id: &str) -> Result<ThreadsResponse, ApiError> {
        let url = format!(
            "{}/{}/replies?fields=id,text,username,timestamp&access_token={}",
            BASE_URL, thread_id, self.access_token
        );

        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(ApiError::Api(body));
        }

        Ok(response.json().await?)
    }

    /// Get replies to a thread with nested replies (recursive)
    pub async fn get_thread_replies_nested(
        &self,
        thread_id: &str,
        depth: u8,
    ) -> Result<Vec<ReplyThread>, ApiError> {
        let replies_resp = self.get_thread_replies(thread_id).await?;

        if depth == 0 || replies_resp.data.is_empty() {
            return Ok(replies_resp
                .data
                .into_iter()
                .map(|t| ReplyThread {
                    thread: t,
                    replies: Vec::new(),
                })
                .collect());
        }

        // Fetch nested replies in parallel
        let nested_futures: Vec<_> = replies_resp
            .data
            .iter()
            .map(|reply| {
                let client = self.clone();
                let reply_id = reply.id.clone();
                async move {
                    client
                        .get_thread_replies_nested(&reply_id, depth - 1)
                        .await
                        .unwrap_or_default()
                }
            })
            .collect();

        let nested_results = join_all(nested_futures).await;

        Ok(replies_resp
            .data
            .into_iter()
            .zip(nested_results)
            .map(|(thread, replies)| ReplyThread { thread, replies })
            .collect())
    }

    /// Wait for container to be ready (poll until FINISHED or ERROR)
    async fn wait_for_container(&self, container_id: &str) -> Result<String, ApiError> {
        #[derive(Deserialize)]
        struct StatusResponse {
            status: Option<String>,
            error_message: Option<String>,
        }

        let url = format!(
            "{}/{}?fields=status,error_message&access_token={}",
            BASE_URL, container_id, self.access_token
        );

        // Poll up to 15 times with 2s delay (30 seconds max)
        for attempt in 0..15 {
            let response = self.client.get(&url).send().await?;
            let body = response.text().await.unwrap_or_default();

            let status_resp: StatusResponse =
                serde_json::from_str(&body).unwrap_or(StatusResponse {
                    status: Some("UNKNOWN".to_string()),
                    error_message: None,
                });

            let status = status_resp.status.unwrap_or_else(|| "UNKNOWN".to_string());
            tracing::debug!("Container status check {}: {}", attempt + 1, status);

            if let Some(err) = &status_resp.error_message {
                tracing::warn!("Container error: {}", err);
            }

            match status.as_str() {
                "FINISHED" => return Ok(status),
                "ERROR" => {
                    let err_msg = status_resp
                        .error_message
                        .unwrap_or_else(|| "Unknown error".to_string());
                    return Err(ApiError::Api(format!("Container failed: {}", err_msg)));
                }
                "IN_PROGRESS" => {
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                }
                _ => {
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                }
            }
        }

        Err(ApiError::Api("Container processing timed out".to_string()))
    }

    /// Create a reply to a thread (two-step: create container, then publish)
    pub async fn reply_to_thread(
        &self,
        reply_to_id: &str,
        text: &str,
    ) -> Result<PublishResponse, ApiError> {
        tracing::debug!("Attempting reply to thread ID: {}", reply_to_id);

        // Step 1: Create container
        let container_url = format!(
            "{}/me/threads?media_type=TEXT&text={}&reply_to_id={}&access_token={}",
            BASE_URL,
            urlencoding::encode(text),
            reply_to_id,
            self.access_token
        );

        let response = self.client.post(&container_url).send().await?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();

        tracing::debug!("Container creation response ({}): {}", status, body);

        if !status.is_success() {
            return Err(ApiError::Api(format!(
                "Container creation failed: {}",
                body
            )));
        }

        // Check for error in response body (API sometimes returns 200 with error)
        if body.contains("\"error\"") {
            return Err(ApiError::Api(format!(
                "Cannot reply to this thread: {}",
                body
            )));
        }

        let container: ContainerResponse = serde_json::from_str(&body)
            .map_err(|e| ApiError::Api(format!("Invalid container response: {} - {}", e, body)))?;

        tracing::debug!("Container created with ID: {}", container.id);

        // Wait for container to be ready (poll until FINISHED or ERROR)
        let status = self.wait_for_container(&container.id).await?;
        tracing::debug!("Final container status: {}", status);

        if status != "FINISHED" {
            return Err(ApiError::Api(format!(
                "Container not ready for publish: {}",
                status
            )));
        }

        // Step 2: Publish
        let publish_url = format!(
            "{}/me/threads_publish?creation_id={}&access_token={}",
            BASE_URL, container.id, self.access_token
        );

        let response = self.client.post(&publish_url).send().await?;

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(ApiError::Api(format!("Publish failed: {}", body)));
        }

        Ok(response.json().await?)
    }

    /// Post a new thread (not a reply)
    pub async fn post_thread(&self, text: &str) -> Result<PublishResponse, ApiError> {
        // Step 1: Create container
        let container_url = format!(
            "{}/me/threads?media_type=TEXT&text={}&access_token={}",
            BASE_URL,
            urlencoding::encode(text),
            self.access_token
        );

        let response = self.client.post(&container_url).send().await?;

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(ApiError::Api(format!(
                "Container creation failed: {}",
                body
            )));
        }

        let container: ContainerResponse = response.json().await?;

        // Step 2: Wait for container to be ready
        let status = self.wait_for_container(&container.id).await?;
        if status != "FINISHED" {
            return Err(ApiError::Api(format!(
                "Container not ready for publish: {}",
                status
            )));
        }

        // Step 3: Publish
        let publish_url = format!(
            "{}/me/threads_publish?creation_id={}&access_token={}",
            BASE_URL, container.id, self.access_token
        );

        let response = self.client.post(&publish_url).send().await?;

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(ApiError::Api(format!("Publish failed: {}", body)));
        }

        Ok(response.json().await?)
    }
}

// Implement the platform abstraction trait for ThreadsClient
#[async_trait]
impl SocialClient for ThreadsClient {
    fn platform(&self) -> Platform {
        Platform::Threads
    }

    async fn get_profile(&self) -> Result<PlatformUserProfile, PlatformError> {
        let profile = self.get_profile().await?;
        Ok(PlatformUserProfile {
            id: profile.id,
            handle: profile.username,
            display_name: profile.name,
            avatar_url: profile.threads_profile_picture_url,
            bio: profile.threads_biography,
            platform: Platform::Threads,
        })
    }

    async fn get_posts(&self, limit: Option<u32>) -> Result<Vec<Post>, PlatformError> {
        let response = self.get_threads(limit).await?;
        Ok(response
            .data
            .into_iter()
            .map(|t| Post {
                id: t.id,
                text: t.text,
                author_handle: t.username,
                author_name: None,
                timestamp: t.timestamp,
                permalink: t.permalink,
                platform: Platform::Threads,
            })
            .collect())
    }

    async fn get_post_replies(
        &self,
        post_id: &str,
        depth: u8,
    ) -> Result<Vec<PlatformReplyThread>, PlatformError> {
        let replies = self.get_thread_replies_nested(post_id, depth).await?;
        Ok(convert_reply_threads(replies))
    }

    async fn create_post(&self, text: &str) -> Result<PostResult, PlatformError> {
        let response = self.post_thread(text).await?;
        Ok(PostResult {
            id: response.id,
            platform: Platform::Threads,
        })
    }

    async fn reply_to_post(&self, post_id: &str, text: &str) -> Result<PostResult, PlatformError> {
        let response = self.reply_to_thread(post_id, text).await?;
        Ok(PostResult {
            id: response.id,
            platform: Platform::Threads,
        })
    }

    fn clone_client(&self) -> Box<dyn SocialClient> {
        Box::new(self.clone())
    }
}

// Helper to convert Threads reply threads to platform reply threads
fn convert_reply_threads(threads: Vec<ReplyThread>) -> Vec<PlatformReplyThread> {
    threads
        .into_iter()
        .map(|rt| PlatformReplyThread {
            post: Post {
                id: rt.thread.id,
                text: rt.thread.text,
                author_handle: rt.thread.username,
                author_name: None,
                timestamp: rt.thread.timestamp,
                permalink: rt.thread.permalink,
                platform: Platform::Threads,
            },
            replies: convert_reply_threads(rt.replies),
        })
        .collect()
}
