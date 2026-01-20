use async_trait::async_trait;
use atrium_api::app::bsky::feed::post::RecordData;
use atrium_api::types::string::Datetime;
use bsky_sdk::BskyAgent;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::platform::{Platform, PlatformError, Post, PostResult, ReplyThread, SocialClient, UserProfile};

#[derive(Clone)]
pub struct BlueskyClient {
    agent: Arc<RwLock<BskyAgent>>,
}

impl BlueskyClient {
    /// Create a new Bluesky client and login
    pub async fn login(identifier: &str, password: &str) -> Result<Self, PlatformError> {
        let agent = BskyAgent::builder()
            .build()
            .await
            .map_err(|e| PlatformError::Auth(format!("Failed to create agent: {}", e)))?;

        agent
            .login(identifier, password)
            .await
            .map_err(|e| PlatformError::Auth(format!("Login failed: {}", e)))?;

        Ok(Self {
            agent: Arc::new(RwLock::new(agent)),
        })
    }

    /// Create a client from an existing session (for session persistence)
    pub async fn from_session(_session_data: String) -> Result<Self, PlatformError> {
        // TODO: Implement session restoration using FileStore
        // For now, this is a placeholder
        Err(PlatformError::NotImplemented)
    }

    /// Get the session data for persistence
    pub async fn get_session(&self) -> Result<String, PlatformError> {
        // TODO: Implement session serialization
        // For now, this is a placeholder
        Err(PlatformError::NotImplemented)
    }
}

#[async_trait]
impl SocialClient for BlueskyClient {
    fn platform(&self) -> Platform {
        Platform::Bluesky
    }

    async fn get_profile(&self) -> Result<UserProfile, PlatformError> {
        let agent = self.agent.read().await;

        // Get the session to get the DID
        let session = agent
            .get_session()
            .await
            .ok_or_else(|| PlatformError::Auth("No active session".to_string()))?;

        let did = session.did.clone();

        // Get the profile using the actor profile endpoint
        let profile = agent
            .api
            .app
            .bsky
            .actor
            .get_profile(
                atrium_api::app::bsky::actor::get_profile::ParametersData {
                    actor: did.into(),
                }
                .into(),
            )
            .await
            .map_err(|e| PlatformError::Api(format!("Failed to get profile: {}", e)))?;

        Ok(UserProfile {
            id: profile.data.did.to_string(),
            handle: Some(profile.data.handle.to_string()),
            display_name: profile.data.display_name.clone(),
            avatar_url: profile.data.avatar.clone(),
            bio: profile.data.description.clone(),
            platform: Platform::Bluesky,
        })
    }

    async fn get_posts(&self, limit: Option<u32>) -> Result<Vec<Post>, PlatformError> {
        let agent = self.agent.read().await;

        let timeline = agent
            .api
            .app
            .bsky
            .feed
            .get_timeline(
                atrium_api::app::bsky::feed::get_timeline::ParametersData {
                    algorithm: None,
                    cursor: None,
                    limit: None, // TODO: Fix limit conversion to proper type
                }
                .into(),
            )
            .await
            .map_err(|e| PlatformError::Api(format!("Failed to get timeline: {}", e)))?;

        Ok(timeline
            .data
            .feed
            .iter()
            .map(|feed_view| {
                // Extract text from the record
                // Note: This is simplified - in a real implementation we'd properly deserialize the record
                let text = Some(String::new()); // TODO: Extract text from record properly

                Post {
                    id: feed_view.post.uri.to_string(),
                    text,
                    author_handle: Some(feed_view.post.author.handle.as_str().to_string()),
                    author_name: feed_view.post.author.display_name.clone(),
                    timestamp: Some(feed_view.post.indexed_at.as_ref().to_string()),
                    permalink: Some(format!(
                        "https://bsky.app/profile/{}/post/{}",
                        feed_view.post.author.handle.as_str(),
                        feed_view.post.uri.to_string().split('/').last().unwrap_or("")
                    )),
                    platform: Platform::Bluesky,
                }
            })
            .collect())
    }

    async fn get_post_replies(
        &self,
        post_id: &str,
        depth: u8,
    ) -> Result<Vec<ReplyThread>, PlatformError> {
        let agent = self.agent.read().await;

        // TODO: Implement proper post thread fetching with depth
        // For now, return empty replies as this needs proper URI parsing and depth handling
        Ok(Vec::new())
    }

    async fn create_post(&self, text: &str) -> Result<PostResult, PlatformError> {
        let agent = self.agent.read().await;

        let response = agent
            .create_record(RecordData {
                created_at: Datetime::now(),
                embed: None,
                entities: None,
                facets: None,
                labels: None,
                langs: None,
                reply: None,
                tags: None,
                text: text.to_string(),
            })
            .await
            .map_err(|e| PlatformError::Api(format!("Failed to create post: {}", e)))?;

        Ok(PostResult {
            id: response.uri.to_string(),
            platform: Platform::Bluesky,
        })
    }

    async fn reply_to_post(
        &self,
        post_id: &str,
        text: &str,
    ) -> Result<PostResult, PlatformError> {
        let agent = self.agent.read().await;

        // To reply, we need to construct a reply reference
        // This requires getting the post we're replying to
        // For now, return not implemented as we need to properly handle URI parsing
        Err(PlatformError::NotImplemented)
    }

    fn clone_client(&self) -> Box<dyn SocialClient> {
        Box::new(self.clone())
    }
}
