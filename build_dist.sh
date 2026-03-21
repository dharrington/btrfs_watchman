#!/usr/bin/env bash
set -e

VERSION=$(cargo pkgid | cut -d@ -f2)
echo "Building release artifacts for version $VERSION..."
cargo build --release

DIST_DIR="dist"
ARCHIVE_NAME="btrfs-watchman-v${VERSION}.tar.gz"

echo "Preparing dist directory..."
rm -rf "$DIST_DIR"
mkdir -p "$DIST_DIR/btrfs-watchman/bin"

echo "Copying binaries..."
cp target/release/watchman_server "$DIST_DIR/btrfs-watchman/bin/watchman"
cp target/release/btrfs_diff "$DIST_DIR/btrfs-watchman/bin/"

echo "Packaging install script..."
cp install.sh "$DIST_DIR/btrfs-watchman/install.sh"

chmod +x "$DIST_DIR/btrfs-watchman/install.sh"

echo "Packaging release into $ARCHIVE_NAME..."
tar -czf "$ARCHIVE_NAME" -C "$DIST_DIR" .

echo "Cleaning up..."
rm -rf "$DIST_DIR"

echo "Done! Distribution is ready: $ARCHIVE_NAME"
