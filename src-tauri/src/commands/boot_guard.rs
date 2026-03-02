//! Boot Guard — protects systemd-boot entries against accidental modification.
//!
//! Features:
//! 1. Automatic backup of boot entries (called from pacman hook + on-demand)
//! 2. Health checks: kernel/module match, /boot mounted, custom params intact
//! 3. One-click restore of boot entries from backup
//! 4. Diff view of what changed

use std::collections::HashSet;
use crate::config::config_dir;
use super::fstab::write_privileged;
use super::helpers::{run_privileged, list_conf_files};
use serde::{Deserialize, Serialize};

/// Overall boot health status.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    Critical,
    Warning,
    #[default]
    Healthy,
}
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

// ─── Types ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootHealth {
    /// Is /boot mounted?
    pub boot_mounted: bool,
    /// Boot device info (e.g. "nvme0n1p1 (SAMSUNG-EFI)")
    pub boot_device: Option<String>,
    /// Running kernel version
    pub running_kernel: String,
    /// Module directories found on root
    pub installed_modules: Vec<String>,
    /// Do running kernel modules exist on disk?
    pub kernel_module_match: bool,
    /// Boot entry health per entry file
    pub entries: Vec<EntryHealth>,
    /// Overall status
    pub status: HealthStatus,
    /// Human-readable issues list
    pub issues: Vec<String>,
    /// Available backup timestamps
    pub backups: Vec<BackupInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryHealth {
    pub filename: String,
    pub title: String,
    /// Kernel file referenced in entry exists on /boot?
    pub kernel_exists: bool,
    /// Initramfs file referenced in entry exists on /boot?
    pub initramfs_exists: bool,
    /// Custom kernel params present (amdgpu.*, mitigations, etc.)
    pub custom_params_intact: bool,
    /// List of expected params that are missing
    pub missing_params: Vec<String>,
    /// Full options line
    pub options: String,
    /// Changed compared to last backup?
    pub changed_since_backup: bool,
    /// Diff lines (if changed)
    pub diff: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupInfo {
    pub timestamp: u64,
    pub label: String,
    pub entry_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreResult {
    pub success: bool,
    pub restored: Vec<String>,
    pub errors: Vec<String>,
}

// ─── Constants ────────────────────────────────────────────────

const BOOT_ENTRIES_DIR: &str = "/boot/loader/entries";

/// Parameters that should always be present if they were in the backup.
/// These are "important" custom params that users typically add.
const TRACKED_PARAMS: &[&str] = &[
    "mitigations=",
    "amdgpu.",
    "i915.",
    "nvidia.",
    "pcie_aspm=",
    "nowatchdog",
    "quiet",
    "splash",
];

// ─── Helper: backup directory ─────────────────────────────────

fn guard_dir() -> PathBuf {
    config_dir().join("boot-guard")
}

fn backup_dir_for(timestamp: u64) -> PathBuf {
    guard_dir().join(format!("backup-{}", timestamp))
}

// ─── Helper: read boot entries ────────────────────────────────

fn read_entries(dir: &Path) -> Vec<(String, String)> {
    let mut entries = Vec::new();
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            let path = e.path();
            let is_conf = path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("conf"));
            let name = e.file_name().to_string_lossy().into_owned();
            if is_conf && !name.contains(".bak") {
                if let Ok(content) = fs::read_to_string(e.path()) {
                    entries.push((name, content));
                }
            }
        }
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
}

fn read_entries_privileged(dir: &Path) -> Vec<(String, String)> {
    // Try normal read first, fall back to privileged
    let entries = read_entries(dir);
    if !entries.is_empty() {
        return entries;
    }
    // Privileged fallback: use run_privileged (pkexec / root)
    let dir_str = dir.to_str().unwrap_or_default();
    let mut entries: Vec<(String, String)> = list_conf_files(dir_str)
        .into_iter()
        .filter(|name| !name.contains(".bak"))
        .filter_map(|name| {
            let full = format!("{}/{}", dir_str, name);
            let r = run_privileged("cat", &[&full]);
            if r.success && !r.stdout.is_empty() {
                Some((name, r.stdout))
            } else {
                None
            }
        })
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
}

fn parse_entry_field(content: &str, field: &str) -> String {
    content
        .lines()
        .find(|l| l.starts_with(field))
        .map(|l| l[field.len()..].trim().to_string())
        .unwrap_or_default()
}

// ─── Health Check ─────────────────────────────────────────────

fn check_boot_mounted() -> (bool, Option<String>) {
    if let Ok(mounts) = fs::read_to_string("/proc/mounts") {
        for line in mounts.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 && parts[1] == "/boot" {
                let dev = parts[0].to_string();
                // Try to get label
                let r = run_privileged("lsblk", &["-no", "LABEL", &dev]);
                let label = if r.success {
                    let l = r.stdout.trim().to_string();
                    if l.is_empty() { None } else { Some(l) }
                } else {
                    None
                };
                let info = match label {
                    Some(l) => format!("{} ({})", dev, l),
                    None => dev,
                };
                return (true, Some(info));
            }
        }
    }
    (false, None)
}

fn get_running_kernel() -> String {
    fs::read_to_string("/proc/version")
        .ok()
        .and_then(|v| {
            v.split_whitespace()
                .nth(2)
                .map(std::string::ToString::to_string)
        })
        .unwrap_or_else(|| "unknown".to_string())
}

fn get_installed_modules() -> Vec<String> {
    let mut mods = Vec::new();
    if let Ok(rd) = fs::read_dir("/usr/lib/modules") {
        for e in rd.flatten() {
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                mods.push(e.file_name().to_string_lossy().into_owned());
            }
        }
    }
    mods.sort();
    mods
}

fn get_backups() -> Vec<BackupInfo> {
    let dir = guard_dir();
    let mut backups = Vec::new();
    if let Ok(rd) = fs::read_dir(&dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(ts_str) = name.strip_prefix("backup-") {
                if let Ok(ts) = ts_str.parse::<u64>() {
                    // Count .conf files
                    let count = fs::read_dir(e.path())
                        .map(|rd| rd.flatten().filter(|f| {
                            f.file_name().to_string_lossy().ends_with(".conf")
                        }).count())
                        .unwrap_or_default();
                    // Read label if exists
                    let label = fs::read_to_string(e.path().join("label.txt"))
                        .unwrap_or_else(|_| {
                            // Format timestamp as human-readable
                            let dt = chrono_format(ts);
                            format!("Backup {}", dt)
                        });
                    backups.push(BackupInfo {
                        timestamp: ts,
                        label: label.trim().to_string(),
                        entry_count: count,
                    });
                }
            }
        }
    }
    backups.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    backups
}

fn chrono_format(epoch: u64) -> String {
    use chrono::DateTime;
    // CAST-SAFETY: epoch seconds always fit i64 (up to year 292 billion)
    #[allow(clippy::cast_possible_wrap)]
    DateTime::from_timestamp(epoch as i64, 0).map_or_else(|| format!("{}", epoch), |dt| dt.format("%d.%m.%Y %H:%M").to_string())
}

fn latest_backup_entries() -> HashMap<String, String> {
    let backups = get_backups();
    if let Some(latest) = backups.first() {
        let dir = backup_dir_for(latest.timestamp);
        return read_entries(&dir).into_iter().collect();
    }
    HashMap::new()
}

fn compute_diff(old: &str, new: &str) -> Vec<String> {
    let old_set: HashSet<&str> = old.lines().collect();
    let new_set: HashSet<&str> = new.lines().collect();
    let mut diff = Vec::new();
    for l in old.lines() {
        if !new_set.contains(l) {
            diff.push(format!("- {l}"));
        }
    }
    for l in new.lines() {
        if !old_set.contains(l) {
            diff.push(format!("+ {l}"));
        }
    }
    diff
}

// ─── Tauri Commands ───────────────────────────────────────────

/// Check a single boot entry against its backup, returning health + issues.
fn check_entry_health(
    name: &str,
    content: &str,
    backup_entries: &HashMap<String, String>,
) -> (EntryHealth, Vec<String>) {
    let mut issues = Vec::new();

    let title = parse_entry_field(content, "title ");
    let linux = parse_entry_field(content, "linux ");
    let initrd = parse_entry_field(content, "initrd ");
    let options = parse_entry_field(content, "options ");

    // Check kernel/initramfs existence
    let kernel_path = format!("/boot{linux}");
    let initrd_path = format!("/boot{initrd}");
    let kernel_exists = Path::new(&kernel_path).exists()
        || run_privileged("test", &["-f", &kernel_path]).success;
    let initramfs_exists = Path::new(&initrd_path).exists()
        || run_privileged("test", &["-f", &initrd_path]).success;

    if !kernel_exists {
        issues.push(format!("{name}: Kernel {linux} fehlt auf /boot"));
    }
    if !initramfs_exists {
        issues.push(format!("{name}: Initramfs {initrd} fehlt auf /boot"));
    }

    // Check custom params against backup
    let mut missing_params = Vec::new();
    if let Some(backup_content) = backup_entries.get(name) {
        let backup_options = parse_entry_field(backup_content, "options ");
        for param in TRACKED_PARAMS {
            let was_in_backup = backup_options.split_whitespace().any(|p| p.starts_with(param));
            let is_in_current = options.split_whitespace().any(|p| p.starts_with(param));
            if was_in_backup && !is_in_current {
                let full_param = backup_options
                    .split_whitespace()
                    .find(|p| p.starts_with(param))
                    .unwrap_or(param);
                missing_params.push(full_param.to_string());
            }
        }
    }

    let custom_params_intact = missing_params.is_empty();
    if !custom_params_intact {
        issues.push(format!(
            "{name}: Fehlende Kernel-Parameter: {}",
            missing_params.join(", ")
        ));
    }

    let changed_since_backup = backup_entries
        .get(name)
        .is_some_and(|backup| backup.trim() != content.trim());
    let diff = backup_entries
        .get(name)
        .map(|backup| compute_diff(backup, content))
        .unwrap_or_default();

    let entry = EntryHealth {
        filename: name.to_string(),
        title,
        kernel_exists,
        initramfs_exists,
        custom_params_intact,
        missing_params,
        options,
        changed_since_backup,
        diff,
    };

    (entry, issues)
}

/// Full health check of boot configuration.
#[tauri::command]
pub async fn get_boot_health() -> Result<BootHealth, String> {
    tokio::task::spawn_blocking(|| {
        let (boot_mounted, boot_device) = check_boot_mounted();
        let running_kernel = get_running_kernel();
        let installed_modules = get_installed_modules();
        let kernel_module_match = installed_modules.iter().any(|m| m == &running_kernel);
        let backups = get_backups();
        let backup_entries_map = latest_backup_entries();

        let mut issues = Vec::new();
        let mut entries_health = Vec::new();

        if !boot_mounted {
            issues.push("/boot ist nicht gemountet! Kernel-Updates landen nicht auf der EFI-Partition.".to_string());
        }

        if !kernel_module_match {
            issues.push(format!(
                "Kernel/Module-Mismatch: Laufender Kernel {} hat keine passenden Module in /usr/lib/modules/. \
                 Vorhandene Module: {}",
                running_kernel,
                installed_modules.join(", ")
            ));
        }

        // Check entries
        let entries_dir = Path::new(BOOT_ENTRIES_DIR);
        let current_entries = if boot_mounted {
            read_entries_privileged(entries_dir)
        } else {
            vec![]
        };

        for (name, content) in &current_entries {
            let (entry, entry_issues) = check_entry_health(name, content, &backup_entries_map);
            issues.extend(entry_issues);
            entries_health.push(entry);
        }

        let status = if !boot_mounted || !kernel_module_match ||
            entries_health.iter().any(|e| !e.kernel_exists || !e.initramfs_exists) {
            HealthStatus::Critical
        } else if entries_health.iter().any(|e| !e.custom_params_intact || e.changed_since_backup) {
            HealthStatus::Warning
        } else {
            HealthStatus::Healthy
        };

        Ok(BootHealth {
            boot_mounted,
            boot_device,
            running_kernel,
            installed_modules,
            kernel_module_match,
            entries: entries_health,
            status,
            issues,
            backups,
        })
    })
    .await
    .map_err(|e| format!("Thread error: {}", e))?
}

/// Create a backup of all current boot entries.
#[tauri::command]
pub async fn backup_boot_entries(label: Option<String>) -> Result<BackupInfo, String> {
    tokio::task::spawn_blocking(move || {
        let (boot_mounted, _) = check_boot_mounted();
        if !boot_mounted {
            return Err("/boot ist nicht gemountet — Backup nicht möglich.".to_string());
        }

        let entries = read_entries_privileged(Path::new(BOOT_ENTRIES_DIR));
        if entries.is_empty() {
            return Err("Keine Boot-Entries gefunden.".to_string());
        }

        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let dir = backup_dir_for(ts);
        fs::create_dir_all(&dir).map_err(|e| format!("Backup-Verzeichnis: {}", e))?;

        for (name, content) in &entries {
            fs::write(dir.join(name), content)
                .map_err(|e| format!("Schreibe {}: {}", name, e))?;
        }

        // Write label
        let label_text = label.unwrap_or_else(|| {
            format!("Manuelles Backup {}", chrono_format(ts))
        });
        let _ = fs::write(dir.join("label.txt"), &label_text);

        // Also save running kernel version for reference
        let _ = fs::write(dir.join("kernel.txt"), get_running_kernel());

        Ok(BackupInfo {
            timestamp: ts,
            label: label_text,
            entry_count: entries.len(),
        })
    })
    .await
    .map_err(|e| format!("Thread error: {}", e))?
}

/// Restore boot entries from a specific backup.
#[tauri::command]
pub async fn restore_boot_entries(timestamp: u64) -> Result<RestoreResult, String> {
    tokio::task::spawn_blocking(move || {
        let (boot_mounted, _) = check_boot_mounted();
        if !boot_mounted {
            return Err("/boot ist nicht gemountet — Restore nicht möglich.".to_string());
        }

        let dir = backup_dir_for(timestamp);
        if !dir.exists() {
            return Err(format!("Backup {} nicht gefunden.", timestamp));
        }

        let backup_entries = read_entries(&dir);
        if backup_entries.is_empty() {
            return Err("Backup enthält keine Entries.".to_string());
        }

        let mut restored = Vec::new();
        let mut errors = Vec::new();

        for (name, content) in &backup_entries {
            let target = Path::new(BOOT_ENTRIES_DIR).join(name);
            let target_str = target.to_string_lossy().into_owned();
            match write_privileged(&target_str, content) {
                Ok(()) => {
                    restored.push(name.clone());
                }
                Err(e) => {
                    errors.push(format!("{}: {}", name, e));
                }
            }
        }

        Ok(RestoreResult {
            success: errors.is_empty(),
            restored,
            errors,
        })
    })
    .await
    .map_err(|e| format!("Thread error: {}", e))?
}

/// Delete a specific backup.
#[tauri::command]
pub async fn delete_boot_backup(timestamp: u64) -> Result<String, String> {
    let dir = backup_dir_for(timestamp);
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|e| format!("Löschen: {}", e))?;
        Ok("Backup gelöscht".to_string())
    } else {
        Err("Backup nicht gefunden".to_string())
    }
}
