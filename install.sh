#!/bin/sh
set -e

REPO="pgray/ndl"
TMPBIN="/tmp/ndl-install-$$"

# Determine install directory
if [ -n "$INSTALL_DIR" ]; then
    : # user specified
elif [ -w /usr/local/bin ]; then
    INSTALL_DIR=/usr/local/bin
elif echo "$PATH" | grep -q "$HOME/.local/bin"; then
    INSTALL_DIR="$HOME/.local/bin"
    mkdir -p "$INSTALL_DIR"
else
    INSTALL_DIR=/usr/local/bin  # will need sudo
fi

printf '\033[33mInstall directory:\033[0m %s\n' "$INSTALL_DIR"

# Detect OS
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
    Linux)
        case "$ARCH" in
            x86_64)  TARGET="x86_64-unknown-linux-musl" ;;
            aarch64) TARGET="aarch64-unknown-linux-musl" ;;
            arm64)   TARGET="aarch64-unknown-linux-musl" ;;
            *)       echo "Unsupported architecture: $ARCH"; exit 1 ;;
        esac
        ;;
    Darwin)
        case "$ARCH" in
            arm64)   TARGET="aarch64-apple-darwin" ;;
            aarch64) TARGET="aarch64-apple-darwin" ;;
            *)       echo "Unsupported architecture: $ARCH (only Apple Silicon supported)"; exit 1 ;;
        esac
        ;;
    *)
        echo "Unsupported OS: $OS"
        exit 1
        ;;
esac

printf '\033[33mDetected:\033[0m %s %s -> %s\n' "$OS" "$ARCH" "$TARGET"

# Get latest release tag
LATEST=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name"' | cut -d'"' -f4)
if [ -z "$LATEST" ]; then
    echo "Failed to fetch latest release"
    exit 1
fi
printf '\033[33mLatest release:\033[0m %s\n' "$LATEST"

# Download and extract binary to temp file
URL="https://github.com/$REPO/releases/download/$LATEST/ndl-$TARGET.tar.gz"
printf '\033[33mDownloading:\033[0m %s\n' "$URL"
curl -fsSL "$URL" | tar -xzO ndl > "$TMPBIN"
chmod +x "$TMPBIN"

# Install (mv doubles as cleanup)
if [ -w "$INSTALL_DIR" ]; then
    mv "$TMPBIN" "$INSTALL_DIR/ndl"
else
    printf '\033[33mInstalling to:\033[0m %s (requires sudo)\n' "$INSTALL_DIR"
    sudo mv "$TMPBIN" "$INSTALL_DIR/ndl"
fi

# Remove quarantine on macOS
if [ "$OS" = "Darwin" ]; then
    printf '\033[31mNOTE: Automatically removing quarantine with xattr...\033[0m\n'
    xattr -d com.apple.quarantine "$INSTALL_DIR/ndl" 2>/dev/null || true
fi

printf '\033[33mInstalled:\033[0m %s/ndl\n' "$INSTALL_DIR"
printf "\033[33mVersion:\033[0m $($INSTALL_DIR/ndl --version)\n"

# Warn if install dir isn't in PATH
case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *) printf '\033[33mNOTE: %s is not in your PATH\033[0m\n' "$INSTALL_DIR" ;;
esac

echo ""
echo "Thanks for checking out ndl (needle)!"
printf 'Try \033[32mndl login\033[0m or \033[32mndl login bluesky\033[0m to get started\n'
