# Btrfs Watchman

`btrfs_watchman` is a lightweight, drop-in replacement for Facebook's [Watchman](https://facebook.github.io/watchman/) daemon, specifically designed to accelerate filesystem monitoring by leveraging **Btrfs snapshots** instead of `inotify` or full directory crawls.

It is purpose-built to integrate with the [Jujutsu (jj)](https://github.com/martinvonz/jj) version control system, allowing `jj status` to perform fast file checks on even large repositories.

## How it Works

Traditional Watchman instances continuously crawl your active filesystem or consume operating system `inotify` limits to track every single file change in real-time. 

`btrfs_watchman` takes a different approach:
1. When a client like `jj` requests the current state of the repository, the daemon instantly takes a lightweight, Btrfs snapshot of the filesystem.
2. When the client queries for changes "since" a previous point in time, `btrfs_watchman` performs a highly efficient Btrfs tree difference between the previous snapshot and the current state.
3. It translates this diff into standard Watchman protocol responses.

This completely eliminates the need for background filesystem crawling, persistent inotify handles, or running a heavy-weight, memory-hungry daemon in the background.

## Installation

### From Source
Ensure you have `cargo` and Rust installed, then run the installer script:
```bash
git clone https://github.com/your-username/btrfs_watchman.git
cd btrfs_watchman
./install.sh
```

### From Distribution tarball
If you downloaded the pre-compiled `btrfs-watchman-v0.1.0.tar.gz` distribution (or whichever latest version):
```bash
tar -xzf btrfs-watchman-v0.1.0.tar.gz
cd btrfs-watchman
./install.sh
```

## Post-Installation Setup
The `install.sh` script does most of the heavy lifting, including compiling the binaries and moving them to `/usr/local/bin`. However, you may require some manual setup:

### 1. Enable passwordless Btrfs commands
Because Btrfs requires `root` privileges to create and manage snapshots, the daemon includes a helper binary called `btrfs_diff`. The installer places a `sudoers.d` config to allow running this binary without a password. On some systems, you may need to explicitly enable `sudoers.d` configurations. To be sure, ensure that `sudoers.d` is enabled:
```bash
sudo cat /etc/sudoers
```

### Configuring Jujutsu (jj)

To tell `jj` to utilize `btrfs_watchman` for filesystem monitoring, you need to update your Jujutsu configuration for your repository:

```sh
jj config set --repo fsmonitor.backend watchman
jj config set --repo fsmonitor.watchman.register-snapshot-trigger false
```

## Advanced Usage

### Snapshot Cleanup
The daemon automatically creates lightweight Btrfs snapshots dynamically in a hidden `.jj_watchman_snapshots` directory adjacent to your Btrfs subvolumes.

If you ever need to manually purge orphaned or stale snapshots across your entire subvolume, you can run the builtin cleanup tool:
```bash
sudo /usr/local/bin/btrfs_diff cleanup /path/to/btrfs/mount
```
