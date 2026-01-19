# CLAUDE.md

This file provides context for AI assistants working on this codebase.

## Project Overview

**ndl** (needle) is a minimal TUI client for Threads (threads.net). It's a Cargo workspace with two binaries:

- `ndl` - The terminal UI client
- `ndld` - OAuth authentication server

## Directory Structure

```
ndl/
├── Cargo.toml           # Workspace manifest (shared version here)
├── .cargo/config.toml   # Linker config (wild on Linux)
├── ndl/                 # TUI client
│   ├── Cargo.toml
│   ├── build.rs         # Embeds git version at compile time
│   └── src/
│       ├── main.rs      # Entry point, CLI commands (login/logout/--version)
│       ├── config.rs    # Config file handling (~/.config/ndl/config.toml)
│       ├── oauth.rs     # OAuth flows (local + hosted)
│       ├── api.rs       # Threads API client
│       └── tui.rs       # Ratatui-based terminal UI
├── ndld/                # OAuth server
│   ├── Cargo.toml
│   ├── build.rs         # Embeds git version at compile time
│   ├── Dockerfile
│   ├── tests/           # Integration tests
│   └── src/
│       ├── lib.rs       # Library exports for testing
│       ├── main.rs      # Server entry point
│       ├── auth.rs      # Session management (DashMap with TTL)
│       └── routes.rs    # Axum HTTP handlers
└── ndl-core/            # Shared library
    ├── Cargo.toml
    └── src/
        ├── lib.rs
        └── oauth.rs     # Shared OAuth types and token exchange
```

## Key Technologies

- **TUI**: ratatui + crossterm
- **HTTP Server**: axum (both ndl for local OAuth callback, ndld for hosted auth)
- **HTTP Client**: reqwest with rustls
- **Async Runtime**: tokio
- **Serialization**: serde + serde_json, toml for config
- **Linker**: wild (Linux) - configured in `.cargo/config.toml`

## Versioning

Version is defined once in workspace `Cargo.toml` under `[workspace.package]`. Both binaries inherit it via `version.workspace = true`. Git tag/hash is embedded at compile time via `build.rs`.

## Build Commands

```bash
cargo build --workspace          # Build both binaries
cargo build -p ndl               # Build only the TUI client
cargo build -p ndld              # Build only the auth server
cargo run -p ndl                 # Run the TUI
cargo run -p ndld                # Run the auth server
```

## Configuration

Config file: `~/.config/ndl/config.toml`

```toml
access_token = "..."      # Threads API token (set by login)
client_id = "..."         # For local OAuth
client_secret = "..."     # For local OAuth
auth_server = "..."       # Optional: URL of ndld server for hosted auth
```

Environment variables:

- `NDL_OAUTH_ENDPOINT` - OAuth server URL (default: `https://ndl.pgray.dev`, empty string for local OAuth)
- `NDL_CLIENT_ID` / `NDL_CLIENT_SECRET` - Threads app credentials (only needed for local OAuth)
- `NDLD_PUBLIC_URL` - Public URL for ndld (must match Threads redirect URI)
- `NDLD_PORT` - Port for ndld (default: 8080)
- `NDLD_TLS_CERT` / `NDLD_TLS_KEY` - Optional paths to PEM cert/key for manual TLS
- `NDLD_ACME_DOMAIN` - Domain for Let's Encrypt automatic TLS
- `NDLD_ACME_EMAIL` - Email for Let's Encrypt account (required with ACME_DOMAIN)
- `NDLD_ACME_DIR` - Directory to persist ACME certs (default: /var/lib/ndld/acme)
- `NDLD_ACME_STAGING` - If set, use Let's Encrypt staging environment

## Auth Flow

By default, ndl uses hosted OAuth at `https://ndl.pgray.dev`. Set `NDL_OAUTH_ENDPOINT=""` or `auth_server = ""` in config to use local OAuth.

### Local OAuth (ndl only)

1. ndl starts HTTPS server on localhost:1337 with self-signed cert
2. Opens browser to Threads authorization URL
3. User authorizes, Threads redirects to localhost callback
4. ndl exchanges code for token, saves to config

### Hosted OAuth (ndl + ndld)

1. ndl POSTs to `{auth_server}/auth/start`, gets session_id + auth_url
2. Opens browser to auth_url (Threads authorization)
3. User authorizes, Threads redirects to ndld's `/auth/callback`
4. ndld exchanges code for token, stores in session
5. ndl polls `{auth_server}/auth/poll/{session_id}` until completed
6. ndl saves token to config

## Threads API

Base URL: `https://graph.threads.net`

Key endpoints used:

- `GET /me` - User profile
- `GET /me/threads` - User's threads
- `GET /{thread_id}/replies` - Replies to a thread
- `POST /me/threads` - Create container (with `media_type=TEXT`)
- `POST /me/threads_publish` - Publish container

All requests require `access_token` query parameter.

## TUI Architecture

- `App` struct holds all state (threads, selection, input mode, etc.)
- Two panels: threads list (left) and detail view (right)
- Input modes: `Normal`, `Replying`, `Posting`
- Background task refreshes threads every 15 seconds
- Events sent via `mpsc` channel (`AppEvent` enum)

## Code Conventions

- Edition 2024 Rust
- Error handling: `thiserror` for custom error types
- Async: tokio runtime, async/await throughout
- No unwrap in production paths, proper error propagation
- Modules are single files (no `mod/` directories)
- run `cargo search` or `cargo update` to ensure latest deps

## Testing

```bash
cargo test --workspace
```

## Pre-commit Checklist

Before committing changes, run these commands to validate:

```bash
cargo fmt --all                      # Format code
cargo clippy --workspace --all-targets  # Lint for warnings
cargo check --workspace              # Quick compile check
cargo build --workspace              # Full build
cargo test --workspace               # Run all tests
```

All commands must pass without errors. Clippy warnings should be fixed before committing.
