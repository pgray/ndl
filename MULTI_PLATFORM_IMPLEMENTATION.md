# Multi-Platform Support Implementation

## Overview

This document outlines the implementation of multi-platform support for ndl, allowing it to support both Threads and Bluesky simultaneously with platform toggling and cross-posting capabilities.

## Architecture

### Platform Abstraction Layer

**Location:** `ndl/src/platform.rs`

The platform abstraction provides a unified interface for interacting with different social media platforms:

```rust
pub trait SocialClient: Send + Sync {
    fn platform(&self) -> Platform;
    async fn get_profile(&self) -> Result<UserProfile, PlatformError>;
    async fn get_posts(&self, limit: Option<u32>) -> Result<Vec<Post>, PlatformError>;
    async fn get_post_replies(&self, post_id: &str, depth: u8) -> Result<Vec<ReplyThread>, PlatformError>;
    async fn create_post(&self, text: &str) -> Result<PostResult, PlatformError>;
    async fn reply_to_post(&self, post_id: &str, text: &str) -> Result<PostResult, PlatformError>;
    fn clone_client(&self) -> Box<dyn SocialClient>;
}
```

**Key Types:**
- `Platform` enum: Identifies the platform (Threads or Bluesky)
- `Post`: Platform-agnostic post representation
- `UserProfile`: Platform-agnostic user profile
- `ReplyThread`: Recursive reply structure
- `PlatformError`: Unified error type

### Platform Implementations

#### Threads (ndl/src/api.rs)
- ✅ Implemented `SocialClient` trait for `ThreadsClient`
- ✅ Converts Threads-specific types to platform-agnostic types
- ✅ Fully functional with existing OAuth flow

#### Bluesky (ndl/src/bluesky.rs)
- ✅ Created `BlueskyClient` struct with `bsky-sdk` integration
- ✅ Implemented login via identifier/password
- ✅ Implemented timeline fetching
- ✅ Implemented post creation
- ⚠️  Reply functionality stubbed (needs proper AT Protocol URI handling)
- ⚠️  Session persistence not yet implemented
- ⚠️  Post text extraction simplified (TODO: proper record deserialization)

### Configuration (ndl/src/config.rs)

Extended config to support multiple platforms:

```toml
# Threads credentials
access_token = "..."
client_id = "..."
client_secret = "..."
auth_server = "..."

# Bluesky credentials
[bluesky]
identifier = "username.bsky.social"
password = "..."
session = "..." # Optional: for session persistence
```

**Helper methods:**
- `has_bluesky()` - Check if Bluesky is configured
- `has_threads()` - Check if Threads is configured

### TUI Enhancements (ndl/src/tui.rs)

#### Multi-Platform State Management

```rust
pub struct PlatformState {
    pub posts: Vec<Post>,
    pub list_state: ListState,
    pub selected_replies: Vec<PlatformReplyThread>,
    pub loaded_replies_for: Option<String>,
    pub reply_selection: Option<usize>,
}

pub struct App {
    // Multi-platform support
    pub current_platform: Platform,
    pub clients: HashMap<Platform, Arc<Box<dyn SocialClient>>>,
    pub platform_states: HashMap<Platform, PlatformState>,

    // Legacy fields for backwards compatibility
    // ...
}
```

#### New Features

1. **Platform Toggling**
   - `toggle_platform()` - Switch between configured platforms
   - Maintains separate state for each platform

2. **Cross-Posting**
   - New `InputMode::CrossPosting`
   - `send_cross_post()` - Post to all platforms simultaneously
   - Platform-specific success/failure reporting

3. **Platform-Aware Events**
   - `AppEvent::PostsUpdated(Platform, Vec<Post>)`
   - `AppEvent::PlatformPostResult(Platform, Result<(), String>)`
   - `AppEvent::PlatformReplyResult(Platform, Result<(), String>)`

## What's Implemented ✅

1. ✅ **Platform Abstraction Layer**
   - Trait definition for unified API
   - Platform-agnostic data models
   - Error handling

2. ✅ **Threads Integration**
   - Full SocialClient implementation
   - Backwards compatible with existing code

3. ✅ **Bluesky Client**
   - Basic authentication (login)
   - Timeline fetching
   - Post creation
   - Platform integration

4. ✅ **Configuration**
   - Multi-platform credentials storage
   - Backwards compatible config format

5. ✅ **TUI Infrastructure**
   - Multi-platform state management
   - Cross-posting logic
   - Platform-aware event handling
   - Input modes for cross-posting

## What's Not Yet Done ⚠️

### High Priority

1. **Main.rs Integration**
   - Need to detect configured platforms from config
   - Initialize clients for each platform
   - Call `App::new_multi_platform()` instead of `App::new()`
   - Handle mixed scenarios (Threads only, Bluesky only, both)

2. **Keybindings**
   - Add key for platform toggle (e.g., `Tab` or `P`)
   - Add key for cross-posting (e.g., `Shift+P` or `C`)
   - Update help screen with new keybindings

3. **UI Indicators**
   - Show current platform in status bar
   - Show platform badges on posts
   - Indicate when cross-posting is available

4. **Bluesky Login Flow**
   - Add `login bluesky` command to main.rs
   - Prompt for identifier and password
   - Save credentials to config
   - Optional: App-specific password generation guide

### Medium Priority

5. **Bluesky Reply Support**
   - Implement proper AT Protocol URI parsing
   - Fetch parent post CID for reply references
   - Handle reply threading

6. **Session Persistence**
   - Implement Bluesky session save/restore
   - Reduce re-authentication frequency
   - Use bsky-sdk's FileStore or similar

7. **Post Text Extraction**
   - Properly deserialize Bluesky post records
   - Extract facets (mentions, links, etc.)
   - Handle rich text formatting

8. **Platform-Specific Refresh**
   - Background refresh for active platform
   - Refresh all platforms option
   - Platform-specific refresh intervals

### Nice to Have

9. **Quote Posts**
   - Threads quote functionality
   - Bluesky quote posts
   - Platform compatibility handling

10. **Media Support**
    - Image uploads
    - Platform-specific media handling
    - Cross-posting with media

11. **Platform Features**
    - Threads-specific: Carousels
    - Bluesky-specific: Custom feeds, labelers
    - Profile switching

## How to Complete Implementation

### Step 1: Wire Up Multi-Platform in Main

```rust
// In ndl/src/main.rs

async fn run_tui() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::load()?;

    let mut clients: HashMap<Platform, Box<dyn SocialClient>> = HashMap::new();

    // Initialize Threads if configured
    if let Some(token) = config.access_token.clone() {
        let client = ThreadsClient::new(token);
        clients.insert(Platform::Threads, Box::new(client));
    }

    // Initialize Bluesky if configured
    if let Some(bsky_config) = config.bluesky.clone() {
        match BlueskyClient::login(&bsky_config.identifier, &bsky_config.password).await {
            Ok(client) => {
                clients.insert(Platform::Bluesky, Box::new(client));
            }
            Err(e) => {
                eprintln!("Warning: Bluesky login failed: {}", e);
            }
        }
    }

    if clients.is_empty() {
        eprintln!("No platforms configured. Run 'ndl login' or 'ndl login bluesky' first.");
        return Ok(());
    }

    let mut app = App::new_multi_platform(clients);

    // Start refresh tasks for each platform
    for platform in app.clients.keys() {
        app.start_platform_refresh(*platform);
    }

    app.run().await?;
    Ok(())
}
```

### Step 2: Add Platform Refresh

```rust
// In ndl/src/tui.rs

impl App {
    fn start_platform_refresh(&self, platform: Platform) {
        if let Some(client) = self.clients.get(&platform) {
            let client = client.clone();
            let tx = self.event_tx.clone();

            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(15)).await;

                    if let Ok(posts) = client.get_posts(Some(25)).await {
                        let _ = tx.send(AppEvent::PostsUpdated(platform, posts)).await;
                    }
                }
            });
        }
    }
}
```

### Step 3: Add Keybindings

```rust
// In handle_normal_input() method

KeyCode::Tab => {
    self.toggle_platform();
}
KeyCode::Char('P') => {  // Shift+P for cross-post
    self.input_mode = InputMode::CrossPosting;
    self.input_buffer.clear();
}
```

### Step 4: Update UI Display

```rust
// In draw_threads_list() or draw_status_bar()

// Show current platform
let platform_indicator = format!(" [{}] ", self.current_platform);

// Show available platforms
let platforms: Vec<String> = self.clients.keys()
    .map(|p| p.to_string())
    .collect();
let platforms_str = platforms.join(" | ");
```

## Testing Plan

1. **Single Platform Tests**
   - Test with Threads only
   - Test with Bluesky only
   - Verify backwards compatibility

2. **Multi-Platform Tests**
   - Test platform switching
   - Test cross-posting
   - Test per-platform state isolation

3. **Error Handling**
   - Test with invalid credentials
   - Test network failures
   - Test API errors per platform

4. **Edge Cases**
   - No platforms configured
   - One platform fails authentication
   - Cross-post to offline platform

## Dependencies Added

```toml
[dependencies]
# Existing dependencies...

async-trait = "0.1"
bsky-sdk = "0.1"
atrium-api = "0.25"
```

## Migration Notes

- **Backwards Compatible**: Existing single-platform usage still works
- **Config Format**: Extended, but old configs remain valid
- **API**: `App::new()` still exists for legacy code

## Future Enhancements

- Mastodon support
- Twitter/X support (if API available)
- Custom platform plugins
- Platform-specific filters
- Cross-platform analytics

## Sources

- [ATrium Rust SDK](https://github.com/sugyan/atrium)
- [Bluesky API Docs](https://docs.bsky.app/)
- [AT Protocol Spec](https://atproto.com/)
