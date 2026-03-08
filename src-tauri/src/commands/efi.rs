//! EFI partition helpers and backup snapshot cleanup.

use super::helpers::*;
use std::fs;
use std::process::Command;

// ─── EFI Helpers ──────────────────────────────────────────────

pub(crate) fn derive_efi_partition(btrfs_dev: &str) -> String {
    let parent = Command::new("lsblk")
        .args(["-nro", "PKNAME", btrfs_dev])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    if !parent.is_empty() {
        let result = Command::new("lsblk")
            .args(["-nro", "NAME,PARTTYPE", &format!("/dev/{}", parent)])
            .output();
        if let Ok(o) = result {
            if o.status.success() {
                for line in String::from_utf8_lossy(&o.stdout).lines() {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 2 {
                        let parttype = parts[1].to_lowercase();
                        if parttype == "c12a7328-f81f-11d2-ba4b-00a0c93ec93b" {
                            return format!("/dev/{}", parts[0]);
                        }
                    }
                }
            }
        }
    }

    if let Some(pos) = btrfs_dev.rfind('p') {
        if btrfs_dev[pos + 1..].chars().all(|c| c.is_ascii_digit()) {
            return format!("{}1", &btrfs_dev[..=pos]);
        }
    }
    let base = btrfs_dev.trim_end_matches(|c: char| c.is_ascii_digit());
    format!("{}1", base)
}

pub(crate) fn detect_efi_arch() -> String {
    let uname = Command::new("uname")
        .arg("-m")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map_or_else(
            || "x86_64".to_string(),
            |o| String::from_utf8_lossy(&o.stdout).trim().to_string(),
        );
    match uname.as_str() {
        "aarch64" => "arm64".to_string(),
        "i686" => "i386".to_string(),
        other => other.to_string(),
    }
}

pub(crate) fn efi_arch_suffix(arch: &str) -> &str {
    match arch {
        "aarch64" | "arm64" => "aa64",
        "i386" | "i686" => "ia32",
        _ => "x64",
    }
}

pub(crate) fn get_partition_uuid(dev: &str) -> String {
    crate::sysfs::device_uuid(dev).unwrap_or_default()
}

// ─── Backup Snapshot Cleanup ──────────────────────────────────

/// Remove stale snapper snapshots on a backup subvolume mount.
///
/// `.snapshots` is correctly excluded from rsync (we don't want to copy primary
/// snapshots to the backup), but that also means `--delete` never touches them.
/// Over time the backup accumulates snapshots that consume massive amounts of space.
///
/// This function:
/// 1. Finds `.snapshots/*/snapshot` btrfs subvolumes on the backup mount
/// 2. Deletes them with `btrfs subvolume delete`
/// 3. Removes the leftover xml/info dirs
pub(super) fn cleanup_backup_snapshots(backup_mnt: &str, subvol_name: &str, log_path: &str) {
    let snapshots_dir = format!("{}/.snapshots", backup_mnt);
    let Ok(entries) = fs::read_dir(&snapshots_dir) else {
        return;
    };

    let mut deleted = 0u32;
    let mut errors = 0u32;

    for entry in entries.flatten() {
        let snap_path = entry.path().join("snapshot");
        if !snap_path.exists() {
            continue;
        }

        // Delete the btrfs subvolume first
        let result = run_cmd(
            "btrfs",
            &["subvolume", "delete", &snap_path.to_string_lossy()],
        );
        if result.success {
            // Remove the containing directory (holds info.xml etc.)
            let _ = fs::remove_dir_all(entry.path());
            deleted += 1;
        } else {
            errors += 1;
        }
    }

    if deleted > 0 || errors > 0 {
        let msg = if errors == 0 {
            format!(
                "Backup-Snapshot-Cleanup ({}): {} Snapshots gelöscht",
                subvol_name, deleted
            )
        } else {
            format!(
                "Backup-Snapshot-Cleanup ({}): {} gelöscht, {} Fehler",
                subvol_name, deleted, errors
            )
        };
        sync_log(log_path, &msg);
    }
}
