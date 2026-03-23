#!/usr/bin/env bash
set -e

# If we have Cargo.toml, build the artifacts
if [ -f "Cargo.toml" ]; then
    echo "Building release artifacts..."
    cargo build --release --features debug_logging
    BIN_DIR="target/release"
    WATCHMAN_BIN="$BIN_DIR/watchman_server"
elif [ -d "bin" ]; then
    BIN_DIR="bin"
    WATCHMAN_BIN="$BIN_DIR/watchman"
else
    echo "Error: Cannot find built artifacts. Must be run from source or distribution package."
    exit 1
fi

echo "Installing to /usr/local/bin (will request sudo)..."

echo "Terminating any running watchman instances..."
killall watchman watchman_server btrfs_watchman 2>/dev/null || true

sudo cp "$WATCHMAN_BIN" /usr/local/bin/watchman
sudo cp "$WATCHMAN_BIN" /usr/local/bin/btrfs_watchman
sudo cp "$BIN_DIR/btrfs_diff" /usr/local/bin/

sudo chmod 755 /usr/local/bin/watchman /usr/local/bin/btrfs_watchman
sudo chmod 755 /usr/local/bin/btrfs_diff

echo "Configuring sudoers in /etc/sudoers.d/btrfs_watchman..."
sudo bash -c 'cat > /etc/sudoers.d/btrfs_watchman << "EOF"
ALL ALL=(root) NOPASSWD: /usr/local/bin/btrfs_diff
EOF'
sudo chmod 440 /etc/sudoers.d/btrfs_watchman

echo "Installation complete!"
