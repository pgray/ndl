# Plan: Like a post with `i`

## Overview

Pressing `i` on the highlighted post (or on a selected reply in the detail
panel) likes it on the current platform. `i` sits under the same right-hand
finger as `r` (reply), so like/reply are a one-key pair.

## Status

- [x] `SocialClient::like_post(&self, post_id)` added to the platform trait
- [x] Bluesky: creates an `app.bsky.feed.like` record via `bsky-sdk`
- [x] Threads: returns `PlatformError::Unsupported` (no like endpoint in the API)
- [x] TUI: `i` key, `AppEvent::LikeResult`, status messages, help text
- [x] Liked marker (♥) in the list, detail header, and replies, from Bluesky's
      `viewer.like`; `i` on an already-liked post is a no-op
- [x] Engagement counts (♥ 💬 ↻) for Bluesky: right-aligned columns in the list
      with a legend in the panel border, plus a counts line in the detail view.
      `Post.stats` is `None` on Threads, so its panel renders as before.
- [x] Threads counts via `GET /{id}/insights` (likes, replies, reposts, quotes,
      shares), one call per post, cached 5 min in `ThreadsClient`; skipped for
      `REPOST_FACADE`. Requires `threads_manage_insights` (added to
      `OAUTH_SCOPES`); an old token gets a permission error, so insights are
      disabled for the session with a warning to run `ndl login` again.
- [x] Follower count in the panel title with the session delta, polled every
      5 min: Threads `GET /me/threads_insights?metric=followers_count`, Bluesky
      `app.bsky.actor.getProfile`.
- [x] README / CLAUDE.md updated
- [ ] Unlike (toggle) — see "Future" below
- [ ] Like counts in the list/detail view (the original scope of this branch)

## Design

### Target selection

`App::selected_target_id()` picks the post an action applies to: the selected
reply if one is highlighted in the detail panel, otherwise the highlighted
post in the list. Both `r` and `i` use it, so they always act on the same
thing the user is looking at.

### Bluesky

A like is a repo record of type `app.bsky.feed.like` whose `subject` is a
strong ref (`uri` + `cid`) of the target post. Post IDs in ndl are already the
AT URI, and `BlueskyClient::get_post_info()` (used by replies) fetches the
`cid`. `BskyAgent::create_record` accepts `like::RecordData` directly.

### Threads

The Threads API (`graph.threads.net`) exposes publishing, replies,
reply-management (hide/unhide), insights, and search. There is no endpoint to
like a post as the authenticated user, so `ThreadsClient::like_post` returns
`PlatformError::Unsupported` and the TUI shows
`Threads error: liking isn't supported by the Threads API`.

Like *counts* for Threads are available only via the Insights API
(`GET /{media_id}/insights?metric=likes`), one call per post. That was the
original scope of this branch and remains future work.

## Future

- **Unlike / toggle**: Bluesky's `PostView.viewer.like` carries the URI of the
  user's own like record when one exists; `BskyAgent::delete_record(uri)`
  removes it. Toggling means tracking that URI per post.
- **Threads counts**: one Insights call per post (`metric=likes,replies,reposts,shares`),
  so fetch only for the selected post and cache; `PostStats.shares` is already
  there for the Threads `shares` metric.
- **Repost** (`app.bsky.feed.repost`) follows the same record pattern as like.

## References

Meta's developer docs are machine-readable (verified 2026-09-13):

- Index of all products: <https://developers.facebook.com/llms.txt>
- Threads doc index: <https://developers.facebook.com/documentation/threads/llms.txt>
- Any Threads page as markdown by appending `.md`, e.g.
  <https://developers.facebook.com/documentation/threads/reference/reply-management.md>
  and <https://developers.facebook.com/documentation/threads/insights.md>
- No OpenAPI spec; a Postman collection exists under `tools-and-resources`.
