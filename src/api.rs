use reqwest::Client;
use serde::Deserialize;
use thiserror::Error;
use std::sync::Arc;
use futures::future::join_all;

const BASE_URL: &str = "https://graph.threads.net";

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("HTTP request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("API error: {0}")]
    Api(String),
}

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

#[derive(Debug, Deserialize)]
pub struct ThreadsResponse {
    pub data: Vec<Thread>,
    pub paging: Option<Paging>,
}

#[derive(Debug, Deserialize)]
pub struct Paging {
    pub cursors: Option<Cursors>,
    pub next: Option<String>,
    pub previous: Option<String>,
}

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
    pub async fn get_thread_replies_nested(&self, thread_id: &str, depth: u8) -> Result<Vec<ReplyThread>, ApiError> {
        let replies_resp = self.get_thread_replies(thread_id).await?;

        if depth == 0 || replies_resp.data.is_empty() {
            return Ok(replies_resp.data.into_iter().map(|t| ReplyThread {
                thread: t,
                replies: Vec::new(),
            }).collect());
        }

        // Fetch nested replies in parallel
        let nested_futures: Vec<_> = replies_resp.data.iter().map(|reply| {
            let client = self.clone();
            let reply_id = reply.id.clone();
            async move {
                client.get_thread_replies_nested(&reply_id, depth - 1).await.unwrap_or_default()
            }
        }).collect();

        let nested_results = join_all(nested_futures).await;

        Ok(replies_resp.data.into_iter().zip(nested_results).map(|(thread, replies)| {
            ReplyThread { thread, replies }
        }).collect())
    }

    /// Create a reply to a thread (two-step: create container, then publish)
    pub async fn reply_to_thread(&self, reply_to_id: &str, text: &str) -> Result<PublishResponse, ApiError> {
        // Step 1: Create container
        let container_url = format!(
            "{}/me/threads?media_type=TEXT&text={}&reply_to_id={}&access_token={}",
            BASE_URL,
            urlencoding::encode(text),
            reply_to_id,
            self.access_token
        );

        let response = self.client.post(&container_url).send().await?;

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(ApiError::Api(format!("Container creation failed: {}", body)));
        }

        let container: ContainerResponse = response.json().await?;

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
            return Err(ApiError::Api(format!("Container creation failed: {}", body)));
        }

        let container: ContainerResponse = response.json().await?;

        // Step 2: Wait for container to be ready
        self.wait_for_container(&container.id).await?;

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

    /// Wait for a media container to be ready for publishing
    async fn wait_for_container(&self, container_id: &str) -> Result<(), ApiError> {
        let status_url = format!(
            "{}/{}?fields=status&access_token={}",
            BASE_URL, container_id, self.access_token
        );

        for _ in 0..10 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;

            let response = self.client.get(&status_url).send().await?;
            if response.status().is_success() {
                let body: serde_json::Value = response.json().await?;
                match body.get("status").and_then(|s| s.as_str()) {
                    Some("FINISHED") | None => return Ok(()), // No status = ready for text posts
                    Some("ERROR") => return Err(ApiError::Api("Container processing failed".to_string())),
                    Some("EXPIRED") => return Err(ApiError::Api("Container expired".to_string())),
                    Some(_) => continue, // IN_PROGRESS, keep waiting
                }
            }
        }

        Err(ApiError::Api("Container processing timed out".to_string()))
    }
}
