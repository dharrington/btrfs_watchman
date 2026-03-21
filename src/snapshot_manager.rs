use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use crate::debug_log;

pub struct SnapshotManager {
    btrfs_diff_bin: PathBuf,
}

impl SnapshotManager {
    pub fn new() -> Result<Self> {
        let exe = std::env::current_exe()?;
        let bin_dir = exe.parent().context("Current exe has no parent")?;
        let btrfs_diff_bin = bin_dir.join("btrfs_diff");
        Ok(Self { btrfs_diff_bin })
    }

    pub fn get_subvolume_root(&self, path: &Path) -> Result<PathBuf> {
        let output = Command::new("sudo")
            .arg("-n")
            .arg(&self.btrfs_diff_bin)
            .arg("show-root")
            .arg(path)
            .stderr(std::process::Stdio::inherit())
            .output()?;

        if !output.status.success() {
            anyhow::bail!("Failed to get subvolume root for {}", path.display());
        }
        
        // We now expect stdout to cleanly print out the absolute path to the subvolume root
        let stdout = String::from_utf8_lossy(&output.stdout);
        let root_str = stdout.trim();
        if root_str.is_empty() {
            anyhow::bail!("btrfs_diff show-root returned empty string for {}", path.display());
        }
        Ok(PathBuf::from(root_str))
    }

    pub fn get_snapshot_dir(&self, watch_root: &Path) -> PathBuf {
        let parent = watch_root.parent().unwrap_or(watch_root);
        let name = watch_root.file_name().unwrap_or_default().to_string_lossy();
        parent.join(".jj_watchman_snapshots").join(name.as_ref())
    }

    pub fn ensure_snapshot_dir(&self, watch_root: &Path) -> Result<PathBuf> {
        let snap_dir = self.get_snapshot_dir(watch_root);
        if !snap_dir.exists() {
            std::fs::create_dir_all(&snap_dir)?;
        }
        Ok(snap_dir)
    }

    pub fn create_snapshot(&self, watch_root: &Path, snap_id: &str) -> Result<PathBuf> {
        let snap_dir = self.ensure_snapshot_dir(watch_root)?;
        let snap_path = snap_dir.join(snap_id);
        
        let output = Command::new("sudo")
            .arg("-n")
            .arg(&self.btrfs_diff_bin)
            .arg("snapshot")
            .arg(watch_root)
            .arg(&snap_path)
            .stderr(std::process::Stdio::inherit())
            .output()?;

        if !output.status.success() {
            anyhow::bail!("Failed to create snapshot at {}", snap_path.display());
        }

        Ok(snap_path)
    }

    pub fn delete_snapshot(&self, snap_path: &Path) -> Result<()> {
        let output = Command::new("sudo")
            .arg("-n")
            .arg(&self.btrfs_diff_bin)
            .arg("delete")
            .arg(snap_path)
            .stderr(std::process::Stdio::inherit())
            .output()?;

        if !output.status.success() {
            anyhow::bail!("Failed to delete snapshot at {}", snap_path.display());
        }
        Ok(())
    }

    pub fn diff_snapshots(&self, old_snap: &Path, new_snap: &Path) -> Result<Vec<String>> {
        debug_log!("Spawning btrfs_diff diff {}, {}", old_snap.display(), new_snap.display());
        let output = Command::new("sudo")
            .arg("-n")
            .arg(&self.btrfs_diff_bin)
            .arg("diff")
            .arg(old_snap)
            .arg(new_snap)
            .stderr(std::process::Stdio::inherit())
            .output()?;

        if !output.status.success() {
            anyhow::bail!("Failed to diff snapshots");
        }

        let output_str = String::from_utf8_lossy(&output.stdout);
        let files: Vec<String> = output_str.lines().filter(|s| !s.is_empty()).map(|s| s.to_string()).collect();
        Ok(files)
    }
}
