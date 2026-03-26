#!/bin/bash

BUILD=${1:-debug}

# Ensure we're in the testing directory
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
cd "$SCRIPT_DIR"

ROOT_DIR="$(dirname "$SCRIPT_DIR")"
TARGET_BIN_DIR="$ROOT_DIR/target/$BUILD"
ln -s "$TARGET_BIN_DIR/watchman_server" "$TARGET_BIN_DIR/watchman" || true
SERVER_PATH="$TARGET_BIN_DIR/watchman_server"
export PATH="$TARGET_BIN_DIR:$PATH"
TEST_IMG=test_btrfs.img
MNT_PATH="$SCRIPT_DIR/test_mnt"

# Cleanup

pkill -f "$SERVER_PATH"
pidwait -f "$SERVER_PATH"
rm -f /tmp/btrfs_watchman.log
rm -f -- "$TEST_IMG"
sudo umount "$MNT_PATH" 2>/dev/null || true

# Create a 1GB file
dd if=/dev/zero of=$TEST_IMG bs=1M count=0 seek=1024 || exit 1

# Create a btrfs filesystem
sudo mkfs.btrfs $TEST_IMG || exit 1

# Mount the filesystem

# Clean up previous mount

mkdir -p "$MNT_PATH"
sudo mount $TEST_IMG "$MNT_PATH" || exit 1
sudo chown $USER:$USER "$MNT_PATH"

# Create a subvolume
btrfs subvolume create "$MNT_PATH/sub1" || exit 1

# Clone the repo into the subvolume
git clone "$ROOT_DIR" "$MNT_PATH/sub1/btrfs_watchman" || exit 1

# ==== In the test repository ====
cd "$MNT_PATH/sub1/btrfs_watchman" || exit 1
jj git init --colocate || exit 1
jj config set --repo fsmonitor.backend watchman
jj config set --repo fsmonitor.watchman.register-snapshot-trigger false
jj status

echo "new line" >> src/snapshot_manager.rs
jj diff --name-only

echo "See also watchman logs in: /tmp/btrfs_watchman.log"
