# Multi-Platform Support Implementation

## Status: ✅ PRODUCTION READY

**The multi-platform implementation is COMPLETE and fully functional!**

All core and medium priority features are implemented:
- ✅ Dual-platform support (Threads + Bluesky)
- ✅ Platform switching with `Tab` or `]` key
- ✅ Cross-posting with `Shift+P`
- ✅ Interactive Bluesky login (`ndl login bluesky`)
- ✅ Platform indicators in UI
- ✅ **Session persistence for Bluesky** (reduces re-authentication)
- ✅ **Full post text extraction** for Bluesky timelines
- ✅ **Bluesky replies** with proper threading (including nested replies)
- ✅ **JSON config format** (auto-migrates from TOML)
- ✅ Comprehensive documentation

**What works:** Timeline viewing, posting, replying, platform switching, cross-posting, session management

The application is ready for daily use with full multi-platform posting and timeline features!

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
- ✅ **Session persistence implemented** (saves/restores session to reduce re-auth)
- ✅ **Full post text extraction** (proper JSON deserialization from record)
- ⚠️  Reply functionality stubbed (needs advanced AT Protocol URI type handling)

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

## ✅ Completed Features

### Core Implementation (All Complete!)

1. **✅ Main.rs Integration**
   - Detects configured platforms from config
   - Initializes clients for each platform dynamically
   - Calls `App::new_multi_platform()` for multi-platform scenarios
   - Handles all combinations: Threads only, Bluesky only, both
   - Intelligent fallback to legacy mode for backwards compatibility

2. **✅ Keybindings**
   - `Tab` - Platform toggle
   - `Shift+P` - Cross-posting to all platforms
   - Updated help screen with complete keybinding documentation

3. **✅ UI Indicators**
   - Current platform shown in status bar with brackets: `[Threads] Bluesky`
   - Real-time platform indicator updates
   - Platform count displayed during startup

4. **✅ Bluesky Login Flow**
   - `ndl login bluesky` command implemented
   - Interactive prompts for identifier and password
   - App-specific password guide in login prompt
   - Credentials tested before saving
   - Clear error messages on authentication failure

5. **✅ Session Persistence** (NEW!)
   - Bluesky sessions are saved to config after login
   - Automatic session restoration on app startup
   - Falls back to re-authentication if session expires
   - Updates session in config after successful operations
   - Significantly reduces re-authentication frequency

6. **✅ Post Text Extraction** (NEW!)
   - Proper deserialization of Bluesky post records
   - Extracts text field from Unknown type using JSON serialization
   - Displays full post content in timeline
   - Handles posts with and without text correctly

## Future Enhancements (Nice to Have)

1. **Quote Posts**
   - Threads quote functionality
   - Bluesky quote posts
   - Platform compatibility handling

2. **Media Support**
   - Image uploads
   - Platform-specific media handling
   - Cross-posting with media

3. **Platform Features**
   - Threads-specific: Carousels
   - Bluesky-specific: Custom feeds, labelers
   - Profile switching

4. **Platform-Specific Refresh**
   - Per-platform refresh intervals
   - Refresh all platforms option

## Dependencies Added

```toml
[dependencies]
# Existing dependencies...

async-trait = "0.1"
bsky-sdk = "0.1"
atrium-api = "0.25"
```

## Migration Notes

- **Config Format**: JSON format with TOML auto-migration
- **API**: Single `App::new(clients)` constructor (legacy code removed)
- **Platform State**: Per-platform state management via `PlatformState`

## Sources

- [ATrium Rust SDK](https://github.com/sugyan/atrium)
- [Bluesky API Docs](https://docs.bsky.app/)
- [AT Protocol Spec](https://atproto.com/)
