#!/usr/bin/env bash
set -e

echo "Uninstalling btrfs_watchman..."

echo "Terminating any running watchman instances..."
killall watchman watchman_server btrfs_watchman 2>/dev/null || true

echo "Removing binaries from /usr/local/bin (will request sudo)..."
sudo rm -f /usr/local/bin/watchman
sudo rm -f /usr/local/bin/btrfs_watchman
sudo rm -f /usr/local/bin/btrfs_diff

echo "Uninstallation complete!"
