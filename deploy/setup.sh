#!/bin/sh
set -eu

INSTALL_DIR="${INSTALL_DIR:-/opt/ndld}"
SYSTEMD_DIR="/etc/systemd/system"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

usage() {
    echo "Usage: $0 [install|uninstall|diff]"
    exit 1
}

require_root() {
    if [ "$(id -u)" -ne 0 ]; then
        echo "This script must be run as root (or with sudo)"
        exit 1
    fi
}

install() {
    require_root
    echo "Installing ndld auto-update to $INSTALL_DIR..."

    # Validate source files exist
    for f in update.sh ndld-update.service ndld-update.timer; do
        [ -f "$SCRIPT_DIR/$f" ] || { echo "Missing: $SCRIPT_DIR/$f"; exit 1; }
    done
    [ -f "$REPO_ROOT/docker-compose.yml" ] || { echo "Missing: $REPO_ROOT/docker-compose.yml"; exit 1; }

    # Create install dir (root-owned, no world-write)
    mkdir -p "$INSTALL_DIR"
    chmod 755 "$INSTALL_DIR"

    # Copy files (root:root, not world-writable)
    cp "$SCRIPT_DIR/update.sh" "$INSTALL_DIR/"
    cp "$SCRIPT_DIR/ndld-update.service" "$INSTALL_DIR/"
    cp "$SCRIPT_DIR/ndld-update.timer" "$INSTALL_DIR/"
    cp "$REPO_ROOT/docker-compose.yml" "$INSTALL_DIR/"

    chown root:root "$INSTALL_DIR/update.sh"
    chown root:root "$INSTALL_DIR/ndld-update.service"
    chown root:root "$INSTALL_DIR/ndld-update.timer"
    chown root:root "$INSTALL_DIR/docker-compose.yml"
    chmod 755 "$INSTALL_DIR/update.sh"
    chmod 644 "$INSTALL_DIR/ndld-update.service"
    chmod 644 "$INSTALL_DIR/ndld-update.timer"
    chmod 644 "$INSTALL_DIR/docker-compose.yml"

    # Remind about .env file
    if [ ! -f "$INSTALL_DIR/.env" ]; then
        echo ""
        echo "NOTE: Create $INSTALL_DIR/.env with required secrets:"
        echo "  NDL_CLIENT_ID=..."
        echo "  NDL_CLIENT_SECRET=..."
        echo "  NDLD_PUBLIC_URL=..."
        echo "  NDLD_ACME_DOMAIN=..."
        echo "  NDLD_ACME_EMAIL=..."
        echo ""
    fi

    # Symlink systemd units
    ln -sf "$INSTALL_DIR/ndld-update.service" "$SYSTEMD_DIR/"
    ln -sf "$INSTALL_DIR/ndld-update.timer" "$SYSTEMD_DIR/"

    # Enable and start timer
    systemctl daemon-reload
    systemctl enable --now ndld-update.timer

    echo "Done. Check status with: systemctl status ndld-update.timer"
}

uninstall() {
    require_root
    echo "Uninstalling ndld auto-update..."

    # Stop and disable timer
    systemctl disable --now ndld-update.timer 2>/dev/null || true
    systemctl stop ndld-update.service 2>/dev/null || true

    # Remove symlinks
    rm -f "$SYSTEMD_DIR/ndld-update.service"
    rm -f "$SYSTEMD_DIR/ndld-update.timer"

    # Remove installed files (but not .env)
    rm -f "$INSTALL_DIR/update.sh"
    rm -f "$INSTALL_DIR/ndld-update.service"
    rm -f "$INSTALL_DIR/ndld-update.timer"
    rm -f "$INSTALL_DIR/docker-compose.yml"

    systemctl daemon-reload

    echo "Done. Auto-update disabled."
}

diff_files() {
    echo "Comparing repo -> $INSTALL_DIR"
    echo ""
    changed=0
    for f in update.sh ndld-update.service ndld-update.timer; do
        if [ ! -f "$INSTALL_DIR/$f" ]; then
            echo "$f: not installed"
            changed=1
        elif ! diff -q "$SCRIPT_DIR/$f" "$INSTALL_DIR/$f" >/dev/null 2>&1; then
            echo "$f: differs"
            diff --color=auto -u "$INSTALL_DIR/$f" "$SCRIPT_DIR/$f" || true
            echo ""
            changed=1
        else
            echo "$f: up to date"
        fi
    done
    # docker-compose.yml is in repo root
    if [ ! -f "$INSTALL_DIR/docker-compose.yml" ]; then
        echo "docker-compose.yml: not installed"
        changed=1
    elif ! diff -q "$REPO_ROOT/docker-compose.yml" "$INSTALL_DIR/docker-compose.yml" >/dev/null 2>&1; then
        echo "docker-compose.yml: differs"
        diff --color=auto -u "$INSTALL_DIR/docker-compose.yml" "$REPO_ROOT/docker-compose.yml" || true
        echo ""
        changed=1
    else
        echo "docker-compose.yml: up to date"
    fi
    exit $changed
}

case "${1:-}" in
    install)   install ;;
    uninstall) uninstall ;;
    diff)      diff_files ;;
    *)         usage ;;
esac
