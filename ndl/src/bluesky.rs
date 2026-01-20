use async_trait::async_trait;
use atrium_api::app::bsky::feed::defs::{ThreadViewPostData, ThreadViewPostRepliesItem};
use atrium_api::app::bsky::feed::get_post_thread::OutputThreadRefs;
use atrium_api::app::bsky::feed::post::{RecordData, ReplyRefData};
use atrium_api::com::atproto::repo::strong_ref::MainData as StrongRef;
use atrium_api::types::Union;
use atrium_api::types::string::Datetime;
use bsky_sdk::BskyAgent;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::platform::{PlatformError, Post, ReplyThread, SocialClient};

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
    pub async fn from_session(session_data: String) -> Result<Self, PlatformError> {
        use bsky_sdk::agent::config::Config as BskyConfig;

        // Deserialize the session from JSON
        let config: BskyConfig = serde_json::from_str(&session_data)
            .map_err(|e| PlatformError::Auth(format!("Failed to deserialize session: {}", e)))?;

        // Create agent from config
        let agent = BskyAgent::builder()
            .config(config)
            .build()
            .await
            .map_err(|e| {
                PlatformError::Auth(format!("Failed to create agent from session: {}", e))
            })?;

        Ok(Self {
            agent: Arc::new(RwLock::new(agent)),
        })
    }

    /// Get the session data for persistence
    pub async fn get_session(&self) -> Result<String, PlatformError> {
        let agent = self.agent.read().await;

        // Get the configuration which includes session data
        let config = agent.to_config().await;

        // Serialize to JSON
        serde_json::to_string(&config)
            .map_err(|e| PlatformError::Api(format!("Failed to serialize session: {}", e)))
    }

    /// Extract replies from a thread view post
    fn extract_replies(&self, thread_view: &ThreadViewPostData) -> Vec<ReplyThread> {
        let Some(replies) = &thread_view.replies else {
            return Vec::new();
        };

        replies
            .iter()
            .filter_map(|reply| self.convert_reply_item(reply))
            .collect()
    }

    /// Convert a reply item (Union<ThreadViewPostRepliesItem>) to a ReplyThread
    fn convert_reply_item(&self, item: &Union<ThreadViewPostRepliesItem>) -> Option<ReplyThread> {
        match item {
            Union::Refs(ThreadViewPostRepliesItem::ThreadViewPost(thread_post)) => {
                let post_view = &thread_post.data.post;

                // Extract text from the record
                let text = serde_json::to_value(&post_view.record)
                    .ok()
                    .and_then(|v| v.get("text").and_then(|t| t.as_str()).map(String::from));

                let post = Post {
                    id: post_view.uri.to_string(),
                    text,
                    author_handle: Some(post_view.author.handle.as_str().to_string()),
                    timestamp: Some(post_view.indexed_at.as_ref().to_string()),
                    permalink: Some(format!(
                        "https://bsky.app/profile/{}/post/{}",
                        post_view.author.handle.as_str(),
                        post_view.uri.split('/').next_back().unwrap_or("")
                    )),
                    media_type: None,
                };

                // Recursively extract nested replies
                let nested_replies = self.extract_replies(&thread_post.data);

                Some(ReplyThread {
                    post,
                    replies: nested_replies,
                })
            }
            Union::Refs(ThreadViewPostRepliesItem::BlockedPost(_)) => None,
            Union::Refs(ThreadViewPostRepliesItem::NotFoundPost(_)) => None,
            Union::Unknown(_) => None,
        }
    }

    /// Get the CID and root info for a post by fetching the thread
    /// Returns (cid, Option<(root_uri, root_cid)>)
    async fn get_post_info(
        &self,
        uri: &str,
    ) -> Result<(String, Option<(String, String)>), PlatformError> {
        let agent = self.agent.read().await;

        let thread = agent
            .api
            .app
            .bsky
            .feed
            .get_post_thread(
                atrium_api::app::bsky::feed::get_post_thread::ParametersData {
                    uri: uri.to_string(),
                    depth: Some(atrium_api::types::LimitedU16::try_from(0u16).unwrap()),
                    parent_height: Some(atrium_api::types::LimitedU16::try_from(1u16).unwrap()),
                }
                .into(),
            )
            .await
            .map_err(|e| PlatformError::Api(format!("Failed to get post: {}", e)))?;

        match &thread.data.thread {
            Union::Refs(OutputThreadRefs::AppBskyFeedDefsThreadViewPost(thread_view)) => {
                let cid = thread_view.data.post.cid.as_ref().to_string();

                // Check if this post has a reply reference (meaning it's a reply to something)
                // If so, extract the root from the record
                let root_info = serde_json::to_value(&thread_view.data.post.record)
                    .ok()
                    .and_then(|v| {
                        v.get("reply").and_then(|reply| {
                            let root_uri = reply.get("root")?.get("uri")?.as_str()?.to_string();
                            let root_cid = reply.get("root")?.get("cid")?.as_str()?.to_string();
                            Some((root_uri, root_cid))
                        })
                    });

                Ok((cid, root_info))
            }
            _ => Err(PlatformError::Api("Post not found".to_string())),
        }
    }
}

#[async_trait]
impl SocialClient for BlueskyClient {
    async fn get_posts(&self, limit: Option<u32>) -> Result<Vec<Post>, PlatformError> {
        let agent = self.agent.read().await;

        // Get the user's DID to fetch their own posts (like Threads /me/threads)
        let session = agent
            .get_session()
            .await
            .ok_or_else(|| PlatformError::Auth("No active session".to_string()))?;
        let did = session.did.clone();

        // Convert limit to the proper type (LimitedNonZeroU8, max 100)
        let limit = limit
            .map(|l| l.min(100) as u8)
            .and_then(|l| atrium_api::types::LimitedNonZeroU8::try_from(l).ok());

        // Use get_author_feed to get the user's own posts (not timeline)
        let feed = agent
            .api
            .app
            .bsky
            .feed
            .get_author_feed(
                atrium_api::app::bsky::feed::get_author_feed::ParametersData {
                    actor: did.into(),
                    cursor: None,
                    filter: Some("posts_no_replies".to_string()),
                    include_pins: None,
                    limit,
                }
                .into(),
            )
            .await
            .map_err(|e| PlatformError::Api(format!("Failed to get posts: {}", e)))?;

        Ok(feed
            .data
            .feed
            .iter()
            .map(|feed_view| {
                // Extract text from the record
                // The record is Unknown type, we need to serialize it to JSON and extract text
                let text = serde_json::to_value(&feed_view.post.record)
                    .ok()
                    .and_then(|v| v.get("text").and_then(|t| t.as_str()).map(String::from));

                Post {
                    id: feed_view.post.uri.to_string(),
                    text,
                    author_handle: Some(feed_view.post.author.handle.as_str().to_string()),
                    timestamp: Some(feed_view.post.indexed_at.as_ref().to_string()),
                    permalink: Some(format!(
                        "https://bsky.app/profile/{}/post/{}",
                        feed_view.post.author.handle.as_str(),
                        feed_view.post.uri.split('/').next_back().unwrap_or("")
                    )),
                    media_type: None,
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

        // post_id is the AT URI (e.g., at://did:plc:.../app.bsky.feed.post/...)
        let thread = agent
            .api
            .app
            .bsky
            .feed
            .get_post_thread(
                atrium_api::app::bsky::feed::get_post_thread::ParametersData {
                    uri: post_id.to_string(),
                    depth: Some(
                        atrium_api::types::LimitedU16::try_from(depth as u16)
                            .unwrap_or(atrium_api::types::LimitedU16::MAX),
                    ),
                    parent_height: None,
                }
                .into(),
            )
            .await
            .map_err(|e| PlatformError::Api(format!("Failed to get thread: {}", e)))?;

        // Extract replies from the thread
        match &thread.data.thread {
            Union::Refs(OutputThreadRefs::AppBskyFeedDefsThreadViewPost(thread_view)) => {
                Ok(self.extract_replies(&thread_view.data))
            }
            Union::Refs(OutputThreadRefs::AppBskyFeedDefsBlockedPost(_)) => {
                // Post is blocked, return empty
                Ok(Vec::new())
            }
            Union::Refs(OutputThreadRefs::AppBskyFeedDefsNotFoundPost(_)) => {
                // Post not found, return empty
                Ok(Vec::new())
            }
            Union::Unknown(_) => Ok(Vec::new()),
        }
    }

    async fn create_post(&self, text: &str) -> Result<(), PlatformError> {
        let agent = self.agent.read().await;

        agent
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

        Ok(())
    }

    async fn reply_to_post(&self, post_id: &str, text: &str) -> Result<(), PlatformError> {
        // post_id is the AT URI of the parent post
        // We need to get the CID and root info for the reply reference
        let (parent_cid, root_info) = self.get_post_info(post_id).await?;

        // For replies, we need both parent and root references
        // If the parent is itself a reply, use its root; otherwise parent == root
        let (root_uri, root_cid) =
            root_info.unwrap_or_else(|| (post_id.to_string(), parent_cid.clone()));

        let reply_ref = ReplyRefData {
            parent: StrongRef {
                cid: parent_cid
                    .parse()
                    .map_err(|e| PlatformError::Api(format!("Invalid parent CID: {}", e)))?,
                uri: post_id.to_string(),
            }
            .into(),
            root: StrongRef {
                cid: root_cid
                    .parse()
                    .map_err(|e| PlatformError::Api(format!("Invalid root CID: {}", e)))?,
                uri: root_uri,
            }
            .into(),
        };

        let agent = self.agent.read().await;

        agent
            .create_record(RecordData {
                created_at: Datetime::now(),
                embed: None,
                entities: None,
                facets: None,
                labels: None,
                langs: None,
                reply: Some(reply_ref.into()),
                tags: None,
                text: text.to_string(),
            })
            .await
            .map_err(|e| PlatformError::Api(format!("Failed to create reply: {}", e)))?;

        Ok(())
    }
}
