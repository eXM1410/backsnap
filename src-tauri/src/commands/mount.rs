//! Mount/unmount helpers and RAII guard for backup subvolumes.

use super::helpers::*;
use std::fs;

// ─── Mount / Unmount ──────────────────────────────────────────

pub(super) fn mount_subvol(
    dev: &str,
    mnt: &str,
    subvol: &str,
    mount_opts: &str,
) -> Result<(), String> {
    // /mnt/* requires root to create directories – try userspace first, then escalate
    if fs::create_dir_all(mnt).is_err() {
        let r = run_privileged("mkdir", &["-p", mnt]);
        if !r.success {
            return Err(format!("mkdir {}: {}", mnt, r.stderr.trim()));
        }
    }

    if crate::sysfs::is_mountpoint(mnt) {
        if let Some((source, fsroot)) = crate::sysfs::mount_source_fsroot(mnt) {
            let has_correct_subvol = fsroot.contains(&format!("/{}", subvol));
            let has_correct_dev = source.contains(dev);
            if has_correct_subvol && has_correct_dev {
                return Ok(());
            }
        }
        safe_umount(mnt);
        if fs::create_dir_all(mnt).is_err() {
            let _ = run_privileged("mkdir", &["-p", mnt]);
        }
    }

    let opts = format!("subvol=/{},{}", subvol, mount_opts);
    let result = run_privileged("mount", &["-o", &opts, dev, mnt]);
    if !result.success {
        return Err(format!("mount {} -> {}: {}", dev, mnt, result.stderr));
    }
    Ok(())
}

pub(super) fn safe_umount(mnt: &str) {
    let result = run_privileged("umount", &[mnt]);
    if !result.success {
        let _ = run_privileged("umount", &["-l", mnt]);
    }
    if !crate::sysfs::is_mountpoint(mnt) {
        let _ = fs::remove_dir(mnt);
    }
}

/// RAII guard: automatically unmounts a path when dropped.
/// Prevents mount leaks if the calling code panics or returns early.
pub(super) struct AutoUmount(pub String);

impl Drop for AutoUmount {
    fn drop(&mut self) {
        safe_umount(&self.0);
    }
}

// ─── Backup Subvolume Auto-Creation ───────────────────────────

/// Ensure all configured btrfs subvolumes exist on the backup disk.
/// If a subvolume doesn't exist, mounts the backup top-level (subvolid=5),
/// creates it natively, and unmounts.
pub(super) fn ensure_backup_subvolumes(
    backup_dev: &str,
    subvols: &[&str],
    mount_opts: &str,
    log_path: &str,
) {
    let tmp = "/tmp/arclight-ensure-subvol";
    let _ = fs::create_dir_all(tmp);

    // Mount top-level (subvolid=5) of the backup btrfs
    let opts = format!("subvolid=5,{}", mount_opts);
    let mount_res = run_privileged("mount", &["-o", &opts, backup_dev, tmp]);
    if !mount_res.success {
        sync_log(
            log_path,
            &format!(
                "WARNUNG: Backup-Toplevel mount fehlgeschlagen: {}",
                mount_res.stderr.trim()
            ),
        );
        return;
    }
    let _guard = AutoUmount(tmp.to_string());

    for sv in subvols {
        let sv_path = sv.trim_start_matches('/');
        let full_path = format!("{}/{}", tmp, sv_path);

        if std::path::Path::new(&full_path).exists() {
            continue;
        }

        let create_res = run_privileged("btrfs", &["subvolume", "create", &full_path]);
        if create_res.success {
            sync_log(log_path, &format!("Subvolume {} auf Backup erstellt", sv));
        } else {
            sync_log(
                log_path,
                &format!(
                    "WARNUNG: Subvolume {} erstellen fehlgeschlagen: {}",
                    sv,
                    create_res.stderr.trim()
                ),
            );
        }
    }
}
