# ndld deployment

Auto-update script and systemd units for ndld.

## Install

```bash
# Clone or copy deploy/ to server, then:
sudo ./setup.sh install

# Or specify custom install dir:
sudo INSTALL_DIR=/srv/ndld ./setup.sh install
```

## Uninstall

```bash
sudo ./setup.sh uninstall
```

## Check for changes

```bash
./setup.sh diff   # exits 0 if up to date, 1 if changes detected
```

## Configuration

Edit `/opt/ndld/ndld-update.service` for environment:

| Variable | Default | Description |
|----------|---------|-------------|
| `COMPOSE_DIR` | `/opt/ndld` | Path to docker-compose.yml |
| `HEALTH_URL` | `http://localhost:8080/health` | Health check endpoint |
| `HEALTH_TIMEOUT` | `30` | Seconds to wait for health check |

Edit `/opt/ndld/ndld-update.timer` for schedule:

| Setting | Default | Description |
|---------|---------|-------------|
| `OnBootSec` | `5min` | First run after boot |
| `OnUnitActiveSec` | `15min` | Interval between runs |
| `RandomizedDelaySec` | `2min` | Random jitter |

After changes: `sudo systemctl daemon-reload`

## Commands

```bash
# Check timer status
systemctl status ndld-update.timer

# View logs
journalctl -u ndld-update.service -f

# Run manually
sudo systemctl start ndld-update.service

# List recent runs
systemctl list-timers ndld-update.timer
```

## Security

The setup applies these protections:

- **File permissions**: Scripts owned by root, not world-writable
- **systemd hardening**: `ProtectSystem=strict`, `ProtectHome=yes`, `PrivateTmp=yes`, `NoNewPrivileges=yes`
- **Path validation**: Requires absolute paths, checks compose file exists
- **Health checks**: Verifies service is healthy after restart
- **Rollback**: Attempts to restore previous state if restart fails
