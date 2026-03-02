//! Backup verification logic.

use super::efi::derive_efi_partition;
use super::helpers::*;
use std::fs;
use std::process::Command;

// ─── Backup Verification ──────────────────────────────────────

#[tauri::command]
pub async fn verify_backup() -> Result<super::helpers::BackupVerifyResult, String> {
    tokio::task::spawn_blocking(verify_backup_internal)
        .await
        .map_err(|e| format!("Verify-Thread panicked: {}", e))?
}

fn verify_backup_internal() -> Result<super::helpers::BackupVerifyResult, String> {
    use super::helpers::{BackupCheck, BackupVerifyResult};

    let c = cfg();
    if c.disks.backup_uuid.is_empty() {
        return Err("Backup-Disk nicht konfiguriert".to_string());
    }

    let backup_uuid = c.disks.backup_uuid.clone();
    let primary_uuid = c.disks.primary_uuid.clone();

    // ── Resolve backup block device (no root needed) ──
    let backup_dev = crate::sysfs::resolve_uuid(&backup_uuid)
        .ok_or_else(|| format!("Backup-Disk nicht gefunden (UUID {})", backup_uuid))?;

    let backup_efi_dev = derive_efi_partition(&backup_dev);

    let root_subvol = c
        .sync
        .subvolumes
        .iter()
        .find(|sv| sv.source == "/").map_or_else(|| "@".to_string(), |sv| sv.subvol.trim_start_matches('/').to_string());

    // ── Single pkexec call: native Rust --verify-collect ──
    let args_json = serde_json::json!({
        "backup_dev": backup_dev,
        "efi_dev": backup_efi_dev,
        "root_subvol": root_subvol,
    });

    let exe = std::env::current_exe().map_or_else(|_| "backsnap".to_string(), |p| p.to_string_lossy().into_owned());

    let result = if is_root() {
        run_cmd(&exe, &["--verify-collect", &args_json.to_string()])
    } else {
        run_cmd(
            "pkexec",
            &[&exe, "--verify-collect", &args_json.to_string()],
        )
    };

    if !result.success {
        return Err(format!("verify-collect fehlgeschlagen: {}", result.stderr));
    }

    // Parse JSON output from --verify-collect
    let collected: VerifyCollectResult = serde_json::from_str(&result.stdout).map_err(|e| {
        format!(
            "Verify-Ergebnis parsen: {} (output: {})",
            e,
            &result.stdout[..result.stdout.len().min(200)]
        )
    })?;

    let mut checks: Vec<BackupCheck> = Vec::new();

    // ── Check 1: fstab UUID ──
    let fstab_check = match &collected.fstab {
        VerifyFstab::Content { data: content } => {
            const CRITICAL: &[&str] = &["/", "/home", "/boot", "/root", "/srv", "/var/cache", "/var/tmp", "/var/log"];

            let mut root_has_backup = false;
            let mut root_has_primary = false;

            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with('#') || trimmed.is_empty() {
                    continue;
                }
                let Some(mp) = trimmed.split_whitespace().nth(1) else { continue };
                let normalized = mp.trim_end_matches('/');
                let normalized = if normalized.is_empty() { "/" } else { normalized };
                if !CRITICAL.contains(&normalized) {
                    continue;
                }
                if trimmed.contains(&backup_uuid) {
                    root_has_backup = true;
                }
                if trimmed.contains(&primary_uuid) {
                    root_has_primary = true;
                }
            }

            if root_has_backup && !root_has_primary {
                BackupCheck {
                    name: "fstab UUID".to_string(),
                    ok: true,
                    detail: format!("Backup-UUID {} ✓ in Root-Mounts, Primary-UUID nicht in kritischen Mounts ✓", &backup_uuid[..8]),
                }
            } else if !root_has_backup {
                BackupCheck {
                    name: "fstab UUID".to_string(),
                    ok: false,
                    detail: "Backup-UUID fehlt in fstab — fstab wurde nicht gepatcht".to_string(),
                }
            } else {
                BackupCheck {
                    name: "fstab UUID".to_string(),
                    ok: false,
                    detail: "Primary-UUID noch in kritischen Mounts (/, /home, /boot) — Backup würde falsche Disk booten".to_string(),
                }
            }
        }
        VerifyFstab::Error { msg: e } => BackupCheck {
            name: "fstab UUID".to_string(),
            ok: false,
            detail: e.clone(),
        },
    };
    checks.push(fstab_check);

    // ── Check 2: EFI boot entries ──
    let efi_check = match &collected.efi_entries {
        VerifyEfi::Content { data: content } => {
            let found = content.contains(&backup_uuid);
            BackupCheck {
                name: "EFI Bootloader".to_string(),
                ok: found,
                detail: if found {
                    "Boot-Eintrag enthält Backup-UUID ✓".to_string()
                } else {
                    "Kein Boot-Eintrag mit Backup-UUID — Loader-Eintrag wurde nicht gepatcht"
                        .to_string()
                },
            }
        }
        VerifyEfi::Error { msg: e } => BackupCheck {
            name: "EFI Bootloader".to_string(),
            ok: false,
            detail: e.clone(),
        },
    };
    checks.push(efi_check);

    // ── Check 3: Sync log (no root needed) ──
    let log_check = {
        let log_content = fs::read_to_string(&c.sync.log_path).unwrap_or_default();
        let synced = log_content.contains("Sync fertig");
        BackupCheck {
            name: "Sync-Log".to_string(),
            ok: synced,
            detail: if synced {
                "Mind. ein erfolgreicher Sync im Log vorhanden ✓".to_string()
            } else {
                "Noch kein erfolgreicher Sync durchgeführt".to_string()
            },
        }
    };
    checks.push(log_check);

    let overall_ok = checks.iter().all(|c| c.ok);
    Ok(BackupVerifyResult {
        backup_dev,
        overall_ok,
        checks,
    })
}

// ─── Verify Collect (runs as root via --verify-collect CLI) ───

#[derive(serde::Deserialize)]
pub(crate) struct VerifyCollectArgs {
    pub backup_dev: String,
    pub efi_dev: String,
    pub root_subvol: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct VerifyCollectResult {
    fstab: VerifyFstab,
    efi_entries: VerifyEfi,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(tag = "status")]
enum VerifyFstab {
    #[serde(rename = "ok")]
    Content { data: String },
    #[serde(rename = "error")]
    Error { msg: String },
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(tag = "status")]
enum VerifyEfi {
    #[serde(rename = "ok")]
    Content { data: String },
    #[serde(rename = "error")]
    Error { msg: String },
}

/// Mount a device read-only, run a closure, then unmount + clean up.
fn mount_read_umount<T>(
    dev: &str,
    mount_opts: &str,
    mount_point: &str,
    read_fn: impl FnOnce(&str) -> Result<T, String>,
) -> Result<T, String> {
    let _ = std::fs::create_dir_all(mount_point);
    let mount = Command::new("mount")
        .args(["-o", mount_opts, dev, mount_point])
        .output();
    match mount {
        Ok(o) if o.status.success() => {
            let result = read_fn(mount_point);
            let _ = Command::new("umount").arg(mount_point).output();
            let _ = std::fs::remove_dir(mount_point);
            result
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr).trim().to_string();
            Err(format!("mount fehlgeschlagen: {}", stderr))
        }
        Err(e) => Err(format!("mount nicht ausführbar: {}", e)),
    }
}

/// Called via `pkexec backsnap --verify-collect <json>`.
/// Runs as root: mounts, reads, unmounts. Outputs JSON to stdout.
#[allow(clippy::print_stdout, clippy::print_stderr)]
pub fn run_verify_collect(json: &str) -> i32 {
    let args: VerifyCollectArgs = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("JSON-Fehler: {}", e);
            return 1;
        }
    };

    let fstab = match mount_read_umount(
        &args.backup_dev, "subvolid=5,ro", "/tmp/backsnap-verify",
        |mnt| {
            let path = format!("{}/{}/etc/fstab", mnt, args.root_subvol);
            fs::read_to_string(&path).map_err(|e| format!("fstab nicht lesbar: {}", e))
        },
    ) {
        Ok(content) => VerifyFstab::Content { data: content },
        Err(msg) => VerifyFstab::Error { msg },
    };

    let efi_entries = if args.efi_dev.is_empty() {
        VerifyEfi::Error { msg: "Konnte EFI-Partition nicht ermitteln".to_string() }
    } else {
        match mount_read_umount(
            &args.efi_dev, "ro", "/tmp/backsnap-verify-efi",
            |mnt| {
                let entries_dir = format!("{}/loader/entries", mnt);
                let mut all = String::new();
                if let Ok(rd) = fs::read_dir(&entries_dir) {
                    for entry in rd.flatten() {
                        if entry.path().extension().is_some_and(|x| x == "conf") {
                            if let Ok(content) = fs::read_to_string(entry.path()) {
                                all.push_str(&content);
                                all.push('\n');
                            }
                        }
                    }
                }
                Ok(all)
            },
        ) {
            Ok(content) => VerifyEfi::Content { data: content },
            Err(msg) => VerifyEfi::Error { msg },
        }
    };

    let result = VerifyCollectResult { fstab, efi_entries };
    match serde_json::to_string(&result) {
        Ok(json) => {
            println!("{}", json);
            0
        }
        Err(e) => {
            eprintln!("JSON serialize: {}", e);
            1
        }
    }
}
