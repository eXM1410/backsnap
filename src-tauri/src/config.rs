use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

// ─── Config Structures ────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppConfig {
    pub disks: DiskConfig,
    pub sync: SyncConfig,
    pub boot: BootConfig,
    pub snapper: SnapperConfig,
    pub rollback: RollbackConfig,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DiskConfig {
    pub primary_uuid: String,
    pub primary_label: String,
    pub backup_uuid: String,
    pub backup_label: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SyncConfig {
    pub timer_unit: String,
    pub service_unit: String,
    pub log_path: String,
    pub log_max_lines: usize,
    pub mount_options: String,
    pub mount_base: String,
    pub subvolumes: Vec<SubvolSync>,
    pub system_excludes: Vec<String>,
    pub home_excludes: Vec<String>,
    pub home_extra_excludes: Vec<String>,
    pub extra_excludes_on_primary: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SubvolSync {
    pub name: String,
    pub subvol: String,
    pub source: String,
    pub delete: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BootConfig {
    pub sync_enabled: bool,
    pub excludes: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SnapperConfig {
    pub expected_configs: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RollbackConfig {
    pub max_broken_backups: usize,
    pub recovery_label: String,
    pub root_subvol: String,
}

// ─── Config Path ──────────────────────────────────────────────

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("backsnap")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

// ─── Load / Save ──────────────────────────────────────────────

pub fn load_config() -> Result<AppConfig, String> {
    load_config_from(&config_path())
}

pub fn load_config_from(path: &std::path::Path) -> Result<AppConfig, String> {
    if !path.exists() {
        // Auto-generate on first run
        let config = auto_detect_config();
        save_config(&config)?;
        return Ok(config);
    }
    let content = fs::read_to_string(path)
        .map_err(|e| format!("Config lesen fehlgeschlagen ({}): {}", path.display(), e))?;
    toml::from_str(&content)
        .map_err(|e| format!("Config-Fehler in {}: {}", path.display(), e))
}

pub fn save_config(config: &AppConfig) -> Result<(), String> {
    let dir = config_dir();
    fs::create_dir_all(&dir)
        .map_err(|e| format!("Konnte {} nicht erstellen: {}", dir.display(), e))?;
    let content = toml::to_string_pretty(config)
        .map_err(|e| format!("Config serialisieren: {}", e))?;
    let path = config_path();
    fs::write(&path, &content)
        .map_err(|e| format!("Config schreiben ({}): {}", path.display(), e))?;
    Ok(())
}

// ─── Auto-Detection ───────────────────────────────────────────

/// Detected btrfs disk for the user to choose from
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DetectedDisk {
    pub device: String,
    pub uuid: String,
    pub label: String,
    pub size: String,
    pub model: String,
    pub mountpoint: Option<String>,
    pub is_boot: bool,
}

/// Detect all btrfs partitions on the system
pub fn detect_btrfs_disks() -> Vec<DetectedDisk> {
    // lsblk with all info we need
    let result = Command::new("lsblk")
        .args([
            "-o", "NAME,UUID,LABEL,SIZE,MODEL,MOUNTPOINT,FSTYPE,TYPE",
            "-J", "-b",
        ])
        .output();

    let output = match result {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => return Vec::new(),
    };

    let json: serde_json::Value = match serde_json::from_str(&output) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let boot_uuid = Command::new("findmnt")
        .args(["/", "-o", "UUID", "-n"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    let mut disks = Vec::new();

    fn walk_devices(
        devices: &serde_json::Value,
        parent_model: &str,
        boot_uuid: &str,
        results: &mut Vec<DetectedDisk>,
    ) {
        if let Some(arr) = devices.as_array() {
            for dev in arr {
                let fstype = dev["fstype"].as_str().unwrap_or("");
                let dev_type = dev["type"].as_str().unwrap_or("");
                let uuid = dev["uuid"].as_str().unwrap_or("");
                let model = dev["model"].as_str().unwrap_or(parent_model);
                let name = dev["name"].as_str().unwrap_or("");

                if fstype == "btrfs" && dev_type == "part" && !uuid.is_empty() {
                    let size_bytes: u64 = dev["size"]
                        .as_str()
                        .and_then(|s| s.parse().ok())
                        .or_else(|| dev["size"].as_u64())
                        .unwrap_or(0);
                    let size = format_size(size_bytes);
                    let mountpoint = dev["mountpoint"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string());
                    let label = dev["label"].as_str().unwrap_or("").to_string();

                    results.push(DetectedDisk {
                        device: format!("/dev/{}", name),
                        uuid: uuid.to_string(),
                        label: if label.is_empty() {
                            model.to_string()
                        } else {
                            label
                        },
                        size,
                        model: model.to_string(),
                        mountpoint,
                        is_boot: uuid == boot_uuid,
                    });
                }

                // Recurse into children (partitions)
                if let Some(children) = dev.get("children") {
                    walk_devices(children, model, boot_uuid, results);
                }
            }
        }
    }

    if let Some(devices) = json.get("blockdevices") {
        walk_devices(devices, "", &boot_uuid, &mut disks);
    }

    disks
}

fn format_size(bytes: u64) -> String {
    const GB: u64 = 1_073_741_824;
    const TB: u64 = 1_099_511_627_776;
    if bytes >= TB {
        format!("{:.1} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else {
        format!("{} MB", bytes / (1024 * 1024))
    }
}

fn parse_size_to_bytes(size: &str) -> u64 {
    let s = size.trim();
    if let Some(val) = s.strip_suffix(" TB") {
        (val.trim().parse::<f64>().unwrap_or(0.0) * 1_099_511_627_776.0) as u64
    } else if let Some(val) = s.strip_suffix(" GB") {
        (val.trim().parse::<f64>().unwrap_or(0.0) * 1_073_741_824.0) as u64
    } else if let Some(val) = s.strip_suffix(" MB") {
        val.trim().parse::<u64>().unwrap_or(0) * 1_048_576
    } else {
        0
    }
}

/// Auto-detect config based on current system
pub fn auto_detect_config() -> AppConfig {
    let detected = detect_btrfs_disks();

    let boot_disk = detected.iter().find(|d| d.is_boot);
    // Smart backup disk selection:
    // - Exactly 2 btrfs disks: pick the non-boot one
    // - >2 disks: pick the largest non-boot (user can change in Settings)
    // - 0 or 1: leave empty
    let non_boot: Vec<&DetectedDisk> = detected.iter().filter(|d| !d.is_boot).collect();
    let backup_disk = if non_boot.len() == 1 {
        Some(non_boot[0])
    } else if non_boot.len() > 1 {
        // Pick largest non-boot disk
        non_boot.into_iter().max_by_key(|d| parse_size_to_bytes(&d.size))
    } else {
        None
    };

    let primary_uuid = boot_disk.map(|d| d.uuid.clone()).unwrap_or_default();
    let primary_label = boot_disk
        .map(|d| {
            if d.model.is_empty() {
                d.label.clone()
            } else {
                d.model.clone()
            }
        })
        .unwrap_or_else(|| "Primary Disk".to_string());

    let backup_uuid = backup_disk.map(|d| d.uuid.clone()).unwrap_or_default();
    let backup_label = backup_disk
        .map(|d| {
            if d.model.is_empty() {
                d.label.clone()
            } else {
                d.model.clone()
            }
        })
        .unwrap_or_else(|| "Backup Disk".to_string());

    // Detect snapper configs
    let snapper_configs = Command::new("snapper")
        .args(["list-configs", "--columns", "config"])
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .skip(2)
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    // Detect btrfs subvolumes - find which ones are for system and home
    let subvolumes = vec![
        SubvolSync {
            name: "system".to_string(),
            subvol: "@".to_string(),
            source: "/".to_string(),
            delete: true,
        },
        SubvolSync {
            name: "home".to_string(),
            subvol: "@home".to_string(),
            source: "/home/".to_string(),
            delete: true,
        },
    ];

    // Detect current username for smart excludes
    let username = std::env::var("USER").unwrap_or_else(|_| "user".to_string());

    // User-writable log path (no root needed)
    let log_path = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("backsnap")
        .join("sync.log")
        .to_string_lossy()
        .to_string();

    AppConfig {
        disks: DiskConfig {
            primary_uuid,
            primary_label,
            backup_uuid,
            backup_label,
        },
        sync: SyncConfig {
            timer_unit: "backsnap-sync.timer".to_string(),
            service_unit: "backsnap-sync.service".to_string(),
            log_path,
            log_max_lines: 2000,
            mount_options: "compress=zstd,noatime".to_string(),
            mount_base: "/mnt/backsnap".to_string(),
            subvolumes,
            system_excludes: vec![
                "/home/*".to_string(),
                "/boot/*".to_string(),
                "/mnt/*".to_string(),
                "/proc/*".to_string(),
                "/sys/*".to_string(),
                "/dev/*".to_string(),
                "/run/*".to_string(),
                "/tmp/*".to_string(),
                "/var/tmp/*".to_string(),
                "/var/cache/pacman/pkg/*".to_string(),
                "/.snapshots".to_string(),
                "/var/log/journal/*".to_string(),
                "/swapfile".to_string(),
                "/swap/*".to_string(),
                "/lost+found".to_string(),
            ],
            home_excludes: vec![
                ".cache".to_string(),
                ".local/share/Trash".to_string(),
                ".snapshots".to_string(),
                ".local/share/baloo".to_string(),
                "**/.thumbnails".to_string(),
                "**/__pycache__".to_string(),
                "**/node_modules".to_string(),
                "**/target/debug".to_string(),
                "**/target/release".to_string(),
                "**/.rustup/toolchains".to_string(),
            ],
            home_extra_excludes: vec![
                format!("{}/Games", username),
                format!("{}/.local/share/Steam/steamapps/common", username),
                format!("{}/.local/share/Steam/steamapps/shadercache", username),
                format!("{}/.local/share/Steam/steamapps/compatdata", username),
            ],
            extra_excludes_on_primary: true,
        },
        boot: BootConfig {
            sync_enabled: true,
            excludes: vec![
                "loader/entries/*".to_string(),
                "loader/loader.conf".to_string(),
                "EFI/".to_string(),
            ],
        },
        snapper: SnapperConfig {
            expected_configs: if snapper_configs.is_empty() {
                vec!["root".to_string(), "home".to_string()]
            } else {
                snapper_configs
            },
        },
        rollback: RollbackConfig {
            max_broken_backups: 2,
            recovery_label: "Rescue".to_string(),
            root_subvol: "@".to_string(),
        },
    }
}
