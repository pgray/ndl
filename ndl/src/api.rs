use async_trait::async_trait;
use futures::future::join_all;
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use thiserror::Error;

use crate::platform::{
    PlatformError, Post, PostStats, ReplyThread as PlatformReplyThread, SocialClient,
};

const BASE_URL: &str = "https://graph.threads.net";

/// How long cached engagement counts stay fresh. The Insights API has no
/// batch endpoint and undocumented rate limits, so one call per post per
/// five minutes is as often as we ask.
const INSIGHTS_TTL: Duration = Duration::from_secs(300);

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

/// Engagement counts for one post, with the moment they were fetched
#[derive(Debug, Clone, Copy)]
struct CachedStats {
    fetched: Instant,
    stats: PostStats,
}

/// One `{name, values | total_value}` entry of an insights response
#[derive(Debug, Deserialize)]
struct InsightMetric {
    name: String,
    #[serde(default)]
    values: Vec<InsightValue>,
    #[serde(default)]
    total_value: Option<InsightValue>,
}

#[derive(Debug, Deserialize)]
struct InsightValue {
    #[serde(default)]
    value: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct InsightsResponse {
    #[serde(default)]
    data: Vec<InsightMetric>,
}

impl InsightMetric {
    /// The metric's number, from whichever shape the API used for it
    fn value(&self) -> Option<u64> {
        self.total_value
            .as_ref()
            .and_then(|v| v.value)
            .or_else(|| self.values.first().and_then(|v| v.value))
    }
}

/// Parse a `/{media_id}/insights` response into engagement counts
fn parse_media_insights(body: &str) -> Result<PostStats, ApiError> {
    let response: InsightsResponse = serde_json::from_str(body)
        .map_err(|e| ApiError::Api(format!("Invalid insights response: {} - {}", e, body)))?;

    // Reposts of somebody else's post (REPOST_FACADE) report no metrics at all
    if response.data.is_empty() {
        return Err(ApiError::Api("no insights".to_string()));
    }

    let metric = |name: &str| {
        response
            .data
            .iter()
            .find(|m| m.name == name)
            .and_then(|m| m.value())
    };

    Ok(PostStats {
        likes: metric("likes").unwrap_or(0),
        replies: metric("replies").unwrap_or(0),
        reposts: metric("reposts").unwrap_or(0),
        quotes: metric("quotes").unwrap_or(0),
        shares: metric("shares"),
    })
}

/// Parse a `/me/threads_insights?metric=followers_count` response
fn parse_follower_count(body: &str) -> Result<Option<u64>, ApiError> {
    let response: InsightsResponse = serde_json::from_str(body)
        .map_err(|e| ApiError::Api(format!("Invalid insights response: {} - {}", e, body)))?;

    Ok(response
        .data
        .iter()
        .find(|m| m.name == "followers_count")
        .or_else(|| response.data.first())
        .and_then(|m| m.value()))
}

/// True when `haystack` holds `"code":<code>` that isn't the prefix of a longer code
fn has_error_code(haystack: &str, code: &str) -> bool {
    let needle = format!("\"code\":{}", code);
    haystack.match_indices(&needle).any(|(i, _)| {
        haystack[i + needle.len()..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_ascii_digit())
    })
}

/// Whether the API turned us down for lack of the insights permission
fn is_permission_error(err: &ApiError) -> bool {
    let message = err.to_string().to_lowercase();
    if message.contains("permission") {
        return true;
    }
    let compact: String = message.chars().filter(|c| !c.is_whitespace()).collect();
    has_error_code(&compact, "10") || has_error_code(&compact, "200")
}

#[derive(Clone)]
pub struct ThreadsClient {
    client: Client,
    access_token: Arc<String>,
    /// Engagement counts per media id, refreshed at most once per `INSIGHTS_TTL`
    insights: Arc<Mutex<HashMap<String, CachedStats>>>,
    /// Set once the token turns out to lack `threads_manage_insights`
    insights_disabled: Arc<AtomicBool>,
}

impl ThreadsClient {
    pub fn new(access_token: String) -> Self {
        Self {
            client: Client::new(),
            access_token: Arc::new(access_token),
            insights: Arc::new(Mutex::new(HashMap::new())),
            insights_disabled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Give up on insights for the rest of the session, saying so once
    fn disable_insights(&self) {
        if !self.insights_disabled.swap(true, Ordering::Relaxed) {
            tracing::warn!(
                "Threads insights unavailable: token lacks threads_manage_insights — run `ndl login` again to grant it"
            );
        }
    }

    /// Lifetime engagement counts for one post (needs `threads_manage_insights`)
    pub async fn get_media_insights(&self, media_id: &str) -> Result<PostStats, ApiError> {
        let url = format!(
            "{}/{}/insights?metric=likes,replies,reposts,quotes,shares&access_token={}",
            BASE_URL, media_id, self.access_token
        );

        let response = self.client.get(&url).send().await?;
        let status = response.status();
        let body = response.text().await?;

        // The API sometimes reports permission problems with a 200
        if !status.is_success() || body.contains("\"error\"") {
            return Err(ApiError::Api(body));
        }

        parse_media_insights(&body)
    }

    /// Follower count from the account insights endpoint (needs `threads_manage_insights`)
    pub async fn fetch_follower_count(&self) -> Result<Option<u64>, ApiError> {
        let url = format!(
            "{}/me/threads_insights?metric=followers_count&access_token={}",
            BASE_URL, self.access_token
        );

        let response = self.client.get(&url).send().await?;
        let status = response.status();
        let body = response.text().await?;

        if !status.is_success() || body.contains("\"error\"") {
            return Err(ApiError::Api(body));
        }

        parse_follower_count(&body)
    }

    /// Refresh cached counts for the posts whose entry is missing or stale.
    /// Failures are not fatal: the affected posts simply render without counts.
    async fn refresh_insights(&self, threads: &[Thread]) {
        if self.insights_disabled.load(Ordering::Relaxed) {
            return;
        }

        let stale: Vec<String> = {
            let Ok(cache) = self.insights.lock() else {
                return;
            };
            threads
                .iter()
                // Reposts of other people's posts have no insights of their own
                .filter(|t| t.media_type.as_deref() != Some("REPOST_FACADE"))
                .filter(|t| {
                    cache
                        .get(&t.id)
                        .is_none_or(|c| c.fetched.elapsed() >= INSIGHTS_TTL)
                })
                .map(|t| t.id.clone())
                .collect()
        };

        if stale.is_empty() {
            return;
        }

        let results = join_all(stale.into_iter().map(|id| {
            let client = self.clone();
            async move {
                let stats = client.get_media_insights(&id).await;
                (id, stats)
            }
        }))
        .await;

        let fetched = Instant::now();
        let mut fresh: Vec<(String, CachedStats)> = Vec::new();
        for (id, result) in results {
            match result {
                Ok(stats) => fresh.push((id, CachedStats { fetched, stats })),
                Err(e) if is_permission_error(&e) => self.disable_insights(),
                Err(e) => tracing::debug!("No insights for {}: {}", id, e),
            }
        }

        if !fresh.is_empty()
            && let Ok(mut cache) = self.insights.lock()
        {
            cache.extend(fresh);
        }
    }

    /// Cached counts for each of `threads`, in order (stale entries still count:
    /// a slightly old number reads better than a blank column)
    fn cached_stats(&self, threads: &[Thread]) -> Vec<Option<PostStats>> {
        match self.insights.lock() {
            Ok(cache) => threads
                .iter()
                .map(|t| cache.get(&t.id).map(|c| c.stats))
                .collect(),
            Err(_) => vec![None; threads.len()],
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
            "{}/{}/replies?fields=id,text,username,timestamp,media_type,permalink&access_token={}",
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
    async fn get_posts(&self, limit: Option<u32>) -> Result<Vec<Post>, PlatformError> {
        let response = self.get_threads(limit).await?;
        self.refresh_insights(&response.data).await;
        let stats = self.cached_stats(&response.data);

        Ok(response
            .data
            .into_iter()
            .zip(stats)
            .map(|(t, stats)| {
                let reposted = t.media_type.as_deref() == Some("REPOST_FACADE");
                Post {
                    id: t.id,
                    text: t.text,
                    author_handle: t.username,
                    timestamp: t.timestamp,
                    permalink: t.permalink,
                    media_type: t.media_type,
                    liked: false,
                    reposted,
                    stats,
                }
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

    async fn create_post(&self, text: &str) -> Result<(), PlatformError> {
        self.post_thread(text).await?;
        Ok(())
    }

    async fn reply_to_post(&self, post_id: &str, text: &str) -> Result<(), PlatformError> {
        self.reply_to_thread(post_id, text).await?;
        Ok(())
    }

    async fn like_post(&self, _post_id: &str) -> Result<(), PlatformError> {
        // The Threads API exposes no endpoint for liking a post.
        Err(PlatformError::Unsupported(
            "liking isn't supported by the Threads API".to_string(),
        ))
    }

    async fn get_follower_count(&self) -> Result<Option<u64>, PlatformError> {
        if self.insights_disabled.load(Ordering::Relaxed) {
            return Ok(None);
        }

        match self.fetch_follower_count().await {
            Ok(count) => Ok(count),
            Err(e) if is_permission_error(&e) => {
                self.disable_insights();
                Ok(None)
            }
            Err(e) => Err(e.into()),
        }
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
                timestamp: rt.thread.timestamp,
                permalink: rt.thread.permalink,
                reposted: rt.thread.media_type.as_deref() == Some("REPOST_FACADE"),
                media_type: rt.thread.media_type,
                liked: false,
                stats: None,
            },
            replies: convert_reply_threads(rt.replies),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_metrics_in_values_form() {
        let body = r#"{"data":[
            {"name":"likes","period":"lifetime","values":[{"value":100}],"title":"Likes"},
            {"name":"replies","period":"lifetime","values":[{"value":7}],"title":"Replies"},
            {"name":"reposts","period":"lifetime","values":[{"value":3}],"title":"Reposts"},
            {"name":"quotes","period":"lifetime","values":[{"value":2}],"title":"Quotes"},
            {"name":"shares","period":"lifetime","values":[{"value":5}],"title":"Shares"}
        ]}"#;

        let stats = parse_media_insights(body).expect("insights should parse");
        assert_eq!(
            stats,
            PostStats {
                likes: 100,
                replies: 7,
                reposts: 3,
                quotes: 2,
                shares: Some(5),
            }
        );
    }

    #[test]
    fn parses_metrics_in_total_value_form() {
        let body = r#"{"data":[
            {"name":"likes","period":"lifetime","total_value":{"value":42}},
            {"name":"replies","period":"lifetime","total_value":{"value":1}},
            {"name":"reposts","period":"lifetime","total_value":{"value":0}},
            {"name":"quotes","period":"lifetime","total_value":{"value":9}},
            {"name":"shares","period":"lifetime","total_value":{"value":4}}
        ]}"#;

        let stats = parse_media_insights(body).expect("insights should parse");
        assert_eq!(
            stats,
            PostStats {
                likes: 42,
                replies: 1,
                reposts: 0,
                quotes: 9,
                shares: Some(4),
            }
        );
    }

    #[test]
    fn empty_data_is_an_error() {
        // What a REPOST_FACADE (someone else's post) comes back with
        let err = parse_media_insights(r#"{"data":[]}"#).expect_err("empty data should fail");
        assert!(err.to_string().contains("no insights"), "got {}", err);
    }

    #[test]
    fn missing_metrics_read_as_zero_and_shares_as_none() {
        let body = r#"{"data":[{"name":"likes","period":"lifetime","values":[{"value":8}]}]}"#;

        let stats = parse_media_insights(body).expect("insights should parse");
        assert_eq!(
            stats,
            PostStats {
                likes: 8,
                replies: 0,
                reposts: 0,
                quotes: 0,
                shares: None,
            }
        );
    }

    #[test]
    fn parses_follower_count() {
        let body = r#"{"data":[{"name":"followers_count","period":"day","total_value":{"value":1234},"title":"Followers"}]}"#;
        assert_eq!(parse_follower_count(body).ok().flatten(), Some(1234));

        let values_form =
            r#"{"data":[{"name":"followers_count","period":"day","values":[{"value":7}]}]}"#;
        assert_eq!(parse_follower_count(values_form).ok().flatten(), Some(7));

        assert_eq!(parse_follower_count(r#"{"data":[]}"#).ok().flatten(), None);
    }

    #[test]
    fn spots_permission_errors() {
        assert!(is_permission_error(&ApiError::Api(
            r#"{"error":{"message":"(#10) Application does not have permission for this action","code":10}}"#
                .to_string()
        )));
        assert!(is_permission_error(&ApiError::Api(
            r#"{"error":{"message":"Requires insights scope", "code": 200}}"#.to_string()
        )));
        assert!(!is_permission_error(&ApiError::Api(
            r#"{"error":{"message":"Invalid parameter","code":100}}"#.to_string()
        )));
        assert!(!is_permission_error(&ApiError::Api(
            "no insights".to_string()
        )));
    }
}
