# Plan: Add Like/Heart Count Display

## Overview
Add like/heart count display to posts in the ndl TUI client for both Threads and Bluesky platforms.

## 🔑 Key Research Findings

### Bluesky: ✅ EASY - Data Already Available
- Like counts are **already included** in API responses (`feed_view.post.like_count`)
- **No additional API calls** needed
- Just need to extract the field and display it
- Also includes: `reply_count`, `repost_count`, `quote_count`, `bookmark_count`

### Threads: ⚠️ COMPLEX - Requires Separate API Calls
- Like counts are **NOT in the main media response**
- Requires **separate Insights API calls** per thread: `GET /{thread_id}/insights?metric=likes`
- **Performance concern**: N+1 queries (1 for thread list + N for each thread's insights)
- **Recommendation**: Start by showing like counts for Bluesky only, add Threads later with hybrid approach

### Implementation Recommendation
1. **Phase 1** (Quick Win): Add like counts for **Bluesky only**
   - Update `Post` struct with optional `like_count` field
   - Extract data in `bluesky.rs` (2 small code changes)
   - Update TUI to display counts (handle `None` for Threads)

2. **Phase 2** (Optional): Add Threads support with hybrid approach
   - Fetch insights only for the focused/selected thread
   - Show "-" for unfocused threads to avoid excessive API calls

## Current State
- `Post` struct in `platform.rs:34-43` does not include like count field
- Threads API requests (`api.rs:116`) only fetch: `id,text,username,timestamp,media_type,permalink`
- Bluesky feed parsing (`bluesky.rs:204-228`) does not extract like counts
- TUI (`tui.rs`) does not display like information

## Investigation Phase ✅ COMPLETED

### 1. Research Threads Graph API ✅
- ✅ **Threads API provides like counts via Insights API**
- ✅ **Metric name**: `"likes"` (not "like_count")
- ✅ **Access method**: Separate Insights endpoint, not a direct field
- ✅ **Endpoint**: `GET https://graph.threads.net/v1.0/{media_id}/insights?metric=likes,views,replies,reposts,quotes`
- ⚠️ **IMPORTANT**: This requires a separate API call per thread to get insights
- 📚 **Sources**: [Threads API Documentation](https://www.postman.com/meta/threads/documentation/dht3nzz/threads-api), [Meta Threads API Features](https://data365.co/threads)

### 2. Research Bluesky AT Protocol ✅
- ✅ **Field name**: `like_count` (integer, optional)
- ✅ **Available in**: `PostView` struct from `app.bsky.feed.defs` lexicon
- ✅ **Access path**: `feed_view.post.like_count` (also includes `reply_count`, `repost_count`, `quote_count`, `bookmark_count`)
- ✅ **Already included** in both `get_author_feed` and `get_post_thread` responses
- ✅ **atrium-api version**: 0.25 (confirmed in Cargo.toml)
- 📚 **Sources**: [AT Protocol Lexicon](https://github.com/bluesky-social/atproto/blob/main/lexicons/app/bsky/feed/defs.json), [atrium-api docs](https://docs.rs/atrium-api/latest/atrium_api/)

## Implementation Phase

### 3. Update Core Data Model (`platform.rs`)
- [ ] Add `like_count: Option<u32>` field to `Post` struct (line 34-43)
- [ ] Consider rename to `engagement_count` if platforms differ significantly

### 4. Update Threads Client (`api.rs`)

⚠️ **DECISION REQUIRED**: Threads requires separate API calls to get insights (like counts). Options:

**Option A: Skip like counts for Threads initially**
- Pro: No additional API calls, simpler implementation
- Pro: Avoid rate limiting concerns
- Con: Feature parity with Bluesky lost
- Implementation: Just add `like_count: None` for all Threads posts

**Option B: Fetch insights separately (adds API calls)**
- [ ] Add new method `get_thread_insights(&self, thread_id: &str)` to fetch likes
- [ ] Call insights API: `GET /{thread_id}/insights?metric=likes`
- [ ] Parse response to extract likes count
- [ ] Update `get_threads()` to optionally fetch insights (batch or individual)
- [ ] Update `get_thread_replies_nested()` to fetch insights for replies
- [ ] Update `SocialClient` implementation to populate like_count
- ⚠️ **Performance impact**: N+1 API calls (1 for threads list + N for each thread's insights)
- Pro: Full feature parity with Bluesky
- Con: Significantly more API calls, potential rate limiting

**Option C: Hybrid approach**
- Fetch insights only for the selected/focused thread
- Show "-" or blank for threads in the list
- Implementation: Add `get_thread_insights()` method, call it only in detail view

**RECOMMENDATION**: Start with Option A, then implement Option C if needed

### 5. Update Bluesky Client (`bluesky.rs`) - STRAIGHTFORWARD ✅

**Implementation details** (data already available, just need to extract it):

- [ ] In `get_posts()` at line 215-226, add:
  ```rust
  like_count: feed_view.post.like_count.map(|c| c as u32),
  ```

- [ ] In `convert_reply_item()` at line 94-105, add to Post struct:
  ```rust
  like_count: post_view.like_count.map(|c| c as u32),
  ```

- [ ] No additional API calls needed - data is already in the response
- [ ] The field is optional in the lexicon, so using `.map()` handles missing values gracefully
- [ ] Also available: `reply_count`, `repost_count`, `quote_count` (for future enhancement)

### 6. Update TUI Display (`tui.rs`)
- [ ] Add like count to post list rendering (left panel)
- [ ] Add like count to detail view rendering (right panel)
- [ ] Choose display format (e.g., "❤ 42" or "42 likes")
- [ ] Handle `None` case (e.g., show "❤ -" or hide entirely)
- [ ] Ensure proper spacing/alignment with existing fields

## Testing

### 7. Manual Testing
- [ ] Test Threads posts with various like counts (0, small, large numbers)
- [ ] Test Bluesky posts with various like counts
- [ ] Test posts where like count is not available
- [ ] Test reply threads to ensure nested posts show likes
- [ ] Verify both platforms display correctly
- [ ] Test platform switching maintains correct counts

### 8. Edge Cases
- [ ] Posts with no likes (0 vs None)
- [ ] Posts where API doesn't return like count
- [ ] Very large like counts (formatting)
- [ ] Rapid like count changes (refresh behavior)

## Considerations

- **API Rate Limits**:
  - ✅ **Bluesky**: No additional overhead, data included in existing calls
  - ⚠️ **Threads**: Insights API requires separate calls (1 per thread)
  - Recommendation: Avoid fetching insights for all threads in list view
- **Backwards Compatibility**: Optional `like_count: Option<u32>` field ensures graceful handling
- **Display Formatting**:
  - Consider abbreviated format for large numbers (e.g., "1.2K", "5.3M")
  - Handle `None` case: show "-" or blank for Threads (Phase 1) or unfetched insights
- **Refresh Strategy**: Like counts update on 15-second refresh cycle (existing behavior)
- **Cross-posting**: Like counts are platform-specific (Bluesky will show, Threads won't in Phase 1)
- **Platform Differences**:
  - Bluesky: Real-time like counts always available
  - Threads: Like counts require extra API call (insights endpoint)
  - This asymmetry is acceptable - users will understand Threads limitations

## Future Enhancements (Out of Scope)
- Interactive liking/unliking posts
- Sort posts by like count
- Like count trends/changes indicators
- **Reply counts** (Bluesky: `reply_count`, Threads: available via insights)
- **Repost/quote counts** (Bluesky: `repost_count` + `quote_count`, Threads: `reposts` + `quotes` via insights)
- **View counts** (Bluesky: not available, Threads: `views` via insights)
- **Bookmark counts** (Bluesky: `bookmark_count`, Threads: not available)

## References & Sources

### Threads API Documentation
- [Threads API Documentation (Postman)](https://www.postman.com/meta/threads/documentation/dht3nzz/threads-api)
- [Meta Threads API Features Comparison](https://data365.co/threads)
- [Guide to Getting Threads Metrics via API](https://creativewritingwizard.com/2024/08/13/a-guide-to-getting-threads-metrics-via-threads-api/)
- Official docs: `https://developers.facebook.com/docs/threads/insights` (access restricted)

### Bluesky AT Protocol Documentation
- [AT Protocol Lexicon - Feed Definitions](https://github.com/bluesky-social/atproto/blob/main/lexicons/app/bsky/feed/defs.json)
- [Bluesky API - getPosts Endpoint](https://docs.bsky.app/docs/api/app-bsky-feed-get-posts)
- [atrium-api Rust Crate Documentation](https://docs.rs/atrium-api/latest/atrium_api/)
- [bsky-sdk Rust Crate](https://docs.rs/bsky-sdk)
- [Complete Guide to Bluesky API Integration](https://www.ayrshare.com/complete-guide-to-bluesky-api-integration-authorization-posting-analytics-comments/)
