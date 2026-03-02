//! Sync scope analysis — what will be synced, nested mounts, excludes.

use super::helpers::*;

// ─── Sync Scope (Scan) ────────────────────────────────────────

/// A mount point found inside a sync source path.
#[derive(serde::Serialize, Clone, Debug)]
pub struct NestedMount {
    /// Absolute mount path, e.g. "/home/max/Pi"
    pub path: String,
    /// Relative path inside the subvolume source (for exclude matching)
    pub rel_path: String,
    /// Mount source device/share, e.g. "192.168.0.21:/home/max"
    pub device: String,
    /// Filesystem type, e.g. "nfs4", "fuse.sshfs"
    pub fstype: String,
    /// Whether rsync will skip this (excluded or cross-filesystem with -x)
    pub excluded: bool,
    /// Reason why it's excluded (or empty if it will be synced)
    pub reason: String,
}

/// Info about one subvolume that will be synced.
#[derive(serde::Serialize, Clone, Debug)]
pub struct SyncScopeEntry {
    pub name: String,
    pub source: String,
    pub subvol: String,
    pub delete: bool,
    pub excludes: Vec<String>,
    pub nested_mounts: Vec<NestedMount>,
}

/// Full sync scope overview.
#[derive(serde::Serialize, Clone, Debug)]
pub struct SyncScope {
    pub direction: String,
    pub boot_sync: bool,
    pub subvolumes: Vec<SyncScopeEntry>,
}

/// Check if a mount path is matched by any rsync exclude pattern.
/// `rel` is relative to the sync source, e.g. "max/Pi" for "/home/max/Pi" syncing "/home/".
fn is_excluded_by(rel: &str, excludes: &[String]) -> Option<String> {
    for exc in excludes {
        let pat = exc
            .trim_start_matches('/')
            .trim_end_matches('/')
            .trim_end_matches("/*")
            .trim_end_matches("/**");
        let rel_clean = rel.trim_start_matches('/').trim_end_matches('/');
        // Direct prefix match: exclude "mnt" matches "mnt/whatever"
        if rel_clean == pat || rel_clean.starts_with(&format!("{}/", pat)) {
            return Some(format!("Exclude-Regel: {}", exc));
        }
    }
    None
}

/// Get the st_dev device ID for a path (used to check if -x will cross).
fn get_device_id(path: &str) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).ok().map(|m| m.dev())
}

#[tauri::command]
pub async fn get_sync_scope() -> Result<SyncScope, String> {
    tokio::task::spawn_blocking(|| {
        let c = cfg();
        let current_uuid = get_boot_uuid();
        let is_primary = current_uuid == c.disks.primary_uuid;
        let direction = format!(
            "{} -> {}",
            if is_primary {
                &c.disks.primary_label
            } else {
                &c.disks.backup_label
            },
            if is_primary {
                &c.disks.backup_label
            } else {
                &c.disks.primary_label
            },
        );

        let mut subvolumes = Vec::new();

        for sv in &c.sync.subvolumes {
            let excludes_raw: Vec<String> = if sv.source == "/" {
                c.sync.system_excludes.clone()
            } else if sv.source.trim_end_matches('/')
                == super::helpers::get_home_mountpoint().trim_end_matches('/')
            {
                let mut exc = c.sync.home_excludes.clone();
                if c.sync.extra_excludes_on_primary && is_primary {
                    exc.extend(c.sync.home_extra_excludes.clone());
                }
                exc
            } else {
                Vec::new()
            };
            let excludes = sanitize_excludes(&excludes_raw);

            // Detect nested mounts using native mountinfo parser
            let source_trimmed = sv.source.trim_end_matches('/');
            let source_dev_id = get_device_id(if source_trimmed.is_empty() {
                "/"
            } else {
                source_trimmed
            });
            let mounts_raw = crate::sysfs::nested_mounts(if source_trimmed.is_empty() {
                "/"
            } else {
                source_trimmed
            });

            let mut nested_mounts = Vec::new();
            for (target, device, fstype) in mounts_raw {
                let rel_path = target
                    .strip_prefix(source_trimmed)
                    .unwrap_or(&target)
                    .trim_start_matches('/')
                    .to_string();

                let (excluded, reason) = if let Some(r) = is_excluded_by(&rel_path, &excludes) {
                    (true, r)
                } else {
                    let mount_dev_id = get_device_id(&target);
                    let same_device = source_dev_id.is_some()
                        && mount_dev_id.is_some()
                        && source_dev_id == mount_dev_id;
                    if same_device {
                        (
                            false,
                            format!(
                                "Gleiches Dateisystem ({}), wird NICHT von -x gefiltert!",
                                fstype
                            ),
                        )
                    } else {
                        (
                            true,
                            format!("Anderes Dateisystem ({}), wird mit -x übersprungen", fstype),
                        )
                    }
                };

                nested_mounts.push(NestedMount {
                    path: target,
                    rel_path,
                    device,
                    fstype,
                    excluded,
                    reason,
                });
            }

            subvolumes.push(SyncScopeEntry {
                name: sv.name.clone(),
                source: sv.source.clone(),
                subvol: sv.subvol.clone(),
                delete: sv.delete,
                excludes: excludes.clone(),
                nested_mounts,
            });
        }

        Ok(SyncScope {
            direction,
            boot_sync: c.boot.sync_enabled,
            subvolumes,
        })
    })
    .await
    .map_err(|e| format!("Sync-Scope thread panicked: {}", e))?
}
