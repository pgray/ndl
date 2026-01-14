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
- **Media type indicators** - Reposts, images, videos, and carousels clearly labeled
- **Minimal footprint** - Runs in a terminal, no Electron bloat

## Installation

```bash
cargo install ndl
```

### From source

```bash
git clone https://github.com/pgray/ndl
cd ndl
cargo build --release
```

## Configuration

needle requires a Threads API access token. See [OAUTH.md](OAUTH.md) for detailed setup instructions.

```bash
# Login interactively (opens browser for OAuth)
ndl login

# Logout
ndl logout
```

Config is stored at `~/.config/ndl/config.toml`.

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
| `r`         | Reply to selected thread |
| `R`         | Refresh feed             |
| `Enter`     | Select / focus detail    |
| `Esc`       | Back / cancel            |
| `?`         | Toggle help              |
| `q`         | Quit                     |

## Roadmap

- [x] OAuth login with auto-generated localhost certs
- [x] View threads feed
- [x] View thread details with nested replies
- [x] Reply to threads
- [x] Auto-refresh (15s)
- [ ] Post new threads
- [ ] Like/repost actions
- [ ] Media preview (images)

## License

MIT

## References

- [Threads API docs](https://developers.facebook.com/docs/threads)
- [ratatui](https://docs.rs/ratatui/latest/ratatui/index.html) - TUI framework
- [initial human written readme](./README.human.md)
