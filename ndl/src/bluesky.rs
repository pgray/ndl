use async_trait::async_trait;
use atrium_api::app::bsky::feed::defs::{
    FeedViewPostReasonRefs, PostView, PostViewEmbedRefs, ThreadViewPostData,
    ThreadViewPostRepliesItem,
};
use atrium_api::app::bsky::feed::get_post_thread::OutputThreadRefs;
use atrium_api::app::bsky::feed::like::RecordData as LikeRecordData;
use atrium_api::app::bsky::feed::post::{RecordData, ReplyRefData};
use atrium_api::com::atproto::repo::strong_ref::MainData as StrongRef;
use atrium_api::types::Union;
use atrium_api::types::string::Datetime;
use bsky_sdk::BskyAgent;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::platform::{PlatformError, Post, PostStats, ReplyThread, SocialClient};

/// Text of a post record. Image-only and quote-only posts carry an empty
/// string rather than no text, so treat blank text as absent.
fn record_text(record: &atrium_api::types::Unknown) -> Option<String> {
    serde_json::to_value(record)
        .ok()
        .and_then(|v| v.get("text").and_then(|t| t.as_str()).map(String::from))
        .filter(|t| !t.trim().is_empty())
}

/// Coarse media type from a post's embed, using labels the TUI already knows
fn embed_media_type(embed: Option<&Union<PostViewEmbedRefs>>) -> Option<String> {
    let label = match embed? {
        Union::Refs(PostViewEmbedRefs::AppBskyEmbedImagesView(_)) => "IMAGE",
        Union::Refs(PostViewEmbedRefs::AppBskyEmbedVideoView(_)) => "VIDEO",
        Union::Refs(PostViewEmbedRefs::AppBskyEmbedExternalView(_)) => "LINK",
        Union::Refs(PostViewEmbedRefs::AppBskyEmbedRecordView(_))
        | Union::Refs(PostViewEmbedRefs::AppBskyEmbedRecordWithMediaView(_)) => "QUOTE",
        Union::Unknown(_) => return None,
    };
    Some(label.to_string())
}

/// Engagement counter from the API (absent or negative counts read as zero)
fn count(n: Option<i64>) -> u64 {
    n.and_then(|n| u64::try_from(n).ok()).unwrap_or(0)
}

/// Convert a Bluesky post view into the platform-agnostic Post
fn post_from_view(post_view: &PostView, reposted: bool) -> Post {
    let handle = post_view.author.handle.as_str();
    Post {
        id: post_view.uri.to_string(),
        text: record_text(&post_view.record),
        author_handle: Some(handle.to_string()),
        timestamp: Some(post_view.indexed_at.as_ref().to_string()),
        permalink: Some(format!(
            "https://bsky.app/profile/{}/post/{}",
            handle,
            post_view.uri.split('/').next_back().unwrap_or("")
        )),
        media_type: embed_media_type(post_view.embed.as_ref()),
        liked: post_view.viewer.as_ref().is_some_and(|v| v.like.is_some()),
        reposted,
        stats: Some(PostStats {
            likes: count(post_view.like_count),
            replies: count(post_view.reply_count),
            reposts: count(post_view.repost_count),
            quotes: count(post_view.quote_count),
            shares: None,
        }),
    }
}

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
                let post = post_from_view(&thread_post.data.post, false);

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
                // In the author feed a repost shows up as the original post
                // with a repost reason attached.
                let reposted = matches!(
                    feed_view.reason,
                    Some(Union::Refs(FeedViewPostReasonRefs::ReasonRepost(_)))
                );
                post_from_view(&feed_view.post, reposted)
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

    async fn like_post(&self, post_id: &str) -> Result<(), PlatformError> {
        // post_id is the AT URI; a like record needs a strong ref (uri + cid)
        let (cid, _) = self.get_post_info(post_id).await?;

        let subject = StrongRef {
            cid: cid
                .parse()
                .map_err(|e| PlatformError::Api(format!("Invalid CID: {}", e)))?,
            uri: post_id.to_string(),
        };

        let agent = self.agent.read().await;

        agent
            .create_record(LikeRecordData {
                created_at: Datetime::now(),
                subject: subject.into(),
                via: None,
            })
            .await
            .map_err(|e| PlatformError::Api(format!("Failed to like post: {}", e)))?;

        Ok(())
    }

    async fn get_follower_count(&self) -> Result<Option<u64>, PlatformError> {
        let agent = self.agent.read().await;

        let session = agent
            .get_session()
            .await
            .ok_or_else(|| PlatformError::Auth("No active session".to_string()))?;
        let did = session.did.clone();

        let profile = agent
            .api
            .app
            .bsky
            .actor
            .get_profile(
                atrium_api::app::bsky::actor::get_profile::ParametersData { actor: did.into() }
                    .into(),
            )
            .await
            .map_err(|e| PlatformError::Api(format!("Failed to get profile: {}", e)))?;

        Ok(Some(count(profile.data.followers_count)))
    }
}
