# ndl - needle

A minimal TUI client for [Threads](https://threads.net) - stay aware of notifications without the distractions of a full social media interface.

![ndl screenshot](pics/srug.png)

## Why needle?

Social media notifications can pull you out of flow state. needle lets you:

- Monitor your feed in a lightweight terminal interface
- Quickly check and respond without opening a browser
- Keep your focus while staying connected

## Features

- **Vim-style navigation** - `h`, `j`, `k`, `l` for intuitive movement
- **Two-panel layout** - Threads list on left, detail view on right (swappable)
- **Thread feed** - View your threads with auto-refresh every 15 seconds
- **Nested replies** - See replies to threads, including replies-to-replies (2 levels deep)
- **Quick replies** - Respond to threads without leaving the terminal
- **Post new threads** - Create new posts directly from the terminal
- **Media type indicators** - Reposts, images, videos, and carousels clearly labeled
- **Minimal footprint** - Runs in a terminal, no Electron bloat

## Project Structure

This is a Cargo workspace with two binaries:

- **ndl** - The TUI client
- **ndld** - OAuth server for hosted authentication (keeps client_secret secure on server)

## Installation

```bash
cargo install ndl
```

### From source

```bash
git clone https://github.com/pgray/ndl
cd ndl
cargo build --release --workspace
```

## Configuration

needle requires a Threads API access token. See [OAUTH.md](OAUTH.md) for detailed setup instructions.

### Default: Hosted Auth

By default, ndl uses the hosted auth server at `ndl.pgray.dev` - no setup required:

```bash
ndl login
```

### Custom Auth Server

To use a different auth server:

```bash
# Via environment variable
export NDL_OAUTH_ENDPOINT=https://your-ndld-server.com
ndl login

# Or add to ~/.config/ndl/config.toml:
# auth_server = "https://your-ndld-server.com"
```

### Local OAuth

If you have your own Threads API credentials and want to run OAuth locally:

```bash
# Set empty endpoint to disable hosted auth
export NDL_OAUTH_ENDPOINT=""
export NDL_CLIENT_ID=your_client_id
export NDL_CLIENT_SECRET=your_client_secret
ndl login
```

### Logout

```bash
ndl logout
```

Config is stored at `~/.config/ndl/config.toml`.

## Running the Auth Server (ndld)

If you want to host your own OAuth server:

```bash
export NDL_CLIENT_ID=your_client_id
export NDL_CLIENT_SECRET=your_client_secret
export NDLD_PUBLIC_URL=https://your-domain.com  # Must match Threads app redirect URI
export NDLD_PORT=8080  # Optional, defaults to 8080

cargo run -p ndld
```

### With Let's Encrypt (ACME)

Automatic TLS certificates via Let's Encrypt:

```bash
export NDLD_ACME_DOMAIN=ndl.example.com
export NDLD_ACME_EMAIL=admin@example.com
export NDLD_ACME_DIR=/var/lib/ndld/acme  # Optional, for cert persistence
export NDLD_PORT=443
cargo run -p ndld
```

Set `NDLD_ACME_STAGING=1` to use Let's Encrypt staging environment for testing.

### With Manual TLS

```bash
export NDLD_TLS_CERT=/path/to/cert.pem
export NDLD_TLS_KEY=/path/to/key.pem
cargo run -p ndld
```

### Docker Compose (Recommended)

```bash
cp .env.example .env
# Edit .env with your credentials

# Create data directory with correct ownership (ndld runs as UID 10001)
sudo mkdir -p /ndld-data
sudo chown 10001:10001 /ndld-data

docker compose up -d
```

### Docker

```bash
docker build -f ndld/Dockerfile -t ndld .
docker run -p 8080:8080 \
  -e NDL_CLIENT_ID=your_client_id \
  -e NDL_CLIENT_SECRET=your_client_secret \
  -e NDLD_PUBLIC_URL=https://your-domain.com \
  ndld
```

For Let's Encrypt in Docker:

```bash
# Create data directory with correct ownership (ndld runs as UID 10001)
sudo mkdir -p /var/lib/ndld
sudo chown 10001:10001 /var/lib/ndld

docker run -p 443:443 \
  -e NDL_CLIENT_ID=your_client_id \
  -e NDL_CLIENT_SECRET=your_client_secret \
  -e NDLD_PUBLIC_URL=https://your-domain.com \
  -e NDLD_PORT=443 \
  -e NDLD_ACME_DOMAIN=your-domain.com \
  -e NDLD_ACME_EMAIL=admin@your-domain.com \
  -v /var/lib/ndld:/var/lib/ndld \
  ndld
```

For manual TLS in Docker:

```bash
docker run -p 443:443 \
  -e NDL_CLIENT_ID=your_client_id \
  -e NDL_CLIENT_SECRET=your_client_secret \
  -e NDLD_PUBLIC_URL=https://your-domain.com \
  -e NDLD_PORT=443 \
  -e NDLD_TLS_CERT=/certs/cert.pem \
  -e NDLD_TLS_KEY=/certs/key.pem \
  -v /path/to/certs:/certs:ro \
  ndld
```

The server exposes:
- `GET /` - Landing page with project info
- `GET /privacy-policy` - Privacy policy
- `GET /tos` - Terms of service
- `POST /auth/start` - Start OAuth session
- `GET /auth/callback` - OAuth callback (configure in Threads app)
- `GET /auth/poll/{session_id}` - Poll for auth completion
- `GET /health` - Health check

## Usage

```bash
ndl
```

### Keybindings

| Key         | Action                   |
| ----------- | ------------------------ |
| `j`/`Down`  | Move down                |
| `k`/`Up`    | Move up                  |
| `h`/`Left`  | Focus threads panel      |
| `l`/`Right` | Focus detail panel       |
| `t`         | Swap panel positions     |
| `p`         | Post new thread          |
| `r`         | Reply to selected thread |
| `R`         | Refresh feed             |
| `Enter`     | Select / focus detail    |
| `Esc`       | Back / cancel            |
| `?`         | Toggle help              |
| `q`         | Quit                     |

## Roadmap

- [x] OAuth login with auto-generated localhost certs
- [x] Hosted OAuth server (ndld) for secure credential management
- [x] View threads feed
- [x] View thread details with nested replies
- [x] Reply to threads
- [x] Post new threads
- [x] Auto-refresh (15s)
- [ ] Like/repost actions
- [ ] Media preview (images)

## Privacy

ndl and ndld do not track, collect, or store any personal information. See [PRIVACY.md](PRIVACY.md) for details.

## License

MIT

## References

- [Threads API docs](https://developers.facebook.com/docs/threads)
- [ratatui](https://docs.rs/ratatui/latest/ratatui/index.html) - TUI framework
- [initial human written readme](./README.human.md)
