//! Auto-detection logic: system config, subvolumes, snapper, home excludes.

use std::collections::HashSet;
use std::path::PathBuf;

use super::boot::detect_bootloader;
use super::disk::detect_btrfs_disks;
use super::types::BootloaderType;
use super::types::*;
use crate::util::{parse_size_to_bytes, safe_cmd, safe_cmd_timeout};

// ─── Subvolume Detection ──────────────────────────────────────

/// Detect btrfs subvolumes and shared subvolumes (e.g. @games) via
/// `btrfs subvolume list /`.  Returns (sync subvolumes, shared subvolume names).
fn detect_subvolumes_and_shared() -> (Vec<SubvolSync>, Vec<String>) {
    // btrfs subvolume list requires root on most kernels.
    // Try unprivileged first; if that fails, escalate via pkexec (longer timeout).
    let output = safe_cmd("btrfs", &["subvolume", "list", "/"])
        .filter(|o| o.status.success())
        .or_else(|| {
            safe_cmd_timeout(
                "pkexec",
                &["btrfs", "subvolume", "list", "/"],
                std::time::Duration::from_secs(10),
            )
        });
    let raw = match output {
        Some(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => return (fallback_subvolumes(), Vec::new()),
    };

    let fsroot = crate::sysfs::mount_fsroot("/").unwrap_or_default();
    let root_subvol_name = fsroot.trim_start_matches('/');

    let mut subvols: Vec<(String, String)> = Vec::new();
    let mut shared: Vec<String> = Vec::new();
    for line in raw.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if let Some(&path) = parts.last() {
            let top_level = parts
                .iter()
                .position(|&p| p == "top")
                .and_then(|i| parts.get(i + 2))
                .copied()
                .unwrap_or("0");
            if top_level == "5" {
                if path.starts_with('.')
                    || path.contains("/.snapshots")
                    || path.contains("snapshots")
                {
                    continue;
                }
                if path == "@games" {
                    shared.push(path.to_string());
                    continue;
                }
                subvols.push((path.to_string(), guess_subvol_name(path)));
            }
        }
    }

    if subvols.is_empty() {
        return (fallback_subvolumes(), shared);
    }

    let svs = subvols
        .iter()
        .map(|(path, name)| {
            let source = subvol_to_mountpoint(path, root_subvol_name);
            let delete = !matches!(path.as_str(), "@tmp" | "@cache" | "@var-cache");
            SubvolSync {
                name: name.clone(),
                subvol: path.clone(),
                source,
                delete,
            }
        })
        .collect();
    (svs, shared)
}

fn guess_subvol_name(path: &str) -> String {
    match path {
        "@" => "system".to_string(),
        "@home" => "home".to_string(),
        "@root" => "root-home".to_string(),
        "@srv" => "srv".to_string(),
        "@cache" | "@var-cache" => "cache".to_string(),
        "@log" | "@var-log" => "log".to_string(),
        "@tmp" => "tmp".to_string(),
        "@games" => "games".to_string(),
        _ => path.trim_start_matches('@').replace('/', "-"),
    }
}

fn subvol_to_mountpoint(subvol: &str, root_subvol: &str) -> String {
    if subvol == root_subvol {
        return "/".to_string();
    }
    match subvol {
        "@" => "/".to_string(),
        "@home" => crate::commands::helpers::get_home_mountpoint(),
        "@root" => "/root/".to_string(),
        "@srv" => "/srv/".to_string(),
        "@cache" | "@var-cache" => "/var/cache/".to_string(),
        "@log" | "@var-log" => "/var/log/".to_string(),
        "@tmp" => "/tmp/".to_string(),
        "@games" => "/games/".to_string(),
        _ => {
            let clean = subvol.trim_start_matches('@');
            format!("/{}/", clean)
        }
    }
}

fn fallback_subvolumes() -> Vec<SubvolSync> {
    let root_subvol = crate::sysfs::mount_fsroot("/")
        .map(|s| s.trim_start_matches('/').to_string())
        .unwrap_or_default();
    let home_mount = crate::commands::helpers::get_home_mountpoint();
    let home_subvol = crate::sysfs::mount_fsroot(home_mount.trim_end_matches('/'))
        .map(|s| s.trim_start_matches('/').to_string())
        .unwrap_or_default();

    let root_sv = if root_subvol.is_empty() {
        "@".to_string()
    } else {
        root_subvol
    };
    let home_sv = if home_subvol.is_empty() {
        "@home".to_string()
    } else {
        home_subvol
    };

    vec![
        SubvolSync {
            name: guess_subvol_name(&root_sv),
            subvol: root_sv,
            source: "/".to_string(),
            delete: true,
        },
        SubvolSync {
            name: guess_subvol_name(&home_sv),
            subvol: home_sv,
            source: home_mount,
            delete: true,
        },
    ]
}

// ─── Snapper Detection ────────────────────────────────────────

/// Detect snapper root config name (the config that manages /).
pub(crate) fn detect_snapper_root_config(configs: &[String]) -> String {
    if configs.iter().any(|c| c == "root") {
        return "root".to_string();
    }
    for config_name in configs {
        let output = safe_cmd("snapper", &["-c", config_name, "get-config"]);
        if let Some(o) = output {
            let out = String::from_utf8_lossy(&o.stdout);
            for line in out.lines() {
                if line.contains("SUBVOLUME") && line.contains('/') {
                    let parts: Vec<&str> = line.split('|').collect();
                    if parts.len() >= 2 {
                        let val = parts[1].trim();
                        if val == "/" {
                            return config_name.clone();
                        }
                    }
                }
            }
        }
    }
    configs
        .first()
        .cloned()
        .unwrap_or_else(|| "root".to_string())
}

// ─── Mount Options Detection ──────────────────────────────────

fn detect_mount_options() -> String {
    if let Some(opts) = crate::sysfs::mount_options("/") {
        let relevant: Vec<&str> = opts
            .split(',')
            .filter(|o| {
                o.starts_with("compress")
                    || *o == "noatime"
                    || *o == "relatime"
                    || *o == "ssd"
                    || *o == "ssd_spread"
                    || *o == "discard"
                    || o.starts_with("discard=")
                    || *o == "space_cache"
                    || o.starts_with("space_cache=")
            })
            .collect();
        if !relevant.is_empty() {
            return relevant.join(",");
        }
    }
    "compress=zstd,noatime".to_string()
}

// ─── Home Excludes ────────────────────────────────────────────

/// Base home excludes — always included regardless of what's installed.
const BASE_HOME_EXCLUDES: &[&str] = &[
    ".cache",
    ".local/share/Trash",
    ".snapshots",
    "**/.thumbnails",
    "**/__pycache__",
    "**/node_modules",
    "**/target/debug",
    "**/target/release",
    "**/.next",
    "**/dist",
    "**/.venv",
    "**/venv",
];

/// Build the home_excludes list from quick filesystem checks (no `du` or `find`).
fn build_home_excludes(username: &str, has_kde: bool) -> Vec<String> {
    let mut seen: HashSet<String> = BASE_HOME_EXCLUDES
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    let mut exc: Vec<String> = seen.iter().cloned().collect();

    if has_kde {
        let val = ".local/share/baloo".to_string();
        if seen.insert(val.clone()) {
            exc.push(val);
        }
    }

    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from(format!("/home/{username}")));
    if !home.exists() {
        return exc;
    }

    let fast_excludes: &[(&str, &[&str])] = &[
        // Gaming
        (
            ".local/share/Steam/steamapps/common",
            &[
                ".local/share/Steam/steamapps/common",
                ".local/share/Steam/steamapps/shadercache",
                ".local/share/Steam/steamapps/compatdata",
                ".local/share/Steam/steamapps/downloading",
                ".local/share/Steam/steamapps/temp",
            ],
        ),
        (
            ".local/share/Steam/ubuntu12_32",
            &[
                ".local/share/Steam/ubuntu12_32",
                ".local/share/Steam/ubuntu12_64",
            ],
        ),
        (
            ".local/share/Steam/compatibilitytools.d",
            &[".local/share/Steam/compatibilitytools.d"],
        ),
        (".wine", &[".wine"]),
        (
            ".local/share/lutris/runners",
            &[".local/share/lutris/runners", ".local/share/lutris/runtime"],
        ),
        (".config/heroic/tools", &[".config/heroic/tools"]),
        ("Games", &["Games"]),
        // Toolchains
        (".rustup/toolchains", &[".rustup/toolchains", ".rustup/tmp"]),
        (".cargo/registry", &[".cargo/registry", ".cargo/git"]),
        ("Android/Sdk", &["Android/Sdk"]),
        (".android/avd", &[".android/avd"]),
        (
            ".gradle/caches",
            &[
                ".gradle/caches",
                ".gradle/daemon",
                ".gradle/wrapper/dists",
                ".gradle/native",
            ],
        ),
        (".m2/repository", &[".m2/repository"]),
        // Caches & IDE
        (".npm", &[".npm"]),
        (".vscode/extensions", &[".vscode/extensions"]),
        (".vscode-server", &[".vscode-server"]),
        (".eclipse", &[".eclipse"]),
        // Browsers
        (
            ".mozilla/firefox/Crash Reports",
            &[
                ".mozilla/firefox/*/storage",
                ".mozilla/firefox/*/cache2",
                ".mozilla/firefox/Crash Reports",
            ],
        ),
        (
            ".config/chromium/Default/GPUCache",
            &[
                ".config/chromium/Default/Service Worker",
                ".config/chromium/Default/GPUCache",
                ".config/chromium/ShaderCache",
                ".config/chromium/Crash Reports",
            ],
        ),
        // Communication
        (
            ".config/discord/Cache",
            &[
                ".config/discord/Cache",
                ".config/discord/Code Cache",
                ".config/discord/GPUCache",
            ],
        ),
        // GPU caches
        (".nv", &[".nv"]),
        (".local/share/vulkan", &[".local/share/vulkan"]),
        // Containers/VMs
        (".local/share/flatpak", &[".local/share/flatpak"]),
        (".local/share/docker", &[".local/share/docker"]),
        // Other
        (
            ".local/share/recently-used.xbel",
            &[".local/share/recently-used.xbel"],
        ),
        (".local/share/klipper", &[".local/share/klipper"]),
        ("go/pkg", &["go/pkg"]),
    ];

    for (check, paths) in fast_excludes {
        if home.join(check).exists() {
            for p in *paths {
                let s = (*p).to_string();
                if seen.insert(s.clone()) {
                    exc.push(s);
                }
            }
        }
    }

    exc
}

// ─── Auto-Detect Config ──────────────────────────────────────

/// Auto-detect a complete `AppConfig` based on the current system.
pub fn auto_detect_config() -> AppConfig {
    // Run all independent detections in parallel
    let h_disks = std::thread::spawn(detect_btrfs_disks);
    let h_snapper = std::thread::spawn(|| {
        safe_cmd("snapper", &["list-configs", "--columns", "config"])
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .skip(2)
                    .map(|l| l.trim().to_string())
                    .filter(|l| !l.is_empty())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    });
    let h_subvols = std::thread::spawn(detect_subvolumes_and_shared);
    let h_bootloader = std::thread::spawn(detect_bootloader);
    let h_mount_opts = std::thread::spawn(detect_mount_options);

    let detected = h_disks.join().unwrap_or_default();
    let snapper_configs = h_snapper.join().unwrap_or_default();
    let (subvolumes, shared_subvolumes) = h_subvols
        .join()
        .unwrap_or_else(|_| (Vec::new(), Vec::new()));
    let bootloader = h_bootloader.join().unwrap_or(BootloaderType::SystemdBoot);
    let mount_options = h_mount_opts
        .join()
        .unwrap_or_else(|_| "compress=zstd,noatime".to_string());

    let boot_disk = detected.iter().find(|d| d.is_boot);
    let non_boot: Vec<_> = detected.iter().filter(|d| !d.is_boot).collect();
    let backup_disk = match non_boot.len().cmp(&1) {
        std::cmp::Ordering::Equal => Some(non_boot[0]),
        std::cmp::Ordering::Greater => non_boot
            .into_iter()
            .max_by_key(|d| parse_size_to_bytes(&d.size)),
        std::cmp::Ordering::Less => None,
    };

    let primary_uuid = boot_disk.map(|d| d.uuid.clone()).unwrap_or_default();
    let primary_label = boot_disk.map_or_else(
        || "Primary Disk".to_string(),
        |d| {
            if d.model.is_empty() {
                d.label.clone()
            } else {
                d.model.clone()
            }
        },
    );

    let backup_uuid = backup_disk.map(|d| d.uuid.clone()).unwrap_or_default();
    let backup_label = backup_disk.map_or_else(
        || "Backup Disk".to_string(),
        |d| {
            if d.model.is_empty() {
                d.label.clone()
            } else {
                d.model.clone()
            }
        },
    );

    let is_arch = std::path::Path::new("/var/cache/pacman").exists();
    let is_debian = std::path::Path::new("/var/cache/apt").exists();
    let is_fedora = std::path::Path::new("/var/cache/dnf").exists();
    let has_kde = std::path::Path::new("/usr/bin/plasmashell").exists();

    let root_subvol_name = subvolumes
        .iter()
        .find(|s| s.source == "/")
        .map_or_else(|| "@".to_string(), |s| s.subvol.clone());

    let username = std::env::var("USER").unwrap_or_else(|_| "user".to_string());

    let effective_configs = if snapper_configs.is_empty() {
        vec!["root".to_string(), "home".to_string()]
    } else {
        snapper_configs
    };

    let log_path = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("arclight")
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
            timer_unit: "arclight-sync.timer".to_string(),
            service_unit: "arclight-sync.service".to_string(),
            log_path,
            log_max_lines: 2000,
            mount_options,
            mount_base: "/mnt/arclight".to_string(),
            subvolumes,
            system_excludes: {
                let mut exc = vec![
                    format!("{}*", crate::commands::helpers::get_home_mountpoint()),
                    "/boot/*".to_string(),
                    "/mnt/*".to_string(),
                    "/media/*".to_string(),
                    "/proc/*".to_string(),
                    "/sys/*".to_string(),
                    "/dev/*".to_string(),
                    "/run/*".to_string(),
                    "/tmp/*".to_string(),
                    "/var/tmp/*".to_string(),
                    "/.snapshots".to_string(),
                    "/var/log/journal/*".to_string(),
                    "/swapfile".to_string(),
                    "/swap/*".to_string(),
                    "/lost+found".to_string(),
                ];
                if is_arch {
                    exc.push("/var/cache/pacman/pkg/*".to_string());
                }
                if is_debian {
                    exc.push("/var/cache/apt/archives/*".to_string());
                }
                if is_fedora {
                    exc.push("/var/cache/dnf/*".to_string());
                }
                exc
            },
            home_excludes: build_home_excludes(&username, has_kde),
            home_extra_excludes: Vec::new(),
            extra_excludes_on_primary: true,
            shared_subvolumes,
        },
        boot: BootConfig {
            sync_enabled: true,
            bootloader_type: bootloader,
            excludes: match bootloader {
                BootloaderType::SystemdBoot => vec![
                    "loader/entries/*".to_string(),
                    "loader/loader.conf".to_string(),
                ],
                BootloaderType::Grub => vec![
                    "grub/grub.cfg".to_string(),
                    "grub2/grub.cfg".to_string(),
                    "grub/grubenv".to_string(),
                ],
            },
        },
        snapper: SnapperConfig {
            expected_configs: effective_configs.clone(),
        },
        rollback: RollbackConfig {
            max_broken_backups: 2,
            recovery_label: "Rescue".to_string(),
            root_subvol: root_subvol_name,
            root_config: detect_snapper_root_config(&effective_configs),
        },
        pi_remote: PiRemoteConfig::default(),
    }
}
