//! Configuration data structures — serialized to/from TOML.

use serde::{Deserialize, Serialize};

/// Remote desktop protocol for Pi devices.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RemoteProtocol {
    #[default]
    Rdp,
    Vnc,
}
use crate::util::safe_cmd;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppConfig {
    pub disks: DiskConfig,
    pub sync: SyncConfig,
    pub boot: BootConfig,
    pub snapper: SnapperConfig,
    pub rollback: RollbackConfig,
    #[serde(default)]
    pub pi_remote: PiRemoteConfig,
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
    /// Subvolumes that live only on the primary disk and should be accessible
    /// from both disks.  On the backup's fstab these mounts keep the primary
    /// UUID (not swapped) and get `nofail` so the backup can still boot when
    /// the primary disk is absent.
    #[serde(default)]
    pub shared_subvolumes: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SubvolSync {
    pub name: String,
    pub subvol: String,
    pub source: String,
    pub delete: bool,
}

/// Bootloader type — exhaustive `match` replaces stringly-typed checks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BootloaderType {
    Grub,
    #[default]
    SystemdBoot,
}

impl std::fmt::Display for BootloaderType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Grub => write!(f, "grub"),
            Self::SystemdBoot => write!(f, "systemd-boot"),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BootConfig {
    pub sync_enabled: bool,
    #[serde(default)]
    pub bootloader_type: BootloaderType,
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
    #[serde(default = "default_root_config")]
    pub root_config: String,
}

fn default_root_config() -> String {
    let configs = safe_cmd("snapper", &["list-configs", "--columns", "config"])
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .skip(2)
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    super::detect::detect_snapper_root_config(&configs)
}

// ─── Pi Remote Config ─────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct PiRemoteConfig {
    #[serde(default)]
    pub devices: Vec<PiDeviceConfig>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PiDeviceConfig {
    pub id: String,
    pub label: String,
    pub model: String,
    pub ip: String,
    pub user: String,
    pub ssh_key: String,
    pub mount_point: String,
    /// Remote desktop protocol
    #[serde(default)]
    pub remote_protocol: RemoteProtocol,
    /// Remote desktop port (0 = disabled). Default: 3389 for RDP, 5900 for VNC.
    #[serde(default, alias = "rdp_port")]
    pub remote_port: u16,
    /// RDP password (stored for auto-login, empty = manual login)
    #[serde(default)]
    pub rdp_password: String,
    /// Services to monitor on this Pi
    #[serde(default = "default_pi_services")]
    pub watch_services: Vec<String>,
}

fn default_pi_services() -> Vec<String> {
    vec!["ssh".into(), "xrdp".into(), "nfs-server".into()]
}
