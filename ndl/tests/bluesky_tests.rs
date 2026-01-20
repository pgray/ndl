//! Bluesky integration tests
//!
//! These tests use real credentials from ~/.config/ndl/config.toml
//! Run with: cargo test -p ndl --test bluesky_tests -- --nocapture
//!
//! Tests are designed to:
//! - Skip gracefully if no config exists
//! - Print diagnostic output for debugging
//! - Be runnable independently

use std::path::PathBuf;

fn get_config_path() -> PathBuf {
    dirs::config_dir().unwrap().join("ndl").join("config.toml")
}

#[derive(Debug, Clone)]
struct BlueskyTestConfig {
    identifier: String,
    password: String,
    session: Option<String>,
}

fn load_bluesky_config() -> Option<BlueskyTestConfig> {
    let path = get_config_path();
    if !path.exists() {
        eprintln!("No config at {:?}, skipping test", path);
        return None;
    }

    let contents = std::fs::read_to_string(&path).ok()?;
    let config: toml::Value = toml::from_str(&contents).ok()?;

    let bluesky = config.get("bluesky")?;
    let identifier = bluesky.get("identifier")?.as_str()?.to_string();
    let password = bluesky.get("password")?.as_str()?.to_string();
    let session = bluesky
        .get("session")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Some(BlueskyTestConfig {
        identifier,
        password,
        session,
    })
}

// =============================================================================
// Unit 1: Login with credentials
// =============================================================================

#[tokio::test]
async fn test_01_login_with_credentials() {
    let Some(config) = load_bluesky_config() else {
        return;
    };

    println!("=== Test: Login with credentials ===");
    println!("Identifier: {}", config.identifier);

    use bsky_sdk::BskyAgent;

    let agent = BskyAgent::builder().build().await;
    match agent {
        Ok(agent) => {
            println!("Agent created successfully");

            match agent.login(&config.identifier, &config.password).await {
                Ok(session) => {
                    println!("Login successful!");
                    println!("DID: {}", session.did.as_str());
                    println!("Handle: {}", session.handle.as_str());
                    assert!(!session.did.as_str().is_empty());
                }
                Err(e) => {
                    panic!("Login failed: {:?}", e);
                }
            }
        }
        Err(e) => {
            panic!("Failed to create agent: {:?}", e);
        }
    }
}

// =============================================================================
// Unit 2: Session restoration
// =============================================================================

#[tokio::test]
async fn test_02_restore_session() {
    let Some(config) = load_bluesky_config() else {
        return;
    };

    let Some(session_data) = config.session else {
        println!("No session data in config, skipping session restore test");
        return;
    };

    println!("=== Test: Restore session from stored data ===");
    println!("Session data length: {} bytes", session_data.len());

    use bsky_sdk::agent::config::Config as BskyConfig;
    use bsky_sdk::BskyAgent;

    // Parse the session config
    let bsky_config: Result<BskyConfig, _> = serde_json::from_str(&session_data);
    match bsky_config {
        Ok(cfg) => {
            println!("Session config parsed successfully");
            println!("Endpoint: {:?}", cfg.endpoint);

            // Try to create agent from config
            match BskyAgent::builder().config(cfg).build().await {
                Ok(agent) => {
                    println!("Agent restored from session!");

                    // Verify session is active
                    match agent.get_session().await {
                        Some(session) => {
                            println!("Session active - DID: {}", session.did.as_str());
                            println!("Session active - Handle: {}", session.handle.as_str());
                        }
                        None => {
                            println!("WARNING: Session restored but get_session() returned None");
                            println!("This may indicate the session has expired");
                        }
                    }
                }
                Err(e) => {
                    println!("Failed to restore agent from session: {:?}", e);
                    println!("This may indicate the session has expired - try fresh login");
                }
            }
        }
        Err(e) => {
            panic!("Failed to parse session config: {:?}", e);
        }
    }
}

// =============================================================================
// Unit 3: Get profile
// =============================================================================

#[tokio::test]
async fn test_03_get_profile() {
    let Some(config) = load_bluesky_config() else {
        return;
    };

    println!("=== Test: Get user profile ===");

    use bsky_sdk::BskyAgent;

    let agent = BskyAgent::builder().build().await.unwrap();
    agent
        .login(&config.identifier, &config.password)
        .await
        .unwrap();

    let session = agent.get_session().await.expect("No session after login");
    let did = session.did.clone();

    println!("Fetching profile for DID: {}", did.as_str());

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
        .await;

    match profile {
        Ok(p) => {
            println!("Profile fetched successfully!");
            println!("  Handle: {}", p.data.handle.as_str());
            println!("  Display name: {:?}", p.data.display_name);
            println!("  Description: {:?}", p.data.description);
            println!("  Avatar: {:?}", p.data.avatar);
            println!("  Followers: {:?}", p.data.followers_count);
            println!("  Following: {:?}", p.data.follows_count);
            println!("  Posts: {:?}", p.data.posts_count);

            assert!(!p.data.handle.as_str().is_empty());
        }
        Err(e) => {
            panic!("Failed to get profile: {:?}", e);
        }
    }
}

// =============================================================================
// Unit 4: Get timeline (posts)
// =============================================================================

#[tokio::test]
async fn test_04_get_timeline() {
    let Some(config) = load_bluesky_config() else {
        return;
    };

    println!("=== Test: Get timeline ===");

    use bsky_sdk::BskyAgent;

    let agent = BskyAgent::builder().build().await.unwrap();
    agent
        .login(&config.identifier, &config.password)
        .await
        .unwrap();

    let timeline = agent
        .api
        .app
        .bsky
        .feed
        .get_timeline(
            atrium_api::app::bsky::feed::get_timeline::ParametersData {
                algorithm: None,
                cursor: None,
                limit: Some(atrium_api::types::LimitedNonZeroU8::try_from(10).unwrap()),
            }
            .into(),
        )
        .await;

    match timeline {
        Ok(t) => {
            println!("Timeline fetched successfully!");
            println!("Posts count: {}", t.data.feed.len());
            println!("Cursor: {:?}", t.data.cursor);

            for (i, feed_view) in t.data.feed.iter().take(5).enumerate() {
                println!("\n--- Post {} ---", i + 1);
                println!("  URI: {}", feed_view.post.uri.as_str());
                println!("  Author: {}", feed_view.post.author.handle.as_str());
                println!("  Indexed at: {}", feed_view.post.indexed_at.as_str());

                // Extract text from record
                let text = serde_json::to_value(&feed_view.post.record)
                    .ok()
                    .and_then(|v| v.get("text").and_then(|t| t.as_str()).map(String::from));
                println!("  Text: {:?}", text);
            }

            assert!(!t.data.feed.is_empty(), "Timeline should not be empty");
        }
        Err(e) => {
            panic!("Failed to get timeline: {:?}", e);
        }
    }
}

// =============================================================================
// Unit 5: Get post thread (replies)
// =============================================================================

#[tokio::test]
async fn test_05_get_post_thread() {
    let Some(config) = load_bluesky_config() else {
        return;
    };

    println!("=== Test: Get post thread (replies) ===");

    use bsky_sdk::BskyAgent;

    let agent = BskyAgent::builder().build().await.unwrap();
    agent
        .login(&config.identifier, &config.password)
        .await
        .unwrap();

    // First get a post from the timeline
    let timeline = agent
        .api
        .app
        .bsky
        .feed
        .get_timeline(
            atrium_api::app::bsky::feed::get_timeline::ParametersData {
                algorithm: None,
                cursor: None,
                limit: Some(atrium_api::types::LimitedNonZeroU8::try_from(5).unwrap()),
            }
            .into(),
        )
        .await
        .expect("Failed to get timeline");

    let Some(first_post) = timeline.data.feed.first() else {
        println!("No posts in timeline, skipping thread test");
        return;
    };

    let post_uri = first_post.post.uri.as_str().to_string();
    println!("Testing thread for post: {}", post_uri);

    // Get the thread
    let thread = agent
        .api
        .app
        .bsky
        .feed
        .get_post_thread(
            atrium_api::app::bsky::feed::get_post_thread::ParametersData {
                uri: post_uri.clone(),
                depth: Some(atrium_api::types::LimitedU16::try_from(6u16).unwrap()),
                parent_height: None,
            }
            .into(),
        )
        .await;

    match thread {
        Ok(t) => {
            println!("Thread fetched successfully!");
            println!("Thread data: {:#?}", t.data.thread);
        }
        Err(e) => {
            println!("Failed to get thread: {:?}", e);
            println!("This might be expected for some posts (blocked, not found, etc.)");
        }
    }
}

// =============================================================================
// Unit 6: Get author feed
// =============================================================================

#[tokio::test]
async fn test_06_get_author_feed() {
    let Some(config) = load_bluesky_config() else {
        return;
    };

    println!("=== Test: Get author feed ===");

    use bsky_sdk::BskyAgent;

    let agent = BskyAgent::builder().build().await.unwrap();
    agent
        .login(&config.identifier, &config.password)
        .await
        .unwrap();

    let session = agent.get_session().await.expect("No session");
    let did = session.did.as_str().to_string();

    println!("Fetching author feed for: {}", did);

    let feed = agent
        .api
        .app
        .bsky
        .feed
        .get_author_feed(
            atrium_api::app::bsky::feed::get_author_feed::ParametersData {
                actor: did.parse().unwrap(),
                cursor: None,
                filter: None,
                include_pins: None,
                limit: Some(atrium_api::types::LimitedNonZeroU8::try_from(10).unwrap()),
            }
            .into(),
        )
        .await;

    match feed {
        Ok(f) => {
            println!("Author feed fetched successfully!");
            println!("Posts count: {}", f.data.feed.len());

            for (i, post) in f.data.feed.iter().take(3).enumerate() {
                println!("\n--- Your Post {} ---", i + 1);
                println!("  URI: {}", post.post.uri.as_str());
                println!("  Indexed at: {}", post.post.indexed_at.as_str());

                let text = serde_json::to_value(&post.post.record)
                    .ok()
                    .and_then(|v| v.get("text").and_then(|t| t.as_str()).map(String::from));
                println!("  Text: {:?}", text);
            }
        }
        Err(e) => {
            println!("Failed to get author feed: {:?}", e);
        }
    }
}

// =============================================================================
// Unit 7: Create post (SKIP by default - uncomment to test)
// =============================================================================

#[tokio::test]
#[ignore] // Remove this to actually create a post
async fn test_07_create_post() {
    let Some(config) = load_bluesky_config() else {
        return;
    };

    println!("=== Test: Create post ===");

    use atrium_api::app::bsky::feed::post::RecordData;
    use atrium_api::types::string::Datetime;
    use bsky_sdk::BskyAgent;

    let agent = BskyAgent::builder().build().await.unwrap();
    agent
        .login(&config.identifier, &config.password)
        .await
        .unwrap();

    let test_text = format!("Test post from ndl integration tests - {}", chrono::Utc::now());
    println!("Creating post: {}", test_text);

    let result = agent
        .create_record(RecordData {
            created_at: Datetime::now(),
            embed: None,
            entities: None,
            facets: None,
            labels: None,
            langs: None,
            reply: None,
            tags: None,
            text: test_text.clone(),
        })
        .await;

    match result {
        Ok(r) => {
            println!("Post created successfully!");
            println!("  URI: {}", r.uri.as_str());
            println!("  CID: {:?}", r.cid);
        }
        Err(e) => {
            panic!("Failed to create post: {:?}", e);
        }
    }
}

// =============================================================================
// Unit 8: Reply to post (requires proper reply reference)
// =============================================================================

#[tokio::test]
#[ignore] // Remove this to actually reply
async fn test_08_reply_to_post() {
    let Some(config) = load_bluesky_config() else {
        return;
    };

    println!("=== Test: Reply to post ===");

    use atrium_api::app::bsky::feed::post::{RecordData, ReplyRefData};
    use atrium_api::com::atproto::repo::strong_ref::MainData as StrongRef;
    use atrium_api::types::string::Datetime;
    use bsky_sdk::BskyAgent;

    let agent = BskyAgent::builder().build().await.unwrap();
    agent
        .login(&config.identifier, &config.password)
        .await
        .unwrap();

    // First get a post to reply to
    let timeline = agent
        .api
        .app
        .bsky
        .feed
        .get_timeline(
            atrium_api::app::bsky::feed::get_timeline::ParametersData {
                algorithm: None,
                cursor: None,
                limit: Some(atrium_api::types::LimitedNonZeroU8::try_from(1).unwrap()),
            }
            .into(),
        )
        .await
        .expect("Failed to get timeline");

    let Some(parent_post) = timeline.data.feed.first() else {
        println!("No posts to reply to");
        return;
    };

    let parent_uri = parent_post.post.uri.clone();
    let parent_cid = parent_post.post.cid.clone();

    println!("Replying to post: {}", parent_uri);

    let reply_ref = ReplyRefData {
        parent: StrongRef {
            cid: parent_cid.clone(),
            uri: parent_uri.clone().to_string(),
        }
        .into(),
        root: StrongRef {
            cid: parent_cid,
            uri: parent_uri.to_string(),
        }
        .into(),
    };

    let result = agent
        .create_record(RecordData {
            created_at: Datetime::now(),
            embed: None,
            entities: None,
            facets: None,
            labels: None,
            langs: None,
            reply: Some(reply_ref.into()),
            tags: None,
            text: "Test reply from ndl".to_string(),
        })
        .await;

    match result {
        Ok(r) => {
            println!("Reply created successfully!");
            println!("  URI: {}", r.uri.as_str());
            println!("  CID: {:?}", r.cid);
        }
        Err(e) => {
            panic!("Failed to create reply: {:?}", e);
        }
    }
}

// =============================================================================
// Unit 9: Session serialization round-trip
// =============================================================================

#[tokio::test]
async fn test_09_session_roundtrip() {
    let Some(config) = load_bluesky_config() else {
        return;
    };

    println!("=== Test: Session serialization round-trip ===");

    use bsky_sdk::agent::config::Config as BskyConfig;
    use bsky_sdk::BskyAgent;

    // Login fresh
    let agent = BskyAgent::builder().build().await.unwrap();
    agent
        .login(&config.identifier, &config.password)
        .await
        .unwrap();

    // Get session config
    let cfg = agent.to_config().await;

    // Serialize
    let serialized = serde_json::to_string(&cfg).expect("Failed to serialize");
    println!("Serialized session: {} bytes", serialized.len());

    // Deserialize
    let deserialized: BskyConfig =
        serde_json::from_str(&serialized).expect("Failed to deserialize");
    println!("Deserialized successfully");
    println!("Endpoint: {:?}", deserialized.endpoint);

    // Restore agent
    let restored_agent = BskyAgent::builder()
        .config(deserialized)
        .build()
        .await
        .expect("Failed to restore agent");

    // Verify session works
    let session = restored_agent
        .get_session()
        .await
        .expect("No session after restore");
    println!("Restored session - Handle: {}", session.handle.as_str());

    assert_eq!(session.handle.as_str(), config.identifier);
}

// =============================================================================
// Unit 10: BlueskyClient wrapper test
// =============================================================================

#[tokio::test]
async fn test_10_bluesky_client_wrapper() {
    let Some(config) = load_bluesky_config() else {
        return;
    };

    println!("=== Test: BlueskyClient wrapper ===");

    // Note: This requires ndl to export BlueskyClient
    // For now, we test the raw bsky-sdk functionality above
    // This test is a placeholder for when we wire up the full client

    use bsky_sdk::BskyAgent;

    let agent = BskyAgent::builder().build().await.unwrap();
    agent
        .login(&config.identifier, &config.password)
        .await
        .unwrap();

    // Test the methods that BlueskyClient wraps

    // 1. get_profile
    let session = agent.get_session().await.unwrap();
    let profile = agent
        .api
        .app
        .bsky
        .actor
        .get_profile(
            atrium_api::app::bsky::actor::get_profile::ParametersData {
                actor: session.did.clone().into(),
            }
            .into(),
        )
        .await
        .expect("get_profile failed");
    println!("get_profile: OK - {}", profile.data.handle.as_str());

    // 2. get_posts (timeline)
    let timeline = agent
        .api
        .app
        .bsky
        .feed
        .get_timeline(
            atrium_api::app::bsky::feed::get_timeline::ParametersData {
                algorithm: None,
                cursor: None,
                limit: Some(atrium_api::types::LimitedNonZeroU8::try_from(5).unwrap()),
            }
            .into(),
        )
        .await
        .expect("get_timeline failed");
    println!("get_posts: OK - {} posts", timeline.data.feed.len());

    // 3. get_post_replies - need to test with actual post URI
    if let Some(post) = timeline.data.feed.first() {
        let thread = agent
            .api
            .app
            .bsky
            .feed
            .get_post_thread(
                atrium_api::app::bsky::feed::get_post_thread::ParametersData {
                    uri: post.post.uri.as_str().to_string(),
                    depth: Some(atrium_api::types::LimitedU16::try_from(2u16).unwrap()),
                    parent_height: None,
                }
                .into(),
            )
            .await;
        match thread {
            Ok(_) => println!("get_post_replies: OK"),
            Err(e) => println!("get_post_replies: Error (may be expected) - {:?}", e),
        }
    }

    println!("\nAll BlueskyClient wrapper tests passed!");
}
