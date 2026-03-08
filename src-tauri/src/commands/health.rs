//! Health check and system status commands.

use super::boot::BootValidation;
use super::helpers::*;

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct HealthCheck {
    pub primary_present: bool,
    pub backup_present: bool,
    pub snapper_installed: bool,
    pub rsync_installed: bool,
    pub btrfs_tools: bool,
    pub boot_disk: String,
    pub issues: Vec<String>,
    pub boot_validation: Option<BootValidation>,
}

#[tauri::command]
pub async fn get_health() -> Result<HealthCheck, String> {
    tokio::task::spawn_blocking(|| Ok(get_health_sync()))
        .await
        .map_err(|e| format!("Health-Thread panicked: {}", e))?
}

fn get_health_sync() -> HealthCheck {
    let c = cfg();
    let mut issues = Vec::new();

    let snapper_installed = cmd_exists("snapper");
    let rsync_installed = cmd_exists("rsync");
    let btrfs_tools = cmd_exists("btrfs");
    if !snapper_installed {
        issues.push("snapper nicht installiert".to_string());
    }
    if !rsync_installed {
        issues.push("rsync nicht installiert".to_string());
    }
    if !btrfs_tools {
        issues.push("btrfs-progs nicht installiert".to_string());
    }

    let primary_present = if c.disks.primary_uuid.is_empty() {
        issues.push("Primary Disk nicht konfiguriert".to_string());
        false
    } else {
        let r = crate::sysfs::uuid_exists(&c.disks.primary_uuid);
        if !r {
            issues.push(format!("{} nicht erkannt", c.disks.primary_label));
        }
        r
    };

    let backup_present = if c.disks.backup_uuid.is_empty() {
        issues.push("Backup Disk nicht konfiguriert".to_string());
        false
    } else {
        let r = crate::sysfs::uuid_exists(&c.disks.backup_uuid);
        if !r {
            issues.push(format!("{} nicht erkannt", c.disks.backup_label));
        }
        r
    };

    let boot_uuid = get_boot_uuid();
    let boot_disk = disk_label(&boot_uuid, &c);

    if !c.disks.backup_uuid.is_empty() && boot_uuid == c.disks.backup_uuid {
        issues.push("ACHTUNG: Gebootet von Backup-Disk!".to_string());
    }

    if snapper_installed {
        let configs = get_snapper_configs();
        for expected in &c.snapper.expected_configs {
            if !configs.contains(expected) {
                issues.push(format!("Snapper-Config '{}' fehlt", expected));
            }
        }

        // Automatic timeline snapshot creation/cleanup is handled by snapper's systemd timers.
        // arclight itself does not run periodic snapper cleanup.
        if cmd_exists("systemctl") {
            let timeline = run_cmd("systemctl", &["is-active", "snapper-timeline.timer"]);
            if timeline.stdout.trim() != "active" {
                issues.push(
                    "snapper-timeline.timer nicht aktiv — keine automatischen Timeline-Snapshots"
                        .to_string(),
                );
            }

            let cleanup = run_cmd("systemctl", &["is-active", "snapper-cleanup.timer"]);
            if cleanup.stdout.trim() != "active" {
                issues.push(
                    "snapper-cleanup.timer nicht aktiv — alte Timeline-Snapshots werden nicht automatisch gelöscht"
                        .to_string(),
                );
            }
        }
    }

    let timer = run_cmd("systemctl", &["is-active", &c.sync.timer_unit]);
    if timer.stdout.trim() != "active" {
        issues.push(format!("{} nicht aktiv", c.sync.timer_unit));
    }

    let boot_validation = if backup_present && c.boot.sync_enabled {
        let other_uuid = if boot_uuid == c.disks.primary_uuid {
            &c.disks.backup_uuid
        } else {
            &c.disks.primary_uuid
        };
        if let Some(backup_dev) = crate::sysfs::resolve_uuid(other_uuid) {
            let backup_efi = super::efi::derive_efi_partition(&backup_dev);
            let validation = super::boot::get_cached_boot_validation(&backup_efi, &c);
            for issue in &validation.entry_issues {
                issues.push(format!("Boot: {}", issue));
            }
            Some(validation)
        } else {
            None
        }
    } else {
        None
    };

    HealthCheck {
        primary_present,
        backup_present,
        snapper_installed,
        rsync_installed,
        btrfs_tools,
        boot_disk,
        issues,
        boot_validation,
    }
}

#[tauri::command]
pub async fn get_system_status() -> Result<SystemStatus, String> {
    tokio::task::spawn_blocking(|| Ok(get_system_status_sync()))
        .await
        .map_err(|e| format!("Status-Thread panicked: {}", e))?
}

fn get_system_status_sync() -> SystemStatus {
    let c = cfg();
    let hostname = read_sys("/proc/sys/kernel/hostname");
    let kernel = read_sys("/proc/sys/kernel/osrelease");

    let uptime_raw = read_sys("/proc/uptime");
    let uptime_secs: f64 = uptime_raw
        .split_whitespace()
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);
    // CAST-SAFETY: /proc/uptime is always ≥0; quotients are small positive values that fit u64
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let (days, hours, mins) = (
        (uptime_secs / 86400.0) as u64,
        ((uptime_secs % 86400.0) / 3600.0) as u64,
        ((uptime_secs % 3600.0) / 60.0) as u64,
    );
    let uptime = if days > 0 {
        format!("{}d {}h {}m", days, hours, mins)
    } else if hours > 0 {
        format!("{}h {}m", hours, mins)
    } else {
        format!("{}m", mins)
    };

    let boot_uuid = get_boot_uuid();
    let boot_disk = disk_label(&boot_uuid, &c);
    let backup_disk = if boot_uuid == c.disks.primary_uuid {
        c.disks.backup_label.clone()
    } else {
        c.disks.primary_label.clone()
    };

    let disks = get_disk_info();
    let snapper_configs = get_snapper_configs();
    // Parallelize snapshot counts — each snapper call is an independent subprocess
    let snapshot_counts: Vec<SnapshotCount> = std::thread::scope(|s| {
        let handles: Vec<_> = snapper_configs
            .iter()
            .map(|config| {
                let config = config.clone();
                s.spawn(move || SnapshotCount {
                    count: get_snapshot_count(&config),
                    config,
                })
            })
            .collect();
        handles.into_iter().filter_map(|h| h.join().ok()).collect()
    });
    // Reuse already-computed boot_uuid to avoid a second findmnt call inside get_sync_status_internal
    let sync_status = super::sync_cmd::get_sync_status_internal(&c, Some(&boot_uuid));

    let boot_info = super::boot::get_cached_boot_info(&c);

    SystemStatus {
        hostname,
        kernel,
        uptime,
        boot_disk,
        backup_disk,
        boot_uuid,
        disks,
        snapper_configs,
        snapshot_counts,
        sync_status,
        boot_info: Some(boot_info),
    }
}
