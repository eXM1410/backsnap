//! Privileged file I/O and fstab patching for backup disks.

use super::helpers::*;
use std::fs;

/// Swap all occurrences of `a` and `b` in `s` via a temporary placeholder.
fn three_way_swap(s: &str, a: &str, b: &str) -> String {
    if a.is_empty() || b.is_empty() || a == b { return s.to_string(); }
    const PH: &str = "___BACKSNAP_SWAP___";
    s.replace(a, PH).replace(b, a).replace(PH, b)
}

// ─── Privileged file I/O ──────────────────────────────────────

/// Write content to a file that may be owned by root.
/// Tries direct write first, falls back to a single privileged file-op via pkexec.
pub(super) fn write_privileged(path: &str, content: &str) -> Result<(), String> {
    if fs::write(path, content).is_ok() {
        return Ok(());
    }

    run_file_ops_batch(&[FileOp::Write {
        path: path.to_string(),
        content: content.to_string(),
    }])
}

/// Read a file that may be owned by root.
pub(super) fn read_privileged(path: &str) -> Result<String, String> {
    fs::read_to_string(path).or_else(|_| {
        let r = run_privileged("cat", &[path]);
        if r.success {
            Ok(r.stdout)
        } else {
            Err(format!("Lesen fehlgeschlagen: {}", r.stderr.trim()))
        }
    })
}

// ─── Fstab Patching ──────────────────────────────────────────

pub(super) fn patch_backup_fstab(
    backup_mnt: &str,
    primary_btrfs_uuid: &str,
    backup_btrfs_uuid: &str,
    primary_efi_uuid: &str,
    backup_efi_uuid: &str,
    shared_subvolumes: &[String],
    log_path: &str,
) -> Result<(), String> {
    validate_safe_path(backup_mnt, "backup_mnt")?;

    let fstab_path = format!("{}/etc/fstab", backup_mnt);

    let content = read_privileged(&fstab_path).map_err(|e| format!("fstab lesen: {}", e))?;

    let mut patched = content.clone();

    // Swap Btrfs UUIDs
    patched = three_way_swap(&patched, primary_btrfs_uuid, backup_btrfs_uuid);
    // Swap EFI UUIDs
    patched = three_way_swap(&patched, primary_efi_uuid, backup_efi_uuid);

    // ── Shared subvolumes: force UUID back to primary + add nofail ──
    if !shared_subvolumes.is_empty() {
        let mut fixed_lines: Vec<String> = Vec::new();
        let mut shared_count = 0u32;
        for line in patched.lines() {
            let is_shared = shared_subvolumes.iter().any(|sv| {
                line.contains(&format!("subvol=/{}", sv))
                    || line.contains(&format!("subvol=/{},", sv))
            });
            if is_shared && !line.trim_start().starts_with('#') {
                let mut fixed = line.replace(
                    &format!("UUID={}", backup_btrfs_uuid),
                    &format!("UUID={}", primary_btrfs_uuid),
                );
                if !fixed.contains("nofail") {
                    if let Some(pos) = fixed.rfind(" 0 ") {
                        fixed.insert_str(pos, ",nofail");
                    } else if let Some(pos) = fixed.rfind(" 0\n") {
                        fixed.insert_str(pos, ",nofail");
                    } else {
                        let trimmed = fixed.trim_end().to_string();
                        fixed = format!("{},nofail", trimmed);
                    }
                }
                fixed_lines.push(fixed);
                shared_count += 1;
            } else {
                fixed_lines.push(line.to_string());
            }
        }
        patched = fixed_lines.join("\n");
        if shared_count > 0 {
            sync_log(
                log_path,
                &format!(
                    "fstab-Patch: {} Shared-Mount(s) → UUID bleibt auf Primary + nofail",
                    shared_count
                ),
            );
        }
    }

    if patched == content {
        sync_log(log_path, "fstab-Patch: Keine UUIDs zum Tauschen.");
        return Ok(());
    }

    write_privileged(&fstab_path, &patched).map_err(|e| {
        let msg = format!("fstab schreiben: {}", e);
        sync_log(log_path, &msg);
        msg
    })?;

    sync_log(
        log_path,
        &format!(
            "fstab-Patch OK: UUIDs getauscht (Btrfs: {} ↔ {}, EFI: {} ↔ {})",
            primary_btrfs_uuid, backup_btrfs_uuid, primary_efi_uuid, backup_efi_uuid
        ),
    );
    Ok(())
}
